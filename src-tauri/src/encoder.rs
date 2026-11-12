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

/// The video format a recording is written in. Exports are always H.264, which plays
/// everywhere; HEVC is for recordings, at about half the size for the same picture.
///
/// AV1 is not offered: the buffer is cut into MPEG-TS segments, and FFmpeg 6 (the oldest
/// Replix accepts on Linux and macOS) cannot put AV1 into MPEG-TS.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Codec {
    #[default]
    H264,
    Hevc,
}

impl Codec {
    /// How much bitrate this format needs for the same picture as H.264.
    pub fn bitrate_share(self) -> f64 {
        match self {
            Codec::H264 => 1.0,
            Codec::Hevc => 0.6,
        }
    }

    /// Output options that name the format in an MP4. HEVC is marked `hvc1`, which Apple's
    /// players (and so the editor on macOS) need; FFmpeg would write `hev1`.
    pub fn mp4_tag(self) -> &'static [&'static str] {
        match self {
            Codec::H264 => &[],
            Codec::Hevc => &["-tag:v", "hvc1"],
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Encoder {
    pub id: &'static str,
    pub label: &'static str,
    /// The FFmpeg codec, also used when exporting.
    pub codec: &'static str,
    /// The format it writes.
    pub format: Codec,
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
    Encoder { id, label, codec, format: Codec::H264, filters, export_pix_fmt, device: &[] }
}

/// An HEVC encoder. Its id is the H.264 one's with `-hevc`: it is tried only where that
/// one works, in the same way (straight from the graphics card, or copied first).
const fn hevc(
    id: &'static str,
    label: &'static str,
    codec: &'static str,
    filters: &'static str,
    export_pix_fmt: &'static str,
) -> Encoder {
    Encoder { id, label, codec, format: Codec::Hevc, filters, export_pix_fmt, device: &[] }
}

/// H.264 best first, the software one last as it always works; then HEVC, which only the
/// graphics card can encode fast enough.
#[cfg(windows)]
pub const CANDIDATES: &[Encoder] = &[
    encoder("nvenc", "NVIDIA NVENC", "h264_nvenc", "", "yuv420p"),
    encoder("nvenc-copy", "NVIDIA NVENC (copy)", "h264_nvenc", ",hwdownload,format=bgra", "yuv420p"),
    encoder("amf", "AMD AMF", "h264_amf", "", "yuv420p"),
    encoder("amf-copy", "AMD AMF (copy)", "h264_amf", ",hwdownload,format=bgra", "yuv420p"),
    encoder("qsv-copy", "Intel Quick Sync", "h264_qsv", ",hwdownload,format=bgra,format=nv12", "nv12"),
    encoder("x264", "Software (x264)", "libx264", ",hwdownload,format=bgra,format=yuv420p", "yuv420p"),
    hevc("nvenc-hevc", "NVIDIA NVENC HEVC", "hevc_nvenc", "", "yuv420p"),
    hevc("nvenc-copy-hevc", "NVIDIA NVENC HEVC (copy)", "hevc_nvenc", ",hwdownload,format=bgra", "yuv420p"),
    hevc("amf-hevc", "AMD AMF HEVC", "hevc_amf", "", "yuv420p"),
    hevc("amf-copy-hevc", "AMD AMF HEVC (copy)", "hevc_amf", ",hwdownload,format=bgra", "yuv420p"),
    hevc("qsv-copy-hevc", "Intel Quick Sync HEVC", "hevc_qsv", ",hwdownload,format=bgra,format=nv12", "nv12"),
];

/// H.264 best first, the software one last as it always works; then HEVC, which only the
/// graphics card can encode fast enough.
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
    hevc("nvenc-hevc", "NVIDIA NVENC HEVC", "hevc_nvenc", ",format=yuv420p", "yuv420p"),
    Encoder {
        device: &["-vaapi_device", "/dev/dri/renderD128"],
        ..hevc("vaapi-hevc", "VA-API HEVC (AMD, Intel)", "hevc_vaapi", ",format=nv12,hwupload", "nv12")
    },
];

/// H.264 best first, the software one last as it always works; then HEVC, which only the
/// graphics hardware can encode fast enough.
#[cfg(target_os = "macos")]
pub const CANDIDATES: &[Encoder] = &[
    encoder("videotoolbox", "Apple VideoToolbox", "h264_videotoolbox", ",format=nv12", "nv12"),
    encoder("x264", "Software (x264)", "libx264", ",format=yuv420p", "yuv420p"),
    hevc("videotoolbox-hevc", "Apple VideoToolbox HEVC", "hevc_videotoolbox", ",format=nv12", "nv12"),
];

pub fn by_id(id: &str) -> Option<&'static Encoder> {
    CANDIDATES.iter().find(|e| e.id == id)
}

pub fn software() -> &'static Encoder {
    by_id("x264").unwrap()
}

impl Encoder {
    /// The encoder for work on decoded files, in the same format: blacking out a saved
    /// clip. One that needs extra device options (VA-API) hands that work to the software
    /// encoder.
    pub fn for_export(&'static self) -> &'static Encoder {
        if self.device.is_empty() {
            self
        } else {
            software()
        }
    }

    /// The H.264 encoder it belongs to: the id the settings name for this graphics card.
    pub fn base(&self) -> &'static str {
        self.id.strip_suffix("-hevc").unwrap_or(self.id)
    }
}

/// The encoder for exports, which are always H.264: the best one that works on decoded files.
pub fn for_exports(found: &[&'static Encoder]) -> &'static Encoder {
    found.iter().find(|e| e.format == Codec::H264).copied().unwrap_or_else(software).for_export()
}

/// The H.264 reference: what each quality needs at 1080p and 60 frames a second.
const REFERENCE_PIXELS: f64 = 1920.0 * 1080.0;
const REFERENCE_FPS: f64 = 60.0;

/// The bitrate for a picture of this size and frame rate, in kbit/s. It grows with both,
/// but less than in step: more pixels and more frames each carry less new detail. A picture
/// of unknown size counts as 1080p.
pub fn bitrate(reference_kbps: u32, size: Option<(u32, u32)>, fps: u32, format: Codec) -> u32 {
    let pixels = size.filter(|(w, h)| *w > 0 && *h > 0).map_or(REFERENCE_PIXELS, |(w, h)| w as f64 * h as f64);
    let fps = (fps.max(1)) as f64;
    let kbps = reference_kbps as f64
        * (pixels / REFERENCE_PIXELS).powf(0.85)
        * (fps / REFERENCE_FPS).powf(0.7)
        * format.bitrate_share();
    (kbps.round() as u32).clamp(2_000, 80_000)
}

/// Encoder options for a target bitrate.
pub fn args(encoder: &Encoder, kbps: u32) -> Vec<String> {
    let b = format!("{kbps}k");
    let buf = format!("{}k", kbps * 2);
    let mut a: Vec<String> = vec!["-c:v".into(), encoder.codec.into()];
    let extra: &[&str] = match encoder.codec {
        "h264_nvenc" | "hevc_nvenc" => &["-preset", "p4", "-rc", "cbr"],
        "h264_amf" | "hevc_amf" => &["-quality", "speed", "-rc", "cbr"],
        "h264_qsv" | "hevc_qsv" => &["-preset", "veryfast"],
        "h264_vaapi" | "hevc_vaapi" => &["-rc_mode", "CBR"],
        "h264_videotoolbox" | "hevc_videotoolbox" => &["-realtime", "1"],
        _ => &["-preset", "veryfast"],
    };
    a.extend(extra.iter().map(|s| s.to_string()));
    a.extend(["-b:v".into(), b.clone(), "-maxrate".into(), b, "-bufsize".into(), buf]);
    a
}

fn works(encoder: &Encoder) -> anyhow::Result<()> {
    let capture = platform::test_capture(encoder.filters);
    let mut args: Vec<String> = encoder.device.iter().map(|a| a.to_string()).collect();
    args.extend(capture.input);
    if let Some(filter) = capture.filter {
        args.extend(["-filter:v".into(), filter]);
    }
    // One frame initializes and flushes the encoder. Graphics Capture may not supply
    // another until the desktop changes, so asking for more can reject working encoders.
    args.extend(["-frames:v".into(), "1".into()]);
    args.extend(self::args(encoder, 4_000));
    args.extend(["-f".into(), "null".into(), "-".into()]);
    ffmpeg::run_with_timeout(&args, Duration::from_secs(10))
}

#[derive(Serialize, Deserialize)]
struct Cached {
    machine: String,
    ids: Vec<String>,
}

/// What the test result depends on: the graphics adapters with their drivers, and FFmpeg.
fn machine() -> String {
    let mut parts = platform::gpu_signature();
    // Which encoders there are to try; a list from before HEVC is tested again.
    parts.push(format!("candidates {}", CANDIDATES.len()));
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

/// All encoders that work here, best first, H.264 before HEVC. Takes a few seconds.
pub fn detect() -> Vec<&'static Encoder> {
    let mut found: Vec<&'static Encoder> = Vec::new();
    for candidate in CANDIDATES {
        // A copy variant only matters when the direct one failed.
        if candidate.id.ends_with("-copy") && found.iter().any(|e: &&Encoder| e.codec == candidate.codec) {
            continue;
        }
        // HEVC is tried only on hardware whose H.264 encoder works, in the same way.
        if candidate.format == Codec::Hevc && !found.iter().any(|e| e.format == Codec::H264 && e.id == candidate.base()) {
            continue;
        }
        match works(candidate) {
            Ok(()) => {
                log::info!("encoder works: {}", candidate.id);
                found.push(candidate);
            }
            Err(e) => log::info!("encoder unavailable: {}: {e:#}", candidate.id),
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

    /// Every HEVC encoder has an H.264 one to fall back on, listed before it.
    #[test]
    fn hevc_follows_its_h264_encoder() {
        for (i, e) in CANDIDATES.iter().enumerate().filter(|(_, e)| e.format == Codec::Hevc) {
            let base = CANDIDATES.iter().position(|b| b.id == e.base() && b.format == Codec::H264);
            assert!(base.is_some_and(|b| b < i), "{} has no H.264 encoder before it", e.id);
            assert!(e.codec.starts_with("hevc_"));
        }
    }

    /// Exports are H.264 even when HEVC is found first.
    #[test]
    fn exports_are_h264() {
        let hevc = CANDIDATES.iter().find(|e| e.format == Codec::Hevc).unwrap();
        assert_eq!(for_exports(&[hevc]).format, Codec::H264);
    }

    #[test]
    fn bitrate_follows_size_and_frame_rate() {
        // The reference stays as it was.
        assert_eq!(bitrate(12_000, Some((1920, 1080)), 60, Codec::H264), 12_000);
        assert_eq!(bitrate(12_000, None, 60, Codec::H264), 12_000);
        // Less for a smaller or slower picture, more for a larger or faster one.
        assert!(bitrate(12_000, Some((1920, 1080)), 30, Codec::H264) < 12_000);
        assert!(bitrate(12_000, Some((2560, 1440)), 60, Codec::H264) > 12_000);
        assert!(bitrate(12_000, Some((2560, 1440)), 120, Codec::H264) > bitrate(12_000, Some((2560, 1440)), 60, Codec::H264));
        // HEVC needs less for the same picture; never below a floor or above a ceiling.
        assert_eq!(bitrate(12_000, Some((1920, 1080)), 60, Codec::Hevc), 7_200);
        assert_eq!(bitrate(6_000, Some((640, 360)), 30, Codec::Hevc), 2_000);
        assert_eq!(bitrate(24_000, Some((7680, 4320)), 240, Codec::H264), 80_000);
    }
}
