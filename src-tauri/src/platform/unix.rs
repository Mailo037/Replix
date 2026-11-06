//! What Linux and macOS do the same way.

use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::Command;

/// Frames from X11 and AVFoundation are already in memory: nothing to download.
pub const DOWNLOAD: &str = "";

/// Starts a helper; `low_priority` lets the game have the CPU first. Unlike on Windows there
/// is no console window to hide.
pub fn background(cmd: &mut Command, low_priority: bool) {
    if low_priority {
        // SAFETY: nice() is async-signal-safe, which is all that may run between fork and exec.
        unsafe {
            cmd.pre_exec(|| {
                libc::nice(10);
                Ok(())
            });
        }
    }
}

/// Free bytes on the drive that holds `path`, or the nearest folder above it that exists.
pub fn disk_free(path: &Path) -> u64 {
    let Some(existing) = path.ancestors().find(|p| p.exists()) else { return 0 };
    let Ok(c) = std::ffi::CString::new(existing.as_os_str().as_encoded_bytes()) else { return 0 };
    // SAFETY: a zeroed statvfs is a valid value to be filled in; the path is NUL-terminated.
    unsafe {
        let mut stats: libc::statvfs = std::mem::zeroed();
        if libc::statvfs(c.as_ptr(), &mut stats) != 0 {
            return 0;
        }
        (stats.f_bavail as u64).saturating_mul(stats.f_frsize as u64)
    }
}

/// Files whose names start with a dot are hidden already; the others stay as they are for
/// the moment they exist.
pub fn hide_file(_path: &Path) {}

/// The system's language as a locale name, e.g. "de-DE".
pub fn ui_language() -> String {
    let raw = sys_locale::get_locale().unwrap_or_else(|| "en-US".into());
    // "de_DE.UTF-8@euro" → "de-DE"
    let base = raw.split(['.', '@']).next().unwrap_or("en-US");
    base.replace('_', "-")
}

/// Shows the toast; it was built unable to take focus (`passive`), so the game stays in front.
pub fn show_passive(window: &tauri::WebviewWindow) {
    let _ = window.show();
}

/// The toast window, made unable to take focus from the game.
pub fn passive<'a, R: tauri::Runtime, M: tauri::Manager<R>>(
    builder: tauri::WebviewWindowBuilder<'a, R, M>,
) -> tauri::WebviewWindowBuilder<'a, R, M> {
    builder.focusable(false)
}

/// WebKit has no memory setting to change; a hidden page already stops painting.
pub fn set_page_memory(_webview: &tauri::Webview, _visible: bool) {}

/// The work area under the foreground window, where the toast goes. Tauri's monitor
/// information is used instead.
pub fn foreground_work_area() -> Option<(i32, i32, i32, i32)> {
    None
}
