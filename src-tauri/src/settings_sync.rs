//! Settings from the page to the file, the hotkeys and the recorder, one change at a time.

use parking_lot::Mutex;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_autostart::ManagerExt as _;

use crate::recorder::Command;
use crate::settings::Settings;
use crate::{audio, blocking, err, i18n, AppState, Res};

/// The settings in effect and the revision they belong to, for a page that just loaded.
/// Changes already handed over are applied first, so a page reloaded during a save starts
/// from that save, not from the settings before it. Off the main thread: a hotkey change
/// waits for it.
#[tauri::command]
pub async fn get_settings(app: AppHandle) -> Res<(Settings, u64)> {
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
pub async fn set_settings(app: AppHandle, settings: Settings, revision: u64) -> Res<u64> {
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
pub(crate) fn apply_waiting_settings(app: &AppHandle, mine: u64) -> Res<u64> {
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
            crate::hotkeys::register_hotkeys(&handle, &next);
            let _ = done.send(());
        })
        .map_err(err)?;
        let _ = wait.recv();
    }
    if previous.launch_at_startup != settings.launch_at_startup {
        let launcher = app.autolaunch();
        let _ = if settings.launch_at_startup { launcher.enable() } else { launcher.disable() };
    }
    if previous.language != settings.language && i18n::set(&settings.language) {
        crate::shell::refresh_tray(app);
        let _ = app.emit("language", i18n::current());
    }
    if previous.clips_dir != settings.clips_dir {
        let _ = std::fs::create_dir_all(&settings.clips_dir);
        let _ = app.asset_protocol_scope().allow_directory(&settings.clips_dir, true);
        let _ = app.emit("library-changed", ());
    }
    state.recorder.send(Command::Apply(Box::new(settings)));
    Ok(())
}

/// Ends Replix once settings changed a moment ago are applied: the page has handed them
/// over, but they may still wait for their turn.
pub(crate) fn quit_after_settings(app: &AppHandle) {
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

#[cfg(test)]
mod tests {
    use super::*;

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
