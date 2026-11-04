mod audio;
mod editor;
mod encoder;
mod ffmpeg;
mod library;
mod media;
mod pipe;
mod recorder;
mod settings;
mod share;
mod win;

use std::path::PathBuf;
use std::str::FromStr;
use std::sync::atomic::Ordering;

use parking_lot::Mutex;
use serde::Serialize;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, State, WebviewUrl, WebviewWindowBuilder, WindowEvent};
use tauri_plugin_autostart::ManagerExt as _;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

use library::{Draft, Index, Item};
use recorder::{Command, Recorder, Status};
use settings::{Settings, ToastPosition};

pub struct AppState {
    settings: Mutex<Settings>,
    settings_path: PathBuf,
    cache: PathBuf,
    recorder: Recorder,
    index: Mutex<Index>,
    thumbs_pending: Mutex<std::collections::HashSet<String>>,
    /// Hotkeys another program already owns.
    hotkey_conflicts: Mutex<Vec<String>>,
    /// Counts items sent to the recycle bin, so a scan can tell it raced one.
    removals: std::sync::atomic::AtomicU64,
    /// Stops the running microphone test.
    mic_test: Mutex<Option<std::sync::Arc<std::sync::atomic::AtomicBool>>>,
}

type Res<T> = Result<T, String>;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

impl AppState {
    fn path_of(&self, id: &str) -> Res<PathBuf> {
        self.index.lock().paths.get(id).cloned().ok_or_else(|| "That clip no longer exists.".to_string())
    }

    fn thumbs_dir(&self) -> PathBuf {
        self.cache.join("thumbs")
    }
}

#[derive(Clone, Serialize)]
struct Toast {
    title: String,
    detail: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ToastShown {
    title: String,
    detail: String,
    position: ToastPosition,
    accent: String,
}

/// A small note in the corner of the screen that does not take focus from the game.
/// The toast window hides itself once its timer runs out.
pub fn toast(app: &AppHandle, title: &str, detail: &str) {
    let _ = app.emit("notice", Toast { title: title.into(), detail: detail.into() });
    corner_toast(app, title, detail);
}

fn corner_toast(app: &AppHandle, title: &str, detail: &str) {
    let state = app.state::<AppState>();
    let (show, position, accent) = {
        let s = state.settings.lock();
        (s.show_toast, s.toast_position, s.accent.clone())
    };
    if !show {
        return;
    }
    let Some(window) = app.get_webview_window("toast") else { return };
    let _ = window.emit("toast", ToastShown { title: title.into(), detail: detail.into(), position, accent });
    if let Some((x, y, w, h)) = win::foreground_work_area() {
        let size = window.outer_size().map(|s| (s.width as i32, s.height as i32)).unwrap_or((340, 76));
        let left = match position {
            ToastPosition::TopLeft | ToastPosition::BottomLeft => x + 24,
            ToastPosition::TopRight | ToastPosition::BottomRight => x + w - size.0 - 24,
        };
        let top = match position {
            ToastPosition::TopLeft | ToastPosition::TopRight => y + 24,
            ToastPosition::BottomLeft | ToastPosition::BottomRight => y + h - size.1 - 24,
        };
        let _ = window.set_position(PhysicalPosition::new(left, top));
    }
    if let Ok(hwnd) = window.hwnd() {
        win::show_passive(hwnd.0 as u64);
    }
}

/// The settings in effect and the revision they belong to, for a page that just loaded.
/// Changes already handed over are applied first, so a page reloaded during a save starts
/// from that save, not from the settings before it. Off the main thread: a hotkey change
/// waits for it.
#[tauri::command]
async fn get_settings(app: AppHandle) -> Res<(Settings, u64)> {
    blocking(move || {
        let newest = SETTINGS_WAITING.lock().revision;
        if let Err(e) = apply_waiting_settings(&app, newest) {
            log::warn!("settings waiting for a reloaded page not saved: {e}");
        }
        // Held while the settings are read, so no change lands between the two.
        let applied = SETTINGS_APPLIED.lock();
        let settings = app.state::<AppState>().settings.lock().clone();
        Ok((settings, applied.0.max(newest)))
    })
    .await
}

/// Writes a file and may touch the registry; hotkeys are handed to the main thread by the plugin.
/// `revision` is the page's number for this change, rising with every change and across
/// reloads; the answer is the revision now in effect, this one or a later one.
#[tauri::command]
async fn set_settings(app: AppHandle, settings: Settings, revision: u64) -> Res<u64> {
    SETTINGS_WAITING.lock().offer(revision, settings);
    blocking(move || apply_waiting_settings(&app, revision)).await
}

/// The newest value offered and not taken yet. One offered after a newer one, as two
/// requests can overtake each other on the way, is dropped: the newer one stands.
struct Newest<T> {
    revision: u64,
    value: Option<T>,
}

impl<T> Newest<T> {
    const fn new() -> Self {
        Self { revision: 0, value: None }
    }

    fn offer(&mut self, revision: u64, value: T) {
        if revision > self.revision {
            self.revision = revision;
            self.value = Some(value);
        }
    }

    fn take(&mut self) -> Option<(u64, T)> {
        self.value.take().map(|v| (self.revision, v))
    }
}

/// The page sends every change at once; while one is applied only the newest waits behind
/// it, so a slider's steps cost a few writes rather than one each, and nothing is left behind
/// in the page when it reloads or Replix quits.
static SETTINGS_WAITING: Mutex<Newest<Settings>> = parking_lot::const_mutex(Newest::new());
/// One change at a time, all of it: the file, the copy in memory, the hotkeys and the
/// recorder then always agree. Holds the revision applied last, and whether that went well.
/// Lock order: this one, then `SETTINGS_WAITING` or the settings in memory, never the
/// other way round; and never taken on the main thread, which a hotkey change waits for.
static SETTINGS_APPLIED: Mutex<(u64, Res<()>)> = parking_lot::const_mutex((0, Ok(())));

/// Applies the newest settings waiting, unless a revision from `mine` on already went
/// through; says which revision is in effect, or why the newest one is not. Not on the
/// main thread: a hotkey change waits for it.
fn apply_waiting_settings(app: &AppHandle, mine: u64) -> Res<u64> {
    let mut applied = SETTINGS_APPLIED.lock();
    if applied.0 < mine {
        let waiting = SETTINGS_WAITING.lock().take();
        if let Some((revision, settings)) = waiting {
            *applied = (revision, apply_settings(app, settings));
        }
    }
    applied.1.clone().map(|()| applied.0)
}

fn apply_settings(app: &AppHandle, settings: Settings) -> Res<()> {
    let state = app.state::<AppState>();
    let previous = state.settings.lock().clone();
    settings.save(&state.settings_path).map_err(err)?;
    *state.settings.lock() = settings.clone();
    audio::set_mic_gain(settings.mic_volume);
    audio::set_mic_denoise(settings.mic_noise_suppression);
    if previous.hotkeys != settings.hotkeys {
        // The shortcut plugin holds its lock while it waits for the main thread, and a key
        // press handled there wants the same lock. Doing the whole update on the main thread,
        // as before, keeps that from deadlocking; this thread only waits for it.
        let (done, wait) = std::sync::mpsc::channel();
        let (handle, next) = (app.clone(), settings.clone());
        app.run_on_main_thread(move || {
            register_hotkeys(&handle, &next);
            let _ = done.send(());
        })
        .map_err(err)?;
        let _ = wait.recv();
    }
    if previous.launch_at_startup != settings.launch_at_startup {
        let launcher = app.autolaunch();
        let _ = if settings.launch_at_startup { launcher.enable() } else { launcher.disable() };
    }
    if previous.clips_dir != settings.clips_dir {
        let _ = std::fs::create_dir_all(&settings.clips_dir);
        let _ = app.asset_protocol_scope().allow_directory(&settings.clips_dir, true);
        let _ = app.emit("library-changed", ());
    }
    state.recorder.send(Command::Apply(Box::new(settings)));
    Ok(())
}

#[tauri::command]
fn get_status(state: State<AppState>) -> Status {
    state.recorder.status.lock().clone()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct EncoderOption {
    id: String,
    label: String,
}

#[tauri::command]
fn get_encoders(state: State<AppState>) -> Vec<EncoderOption> {
    state.recorder.encoders.lock().iter().map(|e| EncoderOption { id: e.id.into(), label: e.label.into() }).collect()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AppWindow {
    exe: String,
    name: String,
    title: String,
}

/// Reads every program's version resource for its name, so it stays off the main thread.
#[tauri::command]
async fn list_apps() -> Vec<AppWindow> {
    tauri::async_runtime::spawn_blocking(|| {
        let mut seen = std::collections::HashSet::new();
        win::list_windows()
            .into_iter()
            .filter(|w| seen.insert(w.exe.to_lowercase()))
            .map(|w| AppWindow { name: win::app_name(&w.exe_path), exe: w.exe, title: w.title })
            .collect()
    })
    .await
    .unwrap_or_default()
}

#[tauri::command]
fn list_monitors() -> Vec<win::MonitorInfo> {
    win::monitors()
}

#[tauri::command]
async fn list_library(app: AppHandle) -> Res<Vec<Item>> {
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
fn update_meta(app: &AppHandle, id: &str, change: impl FnOnce(&mut library::Meta)) -> Res<(String, Patch)> {
    let path = app.state::<AppState>().path_of(id)?;
    let _held = library::META_LOCK.lock();
    // The clip may have gone to the recycle bin since its path was looked up.
    if !path.exists() {
        return Err("That clip no longer exists.".into());
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

/// Runs file work off the main thread, where commands without `async` run and would hold
/// up the window.
async fn blocking<T: Send + 'static>(work: impl FnOnce() -> Res<T> + Send + 'static) -> Res<T> {
    tauri::async_runtime::spawn_blocking(work).await.map_err(err)?
}

type Job = Box<dyn FnOnce() + Send>;

/// Changes to the library (titles, drafts, favourites, the recycle bin) run one after
/// another on one thread, so a change never races a delete. The UI sends them one at a
/// time, which keeps them in the order they were made. The thread is Replix's own, so the
/// recycle bin can set up COM on it the way it needs to.
async fn in_order<T: Send + 'static>(work: impl FnOnce() -> Res<T> + Send + 'static) -> Res<T> {
    static QUEUE: std::sync::OnceLock<std::sync::mpsc::Sender<Job>> = std::sync::OnceLock::new();
    let queue = QUEUE.get_or_init(|| {
        let (tx, rx) = std::sync::mpsc::channel::<Job>();
        std::thread::Builder::new()
            .name("library".into())
            .spawn(move || rx.into_iter().for_each(|job| job()))
            .expect("thread");
        tx
    });
    let (done, mut result) = tauri::async_runtime::channel(1);
    queue
        .send(Box::new(move || {
            let _ = done.blocking_send(work());
        }))
        .map_err(err)?;
    result.recv().await.unwrap_or_else(|| Err("The task failed.".into()))
}

/// Like `blocking`, on a thread of its own: the audio device list sets up COM in a way a
/// pooled thread may already be set up differently for.
async fn own_thread<T: Send + 'static>(work: impl FnOnce() -> Res<T> + Send + 'static) -> Res<T> {
    blocking(move || std::thread::spawn(work).join().map_err(|_| "The task failed.".to_string())?).await
}

#[tauri::command]
async fn open_editor(app: AppHandle, id: String, round: u64) -> Res<editor::EditorData> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let path = state.path_of(&id)?;
        let root = state.settings.lock().clips_dir.clone();
        let mut meta = library::read_meta(&path).unwrap_or_default();
        if !meta.seen {
            let _held = library::META_LOCK.lock();
            meta = library::meta_or_describe(&path, &root);
            meta.seen = true;
            if library::write_meta(&path, &meta).is_ok() {
                let _ = app.emit("items-meta", [(&id, serde_json::json!({ "seen": true }))]);
            }
        }
        editor::prepare(&id, &path, &meta, &state.cache, round.max(1), strip_ready(&app, &id)).map_err(err)
    })
    .await
    .map_err(err)?
}

/// Tells an open editor that its filmstrip is ready.
fn strip_ready(app: &AppHandle, id: &str) -> impl FnOnce(std::path::PathBuf) + Send + 'static {
    let (app, id) = (app.clone(), id.to_string());
    move |strip| {
        let _ = app.emit("filmstrip", (id, strip.to_string_lossy().into_owned()));
    }
}

/// One track's peaks for the editor, made from the track split off by `open_editor`.
#[tauri::command]
async fn waveform(app: AppHandle, id: String, track: usize, duration: f64, round: u64) -> Res<Vec<f32>> {
    blocking(move || {
        let state = app.state::<AppState>();
        state.path_of(&id)?;
        let audio = state.cache.join("editor").join(&id).join(format!("a{track}.m4a"));
        editor::waveform(&audio, duration, round).map_err(err)
    })
    .await
}

/// Starts a page load's editor rounds: every round an earlier load of the page used ends,
/// with its work, and the page counts on from here, so it never reuses one.
#[tauri::command]
fn first_editor_round() -> u64 {
    media::new_page()
}

/// Ends an editor round: its media work, and that of earlier rounds, stops. The editor
/// closed or the window was hidden.
#[tauri::command]
fn end_editor_round(round: u64) {
    media::end_round(round);
}

/// Prepares a clip's preview material ahead of time, e.g. while the pointer rests on its tile.
#[tauri::command]
/// Says whether the clip was prepared; skipped or stopped, it may be tried again later.
async fn warm_editor(app: AppHandle, id: String) -> Res<bool> {
    tauri::async_runtime::spawn_blocking(move || {
        // Only on a hunch that the clip opens next; never while a game is likely running.
        if !page_visible() || !app_focused() {
            return Ok(false);
        }
        // Hovering over a row of tiles asks for each; two warm-ups at a time are plenty,
        // and the rest are skipped rather than queued.
        static WARMING: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        if WARMING.fetch_add(1, Ordering::Relaxed) >= 2 {
            WARMING.fetch_sub(1, Ordering::Relaxed);
            return Ok(false);
        }
        struct Slot;
        impl Drop for Slot {
            fn drop(&mut self) {
                WARMING.fetch_sub(1, Ordering::Relaxed);
            }
        }
        let _slot = Slot;
        let state = app.state::<AppState>();
        let path = state.path_of(&id)?;
        let meta = library::read_meta(&path).unwrap_or_default();
        // The longest clip, in seconds, prepared before it is opened.
        const WARM_LONGEST: f64 = 300.0;
        // A long session's tracks and filmstrip take a while: they are made when it opens.
        if meta.duration <= 0.0 || meta.duration > WARM_LONGEST {
            return Ok(true);
        }
        match editor::prepare(&id, &path, &meta, &state.cache, media::AHEAD, strip_ready(&app, &id)) {
            Ok(_) => Ok(true),
            Err(e) if media::is_stopped(&e) => Ok(false),
            Err(e) => Err(err(e)),
        }
    })
    .await
    .map_err(err)?
}

#[tauri::command]
async fn save_draft(app: AppHandle, id: String, title: String, draft: Draft) -> Res<()> {
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
async fn rename_item(app: AppHandle, id: String, title: String) -> Res<()> {
    in_order(move || {
        let changed = update_meta(&app, &id, |m| m.title = title)?;
        let _ = app.emit("items-meta", [changed]);
        Ok(())
    })
    .await
}

/// One call and one update for the UI for a whole selection.
#[tauri::command]
async fn set_favorite(app: AppHandle, ids: Vec<String>, favorite: bool) -> Res<()> {
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

/// The recycle bin goes through the Windows shell and can take a moment per file.
#[tauri::command]
async fn delete_items(app: AppHandle, ids: Vec<String>) -> Res<()> {
    in_order(move || {
        let state = app.state::<AppState>();
        let mut removed = Vec::new();
        let result = ids.into_iter().try_for_each(|id| {
            let path = state.path_of(&id)?;
            let _held = library::META_LOCK.lock();
            library::delete(&path).map_err(err)?;
            // A scan already under way may still list it; it checks this count.
            state.removals.fetch_add(1, Ordering::Relaxed);
            state.index.lock().paths.remove(&id);
            removed.push(id);
            Ok(())
        });
        let _ = app.emit("items-removed", &removed);
        result
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
        Err("Only clips from your Replix folder can be shared.".into())
    }
}

#[tauri::command]
async fn copy_items(app: AppHandle, ids: Vec<String>) -> Res<()> {
    blocking(move || {
        let state = app.state::<AppState>();
        let paths = ids.iter().map(|id| state.path_of(id)).collect::<Res<Vec<_>>>()?;
        share::copy_files(&paths).map_err(err)
    })
    .await
}

#[tauri::command]
async fn copy_path(app: AppHandle, path: String) -> Res<()> {
    blocking(move || {
        let path = shareable(&app.state::<AppState>(), &path)?;
        share::copy_files(&[path]).map_err(err)
    })
    .await
}

#[tauri::command]
async fn share_link(app: AppHandle, path: String) -> Res<String> {
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
fn reveal_item(state: State<AppState>, id: String) -> Res<()> {
    let path = state.path_of(&id)?;
    tauri_plugin_opener::reveal_item_in_dir(path).map_err(err)
}

#[tauri::command]
fn reveal_path(path: String) -> Res<()> {
    tauri_plugin_opener::reveal_item_in_dir(path).map_err(err)
}

#[tauri::command]
fn open_clips_folder(state: State<AppState>) -> Res<()> {
    let dir = state.settings.lock().clips_dir.clone();
    let _ = std::fs::create_dir_all(&dir);
    tauri_plugin_opener::open_path(&dir, None::<&str>).map_err(err)
}

#[tauri::command]
async fn export_clip(app: AppHandle, id: String, title: String, request: editor::ExportRequest, ticket: u64) -> Res<String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let source = state.path_of(&id)?;
        let root = state.settings.lock().clips_dir.clone();
        let encoder = state.recorder.encoders.lock().first().copied().unwrap_or_else(encoder::software);
        // One export at a time, from choosing its name to the software retry: a second
        // one waits instead of doubling the load beside a game.
        static ONE_AT_A_TIME: Mutex<()> = parking_lot::const_mutex(());
        let cancelled = || CANCELLED_EXPORTS.lock().has(ticket);
        // Forgotten on every way out; a cancel that comes later finds nothing to stop and
        // is dropped after a while.
        struct Forget(u64);
        impl Drop for Forget {
            fn drop(&mut self) {
                CANCELLED_EXPORTS.lock().forget(self.0);
            }
        }
        let _forget = Forget(ticket);
        // Cancelling one that still waits for its turn ends the wait too.
        let _turn = loop {
            if cancelled() {
                return Err(editor::CANCELLED.into());
            }
            if let Some(turn) = ONE_AT_A_TIME.try_lock_for(std::time::Duration::from_millis(200)) {
                break turn;
            }
        };
        let out = library::export_path(&root, &title, request.preset.extension());
        let emitter = app.clone();
        // Tagged with the export it belongs to: an editor whose export still waits for its
        // turn must not show the progress of the one running.
        let result = editor::export(&source, &request, encoder, &out, |p| {
            let _ = emitter.emit("export-progress", (ticket, p));
        }, &cancelled);
        // A hardware encoder can refuse odd sizes or formats; software always works.
        let result = match result {
            Err(e) if !cancelled() && encoder.codec != "libx264" && request.preset != editor::Preset::Gif => {
                log::warn!("export with {} failed, using software: {e}", encoder.id);
                editor::export(&source, &request, encoder::software(), &out, |p| {
                    let _ = emitter.emit("export-progress", (ticket, p));
                }, &cancelled)
            }
            other => other,
        };
        if let Err(e) = result {
            // The name was reserved with an empty file; nothing came of it.
            let _ = std::fs::remove_file(&out);
            return Err(err(e));
        }
        Ok(out.to_string_lossy().into_owned())
    })
    .await
    .map_err(err)?
}

/// Exports asked to stop, by ticket and when. A cancel can arrive before its export has
/// started on its thread, or after it ended; both are kept a while rather than lost or
/// kept for good.
struct Cancels(std::collections::BTreeMap<u64, std::time::Instant>);

impl Cancels {
    const KEPT: std::time::Duration = std::time::Duration::from_secs(10 * 60);

    const fn new() -> Self {
        Self(std::collections::BTreeMap::new())
    }

    fn cancel(&mut self, ticket: u64, now: std::time::Instant) {
        self.0.retain(|_, at| now.duration_since(*at) < Self::KEPT);
        self.0.insert(ticket, now);
    }

    fn has(&self, ticket: u64) -> bool {
        self.0.contains_key(&ticket)
    }

    fn forget(&mut self, ticket: u64) {
        self.0.remove(&ticket);
    }
}

static CANCELLED_EXPORTS: Mutex<Cancels> = parking_lot::const_mutex(Cancels::new());

#[tauri::command]
fn report_focus(focused: bool) {
    FOCUSED.store(focused, Ordering::Relaxed);
    media::wake();
}

#[tauri::command]
fn cancel_export(ticket: u64) {
    CANCELLED_EXPORTS.lock().cancel(ticket, std::time::Instant::now());
}

#[tauri::command]
async fn save_frame(app: AppHandle, id: String, time: f64) -> Res<String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let source = state.path_of(&id)?;
        let meta = library::read_meta(&source).unwrap_or_default();
        let root = state.settings.lock().clips_dir.clone();
        let game = if meta.game.is_empty() { "Screenshots".to_string() } else { meta.game.clone() };
        let out = library::new_path(&root, &game, "png", None);
        editor::save_frame(&source, time, &out).map_err(err)?;
        let info = ffmpeg::probe(&out).unwrap_or_default();
        let _ = library::write_meta(
            &out,
            &library::Meta {
                kind: library::Kind::Screenshot,
                title: "Screenshot".into(),
                game,
                created: audio::now_ms(),
                width: info.width,
                height: info.height,
                seen: true,
                ..Default::default()
            },
        );
        let _ = app.emit("library-changed", ());
        Ok(out.to_string_lossy().into_owned())
    })
    .await
    .map_err(err)?
}

#[tauri::command]
fn save_clip_now(state: State<AppState>) {
    state.recorder.send(Command::SaveClip);
}

#[tauri::command]
fn take_screenshot(state: State<AppState>) {
    state.recorder.send(Command::Screenshot);
}

/// Shows a sample in the chosen corner, without the note inside the app.
#[tauri::command]
fn preview_toast(app: AppHandle) {
    corner_toast(&app, "Notifications show up here", "Hover to keep one on screen");
}

#[tauri::command]
fn toggle_session(state: State<AppState>) {
    state.recorder.send(Command::ToggleSession);
}

fn register_hotkeys(app: &AppHandle, settings: &Settings) {
    let shortcuts = app.global_shortcut();
    let _ = shortcuts.unregister_all();
    let mut conflicts = Vec::new();
    for key in [&settings.hotkeys.clip, &settings.hotkeys.screenshot, &settings.hotkeys.session] {
        if key.trim().is_empty() {
            continue;
        }
        if let Err(e) = shortcuts.register(key.as_str()) {
            log::warn!("hotkey {key} could not be registered: {e}");
            conflicts.push(key.clone());
        }
    }
    *app.state::<AppState>().hotkey_conflicts.lock() = conflicts;
}

#[tauri::command]
fn hotkey_conflicts(state: State<AppState>) -> Vec<String> {
    state.hotkey_conflicts.lock().clone()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StorageInfo {
    clips: u64,
    exports: u64,
    /// Free space on the drive with the clips folder.
    free: u64,
}

#[tauri::command]
async fn storage_info(app: AppHandle) -> StorageInfo {
    let dir = app.state::<AppState>().settings.lock().clips_dir.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let (clips, exports) = library::usage(&dir);
        StorageInfo { clips, exports, free: win::disk_free(&dir) }
    })
    .await
    .unwrap_or(StorageInfo { clips: 0, exports: 0, free: 0 })
}

#[tauri::command]
async fn list_mics() -> Vec<audio::InputDevice> {
    own_thread(|| Ok(audio::input_devices())).await.unwrap_or_default()
}

/// Streams the microphone's level as "mic-level" events until `stop_mic_test`.
#[tauri::command]
fn start_mic_test(app: AppHandle, state: State<AppState>, device: Option<String>) {
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    if let Some(old) = state.mic_test.lock().replace(stop.clone()) {
        old.store(true, Ordering::Relaxed);
    }
    audio::meter(device, stop, move |level| {
        let _ = app.emit("mic-level", level);
    });
}

#[tauri::command]
fn stop_mic_test(state: State<AppState>) {
    if let Some(stop) = state.mic_test.lock().take() {
        stop.store(true, Ordering::Relaxed);
    }
}

fn on_hotkey(app: &AppHandle, shortcut: &Shortcut) {
    let state = app.state::<AppState>();
    let keys = state.settings.lock().hotkeys.clone();
    let is = |text: &str| Shortcut::from_str(text).map(|s| s.id() == shortcut.id()).unwrap_or(false);
    if is(&keys.clip) {
        state.recorder.send(Command::SaveClip);
    } else if is(&keys.screenshot) {
        state.recorder.send(Command::Screenshot);
    } else if is(&keys.session) {
        state.recorder.send(Command::ToggleSession);
    }
}

/// A hidden or minimised window's page keeps rendering and running its animations unless
/// the webview itself is hidden too, which would cost the game frames while Replix sits in
/// the tray. Hidden, WebView2 stops painting and throttles the page's timers.
/// Whether the main window is on screen. Work done ahead of time stops when it is not:
/// then a game is likely running.
pub fn page_visible() -> bool {
    PAGE_VISIBLE.load(Ordering::Relaxed)
}

/// Whether Replix's window has the keyboard focus. Visible alone is not enough to do work
/// ahead of time: Replix may sit on a second monitor next to a running game. The page says
/// so (`report_focus`), so the page and Replix go by the same answer: the window's own
/// focus events and the page's can disagree, the WebView being a window of its own.
pub(crate) static FOCUSED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn app_focused() -> bool {
    FOCUSED.load(Ordering::Relaxed)
}


pub(crate) static PAGE_VISIBLE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

fn set_page_visible(window: &tauri::WebviewWindow, visible: bool) {
    // Resizing reports many sizes in a row; only a change is passed on.
    if PAGE_VISIBLE.load(Ordering::Relaxed) == visible {
        return;
    }
    let webview: &tauri::Webview = window.as_ref();
    // Kept as it was when the WebView did not follow, so the next event tries again.
    if let Err(e) = if visible { webview.show() } else { webview.hide() } {
        log::warn!("could not {} the page: {e}", if visible { "show" } else { "hide" });
        return;
    }
    PAGE_VISIBLE.store(visible, Ordering::Relaxed);
    media::wake();
}

/// Ends Replix once settings changed a moment ago are applied: the page has handed them
/// over, but they may still wait for their turn.
fn quit_after_settings(app: &AppHandle) {
    let app = app.clone();
    // Applying a hotkey change waits for the main thread, which asks for this.
    std::thread::spawn(move || {
        let newest = SETTINGS_WAITING.lock().revision;
        if let Err(e) = apply_waiting_settings(&app, newest) {
            log::warn!("settings not saved on quit: {e}");
        }
        app.exit(0);
    });
}

fn show_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        set_page_visible(&window, true);
        // Showing is only requested of the WebView, and a failure there is not reported
        // back; asking again on every restore keeps one from leaving the page hidden.
        let webview: &tauri::Webview = window.as_ref();
        let _ = webview.show();
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Open Replix", true, None::<&str>)?;
    let clip = MenuItem::with_id(app, "clip", "Save clip", true, None::<&str>)?;
    let shot = MenuItem::with_id(app, "screenshot", "Take screenshot", true, None::<&str>)?;
    let session = MenuItem::with_id(app, "session", "Start or stop session", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit Replix", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let separator2 = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&open, &separator, &clip, &shot, &session, &separator2, &quit])?;
    TrayIconBuilder::with_id("main")
        .icon(app.default_window_icon().cloned().expect("icon"))
        .tooltip("Replix")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| {
            let state = app.state::<AppState>();
            match event.id.as_ref() {
                "open" => show_main(app),
                "clip" => state.recorder.send(Command::SaveClip),
                "screenshot" => state.recorder.send(Command::Screenshot),
                "session" => state.recorder.send(Command::ToggleSession),
                "quit" => quit_after_settings(app),
                _ => {}
            }
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                show_main(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| show_main(app)))
        .plugin(tauri_plugin_log::Builder::new().level(log::LevelFilter::Info).build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_drag::init())
        .plugin(tauri_plugin_autostart::init(tauri_plugin_autostart::MacosLauncher::LaunchAgent, Some(vec!["--minimized"])))
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, shortcut, event| {
                    if event.state == ShortcutState::Pressed {
                        on_hotkey(app, shortcut);
                    }
                })
                .build(),
        )
        .setup(|app| {
            let handle = app.handle().clone();
            let config = app.path().app_config_dir()?;
            let cache = app.path().app_cache_dir()?;
            let settings_path = config.join("settings.json");
            let settings = Settings::load(&settings_path);
            let _ = std::fs::create_dir_all(&settings.clips_dir);
            let _ = std::fs::create_dir_all(&cache);
            let scope = app.asset_protocol_scope();
            let _ = scope.allow_directory(&settings.clips_dir, true);
            let _ = scope.allow_directory(&cache, true);

            let recorder = Recorder::start(handle.clone(), settings.clone());
            app.manage(AppState {
                settings: Mutex::new(settings.clone()),
                settings_path,
                cache,
                recorder,
                index: Mutex::new(Index::default()),
                thumbs_pending: Mutex::new(Default::default()),
                hotkey_conflicts: Mutex::new(Vec::new()),
                mic_test: Mutex::new(None),
                removals: Default::default(),
            });
            audio::set_mic_gain(settings.mic_volume);
            audio::set_mic_denoise(settings.mic_noise_suppression);

            register_hotkeys(&handle, &settings);
            build_tray(&handle)?;

            WebviewWindowBuilder::new(app, "toast", WebviewUrl::App("index.html#toast".into()))
                .title("Replix")
                .inner_size(340.0, 76.0)
                .decorations(false)
                .transparent(true)
                .shadow(false)
                .always_on_top(true)
                .skip_taskbar(true)
                .resizable(false)
                .focused(false)
                .visible(false)
                .build()?;

            if std::env::args().any(|a| a == "--minimized") {
                if let Some(main) = app.get_webview_window("main") {
                    let _ = main.hide();
                    set_page_visible(&main, false);
                }
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() != "main" {
                return;
            }
            let Some(main) = window.app_handle().get_webview_window("main") else { return };
            match event {
                WindowEvent::CloseRequested { api, .. } => {
                    let to_tray = window.app_handle().state::<AppState>().settings.lock().close_to_tray;
                    api.prevent_close();
                    if to_tray {
                        let _ = window.hide();
                        set_page_visible(&main, false);
                    } else {
                        quit_after_settings(window.app_handle());
                    }
                }
                // Minimising and restoring both report a new size. A hidden window can get one
                // too, so its state decides, not the size.
                WindowEvent::Resized(_) => {
                    let shown = window.is_visible().unwrap_or(true) && !window.is_minimized().unwrap_or(false);
                    set_page_visible(&main, shown);
                }
                WindowEvent::Focused(focused) => {
                    // Restored by the taskbar or Alt-Tab rather than the tray: the window's own
                    // state decides, in case a resize went unreported, and the page is asked
                    // to show again in case an earlier request did not take.
                    if *focused {
                        let shown = window.is_visible().unwrap_or(true) && !window.is_minimized().unwrap_or(false);
                        set_page_visible(&main, shown);
                        if shown {
                            let webview: &tauri::Webview = main.as_ref();
                            let _ = webview.show();
                        }
                    }
                    media::wake();
                }
                _ => {}
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_settings,
            set_settings,
            get_status,
            get_encoders,
            list_apps,
            list_monitors,
            list_library,
            open_editor,
            warm_editor,
            waveform,
            end_editor_round,
            first_editor_round,
            cancel_export,
            report_focus,
            save_draft,
            rename_item,
            set_favorite,
            delete_items,
            reveal_item,
            reveal_path,
            open_clips_folder,
            export_clip,
            save_frame,
            save_clip_now,
            take_screenshot,
            toggle_session,
            preview_toast,
            hotkey_conflicts,
            list_mics,
            storage_info,
            copy_items,
            copy_path,
            share_link,
            start_mic_test,
            stop_mic_test,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Replix");
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A cancel that arrives before its export started still stops it; old cancels of
    /// exports that never came, or had ended, are dropped.
    #[test]
    fn a_cancel_holds_whenever_it_arrives() {
        let start = std::time::Instant::now();
        let mut cancels = Cancels::new();
        cancels.cancel(1, start);
        assert!(cancels.has(1), "cancelled before it started");
        cancels.forget(1);
        assert!(!cancels.has(1));
        cancels.cancel(2, start);
        cancels.cancel(3, start + Cancels::KEPT + std::time::Duration::from_secs(1));
        assert!(!cancels.has(2) && cancels.has(3), "an old cancel is dropped");
    }

    /// Changes that overtake each other on the way: the newest stands, an older one that
    /// arrives later is dropped, and a later one after a take is kept again.
    #[test]
    fn the_newest_settings_stand() {
        let mut waiting = Newest::new();
        waiting.offer(5, "five");
        waiting.offer(3, "three");
        assert_eq!(waiting.take(), Some((5, "five")));
        assert_eq!(waiting.take(), None);
        waiting.offer(4, "four");
        assert_eq!(waiting.take(), None, "older than what was applied");
        waiting.offer(6, "six");
        waiting.offer(7, "seven");
        assert_eq!(waiting.take(), Some((7, "seven")));
    }
}
