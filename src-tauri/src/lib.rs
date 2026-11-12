mod audio;
mod commands;
mod editor;
mod encoder;
mod ffmpeg;
mod hotkeys;
mod i18n;
mod install;
mod library;
mod media;
mod pipe;
mod recorder;
mod settings;
mod settings_sync;
mod share;
mod shell;
mod sound;
mod platform;

use std::path::PathBuf;
use std::sync::atomic::Ordering;

use parking_lot::Mutex;
use tauri::{Manager, WebviewUrl, WebviewWindowBuilder, WindowEvent};
use tauri_plugin_global_shortcut::ShortcutState;

use commands::{editor as editing, library as clips, system};
use library::Index;
use recorder::Recorder;
use settings::Settings;
pub use shell::{app_focused, page_visible, toast};
#[cfg(test)]
pub(crate) use shell::PAGE_VISIBLE;
pub(crate) use shell::FOCUSED;

pub struct AppState {
    pub(crate) settings: Mutex<Settings>,
    pub(crate) settings_path: PathBuf,
    pub(crate) cache: PathBuf,
    pub(crate) recorder: Recorder,
    pub(crate) index: Mutex<Index>,
    pub(crate) thumbs_pending: Mutex<std::collections::HashSet<String>>,
    /// Hotkeys another program already owns.
    pub(crate) hotkey_conflicts: Mutex<Vec<String>>,
    /// Counts items sent to the recycle bin, so a scan can tell it raced one.
    pub(crate) removals: std::sync::atomic::AtomicU64,
    /// Stops the running microphone test.
    pub(crate) mic_test: Mutex<Option<std::sync::Arc<std::sync::atomic::AtomicBool>>>,
}

pub(crate) type Res<T> = Result<T, String>;

pub(crate) fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

impl AppState {
    pub(crate) fn path_of(&self, id: &str) -> Res<PathBuf> {
        self.index.lock().paths.get(id).cloned().ok_or_else(|| i18n::tr("That clip no longer exists."))
    }

    pub(crate) fn thumbs_dir(&self) -> PathBuf {
        self.cache.join("thumbs")
    }
}

/// Runs file work off the main thread, where commands without `async` run and would hold
/// up the window.
pub(crate) async fn blocking<T: Send + 'static>(work: impl FnOnce() -> Res<T> + Send + 'static) -> Res<T> {
    tauri::async_runtime::spawn_blocking(work).await.map_err(err)?
}

type Job = Box<dyn FnOnce() + Send>;

/// Changes to the library (titles, drafts, favourites, the recycle bin) run one after
/// another on one thread, so a change never races a delete. The UI sends them one at a
/// time, which keeps them in the order they were made. The thread is Replix's own, so the
/// recycle bin can set up COM on it the way it needs to.
pub(crate) async fn in_order<T: Send + 'static>(work: impl FnOnce() -> Res<T> + Send + 'static) -> Res<T> {
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
    result.recv().await.unwrap_or_else(|| Err(i18n::tr("The task failed.")))
}

/// Like `blocking`, on a thread of its own: the audio device list sets up COM in a way a
/// pooled thread may already be set up differently for.
pub(crate) async fn own_thread<T: Send + 'static>(work: impl FnOnce() -> Res<T> + Send + 'static) -> Res<T> {
    blocking(move || std::thread::spawn(work).join().map_err(|_| i18n::tr("The task failed."))?).await
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| shell::show_main(app)))
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
                        hotkeys::on_hotkey(app, shortcut);
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
            i18n::set(&settings.language);
            let _ = std::fs::create_dir_all(&settings.clips_dir);
            let _ = std::fs::create_dir_all(&cache);
            let scope = app.asset_protocol_scope();
            let _ = scope.allow_directory(&settings.clips_dir, true);
            let _ = scope.allow_directory(&cache, true);

            install::start(&handle);
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

            hotkeys::register_hotkeys(&handle, &settings);
            shell::build_tray(&handle)?;

            let toast = WebviewWindowBuilder::new(app, "toast", WebviewUrl::App("index.html#toast".into()))
                .title("Replix")
                .inner_size(340.0, 76.0)
                .decorations(false)
                .transparent(true)
                .shadow(false)
                .always_on_top(true)
                .skip_taskbar(true)
                .resizable(false)
                .focused(false)
                .visible(false);
            platform::passive(toast).build()?;

            if std::env::args().any(|a| a == "--minimized") {
                if let Some(main) = app.get_webview_window("main") {
                    let _ = main.hide();
                    shell::set_page_visible(&main, false);
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
                        shell::set_page_visible(&main, false);
                    } else {
                        settings_sync::quit_after_settings(window.app_handle());
                    }
                }
                // Minimising and restoring both report a new size. A hidden window can get one
                // too, so its state decides, not the size.
                WindowEvent::Resized(_) => {
                    let shown = window.is_visible().unwrap_or(true) && !window.is_minimized().unwrap_or(false);
                    shell::set_page_visible(&main, shown);
                }
                WindowEvent::Focused(focused) => {
                    // The page reports its focus; gaining the window's is a backstop, should
                    // that report be missed after a restore.
                    if *focused {
                        FOCUSED.store(true, Ordering::Relaxed);
                    }
                    // Restored by the taskbar or Alt-Tab rather than the tray: the window's own
                    // state decides, in case a resize went unreported, and the page is asked
                    // to show again in case an earlier request did not take.
                    if *focused {
                        let shown = window.is_visible().unwrap_or(true) && !window.is_minimized().unwrap_or(false);
                        shell::set_page_visible(&main, shown);
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
            settings_sync::get_settings,
            settings_sync::set_settings,
            system::get_status,
            system::ffmpeg_setup,
            system::retry_ffmpeg,
            system::get_encoders,
            system::get_codecs,
            system::list_apps,
            system::list_monitors,
            system::report_focus,
            system::save_clip_now,
            system::take_screenshot,
            system::toggle_session,
            system::add_marker,
            system::preview_toast,
            system::preview_sound,
            system::app_icon,
            system::list_mics,
            system::start_mic_test,
            system::stop_mic_test,
            system::ui_language,
            clips::list_library,
            clips::save_draft,
            clips::rename_item,
            clips::set_favorite,
            clips::set_markers,
            clips::delete_items,
            clips::restore_items,
            clips::reveal_item,
            clips::reveal_path,
            clips::open_clips_folder,
            clips::storage_info,
            clips::copy_items,
            clips::copy_path,
            clips::share_link,
            clips::import_files,
            editing::open_editor,
            editing::warm_editor,
            editing::waveform,
            editing::end_editor_round,
            editing::first_editor_round,
            editing::cancel_export,
            editing::export_clip,
            editing::save_frame,
            editing::extract_track,
            hotkeys::hotkey_conflicts,
            hotkeys::pause_hotkeys,
            hotkeys::check_hotkey,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Replix");
}
