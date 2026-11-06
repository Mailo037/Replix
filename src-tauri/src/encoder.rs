//! Picks a video encoder that actually works on this PC.
//!
//! On Windows, capture frames live on the GPU that drives the monitor. Some encoders can
//! take them directly; others need the frames copied to system memory first (a GPU from
//! another vendor, an outdated driver). On Linux and macOS frames arrive in memory and go to
//! the encoder from there. Every candidate is tried for real, so the result holds for this
//! machine rather than for what the hardware claims.

use std::path::Path;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::{ffmpeg, platform};

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
    /// Options in front of the inputs, e.g. the graphics device VA-API encodes on.
    #[serde(skip)]
    pub device: &'static [&'static str],
}

const fn encoder(
    id: &'static str,
    label: &'static str,
    codec: &'static str,
    filters: &'static str,
    export_pix_fmt: &'static str,
) -> Encoder {
    Encoder { id, label, codec, filters, export_pix_fmt, device: &[] }
}

/// Best first; the software one last, as it always works.
#[cfg(windows)]
pub const CANDIDATES: &[Encoder] = &[
    encoder("nvenc", "NVIDIA NVENC", "h264_nvenc", "", "yuv420p"),
    encoder("nvenc-copy", "NVIDIA NVENC (copy)", "h264_nvenc", ",hwdownload,format=bgra", "yuv420p"),
    encoder("amf", "AMD AMF", "h264_amf", "", "yuv420p"),
    encoder("amf-copy", "AMD AMF (copy)", "h264_amf", ",hwdownload,format=bgra", "yuv420p"),
    encoder("qsv-copy", "Intel Quick Sync", "h264_qsv", ",hwdownload,format=bgra,format=nv12", "nv12"),
    encoder("x264", "Software (x264)", "libx264", ",hwdownload,format=bgra,format=yuv420p", "yuv420p"),
];

/// Best first; the software one last, as it always works.
#[cfg(target_os = "linux")]
pub const CANDIDATES: &[Encoder] = &[
    encoder("nvenc", "NVIDIA NVENC", "h264_nvenc", ",format=yuv420p", "yuv420p"),
    // AMD and Intel through Mesa or Intel's media driver. Frames are uploaded to the GPU
    // here, so it records only; exports use the software encoder (`for_export`).
    Encoder {
        device: &["-vaapi_device", "/dev/dri/renderD128"],
        ..encoder("vaapi", "VA-API (AMD, Intel)", "h264_vaapi", ",format=nv12,hwupload", "nv12")
    },
    encoder("x264", "Software (x264)", "libx264", ",format=yuv420p", "yuv420p"),
];

/// Best first; the software one last, as it always works.
#[cfg(target_os = "macos")]
pub const CANDIDATES: &[Encoder] = &[
    encoder("videotoolbox", "Apple VideoToolbox", "h264_videotoolbox", ",format=nv12", "nv12"),
    encoder("x264", "Software (x264)", "libx264", ",format=yuv420p", "yuv420p"),
];

pub fn by_id(id: &str) -> Option<&'static Encoder> {
    CANDIDATES.iter().find(|e| e.id == id)
}

pub fn software() -> &'static Encoder {
    by_id("x264").unwrap()
}

impl Encoder {
    /// The encoder for exports and other work on decoded files. One that needs extra
    /// device options (VA-API) hands that work to the software encoder.
    pub fn for_export(&'static self) -> &'static Encoder {
        if self.device.is_empty() {
            self
        } else {
            software()
        }
    }
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
        "h264_vaapi" => &["-rc_mode", "CBR"],
        "h264_videotoolbox" => &["-realtime", "1"],
        _ => &["-preset", "veryfast"],
    };
    a.extend(extra.iter().map(|s| s.to_string()));
    a.extend(["-b:v".into(), b.clone(), "-maxrate".into(), b, "-bufsize".into(), buf]);
    a
}

fn works(encoder: &Encoder) -> bool {
    let capture = platform::test_capture(encoder.filters);
    let mut args: Vec<String> = encoder.device.iter().map(|a| a.to_string()).collect();
    args.extend(capture.input);
    if let Some(filter) = capture.filter {
        args.extend(["-filter:v".into(), filter]);
    }
    args.extend(["-frames:v".into(), "15".into()]);
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
    let mut parts = platform::gpu_signature();
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Every system has the software encoder to fall back on, and exports never need
    /// device options.
    #[test]
    fn software_is_always_there_and_exports_need_no_device() {
        assert_eq!(software().codec, "libx264");
        assert!(CANDIDATES.iter().all(|e| e.for_export().device.is_empty()));
    }
}
