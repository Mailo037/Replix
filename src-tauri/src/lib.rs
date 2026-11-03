mod audio;
mod editor;
mod encoder;
mod ffmpeg;
mod library;
mod pipe;
mod recorder;
mod settings;
mod share;
mod win;

use std::path::PathBuf;
use std::str::FromStr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use parking_lot::Mutex;
use serde::Serialize;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, State, WebviewUrl, WebviewWindowBuilder, WindowEvent};
use tauri_plugin_autostart::ManagerExt as _;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

use library::{Draft, Index, Item};
use recorder::{Command, Recorder, Status};
use settings::Settings;

pub struct AppState {
    settings: Mutex<Settings>,
    settings_path: PathBuf,
    cache: PathBuf,
    recorder: Recorder,
    index: Mutex<Index>,
    thumbs_pending: Mutex<std::collections::HashSet<String>>,
    /// Hotkeys another program already owns.
    hotkey_conflicts: Mutex<Vec<String>>,
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

static TOAST_GENERATION: AtomicU64 = AtomicU64::new(0);

/// A small note in the corner of the screen that does not take focus from the game.
pub fn toast(app: &AppHandle, title: &str, detail: &str) {
    let show = app.state::<AppState>().settings.lock().show_toast;
    let _ = app.emit("notice", Toast { title: title.into(), detail: detail.into() });
    if !show {
        return;
    }
    let Some(window) = app.get_webview_window("toast") else { return };
    let _ = window.emit("toast", Toast { title: title.into(), detail: detail.into() });
    if let Some((x, y, w, _)) = win::foreground_work_area() {
        let size = window.outer_size().map(|s| s.width as i32).unwrap_or(340);
        let _ = window.set_position(PhysicalPosition::new(x + w - size - 24, y + 24));
    }
    if let Ok(hwnd) = window.hwnd() {
        win::show_passive(hwnd.0 as u64);
    }
    let generation = TOAST_GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(2800));
        if TOAST_GENERATION.load(Ordering::SeqCst) == generation {
            let _ = window.hide();
        }
    });
}

#[tauri::command]
fn get_settings(state: State<AppState>) -> Settings {
    state.settings.lock().clone()
}

#[tauri::command]
fn set_settings(app: AppHandle, state: State<AppState>, settings: Settings) -> Res<()> {
    let previous = state.settings.lock().clone();
    settings.save(&state.settings_path).map_err(err)?;
    *state.settings.lock() = settings.clone();
    audio::set_mic_gain(settings.mic_volume);
    audio::set_mic_denoise(settings.mic_noise_suppression);
    if previous.hotkeys != settings.hotkeys {
        register_hotkeys(&app, &settings);
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

#[tauri::command]
fn list_apps() -> Vec<AppWindow> {
    let mut seen = std::collections::HashSet::new();
    win::list_windows()
        .into_iter()
        .filter(|w| seen.insert(w.exe.to_lowercase()))
        .map(|w| AppWindow { name: win::app_name(&w.exe_path), exe: w.exe, title: w.title })
        .collect()
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
        let items = library::scan(&root, &state.thumbs_dir());
        {
            let mut index = state.index.lock();
            index.paths = items.iter().map(|i| (i.id.clone(), PathBuf::from(&i.path))).collect();
        }
        queue_thumbs(&app, &items);
        items
    })
    .await
    .map_err(err)
}

/// Makes missing thumbnails one after another and tells the UI about each.
fn queue_thumbs(app: &AppHandle, items: &[Item]) {
    let state = app.state::<AppState>();
    let dir = state.thumbs_dir();
    let mut pending = state.thumbs_pending.lock();
    let todo: Vec<(String, PathBuf, library::Kind, f64)> = items
        .iter()
        .filter(|i| i.thumb.is_none() && pending.insert(i.id.clone()))
        .map(|i| (i.id.clone(), PathBuf::from(&i.path), i.meta.kind, i.meta.duration))
        .collect();
    if todo.is_empty() {
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        for (id, path, kind, duration) in todo {
            let out = dir.join(format!("{id}.jpg"));
            if library::make_thumb(&path, kind, duration, &out).is_ok() {
                let _ = app.emit("thumb", (id.clone(), out.to_string_lossy().into_owned()));
            }
            app.state::<AppState>().thumbs_pending.lock().remove(&id);
        }
    });
}

fn update_meta(state: &AppState, id: &str, change: impl FnOnce(&mut library::Meta)) -> Res<()> {
    let path = state.path_of(id)?;
    let mut meta = library::read_meta(&path).unwrap_or_default();
    change(&mut meta);
    library::write_meta(&path, &meta);
    Ok(())
}

#[tauri::command]
async fn open_editor(app: AppHandle, id: String) -> Res<editor::EditorData> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let path = state.path_of(&id)?;
        let mut meta = library::read_meta(&path).unwrap_or_default();
        if !meta.seen {
            meta.seen = true;
            library::write_meta(&path, &meta);
        }
        editor::prepare(&id, &path, &meta, &state.cache, strip_ready(&app, &id)).map_err(err)
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

/// Prepares a clip's preview material ahead of time, e.g. while the pointer rests on its tile.
#[tauri::command]
async fn warm_editor(app: AppHandle, id: String) -> Res<()> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let path = state.path_of(&id)?;
        let meta = library::read_meta(&path).unwrap_or_default();
        editor::prepare(&id, &path, &meta, &state.cache, strip_ready(&app, &id)).map(|_| ()).map_err(err)
    })
    .await
    .map_err(err)?
}

#[tauri::command]
fn save_draft(app: AppHandle, state: State<AppState>, id: String, title: String, draft: Draft) -> Res<()> {
    update_meta(&state, &id, |m| {
        m.title = title;
        m.draft = Some(draft);
    })?;
    let _ = app.emit("library-changed", ());
    Ok(())
}

#[tauri::command]
fn rename_item(app: AppHandle, state: State<AppState>, id: String, title: String) -> Res<()> {
    update_meta(&state, &id, |m| m.title = title)?;
    let _ = app.emit("library-changed", ());
    Ok(())
}

#[tauri::command]
fn set_favorite(app: AppHandle, state: State<AppState>, id: String, favorite: bool) -> Res<()> {
    update_meta(&state, &id, |m| m.favorite = favorite)?;
    let _ = app.emit("library-changed", ());
    Ok(())
}

#[tauri::command]
fn delete_items(app: AppHandle, state: State<AppState>, ids: Vec<String>) -> Res<()> {
    for id in ids {
        let path = state.path_of(&id)?;
        library::delete(&path).map_err(err)?;
    }
    let _ = app.emit("library-changed", ());
    Ok(())
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
fn copy_items(state: State<AppState>, ids: Vec<String>) -> Res<()> {
    let paths = ids.iter().map(|id| state.path_of(id)).collect::<Res<Vec<_>>>()?;
    share::copy_files(&paths).map_err(err)
}

#[tauri::command]
fn copy_path(state: State<AppState>, path: String) -> Res<()> {
    let path = shareable(&state, &path)?;
    share::copy_files(&[path]).map_err(err)
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
async fn export_clip(app: AppHandle, id: String, title: String, request: editor::ExportRequest) -> Res<String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let source = state.path_of(&id)?;
        let root = state.settings.lock().clips_dir.clone();
        let encoder = state.recorder.encoders.lock().first().copied().unwrap_or_else(encoder::software);
        let out = library::export_path(&root, &title, request.preset.extension());
        let emitter = app.clone();
        let result = editor::export(&source, &request, encoder, &out, |p| {
            let _ = emitter.emit("export-progress", p);
        });
        // A hardware encoder can refuse odd sizes or formats; software always works.
        let result = match result {
            Err(e) if encoder.codec != "libx264" && request.preset != editor::Preset::Gif => {
                log::warn!("export with {} failed, using software: {e}", encoder.id);
                editor::export(&source, &request, encoder::software(), &out, |p| {
                    let _ = emitter.emit("export-progress", p);
                })
            }
            other => other,
        };
        result.map_err(err)?;
        Ok(out.to_string_lossy().into_owned())
    })
    .await
    .map_err(err)?
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
        library::write_meta(
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
    tauri::async_runtime::spawn_blocking(audio::input_devices).await.unwrap_or_default()
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

fn show_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
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
                "quit" => app.exit(0),
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
                }
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main" {
                    let to_tray = window.app_handle().state::<AppState>().settings.lock().close_to_tray;
                    if to_tray {
                        api.prevent_close();
                        let _ = window.hide();
                    }
                }
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
