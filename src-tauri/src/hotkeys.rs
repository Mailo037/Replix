//! Replix's global hotkeys.

use std::str::FromStr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Manager, State};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut};

use crate::i18n::tr;
use crate::platform::{self, DesktopHotkey};
use crate::recorder::Command;
use crate::settings::Settings;
use crate::AppState;

pub(crate) fn register_hotkeys(app: &AppHandle, settings: &Settings) {
    let shortcuts = app.global_shortcut();
    let _ = shortcuts.unregister_all();
    let mut conflicts = Vec::new();
    for key in settings.hotkeys.all() {
        if key.trim().is_empty() {
            continue;
        }
        if let Err(e) = shortcuts.register(key.as_str()) {
            log::warn!("hotkey {key} could not be registered: {e}");
            conflicts.push(key.clone());
        }
    }
    *app.state::<AppState>().hotkey_conflicts.lock() = conflicts;

    // Where the system hands hotkeys out itself (Wayland), it is asked for them too.
    let h = &settings.hotkeys;
    let wanted: Vec<DesktopHotkey> = [
        ("clip", tr("Save clip"), &h.clip),
        ("screenshot", tr("Screenshot"), &h.screenshot),
        ("session", tr("Start or stop session"), &h.session),
        ("marker", tr("Add marker"), &h.marker),
    ]
    .into_iter()
    .filter(|(_, _, keys)| !keys.trim().is_empty())
    .map(|(id, description, keys)| DesktopHotkey { id: id.into(), description, keys: keys.clone() })
    .collect();
    let app = app.clone();
    platform::desktop_hotkeys(wanted, Arc::new(move |id: &str| {
        if let Some(command) = command_for(id) {
            act(&app, id, command);
        }
    }));
}

/// The recorder's command for a hotkey's name.
fn command_for(id: &str) -> Option<Command> {
    match id {
        "clip" => Some(Command::SaveClip),
        "screenshot" => Some(Command::Screenshot),
        "session" => Some(Command::ToggleSession),
        "marker" => Some(Command::Marker),
        _ => None,
    }
}

/// A press heard twice at once (the key grab and the desktop's portal) counts once.
const TWICE: Duration = Duration::from_millis(400);

/// Whether a press of `id` at `now` is new, given the last one heard.
fn fresh(last: &mut Option<(String, Instant)>, id: &str, now: Instant) -> bool {
    let twice = last.as_ref().is_some_and(|(was, at)| was == id && now.duration_since(*at) < TWICE);
    *last = Some((id.to_string(), now));
    !twice
}

fn act(app: &AppHandle, id: &str, command: Command) {
    static LAST: parking_lot::Mutex<Option<(String, Instant)>> = parking_lot::const_mutex(None);
    if fresh(&mut LAST.lock(), id, Instant::now()) {
        app.state::<AppState>().recorder.send(command);
    }
}

/// While a new hotkey is being picked, Replix's own are let go: pressing the current one
/// should show up in the picker, not save a clip.
#[tauri::command]
pub fn pause_hotkeys(app: AppHandle, paused: bool) {
    if paused {
        let _ = app.global_shortcut().unregister_all();
        platform::desktop_hotkeys(Vec::new(), Arc::new(|_: &str| {}));
    } else {
        let settings = app.state::<AppState>().settings.lock().clone();
        register_hotkeys(&app, &settings);
    }
}

/// Whether the system would give Replix this key combination: "free", "taken" by another
/// program, or "invalid". Tried by registering it for a moment.
#[tauri::command]
pub fn check_hotkey(app: AppHandle, keys: String) -> &'static str {
    let Ok(shortcut) = keys.parse::<Shortcut>() else { return "invalid" };
    let shortcuts = app.global_shortcut();
    if shortcuts.is_registered(shortcut) {
        return "free";
    }
    match shortcuts.register(shortcut) {
        Ok(()) => {
            let _ = shortcuts.unregister(shortcut);
            "free"
        }
        Err(_) => "taken",
    }
}

#[tauri::command]
pub fn hotkey_conflicts(state: State<AppState>) -> Vec<String> {
    state.hotkey_conflicts.lock().clone()
}

pub(crate) fn on_hotkey(app: &AppHandle, shortcut: &Shortcut) {
    let state = app.state::<AppState>();
    let keys = state.settings.lock().hotkeys.clone();
    let is = |text: &str| Shortcut::from_str(text).map(|s| s.id() == shortcut.id()).unwrap_or(false);
    let id = if is(&keys.clip) {
        "clip"
    } else if is(&keys.screenshot) {
        "screenshot"
    } else if is(&keys.session) {
        "session"
    } else if is(&keys.marker) {
        "marker"
    } else {
        return;
    };
    if let Some(command) = command_for(id) {
        act(app, id, command);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_press_heard_twice_counts_once() {
        let start = Instant::now();
        let mut last = None;
        assert!(fresh(&mut last, "clip", start));
        assert!(!fresh(&mut last, "clip", start + Duration::from_millis(50)), "the same press from the portal");
        assert!(fresh(&mut last, "marker", start + Duration::from_millis(60)), "another key");
        assert!(fresh(&mut last, "marker", start + Duration::from_secs(1)), "pressed again later");
    }
}
