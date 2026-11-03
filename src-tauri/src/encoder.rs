//! Picks a video encoder that actually works on this PC.
//!
//! Capture frames live on the GPU that drives the monitor. Some encoders can take them
//! directly; others need the frames copied to system memory first (a GPU from another
//! vendor, an outdated driver). Every candidate is tried with a real capture, so the
//! result holds for this machine rather than for what the hardware claims.

use std::path::Path;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::{ffmpeg, win};

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Encoder {
    pub id: &'static str,
    pub label: &'static str,
    /// The FFmpeg codec, also used when exporting.
    pub codec: &'static str,
    /// Appended to the capture filter chain.
    pub filters: &'static str,
    /// Pixel format the encoder wants for frames decoded in system memory (exports).
    pub export_pix_fmt: &'static str,
}

pub const CANDIDATES: &[Encoder] = &[
    Encoder { id: "nvenc", label: "NVIDIA NVENC", codec: "h264_nvenc", filters: "", export_pix_fmt: "yuv420p" },
    Encoder { id: "nvenc-copy", label: "NVIDIA NVENC (copy)", codec: "h264_nvenc", filters: ",hwdownload,format=bgra", export_pix_fmt: "yuv420p" },
    Encoder { id: "amf", label: "AMD AMF", codec: "h264_amf", filters: "", export_pix_fmt: "yuv420p" },
    Encoder { id: "amf-copy", label: "AMD AMF (copy)", codec: "h264_amf", filters: ",hwdownload,format=bgra", export_pix_fmt: "yuv420p" },
    Encoder { id: "qsv-copy", label: "Intel Quick Sync", codec: "h264_qsv", filters: ",hwdownload,format=bgra,format=nv12", export_pix_fmt: "nv12" },
    Encoder { id: "x264", label: "Software (x264)", codec: "libx264", filters: ",hwdownload,format=bgra,format=yuv420p", export_pix_fmt: "yuv420p" },
];

pub fn by_id(id: &str) -> Option<&'static Encoder> {
    CANDIDATES.iter().find(|e| e.id == id)
}

pub fn software() -> &'static Encoder {
    by_id("x264").unwrap()
}

/// Encoder options for a target bitrate.
pub fn args(encoder: &Encoder, kbps: u32) -> Vec<String> {
    let b = format!("{kbps}k");
    let buf = format!("{}k", kbps * 2);
    let mut a: Vec<String> = vec!["-c:v".into(), encoder.codec.into()];
    let extra: &[&str] = match encoder.codec {
        "h264_nvenc" => &["-preset", "p4", "-rc", "cbr"],
        "h264_amf" => &["-quality", "speed", "-rc", "cbr"],
        "h264_qsv" => &["-preset", "veryfast"],
        _ => &["-preset", "veryfast"],
    };
    a.extend(extra.iter().map(|s| s.to_string()));
    a.extend(["-b:v".into(), b.clone(), "-maxrate".into(), b, "-bufsize".into(), buf]);
    a
}

fn works(encoder: &Encoder) -> bool {
    let source = format!("gfxcapture=monitor_idx=0:max_framerate=30{}", encoder.filters);
    let mut args: Vec<String> = vec!["-f".into(), "lavfi".into(), "-i".into(), source, "-frames:v".into(), "15".into()];
    args.extend(self::args(encoder, 4_000));
    args.extend(["-f".into(), "null".into(), "-".into()]);
    ffmpeg::run_with_timeout(&args, Duration::from_secs(10)).is_ok()
}

#[derive(Serialize, Deserialize)]
struct Cached {
    machine: String,
    ids: Vec<String>,
}

/// What the test result depends on: the graphics adapters with their drivers, and FFmpeg.
fn machine() -> String {
    let mut parts = win::gpu_signature();
    if let Ok(meta) = std::fs::metadata(ffmpeg::exe()) {
        parts.push(format!("ffmpeg {}", meta.len()));
    }
    parts.join("; ")
}

/// The encoders from the last test on this machine; tests again when a driver, the
/// graphics card or FFmpeg changed. The flag says whether the result came from the cache.
pub fn detect_cached(cache: &Path) -> (Vec<&'static Encoder>, bool) {
    let machine = machine();
    let saved = std::fs::read_to_string(cache).ok().and_then(|t| serde_json::from_str::<Cached>(&t).ok());
    if let Some(saved) = saved.filter(|s| s.machine == machine) {
        let found: Vec<&'static Encoder> = saved.ids.iter().filter_map(|id| by_id(id)).collect();
        if !found.is_empty() {
            log::info!("encoders from last test: {}", saved.ids.join(", "));
            return (found, true);
        }
    }
    let found = detect();
    let ids = found.iter().map(|e| e.id.to_string()).collect();
    if let Ok(text) = serde_json::to_string(&Cached { machine, ids }) {
        let _ = std::fs::write(cache, text);
    }
    (found, false)
}

/// All encoders that work here, best first. Takes a few seconds.
pub fn detect() -> Vec<&'static Encoder> {
    let mut found = Vec::new();
    for candidate in CANDIDATES {
        // A copy variant only matters when the direct one failed.
        if candidate.id.ends_with("-copy") && found.iter().any(|e: &&Encoder| e.codec == candidate.codec) {
            continue;
        }
        if works(candidate) {
            log::info!("encoder works: {}", candidate.id);
            found.push(candidate);
        } else {
            log::info!("encoder unavailable: {}", candidate.id);
        }
    }
    found
}
