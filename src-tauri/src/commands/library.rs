//! The library: listing, thumbnails, titles, favourites, markers, the recycle bin,
//! sharing and imports.

use std::path::PathBuf;
use std::sync::atomic::Ordering;

use parking_lot::Mutex;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::i18n::tr;
use crate::library::{self, Draft, Item};
use crate::{blocking, err, in_order, media, share, platform, AppState, Res};

#[tauri::command]
pub async fn list_library(app: AppHandle) -> Res<Vec<Item>> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let root = state.settings.lock().clips_dir.clone();
        let _ = std::fs::create_dir_all(&root);
        let removals = state.removals.load(Ordering::Relaxed);
        let (mut items, deferred) = library::scan(&root, &state.thumbs_dir());
        // Some files waited to be read; the page scans again once Replix is free.
        if deferred {
            let _ = app.emit("scan-deferred", ());
        }
        {
            let mut index = state.index.lock();
            // Something went to the recycle bin while the scan ran; leave it out.
            if state.removals.load(Ordering::Relaxed) != removals {
                items.retain(|i| std::path::Path::new(&i.path).exists());
            }
            index.paths = items.iter().map(|i| (i.id.clone(), PathBuf::from(&i.path))).collect();
        }
        queue_thumbs(&app, &items);
        items
    })
    .await
    .map_err(err)
}

type ThumbJob = (String, PathBuf, library::Kind, f64);

/// Missing thumbnails wait here for the one thread that makes them.
static THUMBS: (Mutex<std::collections::VecDeque<ThumbJob>>, parking_lot::Condvar) =
    (parking_lot::const_mutex(std::collections::VecDeque::new()), parking_lot::Condvar::new());

/// Queues missing thumbnails; one thread makes them one after another and tells the UI
/// about each. A second scan adds to the queue rather than starting a thread of its own.
fn queue_thumbs(app: &AppHandle, items: &[Item]) {
    let state = app.state::<AppState>();
    let mut pending = state.thumbs_pending.lock();
    let todo: Vec<ThumbJob> = items
        .iter()
        .filter(|i| i.thumb.is_none() && pending.insert(i.id.clone()))
        .map(|i| (i.id.clone(), PathBuf::from(&i.path), i.meta.kind, i.meta.duration))
        .collect();
    drop(pending);
    if todo.is_empty() {
        return;
    }
    static WORKER: std::sync::Once = std::sync::Once::new();
    let app = app.clone();
    WORKER.call_once(move || {
        std::thread::Builder::new().name("thumbnails".into()).spawn(move || make_thumbs(app)).expect("thread");
    });
    THUMBS.0.lock().extend(todo);
    THUMBS.1.notify_one();
}

fn make_thumbs(app: AppHandle) {
    let dir = app.state::<AppState>().thumbs_dir();
    // Thumbnails are made ahead of time, for the library: they give way to the editor and
    // wait while Replix is hidden or in the background, then pick up where they were.
    let demand = media::Demand::new(media::AHEAD);
    loop {
        let (id, path, kind, duration) = {
            let mut queue = THUMBS.0.lock();
            loop {
                if let Some(job) = queue.pop_front() {
                    break job;
                }
                THUMBS.1.wait(&mut queue);
            }
        };
        let out = dir.join(format!("{id}.jpg"));
        // Made meanwhile, e.g. queued again by a scan that listed the folder before it was.
        if out.exists() {
            let _ = app.emit("thumb", (id.clone(), out.to_string_lossy().into_owned()));
            app.state::<AppState>().thumbs_pending.lock().remove(&id);
            continue;
        }
        // Failed a moment ago and unchanged since: a scan asks again, but it would fail again.
        if library::THUMB_FAILURES.recent(&path) {
            app.state::<AppState>().thumbs_pending.lock().remove(&id);
            continue;
        }
        loop {
            match media::run(&demand, |go| library::make_thumb(&path, kind, duration, &out, go)) {
                media::Outcome::Done(()) => {
                    library::THUMB_FAILURES.forget(&path);
                    let _ = app.emit("thumb", (id.clone(), out.to_string_lossy().into_owned()));
                    break;
                }
                // Hidden, unfocused or the editor's turn: wait until it may run again.
                media::Outcome::Stopped => media::wait_until_ahead_may_run(),
                media::Outcome::Failed(_) => {
                    library::THUMB_FAILURES.note(&path);
                    break;
                }
            }
        }
        app.state::<AppState>().thumbs_pending.lock().remove(&id);
    }
}

/// The fields of an item that changed, by their names in the UI.
type Patch = serde_json::Map<String, serde_json::Value>;

/// Changes one item's sidecar and returns which fields changed. Callers tell the UI about
/// those fields only, instead of making it scan the whole library again; a patch holds no
/// other field, so one sent late cannot put back an older value of something else.
pub(crate) fn update_meta(app: &AppHandle, id: &str, change: impl FnOnce(&mut library::Meta)) -> Res<(String, Patch)> {
    let path = app.state::<AppState>().path_of(id)?;
    let _held = library::META_LOCK.lock();
    // The clip may have gone to the recycle bin since its path was looked up.
    if !path.exists() {
        return Err(tr("That clip no longer exists."));
    }
    let root = app.state::<AppState>().settings.lock().clips_dir.clone();
    let mut meta = library::meta_or_describe(&path, &root);
    let fields = |m: &library::Meta| match serde_json::to_value(m) {
        Ok(serde_json::Value::Object(map)) => map,
        _ => Patch::new(),
    };
    let before = fields(&meta);
    change(&mut meta);
    library::write_meta(&path, &meta).map_err(err)?;
    let patch = fields(&meta).into_iter().filter(|(k, v)| before.get(k) != Some(v)).collect();
    Ok((id.to_string(), patch))
}

#[tauri::command]
pub async fn save_draft(app: AppHandle, id: String, title: String, draft: Draft) -> Res<()> {
    in_order(move || {
        let changed = update_meta(&app, &id, |m| {
            m.title = title;
            m.draft = Some(draft);
        })?;
        let _ = app.emit("items-meta", [changed]);
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn rename_item(app: AppHandle, id: String, title: String) -> Res<()> {
    in_order(move || {
        let changed = update_meta(&app, &id, |m| m.title = title)?;
        let _ = app.emit("items-meta", [changed]);
        Ok(())
    })
    .await
}

/// One call and one update for the UI for a whole selection.
#[tauri::command]
pub async fn set_favorite(app: AppHandle, ids: Vec<String>, favorite: bool) -> Res<()> {
    in_order(move || {
        let mut changed = Vec::new();
        let result = ids.iter().try_for_each(|id| {
            changed.push(update_meta(&app, id, |m| m.favorite = favorite)?);
            Ok(())
        });
        let _ = app.emit("items-meta", &changed);
        result
    })
    .await
}

/// The recycle bin goes through the system's shell and can take a moment per file.
#[tauri::command]
pub async fn delete_items(app: AppHandle, ids: Vec<String>) -> Res<Vec<PathBuf>> {
    in_order(move || {
        let state = app.state::<AppState>();
        let mut removed = Vec::new();
        let mut paths = Vec::new();
        let result = ids.into_iter().try_for_each(|id| {
            let path = state.path_of(&id)?;
            let _held = library::META_LOCK.lock();
            library::delete(&path).map_err(err)?;
            // A scan already under way may still list it; it checks this count.
            state.removals.fetch_add(1, Ordering::Relaxed);
            state.index.lock().paths.remove(&id);
            removed.push(id);
            paths.push(path);
            Ok(())
        });
        let _ = app.emit("items-removed", &removed);
        result.map(|()| paths)
    })
    .await
}

/// Undoes `delete_items`: the clips come back from the recycle bin and the library rescans.
#[tauri::command]
pub async fn restore_items(app: AppHandle, paths: Vec<PathBuf>) -> Res<usize> {
    in_order(move || {
        let _held = library::META_LOCK.lock();
        let back = library::restore(&paths).map_err(|e| format!("{e:#}"))?;
        let _ = app.emit("library-changed", ());
        Ok(back)
    })
    .await
}

/// Only files in the clips folder leave Replix.
fn shareable(state: &AppState, path: &str) -> Res<PathBuf> {
    let root = state.settings.lock().clips_dir.clone();
    let path = PathBuf::from(path);
    let inside = std::fs::canonicalize(&path).ok().zip(std::fs::canonicalize(&root).ok()).is_some_and(|(p, r)| p.starts_with(r));
    if inside {
        Ok(path)
    } else {
        Err(tr("Only clips from your Replix folder can be shared."))
    }
}

#[tauri::command]
pub async fn copy_items(app: AppHandle, ids: Vec<String>) -> Res<()> {
    blocking(move || {
        let state = app.state::<AppState>();
        let paths = ids.iter().map(|id| state.path_of(id)).collect::<Res<Vec<_>>>()?;
        share::copy_files(&paths).map_err(err)
    })
    .await
}

#[tauri::command]
pub async fn copy_path(app: AppHandle, path: String) -> Res<()> {
    blocking(move || {
        let path = shareable(&app.state::<AppState>(), &path)?;
        share::copy_files(&[path]).map_err(err)
    })
    .await
}

#[tauri::command]
pub async fn share_link(app: AppHandle, path: String) -> Res<String> {
    let path = shareable(&app.state::<AppState>(), &path)?;
    tauri::async_runtime::spawn_blocking(move || {
        let url = share::upload(&path)?;
        share::copy_text(&url)?;
        Ok::<_, anyhow::Error>(url)
    })
    .await
    .map_err(err)?
    .map_err(|e| format!("{e:#}"))
}

#[tauri::command]
pub fn reveal_item(state: State<AppState>, id: String) -> Res<()> {
    let path = state.path_of(&id)?;
    tauri_plugin_opener::reveal_item_in_dir(path).map_err(err)
}

#[tauri::command]
pub fn reveal_path(path: String) -> Res<()> {
    tauri_plugin_opener::reveal_item_in_dir(path).map_err(err)
}

#[tauri::command]
pub fn open_clips_folder(state: State<AppState>) -> Res<()> {
    let dir = state.settings.lock().clips_dir.clone();
    let _ = std::fs::create_dir_all(&dir);
    tauri_plugin_opener::open_path(&dir, None::<&str>).map_err(err)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageInfo {
    clips: u64,
    exports: u64,
    /// Free space on the drive with the clips folder.
    free: u64,
}

#[tauri::command]
pub async fn storage_info(app: AppHandle) -> StorageInfo {
    let dir = app.state::<AppState>().settings.lock().clips_dir.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let (clips, exports) = library::usage(&dir);
        StorageInfo { clips, exports, free: platform::disk_free(&dir) }
    })
    .await
    .unwrap_or(StorageInfo { clips: 0, exports: 0, free: 0 })
}

/// The clip's markers as the editor has them now.
#[tauri::command]
pub async fn set_markers(app: AppHandle, id: String, markers: Vec<f64>) -> Res<()> {
    in_order(move || {
        let changed = update_meta(&app, &id, |m| m.markers = library::tidy_markers(markers, m.duration))?;
        let _ = app.emit("items-meta", [changed]);
        Ok(())
    })
    .await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Imported {
    count: usize,
    /// Files left out, by name, with the reason.
    skipped: Vec<(String, String)>,
}

/// Copies videos and pictures into the library; a folder brings the ones directly in it.
/// The copies can take a while, so this runs beside the library's own changes.
#[tauri::command]
pub async fn import_files(app: AppHandle, paths: Vec<PathBuf>) -> Res<Imported> {
    blocking(move || {
        let root = app.state::<AppState>().settings.lock().clips_dir.clone();
        let folder = tr("Imported");
        let mut files = Vec::new();
        for path in paths {
            if path.is_dir() {
                let mut inside: Vec<PathBuf> = std::fs::read_dir(&path)
                    .map(|d| d.flatten().map(|e| e.path()).filter(|p| p.is_file() && media_ext(p)).collect())
                    .unwrap_or_default();
                inside.sort();
                files.extend(inside);
            } else {
                files.push(path);
            }
        }
        let mut result = Imported { count: 0, skipped: Vec::new() };
        for file in files {
            let name = file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            match library::import(&root, &file, &folder) {
                Ok(_) => result.count += 1,
                Err(e) => {
                    log::warn!("import of {} skipped: {e:#}", file.display());
                    result.skipped.push((name, e.to_string()));
                }
            }
        }
        if result.count > 0 {
            let _ = app.emit("library-changed", ());
        }
        Ok(result)
    })
    .await
}

fn media_ext(path: &std::path::Path) -> bool {
    let ext = path.extension().and_then(|e| e.to_str()).map(str::to_lowercase).unwrap_or_default();
    library::VIDEO.contains(&ext.as_str()) || library::IMAGE.contains(&ext.as_str())
}
