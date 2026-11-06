//! Recording controls, devices and what Replix knows about the PC.

use std::sync::atomic::Ordering;

use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use crate::recorder::{Command, Status};
use crate::{audio, install, media, own_thread, platform, AppState, FOCUSED};

#[tauri::command]
pub fn ffmpeg_setup() -> install::Setup {
    install::state()
}

#[tauri::command]
pub fn retry_ffmpeg(app: AppHandle) {
    install::start(&app);
}

#[tauri::command]
pub fn get_status(state: State<AppState>) -> Status {
    state.recorder.status.lock().clone()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EncoderOption {
    id: String,
    label: String,
}

#[tauri::command]
pub fn get_encoders(state: State<AppState>) -> Vec<EncoderOption> {
    state.recorder.encoders.lock().iter().map(|e| EncoderOption { id: e.id.into(), label: e.label.into() }).collect()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppWindow {
    exe: String,
    /// The full path, for the program's icon.
    path: String,
    name: String,
    title: String,
}

/// Reads every program's version resource for its name, so it stays off the main thread.
#[tauri::command]
pub async fn list_apps() -> Vec<AppWindow> {
    tauri::async_runtime::spawn_blocking(|| {
        let mut seen = std::collections::HashSet::new();
        platform::list_windows()
            .into_iter()
            .filter(|w| seen.insert(w.exe.to_lowercase()))
            .map(|w| AppWindow { name: platform::app_name(&w.exe_path), exe: w.exe, path: w.exe_path, title: w.title })
            .collect()
    })
    .await
    .unwrap_or_default()
}

#[tauri::command]
pub fn list_monitors() -> Vec<platform::MonitorInfo> {
    platform::monitors()
}

#[tauri::command]
pub fn report_focus(focused: bool) {
    FOCUSED.store(focused, Ordering::Relaxed);
    media::wake();
}

#[tauri::command]
pub fn save_clip_now(state: State<AppState>) {
    state.recorder.send(Command::SaveClip);
}

#[tauri::command]
pub fn take_screenshot(state: State<AppState>) {
    state.recorder.send(Command::Screenshot);
}

/// Shows a sample in the chosen corner, without the note inside the app.
#[tauri::command]
pub fn preview_toast(app: AppHandle) {
    crate::shell::corner_toast(&app, &crate::i18n::tr("Notifications show up here"), &crate::i18n::tr("Hover to keep one on screen"));
}

#[tauri::command]
pub fn toggle_session(state: State<AppState>) {
    state.recorder.send(Command::ToggleSession);
}

#[derive(Serialize)]
pub struct AppIcon {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

/// A program's icon, for the editor's track heads.
#[tauri::command]
pub async fn app_icon(path: String) -> Option<AppIcon> {
    tauri::async_runtime::spawn_blocking(move || {
        platform::icon_rgba(&path, 48).map(|(width, height, rgba)| AppIcon { width, height, rgba })
    })
    .await
    .ok()
    .flatten()
}

#[tauri::command]
pub async fn list_mics() -> Vec<audio::InputDevice> {
    own_thread(|| Ok(audio::input_devices())).await.unwrap_or_default()
}

/// Streams the microphone's level as "mic-level" events until `stop_mic_test`.
#[tauri::command]
pub fn start_mic_test(app: AppHandle, state: State<AppState>, device: Option<String>) {
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    if let Some(old) = state.mic_test.lock().replace(stop.clone()) {
        old.store(true, Ordering::Relaxed);
    }
    audio::meter(device, stop, move |level| {
        let _ = app.emit("mic-level", level);
    });
}

#[tauri::command]
pub fn stop_mic_test(state: State<AppState>) {
    if let Some(stop) = state.mic_test.lock().take() {
        stop.store(true, Ordering::Relaxed);
    }
}

#[tauri::command]
pub fn add_marker(state: State<AppState>) {
    state.recorder.send(Command::Marker);
}

/// The language the page should use: the chosen one, or the one the system is set to.
#[tauri::command]
pub fn ui_language() -> &'static str {
    crate::i18n::current()
}
