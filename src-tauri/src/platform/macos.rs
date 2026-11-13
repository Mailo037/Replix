//! macOS: windows and displays from Quartz, processes through sysinfo, and AVFoundation
//! (through FFmpeg) for the picture.
//!
//! AVFoundation records whole displays. A game or app is recorded as the display it is on,
//! and kept-out programs are blacked out on it the same way as when recording a screen.
//! Recording needs the Screen Recording permission, which macOS asks for once.

use std::collections::HashMap;
use std::process::Command;
use std::time::Duration;

use core_foundation::base::{CFType, TCFType};
use core_foundation::boolean::CFBoolean;
use core_foundation::dictionary::{CFDictionary, CFDictionaryRef};
use core_foundation::number::CFNumber;
use core_foundation::string::CFString;
use core_graphics::display::CGDisplay;
use core_graphics::window::{
    copy_window_info, kCGNullWindowID, kCGWindowListExcludeDesktopElements, kCGWindowListOptionIncludingWindow,
    kCGWindowListOptionOnScreenOnly, CGWindowListOption,
};
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

use super::{areas_of, exe_name, Area, Capture, Grab, MonitorInfo, ProcessEntry, Seen, WindowInfo, WindowMemo};

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGPreflightScreenCaptureAccess() -> bool;
    fn CGRequestScreenCaptureAccess() -> bool;
}

/// One window as Quartz reports it. Bounds are in points, on the same plane as displays.
struct Win {
    id: u64,
    pid: u32,
    owner: String,
    title: String,
    layer: i64,
    alpha: f64,
    onscreen: bool,
    bounds: (f64, f64, f64, f64),
}

type Dict = CFDictionary<CFString, CFType>;

fn value(d: &Dict, key: &'static str) -> Option<CFType> {
    d.find(CFString::from_static_string(key)).map(|v| v.clone())
}

fn number(d: &Dict, key: &'static str) -> Option<f64> {
    value(d, key)?.downcast::<CFNumber>()?.to_f64()
}

fn string(d: &Dict, key: &'static str) -> Option<String> {
    Some(value(d, key)?.downcast::<CFString>()?.to_string())
}

fn windows(option: CGWindowListOption, relative_to: u32) -> Vec<Win> {
    let Some(list) = copy_window_info(option, relative_to) else { return Vec::new() };
    let mut out = Vec::new();
    for item in list.iter() {
        // SAFETY: every entry of the window list is a dictionary, kept alive by the list.
        let d: Dict = unsafe { Dict::wrap_under_get_rule(*item as CFDictionaryRef) };
        let Some(bounds) = value(&d, "kCGWindowBounds") else { continue };
        // SAFETY: kCGWindowBounds is a dictionary of X, Y, Width and Height.
        let b: Dict = unsafe { Dict::wrap_under_get_rule(bounds.as_CFTypeRef() as CFDictionaryRef) };
        let (x, y, w, h) = (number(&b, "X"), number(&b, "Y"), number(&b, "Width"), number(&b, "Height"));
        let (Some(x), Some(y), Some(w), Some(h)) = (x, y, w, h) else { continue };
        out.push(Win {
            id: number(&d, "kCGWindowNumber").unwrap_or(0.0) as u64,
            pid: number(&d, "kCGWindowOwnerPID").unwrap_or(0.0) as u32,
            owner: string(&d, "kCGWindowOwnerName").unwrap_or_default(),
            // Without the Screen Recording permission there are no titles.
            title: string(&d, "kCGWindowName").unwrap_or_default(),
            layer: number(&d, "kCGWindowLayer").unwrap_or(0.0) as i64,
            alpha: number(&d, "kCGWindowAlpha").unwrap_or(1.0),
            onscreen: value(&d, "kCGWindowIsOnscreen").and_then(|v| v.downcast::<CFBoolean>()).is_some_and(bool::from),
            bounds: (x, y, x + w, y + h),
        });
    }
    out
}

/// Windows on screen, front to back.
fn on_screen() -> Vec<Win> {
    windows(kCGWindowListOptionOnScreenOnly | kCGWindowListExcludeDesktopElements, kCGNullWindowID)
}

fn window(id: u64) -> Option<Win> {
    let id = u32::try_from(id).ok()?;
    windows(kCGWindowListOptionIncludingWindow, id).into_iter().find(|w| w.id == u64::from(id))
}

fn displays() -> Vec<CGDisplay> {
    CGDisplay::active_displays().unwrap_or_default().into_iter().map(CGDisplay::new).collect()
}

fn display_rect(d: &CGDisplay) -> (f64, f64, f64, f64) {
    let b = d.bounds();
    (b.origin.x, b.origin.y, b.origin.x + b.size.width, b.origin.y + b.size.height)
}

fn rounded(r: (f64, f64, f64, f64)) -> (i32, i32, i32, i32) {
    (r.0.round() as i32, r.1.round() as i32, r.2.round() as i32, r.3.round() as i32)
}

fn describe(w: &Win) -> WindowInfo {
    let exe_path = process_path(w.pid).unwrap_or_default();
    let exe = match exe_name(&exe_path) {
        "" => w.owner.clone(),
        name => name.to_string(),
    };
    let fullscreen = displays().iter().map(display_rect).any(|m| {
        w.bounds.0 <= m.0 + 1.0 && w.bounds.1 <= m.1 + 1.0 && w.bounds.2 >= m.2 - 1.0 && w.bounds.3 >= m.3 - 1.0
    });
    let title = if w.title.is_empty() { w.owner.clone() } else { w.title.clone() };
    WindowInfo { hwnd: w.id, title, exe, exe_path, pid: w.pid, fullscreen }
}

/// An app's own window, not a menu, the Dock or a panel.
fn is_app_window(w: &Win, own: u32) -> bool {
    w.layer == 0 && w.pid != own && w.pid != 0 && (w.bounds.2 - w.bounds.0) > 40.0 && (w.bounds.3 - w.bounds.1) > 40.0
}

/// Why recording cannot work here, when it cannot.
pub fn capture_unavailable(_whole_screen: bool) -> Option<String> {
    static ASKED: std::sync::Once = std::sync::Once::new();
    // SAFETY: plain queries; the request shows macOS's own prompt, once.
    let mut allowed = unsafe { CGPreflightScreenCaptureAccess() };
    if !allowed {
        ASKED.call_once(|| allowed = unsafe { CGRequestScreenCaptureAccess() });
    }
    (!allowed).then(|| {
        crate::i18n::tr("Replix may not record the screen yet. Allow it under System Settings › Privacy & Security › Screen Recording, then restart Replix.")
    })
}

pub fn is_window(raw: u64) -> bool {
    window(raw).is_some()
}

pub fn is_minimized(raw: u64) -> bool {
    window(raw).is_some_and(|w| !w.onscreen)
}

pub fn foreground() -> Option<WindowInfo> {
    let own = std::process::id();
    on_screen().iter().find(|w| is_app_window(w, own)).map(describe)
}

/// Visible windows a person would recognise as programs.
pub fn list_windows() -> Vec<WindowInfo> {
    let own = std::process::id();
    on_screen().iter().filter(|w| is_app_window(w, own)).map(describe).filter(|w| !w.exe.is_empty()).collect()
}

/// The display a window is on (the one holding its middle), by index in `monitors()`.
/// AVFoundation records displays, so a window is recorded as its display.
pub fn screen_of_window(raw: u64) -> Option<u32> {
    let w = window(raw)?;
    let (cx, cy) = ((w.bounds.0 + w.bounds.2) / 2.0, (w.bounds.1 + w.bounds.3) / 2.0);
    let found = displays().iter().map(display_rect).position(|m| cx >= m.0 && cx < m.2 && cy >= m.1 && cy < m.3);
    Some(found.unwrap_or(0) as u32)
}

pub fn excluded_areas(index: u32, exes: &[String], memo: &mut WindowMemo) -> Vec<(u64, Area)> {
    if exes.is_empty() {
        memo.windows.clear();
        return Vec::new();
    }
    let all = displays();
    let Some(display) = all.get(index as usize).or(all.first()) else { return Vec::new() };
    let monitor = rounded(display_rect(display));
    let seen = on_screen().into_iter().filter(|w| w.pid != 0).map(|w| Seen {
        id: w.id,
        pid: w.pid,
        rect: rounded(w.bounds),
        solid: w.layer == 0 && w.alpha >= 1.0,
    });
    areas_of(seen, monitor, exes, memo)
}

pub fn monitors() -> Vec<MonitorInfo> {
    displays()
        .iter()
        .enumerate()
        .map(|(i, d)| MonitorInfo {
            index: i as u32,
            width: d.pixels_wide() as i32,
            height: d.pixels_high() as i32,
            primary: d.is_main(),
        })
        .collect()
}

/// ~/Movies, where macOS keeps videos.
pub fn videos_dir() -> std::path::PathBuf {
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from).unwrap_or_else(|| ".".into());
    home.join("Movies")
}

/// Whether AVFoundation keeps sending pictures while nothing on screen moves is not
/// something to count on, so a still screen never counts as a stuck capture.
pub fn steady_screen(_index: u32) -> bool {
    false
}

/// The FFmpeg input for a window or display; `chain` is added to its filters.
pub fn capture(grab: Grab, fps: u32, cursor: bool, chain: &str) -> Capture {
    let index = match grab {
        Grab::Monitor(index) => index,
        Grab::Window(id) => screen_of_window(id).unwrap_or(0),
    };
    let mut input: Vec<String> = ["-thread_queue_size", "512", "-f", "avfoundation", "-pixel_format", "nv12"].map(String::from).to_vec();
    input.extend(["-capture_cursor".into(), u8::from(cursor).to_string(), "-capture_mouse_clicks".into(), "0".into()]);
    // FFmpeg names displays in the order Quartz lists them, the same order as `monitors()`.
    input.extend(["-framerate".into(), fps.to_string(), "-i".into(), format!("Capture screen {index}:none")]);
    Capture { input, filter: Some(format!("scale=trunc(iw/2)*2:trunc(ih/2)*2{chain}")) }
}

/// A test picture as an encoder gets it while recording, for trying encoders.
pub fn test_capture(chain: &str) -> Capture {
    Capture {
        input: ["-f", "lavfi", "-i", "testsrc2=size=1280x720:rate=30"].map(String::from).to_vec(),
        filter: Some(format!("null{chain}")),
    }
}

fn system_with(pids: &[Pid], kind: ProcessRefreshKind) -> System {
    let mut system = System::new();
    let which = if pids.is_empty() { ProcessesToUpdate::All } else { ProcessesToUpdate::Some(pids) };
    system.refresh_processes_specifics(which, true, kind);
    system
}

pub fn process_path(pid: u32) -> Option<String> {
    let pid = Pid::from_u32(pid);
    let system = system_with(&[pid], ProcessRefreshKind::nothing().with_exe(UpdateKind::Always));
    let exe = system.process(pid)?.exe()?;
    Some(exe.to_string_lossy().into_owned())
}

/// Every running process by pid.
pub fn processes() -> HashMap<u32, ProcessEntry> {
    let system = system_with(&[], ProcessRefreshKind::nothing().with_exe(UpdateKind::OnlyIfNotSet));
    system
        .processes()
        .iter()
        .map(|(pid, p)| {
            let exe = p
                .exe()
                .map(|e| exe_name(&e.to_string_lossy()).to_string())
                .filter(|e| !e.is_empty())
                .unwrap_or_else(|| p.name().to_string_lossy().into_owned());
            let pid = pid.as_u32();
            let parent = p.parent().map(|p| p.as_u32()).unwrap_or(0);
            (pid, ProcessEntry { pid, parent, exe_lower: exe.to_lowercase(), exe })
        })
        .collect()
}

/// "Discord" for /Applications/Discord.app/Contents/MacOS/Discord: the app bundle's name.
pub(super) fn file_description(path: &str) -> Option<String> {
    let bundle = path.split("/Contents/MacOS/").next().filter(|b| b.len() < path.len())?;
    let name = exe_name(bundle).strip_suffix(".app")?;
    Some(name.to_string())
}

fn sysctl(name: &str) -> String {
    let Ok(key) = std::ffi::CString::new(name) else { return String::new() };
    let mut buf = [0u8; 256];
    let mut len = buf.len();
    // SAFETY: the buffer and its length match; sysctl writes at most `len` bytes.
    let ok = unsafe { libc::sysctlbyname(key.as_ptr(), buf.as_mut_ptr().cast(), &mut len, std::ptr::null_mut(), 0) } == 0;
    if !ok {
        return String::new();
    }
    String::from_utf8_lossy(&buf[..len]).trim_end_matches('\0').to_string()
}

/// The Mac and its system version: a new graphics driver comes with a new macOS.
pub fn gpu_signature() -> Vec<String> {
    vec![sysctl("hw.model"), sysctl("machdep.cpu.brand_string"), sysctl("kern.osversion")]
}

/// Lets the game have the CPU first: for threads doing work nobody waits for.
pub fn lower_thread_priority() {
    // SAFETY: changes only the calling thread's class.
    unsafe {
        libc::pthread_set_qos_class_self_np(libc::qos_class_t::QOS_CLASS_UTILITY, 0);
    }
}

/// CPU time a process has used so far, in user and kernel mode together.
pub fn cpu_time(child: &std::process::Child) -> Option<Duration> {
    let pid = Pid::from_u32(child.id());
    let system = system_with(&[pid], ProcessRefreshKind::nothing().with_cpu());
    Some(Duration::from_millis(system.process(pid)?.accumulated_cpu_time()))
}

/// macOS has no way to end a child with its parent; FFmpeg ends on its own once Replix stops
/// reading from it or feeding it, and an update may replace files that are in use.
pub fn prepare_tied(_cmd: &mut Command) {}

pub fn tie_to_app(_child: &std::process::Child) {}

/// Icons of macOS apps live in their bundles as .icns; the page shows its own symbol instead.
pub fn icon_rgba(_path: &str, _size: u32) -> Option<(u32, u32, Vec<u8>)> {
    None
}

/// Plays a short WAV without waiting, with `afplay`, which every Mac has.
pub fn play_sound(wav: &'static [u8], file: &std::path::Path) {
    super::unix::play_file(&["afplay"], wav, file);
}

/// Hotkeys the desktop hands out itself. Only Wayland needs that; here Replix's own key
/// grab hears its keys whatever window is in front, so there is nothing to ask for.
pub fn desktop_hotkeys(_keys: Vec<super::DesktopHotkey>, _pressed: std::sync::Arc<dyn Fn(&str) + Send + Sync>) {}
