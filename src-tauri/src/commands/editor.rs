//! The editor: preview material, drafts, exports and stills.

use std::sync::atomic::Ordering;

use parking_lot::Mutex;
use tauri::{AppHandle, Emitter, Manager};

use crate::{app_focused, audio, blocking, editor, encoder, err, ffmpeg, library, media, montage, page_visible, AppState, Res};

#[tauri::command]
pub async fn open_editor(app: AppHandle, id: String, round: u64) -> Res<editor::EditorData> {
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
        // The page shows how far it is; a clip prepared before opens before anything is said.
        let report = |step: editor::Preparing| {
            let _ = app.emit("editor-progress", (&id, step));
        };
        editor::prepare(&id, &path, &meta, &state.cache, round.max(1), strip_ready(&app, &id), &report).map_err(err)
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
pub async fn waveform(app: AppHandle, id: String, track: usize, duration: f64, round: u64) -> Res<Vec<f32>> {
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
pub fn first_editor_round() -> u64 {
    media::new_page()
}

/// Ends an editor round: its media work, and that of earlier rounds, stops. The editor
/// closed or the window was hidden.
#[tauri::command]
pub fn end_editor_round(round: u64) {
    media::end_round(round);
}

/// Prepares a clip's preview material ahead of time, e.g. while the pointer rests on its tile.
#[tauri::command]
/// Says whether the clip was prepared; skipped or stopped, it may be tried again later.
pub async fn warm_editor(app: AppHandle, id: String) -> Res<bool> {
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
        match editor::prepare(&id, &path, &meta, &state.cache, media::AHEAD, strip_ready(&app, &id), &|_| {}) {
            Ok(_) => Ok(true),
            Err(e) if media::is_stopped(&e) => Ok(false),
            Err(e) => Err(err(e)),
        }
    })
    .await
    .map_err(err)?
}

/// One export (or montage) at a time, from choosing its name to the software retry: a
/// second one waits instead of doubling the load beside a game.
static ONE_AT_A_TIME: Mutex<()> = parking_lot::const_mutex(());

/// Waits for the export's turn. Cancelling one that still waits ends the wait too.
fn take_turn(cancelled: &dyn Fn() -> bool) -> Res<parking_lot::MutexGuard<'static, ()>> {
    loop {
        if cancelled() {
            return Err(editor::CANCELLED.into());
        }
        if let Some(turn) = ONE_AT_A_TIME.try_lock_for(std::time::Duration::from_millis(200)) {
            return Ok(turn);
        }
    }
}

/// Forgets a cancel on every way out; one that comes later finds nothing to stop and is
/// dropped after a while.
struct Forget(u64);

impl Drop for Forget {
    fn drop(&mut self) {
        CANCELLED_EXPORTS.lock().forget(self.0);
    }
}

#[tauri::command]
pub async fn export_clip(app: AppHandle, id: String, title: String, request: editor::ExportRequest, ticket: u64) -> Res<String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let source = state.path_of(&id)?;
        let root = state.settings.lock().clips_dir.clone();
        // Exports are H.264 even where recordings are HEVC: they have to play everywhere.
        let encoder = encoder::for_exports(&state.recorder.encoders.lock());
        let cancelled = || CANCELLED_EXPORTS.lock().has(ticket);
        let _forget = Forget(ticket);
        let _turn = take_turn(&cancelled)?;
        let out = library::export_path(&root, &title, request.preset.extension());
        let emitter = app.clone();
        // Tagged with the export it belongs to: an editor whose export still waits for its
        // turn must not show the progress of the one running.
        let names: Vec<String> = library::read_meta(&source).map(|m| m.tracks.into_iter().map(|t| t.name).collect()).unwrap_or_default();
        let result = editor::export(&source, &request, &names, encoder, &out, |p| {
            let _ = emitter.emit("export-progress", (ticket, p));
        }, &cancelled);
        // A hardware encoder can refuse odd sizes or formats; software always works.
        let result = match result {
            Err(e) if !cancelled() && encoder.codec != "libx264" && request.preset != editor::Preset::Gif => {
                log::warn!("export with {} failed, using software: {e}", encoder.id);
                editor::export(&source, &request, &names, encoder::software(), &out, |p| {
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

/// Joins clips, oldest first, into a new clip in the library; says its id. Progress and
/// cancelling go by `ticket`, as for an export.
#[tauri::command]
pub async fn combine_clips(app: AppHandle, ids: Vec<String>, title: String, ticket: u64) -> Res<String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let (root, defaults) = {
            let s = state.settings.lock();
            (s.clips_dir.clone(), s.track_defaults.clone())
        };
        let mut parts = Vec::new();
        for id in &ids {
            let source = state.path_of(id)?;
            let meta = library::meta_or_describe(&source, &root);
            if meta.kind != library::Kind::Screenshot && meta.duration > 0.0 {
                parts.push(montage::Part { source, meta });
            }
        }
        parts.sort_by_key(|p| p.meta.created);
        let encoder = encoder::for_exports(&state.recorder.encoders.lock());
        let cancelled = || CANCELLED_EXPORTS.lock().has(ticket);
        let _forget = Forget(ticket);
        let _turn = take_turn(&cancelled)?;

        // Filed under the game when every clip is from the same one.
        let game = match parts.first().map(|p| p.meta.game.clone()) {
            Some(g) if !g.is_empty() && parts.iter().all(|p| p.meta.game == g) => g,
            _ => "Montage".to_string(),
        };
        let out = library::new_path(&root, &game, "mp4", Some("Montage"));
        let work = state.cache.join("montage").join(ticket.to_string());
        let emitter = app.clone();
        let progress = |p: f64| {
            let _ = emitter.emit("export-progress", (ticket, p));
        };
        let mut result = montage::combine(&parts, &defaults, encoder, &work, &out, progress, &cancelled);
        // A hardware encoder can refuse odd sizes or formats; software always works.
        if result.is_err() && !cancelled() && encoder.codec != "libx264" {
            log::warn!("montage with {} failed, using software: {:#}", encoder.id, result.as_ref().err().unwrap());
            result = montage::combine(&parts, &defaults, encoder::software(), &work, &out, progress, &cancelled);
        }
        let _ = std::fs::remove_dir_all(&work);
        let (length, markers) = result.map_err(|e| {
            let _ = std::fs::remove_file(&out);
            err(e)
        })?;
        let info = ffmpeg::probe(&out).unwrap_or_default();
        let meta = library::Meta {
            kind: library::Kind::Clip,
            title: if title.trim().is_empty() { crate::i18n::tr("Montage") } else { title.trim().to_string() },
            game,
            created: audio::now_ms(),
            duration: if info.duration > 0.0 { info.duration } else { length },
            width: info.width,
            height: info.height,
            tracks: vec![library::TrackMeta { name: crate::i18n::tr("Mix"), kind: audio::TrackKind::Game, exe: None }],
            markers: library::tidy_markers(markers, length),
            ..Default::default()
        };
        library::write_meta(&out, &meta).map_err(err)?;
        let _ = app.emit("library-changed", ());
        Ok(library::id_for(&out))
    })
    .await
    .map_err(err)?
}

/// One audio track on its own, as an M4A in the exports folder, between `start` and `end`.
#[tauri::command]
pub async fn extract_track(app: AppHandle, id: String, track: usize, title: String, start: f64, end: f64) -> Res<String> {
    blocking(move || {
        let state = app.state::<AppState>();
        let source = state.path_of(&id)?;
        let root = state.settings.lock().clips_dir.clone();
        let out = library::export_path(&root, &title, "m4a");
        let args: Vec<String> = vec![
            "-y".into(),
            "-ss".into(),
            format!("{:.3}", start.max(0.0)),
            "-t".into(),
            format!("{:.3}", (end - start).max(0.1)),
            "-i".into(),
            ffmpeg::path_arg(&source),
            "-map".into(),
            format!("0:a:{track}"),
            "-vn".into(),
            "-c:a".into(),
            "aac".into(),
            "-b:a".into(),
            "192k".into(),
            ffmpeg::path_arg(&out),
        ];
        if let Err(e) = ffmpeg::run(&args) {
            let _ = std::fs::remove_file(&out);
            return Err(err(e));
        }
        Ok(out.to_string_lossy().into_owned())
    })
    .await
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
pub fn cancel_export(ticket: u64) {
    CANCELLED_EXPORTS.lock().cancel(ticket, std::time::Instant::now());
}

#[tauri::command]
pub async fn save_frame(app: AppHandle, id: String, time: f64) -> Res<String> {
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
}
