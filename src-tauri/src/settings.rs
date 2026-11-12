use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::encoder::Codec;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CaptureSource {
    /// Follow whatever game is running; record nothing while no game is detected.
    Game,
    /// One monitor, always.
    Screen,
    /// One program, picked by its executable name.
    App,
}

/// The screen corner notifications appear in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ToastPosition {
    TopLeft,
    #[default]
    TopRight,
    BottomLeft,
    BottomRight,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ToastSize {
    Small,
    #[default]
    Medium,
    Large,
}

impl ToastSize {
    pub fn scale(self) -> f64 {
        match self {
            Self::Small => 0.85,
            Self::Medium => 1.0,
            Self::Large => 1.25,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Quality {
    Low,
    Medium,
    High,
}

impl Quality {
    pub fn video_kbps(self) -> u32 {
        match self {
            Quality::Low => 6_000,
            Quality::Medium => 12_000,
            Quality::High => 24_000,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct Hotkeys {
    pub clip: String,
    pub screenshot: String,
    pub session: String,
    /// Marks a moment while recording, to find it again in the editor.
    pub marker: String,
}

impl Default for Hotkeys {
    fn default() -> Self {
        Self { clip: "Alt+C".into(), screenshot: "Alt+S".into(), session: "Alt+X".into(), marker: "Alt+M".into() }
    }
}

impl Hotkeys {
    pub fn all(&self) -> [&String; 4] {
        [&self.clip, &self.screenshot, &self.session, &self.marker]
    }
}

/// A program that never ends up in a recording: not in the picture, not in the sound.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct Excluded {
    /// Executable name, e.g. "KeePassXC.exe"; compared ignoring case.
    pub exe: String,
    pub name: String,
    /// The program's full path, for its icon.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    /// Recording on or off as a whole.
    pub enabled: bool,
    pub buffer_seconds: u32,
    pub source: CaptureSource,
    /// Executable name for `CaptureSource::App`, e.g. "obs64.exe".
    pub app_exe: Option<String>,
    pub monitor: u32,
    pub fps: u32,
    pub quality: Quality,
    /// Output height; 0 keeps the captured size.
    pub height: u32,
    /// "auto" or an encoder id from `encoder::candidates`.
    pub encoder: String,
    /// The format recordings are written in. HEVC is used where a graphics card can encode
    /// it; otherwise recordings stay H.264.
    pub codec: Codec,
    pub record_mic: bool,
    /// The microphone's id (WASAPI on Windows, the PulseAudio source on Linux, the
    /// AVFoundation name on macOS); `None` follows the system default.
    pub mic_device: Option<String>,
    /// 1.0 is the microphone as the system delivers it.
    pub mic_volume: f32,
    /// Filter background noise out of the microphone.
    pub mic_noise_suppression: bool,
    pub capture_cursor: bool,
    pub clips_dir: PathBuf,
    /// 0 means no limit.
    pub storage_limit_gb: u32,
    pub hotkeys: Hotkeys,
    pub accent: String,
    /// Background colours, by id: "plum", "midnight", "graphite" and so on.
    pub theme: String,
    pub launch_at_startup: bool,
    pub close_to_tray: bool,
    pub show_toast: bool,
    /// A short sound when a hotkey saves something, or does nothing.
    pub save_sound: bool,
    pub toast_position: ToastPosition,
    pub toast_size: ToastSize,
    /// Games and apps with their own settings.
    pub profiles: Vec<Profile>,
    /// Programs Replix keeps out of every recording.
    pub excluded: Vec<Excluded>,
    /// "auto" follows the system; otherwise a code from `i18n::LANGUAGES`.
    pub language: String,
    /// The first-start setup was finished or skipped.
    pub onboarded: bool,
    /// How loud each kind of track, or one program's track, starts out in the editor.
    /// Only the page reads them; a clip's own draft always wins.
    pub track_defaults: Vec<TrackDefault>,
}

/// How a track starts out in the editor before anything is changed there.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct TrackDefault {
    /// "game", "app", "desktop" or "mic" for a kind of track, or a program's executable
    /// name in lower case for that program's own track.
    pub key: String,
    /// Shown in the settings for a program.
    pub name: String,
    /// The program's full path, for its icon.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// Heard at all; off starts the track muted.
    pub enabled: bool,
    /// 1.0 is the track as recorded.
    pub volume: f32,
}

impl Default for TrackDefault {
    fn default() -> Self {
        Self { key: String::new(), name: String::new(), path: None, enabled: true, volume: 1.0 }
    }
}

/// One game or app's own settings. `None` keeps the general setting.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct Profile {
    /// Executable name, e.g. "VALORANT-Win64-Shipping.exe"; compared ignoring case.
    pub exe: String,
    /// Shown in the settings.
    pub name: String,
    /// The program's full path, for its icon; learned when it is added.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// Never record this program, even when it looks like a game.
    pub ignore: bool,
    pub buffer_seconds: Option<u32>,
    pub fps: Option<u32>,
    pub quality: Option<Quality>,
    pub record_mic: Option<bool>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: true,
            buffer_seconds: 30,
            source: CaptureSource::Game,
            app_exe: None,
            monitor: 0,
            fps: 60,
            quality: Quality::Medium,
            height: 0,
            encoder: "auto".into(),
            codec: Codec::H264,
            record_mic: true,
            mic_device: None,
            mic_volume: 1.0,
            mic_noise_suppression: true,
            capture_cursor: false,
            clips_dir: default_clips_dir(),
            storage_limit_gb: 0,
            hotkeys: Hotkeys::default(),
            accent: "#C5F26B".into(),
            theme: "plum".into(),
            launch_at_startup: false,
            close_to_tray: true,
            show_toast: true,
            save_sound: true,
            toast_position: ToastPosition::default(),
            toast_size: ToastSize::default(),
            profiles: Vec::new(),
            excluded: Vec::new(),
            language: "auto".into(),
            onboarded: false,
            track_defaults: Vec::new(),
        }
    }
}

/// Videos\Replix on Windows, ~/Videos/Replix on Linux, ~/Movies/Replix on macOS.
fn default_clips_dir() -> PathBuf {
    crate::platform::videos_dir().join("Replix")
}

impl Settings {
    /// The saved settings, or the defaults. A file that cannot be read as settings is kept
    /// next to it rather than overwritten by the next save, so nothing is lost for good.
    pub fn load(path: &Path) -> Self {
        let Ok(text) = std::fs::read_to_string(path) else { return Self::default() };
        match serde_json::from_str(&text) {
            Ok(settings) => settings,
            Err(e) => {
                let kept = path.with_extension(format!("json.broken-{}", chrono::Local::now().format("%Y%m%d-%H%M%S")));
                log::warn!("settings could not be read ({e}); kept as {}", kept.display());
                let _ = std::fs::rename(path, &kept);
                Self::default()
            }
        }
    }

    /// Written next to the old file and renamed over it: a crash or a full drive halfway
    /// through leaves the old settings as they were, never half a file.
    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let temp = path.with_extension("json.tmp");
        std::fs::write(&temp, serde_json::to_string_pretty(self)?)?;
        if let Err(e) = std::fs::rename(&temp, path) {
            let _ = std::fs::remove_file(&temp);
            return Err(e.into());
        }
        Ok(())
    }

    /// Whether a program is kept out of recordings.
    pub fn is_excluded(&self, exe: &str) -> bool {
        self.excluded.iter().any(|e| e.exe.eq_ignore_ascii_case(exe))
    }

    /// The executable names kept out of recordings, in lower case.
    pub fn excluded_exes(&self) -> Vec<String> {
        self.excluded.iter().map(|e| e.exe.to_lowercase()).collect()
    }

    pub fn profile(&self, exe: &str) -> Option<&Profile> {
        self.profiles.iter().find(|p| p.exe.eq_ignore_ascii_case(exe))
    }

    /// The general settings with a program's own choices on top.
    pub fn for_exe(&self, exe: Option<&str>) -> Settings {
        let mut s = self.clone();
        if let Some(p) = exe.and_then(|e| self.profile(e)) {
            s.buffer_seconds = p.buffer_seconds.unwrap_or(s.buffer_seconds);
            s.fps = p.fps.unwrap_or(s.fps);
            s.quality = p.quality.unwrap_or(s.quality);
            s.record_mic = p.record_mic.unwrap_or(s.record_mic);
        }
        s
    }

    /// Whether a change needs the capture pipeline to restart.
    pub fn capture_differs(&self, other: &Settings) -> bool {
        self.enabled != other.enabled
            || self.source != other.source
            || self.app_exe != other.app_exe
            || self.monitor != other.monitor
            || self.fps != other.fps
            || self.quality != other.quality
            || self.height != other.height
            || self.encoder != other.encoder
            || self.codec != other.codec
            || self.record_mic != other.record_mic
            || self.mic_device != other.mic_device
            || self.capture_cursor != other.capture_cursor
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("replix-settings-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("settings.json")
    }

    /// Settings from before markers, exclusions and languages load with the new defaults.
    #[test]
    fn older_settings_get_the_new_defaults() {
        let path = temp("old");
        std::fs::write(&path, r#"{"bufferSeconds":60,"hotkeys":{"clip":"F8"}}"#).unwrap();
        let s = Settings::load(&path);
        assert_eq!(s.buffer_seconds, 60);
        assert_eq!(s.hotkeys.clip, "F8");
        assert_eq!(s.hotkeys.marker, "Alt+M");
        assert_eq!(s.language, "auto");
        assert!(s.excluded.is_empty());
    }

    /// A file that is not settings is kept aside, not overwritten by the next save.
    #[test]
    fn a_broken_file_is_kept_aside() {
        let path = temp("broken");
        std::fs::write(&path, "{ not json").unwrap();
        let s = Settings::load(&path);
        assert_eq!(s, Settings::default());
        assert!(!path.exists());
        let kept = std::fs::read_dir(path.parent().unwrap()).unwrap().flatten().any(|e| e.file_name().to_string_lossy().contains("broken-"));
        assert!(kept);
        s.save(&path).unwrap();
        assert_eq!(Settings::load(&path), s);
    }

    #[test]
    fn exclusions_ignore_case() {
        let s = Settings { excluded: vec![Excluded { exe: "KeePassXC.exe".into(), ..Default::default() }], ..Default::default() };
        assert!(s.is_excluded("keepassxc.EXE"));
        assert!(!s.is_excluded("game.exe"));
        assert_eq!(s.excluded_exes(), vec!["keepassxc.exe".to_string()]);
    }
}
