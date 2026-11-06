//! Replix's windows and tray: the corner toast, the main window coming and going, and
//! what work may run while it is away.

use std::sync::atomic::Ordering;

use serde::Serialize;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, Wry};

use crate::i18n::tr;
use crate::recorder::Command;
use crate::settings::ToastPosition;
use crate::{media, platform, AppState};

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
    /// Background colours, as in the main window.
    theme: String,
    scale: f64,
}

/// The toast window at medium size, in logical pixels.
const TOAST_SIZE: (f64, f64) = (340.0, 76.0);

/// A small note in the corner of the screen that does not take focus from the game.
/// The toast window hides itself once its timer runs out.
pub fn toast(app: &AppHandle, title: &str, detail: &str) {
    let _ = app.emit("notice", Toast { title: title.into(), detail: detail.into() });
    corner_toast(app, title, detail);
}

pub(crate) fn corner_toast(app: &AppHandle, title: &str, detail: &str) {
    let state = app.state::<AppState>();
    let (show, position, accent, theme, scale) = {
        let s = state.settings.lock();
        (s.show_toast, s.toast_position, s.accent.clone(), s.theme.clone(), s.toast_size.scale())
    };
    if !show {
        return;
    }
    let Some(window) = app.get_webview_window("toast") else { return };
    let _ = window.emit("toast", ToastShown { title: title.into(), detail: detail.into(), position, accent, theme, scale });
    let _ = window.set_size(tauri::LogicalSize::new(TOAST_SIZE.0 * scale, TOAST_SIZE.1 * scale));
    if let Some((x, y, w, h)) = platform::foreground_work_area().or_else(|| work_area_under_cursor(&window)) {
        let dpi = window.scale_factor().unwrap_or(1.0);
        let size = ((TOAST_SIZE.0 * scale * dpi) as i32, (TOAST_SIZE.1 * scale * dpi) as i32);
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
    platform::show_passive(&window);
}

/// The work area of the monitor the mouse is on, which is where the game is played.
fn work_area_under_cursor(window: &tauri::WebviewWindow) -> Option<(i32, i32, i32, i32)> {
    let cursor = window.cursor_position().ok();
    let monitor = cursor
        .and_then(|c| window.monitor_from_point(c.x, c.y).ok().flatten())
        .or_else(|| window.primary_monitor().ok().flatten())?;
    let area = monitor.work_area();
    Some((area.position.x, area.position.y, area.size.width as i32, area.size.height as i32))
}

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

/// A hidden or minimised window's page keeps rendering and running its animations unless
/// the webview itself is hidden too, which would cost the game frames while Replix sits in
/// the tray. Hidden, WebView2 stops painting and throttles the page's timers.
pub(crate) fn set_page_visible(window: &tauri::WebviewWindow, visible: bool) {
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
    platform::set_page_memory(webview, visible);
    media::wake();
}

pub(crate) fn show_main(app: &AppHandle) {
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

/// The tray menu, in the current language.
fn tray_menu(app: &AppHandle) -> tauri::Result<Menu<Wry>> {
    let open = MenuItem::with_id(app, "open", tr("Open Replix"), true, None::<&str>)?;
    let clip = MenuItem::with_id(app, "clip", tr("Save clip"), true, None::<&str>)?;
    let shot = MenuItem::with_id(app, "screenshot", tr("Take screenshot"), true, None::<&str>)?;
    let session = MenuItem::with_id(app, "session", tr("Start or stop session"), true, None::<&str>)?;
    let marker = MenuItem::with_id(app, "marker", tr("Add marker"), true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", tr("Quit Replix"), true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let separator2 = PredefinedMenuItem::separator(app)?;
    Menu::with_items(app, &[&open, &separator, &clip, &shot, &session, &marker, &separator2, &quit])
}

/// The tray menu again, after the language changed.
pub(crate) fn refresh_tray(app: &AppHandle) {
    if let (Some(tray), Ok(menu)) = (app.tray_by_id("main"), tray_menu(app)) {
        let _ = tray.set_menu(Some(menu));
    }
}

pub(crate) fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    TrayIconBuilder::with_id("main")
        .icon(app.default_window_icon().cloned().expect("icon"))
        .tooltip("Replix")
        .menu(&tray_menu(app)?)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| {
            let state = app.state::<AppState>();
            match event.id.as_ref() {
                "open" => show_main(app),
                "clip" => state.recorder.send(Command::SaveClip),
                "screenshot" => state.recorder.send(Command::Screenshot),
                "session" => state.recorder.send(Command::ToggleSession),
                "marker" => state.recorder.send(Command::Marker),
                "quit" => crate::settings_sync::quit_after_settings(app),
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
