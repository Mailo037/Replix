//! Windows: Win32 for windows, processes and monitors; Graphics Capture and Desktop
//! Duplication (through FFmpeg) for the picture.

use std::collections::HashMap;
use std::ffi::c_void;
use std::os::windows::process::CommandExt;
use std::process::Command;

use super::{Area, Capture, Grab, MonitorInfo, ProcessEntry, WindowInfo, WindowMemo};
use windows::core::{BOOL, PCWSTR, PWSTR};
use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM, RECT};
use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};
use windows::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, MonitorFromWindow, HDC, HMONITOR, MONITORINFO, MONITOR_DEFAULTTONEAREST,
};
use windows::Win32::Storage::FileSystem::{GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetForegroundWindow, GetWindow, GetWindowLongPtrW, GetWindowRect, GetWindowTextLengthW,
    GetWindowTextW, GetWindowThreadProcessId, IsIconic, IsWindow, IsWindowVisible, GWL_EXSTYLE, GW_OWNER, MONITORINFOF_PRIMARY,
    WS_EX_TOOLWINDOW,
};

fn hwnd(raw: u64) -> HWND {
    HWND(raw as *mut c_void)
}

pub fn is_window(raw: u64) -> bool {
    unsafe { IsWindow(Some(hwnd(raw))).as_bool() }
}

/// Minimised windows send Graphics Capture no pictures.
pub fn is_minimized(raw: u64) -> bool {
    unsafe { IsIconic(hwnd(raw)).as_bool() }
}

/// Every top-level window, topmost first.
fn top_level_windows() -> Vec<HWND> {
    unsafe extern "system" fn collect(h: HWND, data: LPARAM) -> BOOL {
        let list = &mut *(data.0 as *mut Vec<HWND>);
        list.push(h);
        BOOL(1)
    }
    let mut handles: Vec<HWND> = Vec::new();
    unsafe {
        let _ = EnumWindows(Some(collect), LPARAM(&mut handles as *mut _ as isize));
    }
    handles
}

/// Whether nothing behind a window shows through it: see-through and click-through windows,
/// like overlays, hide nothing.
fn is_solid(h: HWND) -> bool {
    use windows::Win32::UI::WindowsAndMessaging::{WS_EX_LAYERED, WS_EX_NOREDIRECTIONBITMAP, WS_EX_TRANSPARENT};
    let ex = unsafe { GetWindowLongPtrW(h, GWL_EXSTYLE) } as u32;
    ex & (WS_EX_LAYERED.0 | WS_EX_TRANSPARENT.0 | WS_EX_NOREDIRECTIONBITMAP.0) == 0
}

/// Where the windows of programs from `exes` (lower-case executable names) show on the
/// monitor with this index, by window handle; menus and pop-ups count too. Windows that are
/// minimised, on another virtual desktop or entirely behind one solid window are left out.
pub fn excluded_areas(index: u32, exes: &[String], memo: &mut WindowMemo) -> Vec<(u64, Area)> {
    if exes.is_empty() {
        memo.windows.clear();
        return Vec::new();
    }
    if memo.exes != exes {
        memo.exes = exes.to_vec();
        memo.windows.clear();
    }
    let handles = monitor_handles();
    let Some(&monitor) = handles.get(index as usize).or(handles.first()) else { return Vec::new() };
    let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
    if !unsafe { GetMonitorInfoW(monitor, &mut info) }.as_bool() {
        return Vec::new();
    }
    let m = info.rcMonitor;
    let (width, height) = ((m.right - m.left) as f32, (m.bottom - m.top) as f32);
    if width <= 0.0 || height <= 0.0 {
        return Vec::new();
    }
    let own = std::process::id();
    // Only windows this look found stay remembered, so closed ones are forgotten.
    let mut known: HashMap<u64, (u32, bool)> = HashMap::with_capacity(memo.windows.len());
    // Solid windows above the one being looked at, cut to the monitor.
    let mut above: Vec<RECT> = Vec::new();
    let mut out = Vec::new();
    for h in top_level_windows() {
        // Cheapest checks first; the cloaking one asks the compositor.
        let mut r = RECT::default();
        let shown = unsafe { IsWindowVisible(h).as_bool() && !IsIconic(h).as_bool() && GetWindowRect(h, &mut r).is_ok() };
        if !shown {
            continue;
        }
        let r = RECT { left: r.left.max(m.left), top: r.top.max(m.top), right: r.right.min(m.right), bottom: r.bottom.min(m.bottom) };
        if r.right <= r.left || r.bottom <= r.top || is_cloaked(h) {
            continue;
        }
        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(h, Some(&mut pid)) };
        let key = h.0 as u64;
        // A window keeps its process for life; a handle reused by another process is looked up again.
        let remembered = memo.windows.get(&key).filter(|(p, _)| *p == pid).map(|(_, kept)| *kept);
        let excluded = remembered.unwrap_or_else(|| {
            pid != own
                && known.values().find(|(p, _)| *p == pid).map(|(_, kept)| *kept).unwrap_or_else(|| {
                    process_path(pid)
                        .and_then(|p| p.rsplit('\\').next().map(|exe| exes.contains(&exe.to_lowercase())))
                        .unwrap_or(false)
                })
        });
        known.insert(key, (pid, excluded));
        if excluded {
            let hidden = above.iter().any(|a| a.left <= r.left && a.top <= r.top && a.right >= r.right && a.bottom >= r.bottom);
            if !hidden {
                let area = Area {
                    left: (r.left - m.left) as f32 / width,
                    top: (r.top - m.top) as f32 / height,
                    right: (r.right - m.left) as f32 / width,
                    bottom: (r.bottom - m.top) as f32 / height,
                };
                out.push((h.0 as u64, area));
            }
        } else if is_solid(h) {
            above.push(r);
        }
    }
    memo.windows = known;
    out
}

/// The Windows display language as a locale name, e.g. "de-DE".
pub fn ui_language() -> String {
    use windows::Win32::Globalization::{GetUserDefaultUILanguage, LCIDToLocaleName};
    let mut buf = [0u16; 85];
    let n = unsafe { LCIDToLocaleName(GetUserDefaultUILanguage() as u32, Some(&mut buf), 0) };
    if n <= 1 {
        return "en-US".into();
    }
    String::from_utf16_lossy(&buf[..n as usize - 1])
}

pub fn foreground() -> Option<WindowInfo> {
    let h = unsafe { GetForegroundWindow() };
    if h.0.is_null() {
        return None;
    }
    describe(h)
}

fn title_of(h: HWND) -> String {
    unsafe {
        let len = GetWindowTextLengthW(h);
        if len <= 0 {
            return String::new();
        }
        let mut buf = vec![0u16; len as usize + 1];
        let n = GetWindowTextW(h, &mut buf);
        String::from_utf16_lossy(&buf[..n.max(0) as usize])
    }
}

fn is_cloaked(h: HWND) -> bool {
    let mut cloaked: u32 = 0;
    unsafe {
        DwmGetWindowAttribute(h, DWMWA_CLOAKED, &mut cloaked as *mut u32 as *mut c_void, 4).is_ok() && cloaked != 0
    }
}

fn covers_monitor(h: HWND) -> bool {
    unsafe {
        let mut rect = RECT::default();
        if GetWindowRect(h, &mut rect).is_err() {
            return false;
        }
        let monitor = MonitorFromWindow(h, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        if !GetMonitorInfoW(monitor, &mut info).as_bool() {
            return false;
        }
        let m = info.rcMonitor;
        rect.left <= m.left && rect.top <= m.top && rect.right >= m.right && rect.bottom >= m.bottom
    }
}

fn describe(h: HWND) -> Option<WindowInfo> {
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(h, Some(&mut pid)) };
    if pid == 0 {
        return None;
    }
    let exe_path = process_path(pid).unwrap_or_default();
    let exe = exe_path.rsplit('\\').next().unwrap_or("").to_string();
    Some(WindowInfo { hwnd: h.0 as u64, title: title_of(h), exe, exe_path, pid, fullscreen: covers_monitor(h) })
}

/// Visible top-level windows a person would recognise as programs.
pub fn list_windows() -> Vec<WindowInfo> {
    let own = std::process::id();
    top_level_windows()
        .into_iter()
        .filter(|&h| unsafe {
            IsWindowVisible(h).as_bool()
                && GetWindow(h, GW_OWNER).map(|o| o.0.is_null()).unwrap_or(true)
                && (GetWindowLongPtrW(h, GWL_EXSTYLE) as u32 & WS_EX_TOOLWINDOW.0) == 0
                && !is_cloaked(h)
                && GetWindowTextLengthW(h) > 0
        })
        .filter_map(describe)
        .filter(|w| w.pid != own && !w.exe.is_empty())
        .collect()
}

pub fn process_path(pid: u32) -> Option<String> {
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = vec![0u16; 1024];
        let mut len = buf.len() as u32;
        let result = QueryFullProcessImageNameW(handle, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len);
        let _ = CloseHandle(handle);
        result.ok()?;
        Some(String::from_utf16_lossy(&buf[..len as usize]))
    }
}

/// Every running process by pid.
pub fn processes() -> HashMap<u32, ProcessEntry> {
    let mut out = HashMap::new();
    unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else { return out };
        let mut entry = PROCESSENTRY32W { dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        if Process32FirstW(snap, &mut entry).is_ok() {
            loop {
                let len = entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(entry.szExeFile.len());
                let exe = String::from_utf16_lossy(&entry.szExeFile[..len]);
                out.insert(
                    entry.th32ProcessID,
                    ProcessEntry {
                        pid: entry.th32ProcessID,
                        parent: entry.th32ParentProcessID,
                        exe_lower: exe.to_lowercase(),
                        exe,
                    },
                );
                if Process32NextW(snap, &mut entry).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snap);
    }
    out
}

/// The program's own name for itself, from its version resource.
pub(super) fn file_description(path: &str) -> Option<String> {
    let wide: Vec<u16> = path.encode_utf16().chain(Some(0)).collect();
    unsafe {
        let size = GetFileVersionInfoSizeW(PCWSTR(wide.as_ptr()), None);
        if size == 0 {
            return None;
        }
        let mut data = vec![0u8; size as usize];
        GetFileVersionInfoW(PCWSTR(wide.as_ptr()), None, size, data.as_mut_ptr() as *mut c_void).ok()?;
        // Read the first language/codepage pair, then FileDescription for it.
        let mut ptr: *mut c_void = std::ptr::null_mut();
        let mut len = 0u32;
        let query: Vec<u16> = "\\VarFileInfo\\Translation".encode_utf16().chain(Some(0)).collect();
        if !VerQueryValueW(data.as_ptr() as *const c_void, PCWSTR(query.as_ptr()), &mut ptr, &mut len).as_bool() || len < 4 {
            return None;
        }
        let pair = std::slice::from_raw_parts(ptr as *const u16, 2);
        let key = format!("\\StringFileInfo\\{:04x}{:04x}\\FileDescription", pair[0], pair[1]);
        let key: Vec<u16> = key.encode_utf16().chain(Some(0)).collect();
        if !VerQueryValueW(data.as_ptr() as *const c_void, PCWSTR(key.as_ptr()), &mut ptr, &mut len).as_bool() || len == 0 {
            return None;
        }
        let text = std::slice::from_raw_parts(ptr as *const u16, len as usize);
        let end = text.iter().position(|&c| c == 0).unwrap_or(text.len());
        Some(String::from_utf16_lossy(&text[..end]).trim().to_string())
    }
}

fn monitor_handles() -> Vec<HMONITOR> {
    unsafe extern "system" fn collect(m: HMONITOR, _: HDC, _: *mut RECT, data: LPARAM) -> BOOL {
        let list = &mut *(data.0 as *mut Vec<HMONITOR>);
        list.push(m);
        BOOL(1)
    }
    let mut handles: Vec<HMONITOR> = Vec::new();
    unsafe {
        let _ = EnumDisplayMonitors(None, None, Some(collect), LPARAM(&mut handles as *mut _ as isize));
    }
    handles
}

/// The Desktop Duplication output index of a monitor, when the default graphics adapter drives it.
pub fn dxgi_output(index: u32) -> Option<u32> {
    use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIFactory1};
    let target = *monitor_handles().get(index as usize)?;
    unsafe {
        let factory: IDXGIFactory1 = CreateDXGIFactory1().ok()?;
        let adapter = factory.EnumAdapters1(0).ok()?;
        let mut i = 0;
        while let Ok(output) = adapter.EnumOutputs(i) {
            if output.GetDesc().ok()?.Monitor == target {
                return Some(i);
            }
            i += 1;
        }
    }
    None
}

/// Each graphics adapter as "name vendor:device driver", to notice new hardware or drivers.
pub fn gpu_signature() -> Vec<String> {
    use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIDevice, IDXGIFactory1};
    use windows::core::Interface;
    let mut out = Vec::new();
    unsafe {
        let Ok(factory) = CreateDXGIFactory1::<IDXGIFactory1>() else { return out };
        let mut i = 0;
        while let Ok(adapter) = factory.EnumAdapters1(i) {
            i += 1;
            let Ok(desc) = adapter.GetDesc1() else { continue };
            let len = desc.Description.iter().position(|&c| c == 0).unwrap_or(desc.Description.len());
            let name = String::from_utf16_lossy(&desc.Description[..len]);
            let driver = adapter.CheckInterfaceSupport(&IDXGIDevice::IID).unwrap_or(0);
            out.push(format!("{name} {:04x}:{:04x} {driver:x}", desc.VendorId, desc.DeviceId));
        }
    }
    out
}

pub fn monitors() -> Vec<MonitorInfo> {
    monitor_handles()
        .into_iter()
        .enumerate()
        .filter_map(|(i, m)| unsafe {
            let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
            GetMonitorInfoW(m, &mut info).as_bool().then(|| MonitorInfo {
                index: i as u32,
                width: info.rcMonitor.right - info.rcMonitor.left,
                height: info.rcMonitor.bottom - info.rcMonitor.top,
                primary: info.dwFlags & MONITORINFOF_PRIMARY != 0,
            })
        })
        .collect()
}

/// Lets the game have the CPU first: for threads doing work nobody waits for.
pub fn lower_thread_priority() {
    use windows::Win32::System::Threading::{GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_BELOW_NORMAL};
    unsafe {
        let _ = SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL);
    }
}

/// Free bytes on the drive that holds `path`.
pub fn disk_free(path: &std::path::Path) -> u64 {
    use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    let wide: Vec<u16> = path.to_string_lossy().encode_utf16().chain(Some(0)).collect();
    let mut free = 0u64;
    unsafe {
        let _ = GetDiskFreeSpaceExW(PCWSTR(wide.as_ptr()), Some(&mut free), None, None);
    }
    free
}

pub fn hide_file(path: &std::path::Path) {
    use windows::Win32::Storage::FileSystem::{SetFileAttributesW, FILE_ATTRIBUTE_HIDDEN};
    let wide: Vec<u16> = path.to_string_lossy().encode_utf16().chain(Some(0)).collect();
    unsafe {
        let _ = SetFileAttributesW(PCWSTR(wide.as_ptr()), FILE_ATTRIBUTE_HIDDEN);
    }
}

/// Shows a window without taking focus from the game.
pub fn show_passive(window: &tauri::WebviewWindow) {
    if let Ok(hwnd) = window.hwnd() {
        show_passive_hwnd(hwnd.0 as u64);
    }
}

/// The toast window as built: `show_passive` keeps it from taking focus each time it shows.
pub fn passive<'a, R: tauri::Runtime, M: tauri::Manager<R>>(
    builder: tauri::WebviewWindowBuilder<'a, R, M>,
) -> tauri::WebviewWindowBuilder<'a, R, M> {
    builder
}

/// Hidden, WebView2 is asked to keep as little memory as it can: it drops caches and the
/// pictures and video the page let go of, which leaves more for the game. On screen again,
/// it goes back to normal. Older WebView2 runtimes without this simply keep their memory.
pub fn set_page_memory(webview: &tauri::Webview, visible: bool) {
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        ICoreWebView2_19, COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW, COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL,
    };
    use windows::core::Interface;
    let level = if visible { COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL } else { COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW };
    let _ = webview.with_webview(move |platform| {
        // SAFETY: called on the main thread with the controller Tauri keeps alive.
        let result = unsafe {
            platform
                .controller()
                .CoreWebView2()
                .and_then(|core| core.cast::<ICoreWebView2_19>())
                .and_then(|core| core.SetMemoryUsageTargetLevel(level))
        };
        if let Err(e) = result {
            log::debug!("page memory level not set: {e}");
        }
    });
}

/// The Videos folder of the user.
pub fn videos_dir() -> std::path::PathBuf {
    let home = std::env::var_os("USERPROFILE").map(std::path::PathBuf::from).unwrap_or_else(|| ".".into());
    home.join("Videos")
}

/// Graphics Capture records a window by itself, not the screen it is on.
pub fn screen_of_window(_raw: u64) -> Option<u32> {
    None
}

fn show_passive_hwnd(raw: u64) {
    use windows::Win32::UI::WindowsAndMessaging::{
        SetWindowLongPtrW, SetWindowPos, ShowWindow, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
        SW_SHOWNOACTIVATE, WS_EX_NOACTIVATE,
    };
    let h = hwnd(raw);
    unsafe {
        let ex = GetWindowLongPtrW(h, GWL_EXSTYLE) as u32;
        let wanted = ex | WS_EX_NOACTIVATE.0 | WS_EX_TOOLWINDOW.0;
        if ex != wanted {
            SetWindowLongPtrW(h, GWL_EXSTYLE, wanted as isize);
        }
        let _ = ShowWindow(h, SW_SHOWNOACTIVATE);
        let _ = SetWindowPos(h, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
    }
}

/// Work area of the monitor the foreground window is on: (x, y, width, height).
pub fn foreground_work_area() -> Option<(i32, i32, i32, i32)> {
    unsafe {
        let h = GetForegroundWindow();
        let monitor = MonitorFromWindow(h, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        GetMonitorInfoW(monitor, &mut info).as_bool().then(|| {
            let r = info.rcWork;
            (r.left, r.top, r.right - r.left, r.bottom - r.top)
        })
    }
}

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const BELOW_NORMAL_PRIORITY_CLASS: u32 = 0x0000_4000;

/// Starts a helper without a console window; `low_priority` lets the game have the CPU first.
pub fn background(cmd: &mut Command, low_priority: bool) {
    cmd.creation_flags(if low_priority { CREATE_NO_WINDOW | BELOW_NORMAL_PRIORITY_CLASS } else { CREATE_NO_WINDOW });
}

/// Graphics Capture hands FFmpeg its pictures on the GPU; screenshots need them in memory.
pub const DOWNLOAD: &str = ",hwdownload,format=bgra";

/// Whether the capture of a whole monitor sends pictures even while the screen is still.
pub fn steady_screen(index: u32) -> bool {
    dxgi_output(index).is_some()
}

/// The FFmpeg input for a window or monitor; `chain` is added to its filters.
pub fn capture(grab: Grab, fps: u32, cursor: bool, chain: &str) -> Capture {
    let what = match grab {
        Grab::Window(hwnd) => format!("hwnd={hwnd}"),
        Grab::Monitor(index) => match dxgi_output(index) {
            // Graphics Capture sends nothing while the screen is still, which stalls the
            // buffer. Desktop Duplication repeats the last frame, so prefer it for screens.
            Some(output) => {
                let source = format!("ddagrab=output_idx={output}:framerate={fps}:draw_mouse={}{chain}", u8::from(cursor));
                return Capture { input: lavfi(source), filter: None };
            }
            None => format!("monitor_idx={index}"),
        },
    };
    // Even sizes: several encoders reject odd widths. A game that changes its resolution
    // or leaves full screen is scaled into the first size, never cut off.
    let source = format!(
        "gfxcapture={what}:max_framerate={fps}:capture_cursor={}:width=-2:height=-2:resize_mode=scale_aspect{chain}",
        u8::from(cursor)
    );
    Capture { input: lavfi(source), filter: None }
}

fn lavfi(source: String) -> Vec<String> {
    vec!["-f".into(), "lavfi".into(), "-i".into(), source]
}

/// A test picture as an encoder gets it while recording, for trying encoders.
pub fn test_capture(chain: &str) -> Capture {
    Capture { input: lavfi(format!("gfxcapture=monitor_idx=0:max_framerate=30{chain}")), filter: None }
}

/// Why recording cannot work here, when it cannot: Windows always has a way.
pub fn capture_unavailable(_whole_screen: bool) -> Option<String> {
    None
}

/// Nothing to set up before the start: the job takes the helper once it runs.
pub fn prepare_tied(_cmd: &mut Command) {}

/// CPU time a process has used so far, in user and kernel mode together.
pub fn cpu_time(child: &std::process::Child) -> Option<std::time::Duration> {
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::Foundation::{FILETIME, HANDLE};
    use windows::Win32::System::Threading::GetProcessTimes;
    let (mut created, mut exited, mut kernel, mut user) = Default::default();
    // SAFETY: the handle belongs to `child`, which outlives the call.
    unsafe { GetProcessTimes(HANDLE(child.as_raw_handle()), &mut created, &mut exited, &mut kernel, &mut user) }.ok()?;
    let ticks = |t: FILETIME| (u64::from(t.dwHighDateTime) << 32) | u64::from(t.dwLowDateTime);
    // In units of 100 ns.
    Some(std::time::Duration::from_nanos((ticks(kernel) + ticks(user)) * 100))
}

/// Ties a helper process to Replix's lifetime: once Replix ends, however it ends (closed,
/// crashed, killed by an installer), Windows ends the helper too. Without this an FFmpeg
/// left running keeps its exe locked and an update cannot replace it.
pub fn tie_to_app(child: &std::process::Child) {
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::JobObjects::AssignProcessToJobObject;
    let Some(job) = app_job() else { return };
    // SAFETY: the job handle lives for the whole process; the child's handle outlives the call.
    if let Err(e) = unsafe { AssignProcessToJobObject(job, HANDLE(child.as_raw_handle())) } {
        log::warn!("a helper process is not tied to Replix: {e}");
    }
}

/// The job that ends every tied helper when Replix ends. Its handle is never closed:
/// Windows closes it when Replix exits, and that is what ends the helpers.
fn app_job() -> Option<windows::Win32::Foundation::HANDLE> {
    use std::sync::OnceLock;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::JobObjects::{
        CreateJobObjectW, JobObjectExtendedLimitInformation, SetInformationJobObject,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };
    // Kept as an address because a raw handle cannot be shared across threads.
    static JOB: OnceLock<Option<usize>> = OnceLock::new();
    let job = JOB.get_or_init(|| {
        // SAFETY: plain Win32 calls on a job object created and owned here.
        unsafe {
            let job = CreateJobObjectW(None, PCWSTR::null()).ok()?;
            let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            let set = SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const c_void,
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            );
            if let Err(e) = set {
                log::warn!("helper processes are not tied to Replix: {e}");
                let _ = CloseHandle(job);
                return None;
            }
            Some(job.0 as usize)
        }
    });
    job.map(|job| HANDLE(job as *mut c_void))
}

/// A program's icon as RGBA pixels (width, height, pixels), at about `size` pixels.
pub fn icon_rgba(path: &str, size: u32) -> Option<(u32, u32, Vec<u8>)> {
    use windows::Win32::Graphics::Gdi::{
        DeleteObject, GetDC, GetDIBits, GetObjectW, ReleaseDC, BITMAP, BITMAPINFO, BITMAPINFOHEADER, BI_RGB,
        DIB_RGB_COLORS, HGDIOBJ,
    };
    use windows::Win32::UI::Shell::SHDefExtractIconW;
    use windows::Win32::UI::WindowsAndMessaging::{DestroyIcon, GetIconInfo, HICON, ICONINFO};

    let wide: Vec<u16> = path.encode_utf16().chain(Some(0)).collect();
    unsafe {
        let mut icon = HICON::default();
        let found = SHDefExtractIconW(PCWSTR(wide.as_ptr()), 0, 0, Some(&mut icon), None, size);
        if found.is_err() || found.0 != 0 || icon.is_invalid() {
            return None;
        }
        let mut info = ICONINFO::default();
        let got = GetIconInfo(icon, &mut info);
        let result = (|| {
            got.ok()?;
            let mut bm = BITMAP::default();
            if GetObjectW(HGDIOBJ(info.hbmColor.0), size_of::<BITMAP>() as i32, Some(&mut bm as *mut _ as *mut c_void)) == 0 {
                return None;
            }
            let (w, h) = (bm.bmWidth as u32, bm.bmHeight as u32);
            let mut header = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: w as i32,
                    biHeight: -(h as i32),
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut pixels = vec![0u8; (w * h * 4) as usize];
            let dc = GetDC(None);
            let lines = GetDIBits(dc, info.hbmColor, 0, h, Some(pixels.as_mut_ptr() as *mut c_void), &mut header, DIB_RGB_COLORS);
            ReleaseDC(None, dc);
            if lines == 0 {
                return None;
            }
            // BGRA to RGBA. Old icons have no alpha at all: then every pixel is solid.
            let has_alpha = pixels.as_chunks::<4>().0.iter().any(|p| p[3] != 0);
            for p in pixels.as_chunks_mut::<4>().0 {
                p.swap(0, 2);
                if !has_alpha {
                    p[3] = 255;
                }
            }
            Some((w, h, pixels))
        })();
        if !info.hbmColor.is_invalid() {
            let _ = DeleteObject(HGDIOBJ(info.hbmColor.0));
        }
        if !info.hbmMask.is_invalid() {
            let _ = DeleteObject(HGDIOBJ(info.hbmMask.0));
        }
        let _ = DestroyIcon(icon);
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tied helper sits in the job whose closing ends it along with Replix.
    #[test]
    fn a_tied_helper_is_in_the_app_job() {
        use std::os::windows::io::AsRawHandle;
        use windows::Win32::Foundation::HANDLE;
        use windows::Win32::System::JobObjects::IsProcessInJob;
        let mut child = std::process::Command::new("cmd")
            .args(["/C", "ping -n 30 127.0.0.1 >NUL"])
            .stdout(std::process::Stdio::null())
            .spawn()
            .unwrap();
        tie_to_app(&child);
        let mut inside = BOOL(0);
        // SAFETY: both handles are alive for the call.
        unsafe { IsProcessInJob(HANDLE(child.as_raw_handle()), app_job(), &mut inside) }.unwrap();
        let _ = child.kill();
        let _ = child.wait();
        assert!(inside.as_bool());
    }
}
