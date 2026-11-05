//! Small Win32 helpers: windows, processes, monitors.

use std::collections::HashMap;
use std::ffi::c_void;

use serde::Serialize;
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
    GetWindowTextW, GetWindowThreadProcessId, IsWindow, IsWindowVisible, GWL_EXSTYLE, GW_OWNER, MONITORINFOF_PRIMARY,
    WS_EX_TOOLWINDOW,
};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowInfo {
    pub hwnd: u64,
    pub title: String,
    pub exe: String,
    #[serde(skip)]
    pub exe_path: String,
    pub pid: u32,
    pub fullscreen: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MonitorInfo {
    pub index: u32,
    pub width: i32,
    pub height: i32,
    pub primary: bool,
}

fn hwnd(raw: u64) -> HWND {
    HWND(raw as *mut c_void)
}

pub fn is_window(raw: u64) -> bool {
    unsafe { IsWindow(Some(hwnd(raw))).as_bool() }
}

/// Minimised windows send Graphics Capture no pictures.
pub fn is_minimized(raw: u64) -> bool {
    use windows::Win32::UI::WindowsAndMessaging::IsIconic;
    unsafe { IsIconic(hwnd(raw)).as_bool() }
}

/// Whether the outlines of a window and a monitor overlap.
fn on_monitor(h: HWND, monitor: &RECT) -> bool {
    let mut rect = RECT::default();
    if unsafe { GetWindowRect(h, &mut rect) }.is_err() {
        return false;
    }
    rect.left < monitor.right && rect.right > monitor.left && rect.top < monitor.bottom && rect.bottom > monitor.top
}

/// The name of a program from `exes` (lower-case executable names) that has a window
/// open on the monitor with this index, minimised windows aside.
pub fn excluded_on_monitor(index: u32, exes: &[String]) -> Option<String> {
    if exes.is_empty() {
        return None;
    }
    let handles = monitor_handles();
    let monitor = *handles.get(index as usize).or(handles.first())?;
    let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
    if !unsafe { GetMonitorInfoW(monitor, &mut info) }.as_bool() {
        return None;
    }
    list_windows()
        .into_iter()
        .find(|w| exes.contains(&w.exe.to_lowercase()) && !is_minimized(w.hwnd) && on_monitor(hwnd(w.hwnd), &info.rcMonitor))
        .map(|w| app_name(&w.exe_path))
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
    unsafe extern "system" fn collect(h: HWND, data: LPARAM) -> BOOL {
        let list = &mut *(data.0 as *mut Vec<HWND>);
        list.push(h);
        BOOL(1)
    }
    let mut handles: Vec<HWND> = Vec::new();
    unsafe {
        let _ = EnumWindows(Some(collect), LPARAM(&mut handles as *mut _ as isize));
    }
    let own = std::process::id();
    handles
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

pub struct ProcessEntry {
    pub pid: u32,
    pub parent: u32,
    pub exe: String,
    /// `exe` in lower case, made once per snapshot for comparing names.
    pub exe_lower: String,
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

/// The outermost process with the same executable, so a capture includes all of an
/// app's helper processes (Discord and browsers play sound from child processes).
pub fn root_of(pid: u32, all: &HashMap<u32, ProcessEntry>) -> u32 {
    // Embedded browsers play sound on behalf of the app that hosts them.
    const HELPER: &str = "msedgewebview2.exe";
    let mut current = pid;
    for _ in 0..16 {
        let Some(entry) = all.get(&current) else { break };
        match all.get(&entry.parent) {
            Some(parent)
                if parent.pid != current
                    && (parent.exe_lower == entry.exe_lower || entry.exe_lower == HELPER) =>
            {
                current = parent.pid
            }
            _ => break,
        }
    }
    current
}

/// "Discord" for Discord.exe: the file description, or the file name without extension.
pub fn app_name(exe_path: &str) -> String {
    let stem = exe_path
        .rsplit('\\')
        .next()
        .unwrap_or(exe_path)
        .trim_end_matches(".exe")
        .trim_end_matches(".EXE")
        .to_string();
    file_description(exe_path)
        .filter(|d| !d.trim().is_empty() && d.len() <= 40)
        .unwrap_or_else(|| capitalise(&stem))
}

fn capitalise(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

fn file_description(path: &str) -> Option<String> {
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

/// Programs that are never games, even when they cover the screen.
const NOT_GAMES: &[&str] = &[
    "explorer.exe", "searchhost.exe", "startmenuexperiencehost.exe", "shellexperiencehost.exe",
    "applicationframehost.exe", "textinputhost.exe", "lockapp.exe", "chrome.exe", "msedge.exe", "firefox.exe",
    "brave.exe", "opera.exe", "opera_gx.exe", "vivaldi.exe", "arc.exe", "zen.exe", "discord.exe", "spotify.exe",
    "vlc.exe", "mpv.exe", "mpc-hc64.exe", "potplayermini64.exe", "wmplayer.exe", "code.exe", "devenv.exe",
    "obs64.exe", "steam.exe", "steamwebhelper.exe", "epicgameslauncher.exe", "battle.net.exe", "riotclientux.exe",
    "riotclientservices.exe", "eadesktop.exe", "ubisoftconnect.exe", "galaxyclient.exe", "powerpnt.exe",
    "winword.exe", "excel.exe", "teams.exe", "ms-teams.exe", "zoom.exe", "slack.exe", "windowsterminal.exe",
    "claude.exe", "replix.exe", "ffmpeg.exe", "photos.exe", "microsoft.media.player.exe",
];

/// Folders games are installed into by the common stores.
const GAME_DIRS: &[&str] = &[
    "\\steamapps\\common\\", "\\epic games\\", "\\riot games\\", "\\xboxgames\\", "\\gog galaxy\\games\\",
    "\\gog games\\", "\\ubisoft game launcher\\games\\", "\\ea games\\", "\\rockstar games\\", "\\amazon games\\",
    "\\itch\\apps\\", "\\battle.net\\games\\",
];

pub fn looks_like_game(w: &WindowInfo) -> bool {
    let exe = w.exe.to_lowercase();
    if NOT_GAMES.contains(&exe.as_str()) || w.pid == std::process::id() {
        return false;
    }
    let path = w.exe_path.to_lowercase();
    GAME_DIRS.iter().any(|d| path.contains(d)) || w.fullscreen
}

/// The name shown for a game: its window title when that is short, else the
/// store folder or the executable's description.
pub fn game_name(w: &WindowInfo) -> String {
    let title = w.title.trim();
    if !title.is_empty() && title.chars().count() <= 48 {
        return title.to_string();
    }
    let lower = w.exe_path.to_lowercase();
    if let Some(pos) = lower.find("\\steamapps\\common\\") {
        let rest = &w.exe_path[pos + "\\steamapps\\common\\".len()..];
        if let Some(folder) = rest.split('\\').next() {
            return folder.to_string();
        }
    }
    app_name(&w.exe_path)
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
pub fn show_passive(raw: u64) {
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
