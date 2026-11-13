//! Everything Replix needs from the operating system: windows, processes, monitors, how
//! FFmpeg grabs the picture, and small things like free disk space.
//!
//! The rest of the app only talks to this module. Each system has its own file with the
//! same functions (`windows.rs`, `linux.rs`, `macos.rs`); what they share lives here.

use std::collections::HashMap;

use serde::Serialize;

pub mod bin;
#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

#[cfg(target_os = "linux")]
pub use self::linux::*;
#[cfg(target_os = "macos")]
pub use self::macos::*;
#[cfg(unix)]
pub use self::unix::*;
#[cfg(windows)]
pub use self::windows::*;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowInfo {
    /// The system's handle for the window: an HWND, an X11 window or a Quartz window number.
    pub hwnd: u64,
    pub title: String,
    pub exe: String,
    #[serde(skip)]
    pub exe_path: String,
    pub pid: u32,
    pub fullscreen: bool,
}

/// A hotkey to ask the desktop for: what it is called, what it does, and the keys Replix
/// would like for it ("Alt+C").
#[derive(Debug, Clone, PartialEq)]
pub struct DesktopHotkey {
    pub id: String,
    pub description: String,
    pub keys: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MonitorInfo {
    pub index: u32,
    pub width: i32,
    pub height: i32,
    pub primary: bool,
}

/// Part of a monitor, as fractions of its width and height, so it fits the picture at
/// whatever size it is recorded.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Area {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

impl Area {
    /// The smallest area holding both.
    pub fn union(self, other: Area) -> Area {
        Area {
            left: self.left.min(other.left),
            top: self.top.min(other.top),
            right: self.right.max(other.right),
            bottom: self.bottom.max(other.bottom),
        }
    }
}

/// What one look at the screen learnt about windows, so the next look opens no process it
/// already knows: the screen is looked at twice a second.
#[derive(Default)]
pub struct WindowMemo {
    exes: Vec<String>,
    /// Window handle → its process, and whether that process is kept out of recordings.
    windows: HashMap<u64, (u32, bool)>,
}

pub struct ProcessEntry {
    pub pid: u32,
    pub parent: u32,
    pub exe: String,
    /// `exe` in lower case, made once per snapshot for comparing names.
    pub exe_lower: String,
}

/// What FFmpeg records: one window, or one monitor by its index in `monitors()`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Grab {
    Window(u64),
    Monitor(u32),
}

/// FFmpeg arguments for a picture source.
#[derive(Debug, Clone, PartialEq)]
pub struct Capture {
    /// Input options and `-i`, placed before every other input.
    pub input: Vec<String>,
    /// Filters for the picture on the output side (`-vf`), where the system's capture is
    /// an input device rather than a filter source. `None` when they are part of `input`.
    pub filter: Option<String>,
}

/// The file name of a program from its path, for either kind of separator: games under
/// Wine report Windows paths on Linux.
pub fn exe_name(path: &str) -> &str {
    path.rsplit(['\\', '/']).next().unwrap_or(path)
}

/// The outermost process with the same executable, so a capture includes all of an
/// app's helper processes (Discord and browsers play sound from child processes).
pub fn root_of(pid: u32, all: &HashMap<u32, ProcessEntry>) -> u32 {
    // Embedded browsers play sound on behalf of the app that hosts them.
    const HELPER: &str = "msedgewebview2.exe";
    let mut current = pid;
    for _ in 0..16 {
        let Some(entry) = all.get(&current) else {
            break;
        };
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

/// "Discord" for Discord.exe: the name the program gives itself, or the file name without
/// extension.
pub fn app_name(exe_path: &str) -> String {
    let file = exe_name(exe_path);
    let stem = match file.rsplit_once('.') {
        Some((stem, ext)) if ext.eq_ignore_ascii_case("exe") => stem,
        _ => file,
    };
    file_description(exe_path)
        .filter(|d| !d.trim().is_empty() && d.len() <= 40)
        .unwrap_or_else(|| capitalise(stem))
}

fn capitalise(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

/// Programs that are never games, even when they cover the screen, by lower-case
/// executable name on every system.
const NOT_GAMES: &[&str] = &[
    // Windows
    "explorer.exe",
    "searchhost.exe",
    "startmenuexperiencehost.exe",
    "shellexperiencehost.exe",
    "applicationframehost.exe",
    "textinputhost.exe",
    "lockapp.exe",
    "chrome.exe",
    "msedge.exe",
    "firefox.exe",
    "brave.exe",
    "opera.exe",
    "opera_gx.exe",
    "vivaldi.exe",
    "arc.exe",
    "zen.exe",
    "discord.exe",
    "spotify.exe",
    "vlc.exe",
    "mpv.exe",
    "mpc-hc64.exe",
    "potplayermini64.exe",
    "wmplayer.exe",
    "code.exe",
    "devenv.exe",
    "obs64.exe",
    "steam.exe",
    "steamwebhelper.exe",
    "epicgameslauncher.exe",
    "battle.net.exe",
    "riotclientux.exe",
    "riotclientservices.exe",
    "eadesktop.exe",
    "ubisoftconnect.exe",
    "galaxyclient.exe",
    "powerpnt.exe",
    "winword.exe",
    "excel.exe",
    "teams.exe",
    "ms-teams.exe",
    "zoom.exe",
    "slack.exe",
    "windowsterminal.exe",
    "claude.exe",
    "replix.exe",
    "ffmpeg.exe",
    "photos.exe",
    "microsoft.media.player.exe",
    // Linux
    "firefox",
    "firefox-bin",
    "chrome",
    "chromium",
    "brave",
    "opera",
    "vivaldi-bin",
    "zen",
    "discord",
    "vesktop",
    "spotify",
    "vlc",
    "mpv",
    "totem",
    "celluloid",
    "code",
    "codium",
    "obs",
    "steam",
    "steamwebhelper",
    "heroic",
    "lutris",
    "nautilus",
    "dolphin",
    "thunar",
    "nemo",
    "gnome-shell",
    "plasmashell",
    "kwin_x11",
    "kwin_wayland",
    "xfdesktop",
    "xfwm4",
    "gnome-terminal-server",
    "konsole",
    "kitty",
    "alacritty",
    "wezterm-gui",
    "slack",
    "zoom",
    "teams-for-linux",
    "thunderbird",
    "soffice.bin",
    "eog",
    "gwenview",
    "xorg",
    "xwayland",
    "replix",
    "ffmpeg",
    // macOS
    "finder",
    "dock",
    "safari",
    "google chrome",
    "microsoft edge",
    "brave browser",
    "arc",
    "zen browser",
    "opera",
    "steam_osx",
    "steam helper",
    "electron",
    "terminal",
    "iterm2",
    "quicktime player",
    "windowserver",
    "systemuiserver",
    "controlcenter",
    "loginwindow",
    "zoom.us",
    "microsoft teams",
    "preview",
    "photos",
    "tv",
    "music",
];

/// Folders games are installed into by the common stores, with `/` for separators.
const GAME_DIRS: &[&str] = &[
    "/steamapps/common/",
    "/epic games/",
    "/riot games/",
    "/xboxgames/",
    "/gog galaxy/games/",
    "/gog games/",
    "/ubisoft game launcher/games/",
    "/ea games/",
    "/rockstar games/",
    "/amazon games/",
    "/itch/apps/",
    "/battle.net/games/",
    "/heroic/",
    "/lutris/",
    "/.local/share/games/",
];

fn normalised(path: &str) -> String {
    path.to_lowercase().replace('\\', "/")
}

pub fn looks_like_game(w: &WindowInfo) -> bool {
    let exe = w.exe.to_lowercase();
    if NOT_GAMES.contains(&exe.as_str()) || w.pid == std::process::id() {
        return false;
    }
    let path = normalised(&w.exe_path);
    GAME_DIRS.iter().any(|d| path.contains(d)) || w.fullscreen
}

/// The name shown for a game: its window title when that is short, else the
/// store folder or the executable's description.
pub fn game_name(w: &WindowInfo) -> String {
    let title = w.title.trim();
    if !title.is_empty() && title.chars().count() <= 48 {
        return title.to_string();
    }
    const STEAM: &str = "/steamapps/common/";
    if let Some(pos) = normalised(&w.exe_path).find(STEAM) {
        // Lower-casing and swapping separators keep every byte where it was.
        let rest = &w.exe_path[pos + STEAM.len()..];
        if let Some(folder) = rest.split(['\\', '/']).next().filter(|f| !f.is_empty()) {
            return folder.to_string();
        }
    }
    app_name(&w.exe_path)
}

/// A window as the privacy check sees it. Rectangles are (left, top, right, bottom) in the
/// same coordinates as the monitor's.
pub(crate) struct Seen {
    pub id: u64,
    pub pid: u32,
    pub rect: (i32, i32, i32, i32),
    /// Whether nothing behind the window shows through it.
    pub solid: bool,
}

/// Where the windows of programs from `exes` show on a monitor, given every window on it,
/// topmost first. Only the parts not covered by solid windows in front are hidden.
/// Several rectangles can belong to the same window.
pub(crate) fn areas_of(
    windows: impl IntoIterator<Item = Seen>,
    monitor: (i32, i32, i32, i32),
    exes: &[String],
    memo: &mut WindowMemo,
) -> Vec<(u64, Area)> {
    if memo.exes != exes {
        memo.exes = exes.to_vec();
        memo.windows.clear();
    }
    let (ml, mt, mr, mb) = monitor;
    let (width, height) = ((mr - ml) as f32, (mb - mt) as f32);
    if width <= 0.0 || height <= 0.0 {
        return Vec::new();
    }
    let own = std::process::id();
    let mut known: HashMap<u64, (u32, bool)> = HashMap::with_capacity(memo.windows.len());
    let mut above: Vec<(i32, i32, i32, i32)> = Vec::new();
    let mut out = Vec::new();
    for w in windows {
        let r = (
            w.rect.0.max(ml),
            w.rect.1.max(mt),
            w.rect.2.min(mr),
            w.rect.3.min(mb),
        );
        if r.2 <= r.0 || r.3 <= r.1 {
            continue;
        }
        let remembered = memo
            .windows
            .get(&w.id)
            .filter(|(p, _)| *p == w.pid)
            .map(|(_, kept)| *kept);
        let excluded = remembered.unwrap_or_else(|| {
            w.pid != own
                && known
                    .values()
                    .find(|(p, _)| *p == w.pid)
                    .map(|(_, kept)| *kept)
                    .unwrap_or_else(|| {
                        process_path(w.pid)
                            .is_some_and(|p| exes.contains(&exe_name(&p).to_lowercase()))
                    })
        });
        known.insert(w.id, (w.pid, excluded));
        if excluded {
            for (left, top, right, bottom) in uncovered(r, &above) {
                out.push((
                    w.id,
                    Area {
                        left: (left - ml) as f32 / width,
                        top: (top - mt) as f32 / height,
                        right: (right - ml) as f32 / width,
                        bottom: (bottom - mt) as f32 / height,
                    },
                ));
            }
        } else if w.solid {
            above.push(r);
        }
    }
    memo.windows = known;
    out
}

/// Subtracts the solid windows in front without joining the pieces across their openings.
fn uncovered(
    rect: (i32, i32, i32, i32),
    above: &[(i32, i32, i32, i32)],
) -> Vec<(i32, i32, i32, i32)> {
    let mut parts = vec![rect];
    for &(al, at, ar, ab) in above {
        let mut next = Vec::new();
        for (l, t, r, b) in parts {
            let (il, it, ir, ib) = (l.max(al), t.max(at), r.min(ar), b.min(ab));
            if il >= ir || it >= ib {
                next.push((l, t, r, b));
                continue;
            }
            if t < it {
                next.push((l, t, r, it));
            }
            if ib < b {
                next.push((l, ib, r, b));
            }
            if l < il {
                next.push((l, it, il, ib));
            }
            if ir < r {
                next.push((ir, it, r, ib));
            }
        }
        parts = next;
        if parts.is_empty() {
            break;
        }
    }
    parts
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(exe: &str, path: &str, fullscreen: bool) -> WindowInfo {
        WindowInfo {
            hwnd: 1,
            title: String::new(),
            exe: exe.into(),
            exe_path: path.into(),
            pid: 1,
            fullscreen,
        }
    }

    fn seen(id: u64, rect: (i32, i32, i32, i32), solid: bool) -> Seen {
        Seen { id, pid: id as u32 + 100, rect, solid }
    }

    fn privacy_memo() -> (Vec<String>, WindowMemo) {
        let exes = vec!["private".into()];
        let memo = WindowMemo {
            exes: exes.clone(),
            windows: HashMap::from([(1, (101, true)), (2, (102, false)), (3, (103, false))]),
        };
        (exes, memo)
    }

    #[test]
    fn a_foreground_window_stays_visible_over_a_private_background() {
        let (exes, mut memo) = privacy_memo();
        let windows = [seen(2, (25, 25, 75, 75), true), seen(1, (0, 0, 100, 100), true)];
        let areas = areas_of(windows, (0, 0, 100, 100), &exes, &mut memo);
        assert_eq!(areas, vec![
            (1, Area { left: 0.0, top: 0.0, right: 1.0, bottom: 0.25 }),
            (1, Area { left: 0.0, top: 0.75, right: 1.0, bottom: 1.0 }),
            (1, Area { left: 0.0, top: 0.25, right: 0.25, bottom: 0.75 }),
            (1, Area { left: 0.75, top: 0.25, right: 1.0, bottom: 0.75 }),
        ]);
    }

    #[test]
    fn several_foreground_windows_can_fully_cover_a_private_one() {
        let (exes, mut memo) = privacy_memo();
        let windows = [
            seen(2, (0, 0, 50, 100), true),
            seen(3, (50, 0, 100, 100), true),
            seen(1, (0, 0, 100, 100), true),
        ];
        assert!(areas_of(windows, (0, 0, 100, 100), &exes, &mut memo).is_empty());
    }

    #[test]
    fn transparent_windows_do_not_expose_private_content_behind_them() {
        let (exes, mut memo) = privacy_memo();
        let windows = [seen(2, (0, 0, 100, 100), false), seen(1, (0, 0, 100, 100), true)];
        assert_eq!(areas_of(windows, (0, 0, 100, 100), &exes, &mut memo), vec![
            (1, Area { left: 0.0, top: 0.0, right: 1.0, bottom: 1.0 }),
        ]);
    }

    #[test]
    fn windows_behind_a_private_window_do_not_cut_it_out() {
        let (exes, mut memo) = privacy_memo();
        let windows = [seen(1, (25, 25, 75, 75), true), seen(2, (0, 0, 100, 100), true)];
        assert_eq!(areas_of(windows, (0, 0, 100, 100), &exes, &mut memo), vec![
            (1, Area { left: 0.25, top: 0.25, right: 0.75, bottom: 0.75 }),
        ]);
    }

    #[test]
    fn privacy_areas_are_clipped_and_relative_to_the_recorded_monitor() {
        let (exes, mut memo) = privacy_memo();
        let windows = [seen(2, (-100, 0, -50, 100), true), seen(1, (-200, -20, 100, 120), true)];
        assert_eq!(areas_of(windows, (-100, 0, 0, 100), &exes, &mut memo), vec![
            (1, Area { left: 0.5, top: 0.0, right: 1.0, bottom: 1.0 }),
        ]);
    }

    #[test]
    fn subtraction_preserves_exactly_the_uncovered_pixels() {
        let rect = (0, 0, 20, 20);
        let above = [(-5, 3, 7, 12), (4, 7, 16, 15), (8, -4, 13, 5), (15, 10, 30, 25), (20, 0, 25, 20)];
        let parts = uncovered(rect, &above);
        let contains = |r: &(i32, i32, i32, i32), x, y| r.0 <= x && x < r.2 && r.1 <= y && y < r.3;
        assert!(parts.iter().all(|r| r.0 >= 0 && r.1 >= 0 && r.2 <= 20 && r.3 <= 20 && r.0 < r.2 && r.1 < r.3));
        for y in 0..20 {
            for x in 0..20 {
                let count = parts.iter().filter(|r| contains(r, x, y)).count();
                let expected = usize::from(!above.iter().any(|r| contains(r, x, y)));
                assert_eq!(count, expected, "pixel ({x}, {y}) is covered exactly once or left open");
            }
        }
    }

    #[test]
    fn games_are_spotted_on_every_system() {
        assert!(looks_like_game(&window(
            "cs2.exe",
            r"C:\Steam\steamapps\common\Counter-Strike Global Offensive\game\bin\win64\cs2.exe",
            false
        )));
        assert!(looks_like_game(&window("cs2", "/home/a/.local/share/Steam/steamapps/common/Counter-Strike Global Offensive/game/bin/linuxsteamrt64/cs2", false)));
        assert!(looks_like_game(&window(
            "Game.exe",
            r"Z:\home\a\Games\Heroic\Game\Game.exe",
            false
        )));
        assert!(looks_like_game(&window("anything", "/opt/anything", true)));
        assert!(!looks_like_game(&window(
            "firefox",
            "/usr/lib/firefox/firefox",
            true
        )));
        assert!(!looks_like_game(&window(
            "Finder",
            "/System/Library/CoreServices/Finder.app/Contents/MacOS/Finder",
            true
        )));
    }

    #[test]
    fn a_steam_game_is_named_after_its_folder() {
        let mut w = window(
            "cs2",
            "/home/a/.steam/steam/steamapps/common/Counter-Strike Global Offensive/game/cs2",
            false,
        );
        w.title = "x".repeat(60);
        assert_eq!(game_name(&w), "Counter-Strike Global Offensive");
        w.exe_path = r"D:\SteamLibrary\steamapps\common\Hades\Hades.exe".into();
        assert_eq!(game_name(&w), "Hades");
    }

    #[test]
    fn exe_names_come_from_either_kind_of_path() {
        assert_eq!(exe_name(r"C:\Games\Game.exe"), "Game.exe");
        assert_eq!(exe_name("/usr/bin/discord"), "discord");
        assert_eq!(exe_name("plain"), "plain");
    }
}
