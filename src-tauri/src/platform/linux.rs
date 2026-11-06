//! Linux: windows and monitors from the X server (also XWayland), processes from /proc, and
//! x11grab (through FFmpeg) for the picture.
//!
//! Under Wayland, Replix sees the programs that run through XWayland, which covers most
//! games, Proton and Wine included. The whole screen of a Wayland session cannot be grabbed
//! this way, so screen recording there asks for an X11 session.

use std::collections::HashMap;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::time::Duration;

use x11rb::connection::Connection;
use x11rb::protocol::randr::ConnectionExt as _;
use x11rb::protocol::xproto::{AtomEnum, ConnectionExt as _, Window};
use x11rb::rust_connection::RustConnection;

use super::{areas_of, exe_name, Area, Capture, Grab, MonitorInfo, ProcessEntry, Seen, WindowInfo, WindowMemo};

struct Atoms {
    active: u32,
    clients: u32,
    stacking: u32,
    pid: u32,
    name: u32,
    utf8: u32,
    state: u32,
    hidden: u32,
    fullscreen: u32,
    skip_taskbar: u32,
    kind: u32,
    normal: u32,
    opacity: u32,
}

struct X {
    conn: RustConnection,
    root: Window,
    root_size: (u16, u16),
    atoms: Atoms,
}

/// The connection to the X server, made once. `None` without one (a Wayland session with no
/// XWayland, or no display at all).
fn x() -> Option<&'static X> {
    static X: OnceLock<Option<X>> = OnceLock::new();
    X.get_or_init(|| match connect() {
        Ok(x) => Some(x),
        Err(e) => {
            log::warn!("no X server: {e}");
            None
        }
    })
    .as_ref()
}

fn connect() -> anyhow::Result<X> {
    let (conn, screen) = x11rb::connect(None)?;
    let s = &conn.setup().roots[screen];
    let (root, root_size) = (s.root, (s.width_in_pixels, s.height_in_pixels));
    let atom = |name: &str| -> anyhow::Result<u32> { Ok(conn.intern_atom(false, name.as_bytes())?.reply()?.atom) };
    let atoms = Atoms {
        active: atom("_NET_ACTIVE_WINDOW")?,
        clients: atom("_NET_CLIENT_LIST")?,
        stacking: atom("_NET_CLIENT_LIST_STACKING")?,
        pid: atom("_NET_WM_PID")?,
        name: atom("_NET_WM_NAME")?,
        utf8: atom("UTF8_STRING")?,
        state: atom("_NET_WM_STATE")?,
        hidden: atom("_NET_WM_STATE_HIDDEN")?,
        fullscreen: atom("_NET_WM_STATE_FULLSCREEN")?,
        skip_taskbar: atom("_NET_WM_STATE_SKIP_TASKBAR")?,
        kind: atom("_NET_WM_WINDOW_TYPE")?,
        normal: atom("_NET_WM_WINDOW_TYPE_NORMAL")?,
        opacity: atom("_NET_WM_WINDOW_OPACITY")?,
    };
    Ok(X { conn, root, root_size, atoms })
}

fn wayland() -> bool {
    std::env::var("XDG_SESSION_TYPE").is_ok_and(|t| t.eq_ignore_ascii_case("wayland"))
        || std::env::var_os("WAYLAND_DISPLAY").is_some()
}

/// Why recording cannot work here, when it cannot.
pub fn capture_unavailable(whole_screen: bool) -> Option<String> {
    if x().is_none() {
        return Some(crate::i18n::tr("Replix can't see the screen. Recording on Linux needs an X11 session, or XWayland for games."));
    }
    (whole_screen && wayland()).then(|| {
        crate::i18n::tr("Recording the whole screen needs an X11 session. Under Wayland, pick Games or one app instead.")
    })
}

impl X {
    fn prop32(&self, w: Window, atom: u32) -> Vec<u32> {
        self.conn
            .get_property(false, w, atom, AtomEnum::ANY, 0, 4096)
            .ok()
            .and_then(|c| c.reply().ok())
            .and_then(|r| r.value32().map(Iterator::collect))
            .unwrap_or_default()
    }

    fn text(&self, w: Window, atom: u32, kind: u32) -> Option<String> {
        let reply = self.conn.get_property(false, w, atom, kind, 0, 1024).ok()?.reply().ok()?;
        (!reply.value.is_empty()).then(|| String::from_utf8_lossy(&reply.value).trim_end_matches('\0').to_string())
    }

    fn title(&self, w: Window) -> String {
        self.text(w, self.atoms.name, self.atoms.utf8)
            .or_else(|| self.text(w, AtomEnum::WM_NAME.into(), AtomEnum::STRING.into()))
            .unwrap_or_default()
    }

    fn pid(&self, w: Window) -> u32 {
        self.prop32(w, self.atoms.pid).first().copied().unwrap_or(0)
    }

    fn has_state(&self, w: Window, state: u32) -> bool {
        self.prop32(w, self.atoms.state).contains(&state)
    }

    /// (left, top, right, bottom) on the root window.
    fn rect(&self, w: Window) -> Option<(i32, i32, i32, i32)> {
        let g = self.conn.get_geometry(w).ok()?.reply().ok()?;
        let at = self.conn.translate_coordinates(w, self.root, 0, 0).ok()?.reply().ok()?;
        let (l, t) = (i32::from(at.dst_x), i32::from(at.dst_y));
        Some((l, t, l + i32::from(g.width), t + i32::from(g.height)))
    }

    /// Every monitor as (left, top, right, bottom), and whether it is the main one.
    fn monitor_rects(&self) -> Vec<((i32, i32, i32, i32), bool)> {
        let found = self.conn.randr_get_monitors(self.root, true).ok().and_then(|c| c.reply().ok());
        let mut out: Vec<_> = found
            .map(|r| r.monitors)
            .unwrap_or_default()
            .into_iter()
            .map(|m| {
                let (l, t) = (i32::from(m.x), i32::from(m.y));
                ((l, t, l + i32::from(m.width), t + i32::from(m.height)), m.primary)
            })
            .collect();
        if out.is_empty() {
            out.push(((0, 0, i32::from(self.root_size.0), i32::from(self.root_size.1)), true));
        }
        out
    }

    fn describe(&self, w: Window) -> Option<WindowInfo> {
        let pid = self.pid(w);
        if pid == 0 {
            return None;
        }
        let exe_path = process_path(pid).unwrap_or_default();
        let exe = exe_name(&exe_path).to_string();
        let fullscreen = self.has_state(w, self.atoms.fullscreen)
            || self.rect(w).is_some_and(|r| {
                self.monitor_rects().iter().any(|(m, _)| r.0 <= m.0 && r.1 <= m.1 && r.2 >= m.2 && r.3 >= m.3)
            });
        Some(WindowInfo { hwnd: u64::from(w), title: self.title(w), exe, exe_path, pid, fullscreen })
    }
}

pub fn is_window(raw: u64) -> bool {
    let Some(x) = x() else { return false };
    let Ok(w) = Window::try_from(raw) else { return false };
    x.conn.get_window_attributes(w).ok().and_then(|c| c.reply().ok()).is_some()
}

/// Minimised windows send x11grab no new pictures.
pub fn is_minimized(raw: u64) -> bool {
    let (Some(x), Ok(w)) = (x(), Window::try_from(raw)) else { return false };
    x.has_state(w, x.atoms.hidden)
}

pub fn foreground() -> Option<WindowInfo> {
    let x = x()?;
    let active = *x.prop32(x.root, x.atoms.active).first()?;
    if active == 0 {
        return None;
    }
    x.describe(active)
}

/// Visible top-level windows a person would recognise as programs.
pub fn list_windows() -> Vec<WindowInfo> {
    let Some(x) = x() else { return Vec::new() };
    let own = std::process::id();
    x.prop32(x.root, x.atoms.clients)
        .into_iter()
        .filter(|&w| {
            let kinds = x.prop32(w, x.atoms.kind);
            (kinds.is_empty() || kinds.contains(&x.atoms.normal)) && !x.has_state(w, x.atoms.skip_taskbar) && !x.title(w).is_empty()
        })
        .filter_map(|w| x.describe(w))
        .filter(|w| w.pid != own && !w.exe.is_empty())
        .collect()
}

/// Where the windows of programs from `exes` (lower-case executable names) show on the
/// monitor with this index, by window. Minimised windows and those entirely behind one
/// solid window are left out.
pub fn excluded_areas(index: u32, exes: &[String], memo: &mut WindowMemo) -> Vec<(u64, Area)> {
    if exes.is_empty() {
        memo.windows.clear();
        return Vec::new();
    }
    let Some(x) = x() else { return Vec::new() };
    let monitors = x.monitor_rects();
    let Some(&(monitor, _)) = monitors.get(index as usize).or(monitors.first()) else { return Vec::new() };
    // Bottom to top in the list; the check wants the topmost first.
    let stacking = x.prop32(x.root, x.atoms.stacking);
    let seen = stacking.into_iter().rev().filter(|&w| !x.has_state(w, x.atoms.hidden)).filter_map(|w| {
        let rect = x.rect(w)?;
        let opacity = x.prop32(w, x.atoms.opacity).first().copied().unwrap_or(u32::MAX);
        Some(Seen { id: u64::from(w), pid: x.pid(w), rect, solid: opacity == u32::MAX })
    });
    areas_of(seen, monitor, exes, memo)
}

pub fn monitors() -> Vec<MonitorInfo> {
    let Some(x) = x() else { return Vec::new() };
    x.monitor_rects()
        .into_iter()
        .enumerate()
        .map(|(i, ((l, t, r, b), primary))| MonitorInfo { index: i as u32, width: r - l, height: b - t, primary })
        .collect()
}

/// x11grab keeps sending pictures while the screen is still.
pub fn steady_screen(_index: u32) -> bool {
    true
}

/// x11grab records a window by itself, not the screen it is on.
pub fn screen_of_window(_raw: u64) -> Option<u32> {
    None
}

/// The videos folder as the desktop names it ("Vidéos", "Видео"), from user-dirs.dirs,
/// else ~/Videos.
pub fn videos_dir() -> PathBuf {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| ".".into());
    xdg_videos_dir(&home).unwrap_or_else(|| home.join("Videos"))
}

fn xdg_videos_dir(home: &Path) -> Option<PathBuf> {
    let config = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or_else(|| home.join(".config"));
    let text = std::fs::read_to_string(config.join("user-dirs.dirs")).ok()?;
    let line = text.lines().find_map(|l| l.trim().strip_prefix("XDG_VIDEOS_DIR="))?;
    let value = line.trim_matches('"').replace("$HOME", &home.to_string_lossy());
    // A videos folder that is the home folder itself means the user turned it off.
    let path = PathBuf::from(value);
    (path.is_absolute() && path != home).then_some(path)
}

fn display() -> String {
    std::env::var("DISPLAY").ok().filter(|d| !d.is_empty()).unwrap_or_else(|| ":0".into())
}

/// The FFmpeg input for a window or monitor; `chain` is added to its filters.
pub fn capture(grab: Grab, fps: u32, cursor: bool, chain: &str) -> Capture {
    let mut input: Vec<String> = ["-thread_queue_size", "512", "-f", "x11grab"].map(String::from).to_vec();
    input.extend(["-framerate".into(), fps.to_string(), "-draw_mouse".into(), u8::from(cursor).to_string()]);
    match grab {
        Grab::Window(id) => {
            input.extend(["-window_id".into(), format!("0x{id:x}"), "-i".into(), display()]);
        }
        Grab::Monitor(index) => {
            let rects = x().map(X::monitor_rects).unwrap_or_default();
            let ((l, t, r, b), _) = rects.get(index as usize).or(rects.first()).copied().unwrap_or(((0, 0, 1920, 1080), true));
            input.extend(["-video_size".into(), format!("{}x{}", r - l, b - t), "-i".into(), format!("{}+{l},{t}", display())]);
        }
    }
    // Even sizes: several encoders reject odd widths.
    Capture { input, filter: Some(format!("scale=trunc(iw/2)*2:trunc(ih/2)*2{chain}")) }
}

/// A test picture as an encoder gets it while recording, for trying encoders.
pub fn test_capture(chain: &str) -> Capture {
    Capture {
        input: ["-f", "lavfi", "-i", "testsrc2=size=1280x720:rate=30"].map(String::from).to_vec(),
        filter: Some(format!("null{chain}")),
    }
}

/// The program behind a process. Games under Wine and Proton are named by their Windows
/// executable, which is what a person recognises (and what Windows would show).
pub fn process_path(pid: u32) -> Option<String> {
    let cmdline = std::fs::read(format!("/proc/{pid}/cmdline")).ok();
    if let Some(first) = cmdline.as_deref().and_then(|c| c.split(|&b| b == 0).next()) {
        let first = String::from_utf8_lossy(first);
        if first.to_lowercase().ends_with(".exe") {
            return Some(first.into_owned());
        }
    }
    let link = std::fs::read_link(format!("/proc/{pid}/exe")).ok()?;
    Some(link.to_string_lossy().trim_end_matches(" (deleted)").to_string())
}

/// pid, parent pid and short name from /proc/<pid>/stat.
fn stat(pid: u32) -> Option<(u32, String, Vec<String>)> {
    let text = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // The name sits in brackets and may itself hold spaces and brackets.
    let (open, close) = (text.find('(')?, text.rfind(')')?);
    let comm = text[open + 1..close].to_string();
    let rest: Vec<String> = text[close + 1..].split_whitespace().map(String::from).collect();
    let parent = rest.get(1)?.parse().ok()?;
    Some((parent, comm, rest))
}

/// Every running process by pid.
pub fn processes() -> HashMap<u32, ProcessEntry> {
    let mut out = HashMap::new();
    let Ok(entries) = std::fs::read_dir("/proc") else { return out };
    for entry in entries.flatten() {
        let Some(pid) = entry.file_name().to_str().and_then(|n| n.parse::<u32>().ok()) else { continue };
        let Some((parent, comm, _)) = stat(pid) else { continue };
        let exe = process_path(pid).map(|p| exe_name(&p).to_string()).filter(|e| !e.is_empty()).unwrap_or(comm);
        out.insert(pid, ProcessEntry { pid, parent, exe_lower: exe.to_lowercase(), exe });
    }
    out
}

/// No version resource on Linux: the file name stands for the program.
pub(super) fn file_description(_path: &str) -> Option<String> {
    None
}

/// Each graphics card with its driver, to notice new hardware or drivers.
pub fn gpu_signature() -> Vec<String> {
    let mut out = Vec::new();
    let read = |p: String| std::fs::read_to_string(p).map(|s| s.trim().to_string()).unwrap_or_default();
    if let Ok(cards) = std::fs::read_dir("/sys/class/drm") {
        let mut names: Vec<String> =
            cards.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| n.starts_with("card") && !n.contains('-')).collect();
        names.sort();
        for card in names {
            let dev = format!("/sys/class/drm/{card}/device");
            let driver = std::fs::read_link(format!("{dev}/driver"))
                .ok()
                .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
                .unwrap_or_default();
            out.push(format!("{} {}:{} {driver}", card, read(format!("{dev}/vendor")), read(format!("{dev}/device"))));
        }
    }
    out.push(format!("nvidia {}", read("/sys/module/nvidia/version".into())));
    out.push(format!("kernel {}", read("/proc/sys/kernel/osrelease".into())));
    out
}

/// Lets the game have the CPU first: for threads doing work nobody waits for.
pub fn lower_thread_priority() {
    // On Linux the nice value belongs to the thread.
    // SAFETY: plain system calls on the calling thread.
    unsafe {
        let tid = libc::syscall(libc::SYS_gettid) as libc::id_t;
        libc::setpriority(libc::PRIO_PROCESS, tid, 10);
    }
}

/// CPU time a process has used so far, in user and kernel mode together.
pub fn cpu_time(child: &std::process::Child) -> Option<Duration> {
    let (_, _, rest) = stat(child.id())?;
    // utime and stime are fields 14 and 15 of the whole line; `rest` starts at field 3.
    let ticks: u64 = rest.get(11)?.parse::<u64>().ok()? + rest.get(12)?.parse::<u64>().ok()?;
    // SAFETY: sysconf only reads a value.
    let per_second = unsafe { libc::sysconf(libc::_SC_CLK_TCK) }.max(1) as u64;
    Some(Duration::from_nanos(ticks * 1_000_000_000 / per_second))
}

/// Ties a helper process to Replix's lifetime: the kernel ends it when Replix ends, however
/// Replix ends. Set up before the start, since that is the only time it can be.
pub fn prepare_tied(cmd: &mut Command) {
    let parent = std::process::id();
    // SAFETY: prctl, getppid and _exit are async-signal-safe.
    unsafe {
        cmd.pre_exec(move || {
            libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL);
            // Replix ended between the fork and the prctl: nothing would end this one.
            if libc::getppid() as u32 != parent {
                libc::_exit(0);
            }
            Ok(())
        });
    }
}

/// Done in `prepare_tied` already.
pub fn tie_to_app(_child: &std::process::Child) {}

/// A program's icon as RGBA pixels (width, height, pixels): the theme icon named after the
/// program, where there is one.
pub fn icon_rgba(path: &str, size: u32) -> Option<(u32, u32, Vec<u8>)> {
    let name = exe_name(path).trim_end_matches(".exe").to_lowercase();
    if name.is_empty() {
        return None;
    }
    let home = std::env::var("HOME").unwrap_or_default();
    let mut sizes = [16u32, 24, 32, 48, 64, 128, 256].to_vec();
    // The closest size at or above the one asked for first.
    sizes.sort_by_key(|s| if *s >= size { s - size } else { 1000 + size - s });
    let roots = [format!("{home}/.local/share/icons/hicolor"), "/usr/share/icons/hicolor".into()];
    let mut candidates: Vec<String> = Vec::new();
    for s in &sizes {
        for root in &roots {
            candidates.push(format!("{root}/{s}x{s}/apps/{name}.png"));
        }
    }
    candidates.push(format!("/usr/share/pixmaps/{name}.png"));
    let bytes = candidates.iter().find_map(|c| std::fs::read(c).ok())?;
    let image = tauri::image::Image::from_bytes(&bytes).ok()?;
    Some((image.width(), image.height(), image.rgba().to_vec()))
}
