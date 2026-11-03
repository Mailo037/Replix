use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

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
}

impl Default for Hotkeys {
    fn default() -> Self {
        Self { clip: "Alt+C".into(), screenshot: "Alt+S".into(), session: "Alt+X".into() }
    }
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
    pub record_mic: bool,
    /// WASAPI id of the microphone; `None` follows the Windows default.
    pub mic_device: Option<String>,
    /// 1.0 is the microphone as Windows delivers it.
    pub mic_volume: f32,
    /// Filter background noise out of the microphone.
    pub mic_noise_suppression: bool,
    pub capture_cursor: bool,
    pub clips_dir: PathBuf,
    /// 0 means no limit.
    pub storage_limit_gb: u32,
    pub hotkeys: Hotkeys,
    pub accent: String,
    pub launch_at_startup: bool,
    pub close_to_tray: bool,
    pub show_toast: bool,
    /// Games and apps with their own settings.
    pub profiles: Vec<Profile>,
    /// The first-start setup was finished or skipped.
    pub onboarded: bool,
}

/// One game or app's own settings. `None` keeps the general setting.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct Profile {
    /// Executable name, e.g. "VALORANT-Win64-Shipping.exe"; compared ignoring case.
    pub exe: String,
    /// Shown in the settings.
    pub name: String,
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
            record_mic: true,
            mic_device: None,
            mic_volume: 1.0,
            mic_noise_suppression: true,
            capture_cursor: false,
            clips_dir: default_clips_dir(),
            storage_limit_gb: 0,
            hotkeys: Hotkeys::default(),
            accent: "#FF9A3C".into(),
            launch_at_startup: false,
            close_to_tray: true,
            show_toast: true,
            profiles: Vec::new(),
            onboarded: false,
        }
    }
}

fn default_clips_dir() -> PathBuf {
    let base = std::env::var_os("USERPROFILE").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    base.join("Videos").join("Replix")
}

impl Settings {
    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, serde_json::to_string_pretty(self)?)?;
        Ok(())
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
            || self.record_mic != other.record_mic
            || self.mic_device != other.mic_device
            || self.capture_cursor != other.capture_cursor
    }
}
