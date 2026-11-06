//! Replix's global hotkeys.

use std::str::FromStr;

use tauri::{AppHandle, Manager, State};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut};

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
}

/// While a new hotkey is being picked, Replix's own are let go: pressing the current one
/// should show up in the picker, not save a clip.
#[tauri::command]
pub fn pause_hotkeys(app: AppHandle, paused: bool) {
    if paused {
        let _ = app.global_shortcut().unregister_all();
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
    if is(&keys.clip) {
        state.recorder.send(Command::SaveClip);
    } else if is(&keys.screenshot) {
        state.recorder.send(Command::Screenshot);
    } else if is(&keys.session) {
        state.recorder.send(Command::ToggleSession);
    } else if is(&keys.marker) {
        state.recorder.send(Command::Marker);
    }
}
