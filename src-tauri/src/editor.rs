//! Editing: preview material, drafts, exports and stills.

use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, OnceLock};

use anyhow::{bail, Context, Result};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::encoder::Encoder;
use crate::ffmpeg;
use crate::library::{Draft, Meta, TrackEdit, TrackMeta};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorTrack {
    #[serde(flatten)]
    pub meta: TrackMeta,
    /// The track on its own, so the editor can play and mute each one.
    pub audio: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorData {
    pub id: String,
    pub path: String,
    pub title: String,
    pub game: String,
    pub created: u64,
    pub duration: f64,
    pub width: u32,
    pub height: u32,
    pub tracks: Vec<EditorTrack>,
    pub filmstrip: Option<String>,
    pub draft: Option<Draft>,
}

pub const FILMSTRIP_FRAMES: u32 = 14;

/// What ffprobe said about a clip, kept next to its preview material so a second open skips it.
#[derive(Serialize, Deserialize)]
struct Probed {
    duration: f64,
    width: u32,
    height: u32,
    titles: Vec<Option<String>>,
}

/// One preparation per clip at a time: opening while a warm-up runs waits for it instead of repeating it.
fn lock_for(id: &str) -> Arc<Mutex<()>> {
    static LOCKS: OnceLock<Mutex<HashMap<String, Arc<Mutex<()>>>>> = OnceLock::new();
    LOCKS.get_or_init(Default::default).lock().entry(id.to_string()).or_default().clone()
}

/// Clips whose filmstrip is being made right now.
fn pending() -> &'static Mutex<HashSet<String>> {
    static PENDING: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    PENDING.get_or_init(Default::default)
}

/// Gets a clip ready for the editor. `strip_ready` runs once a missing filmstrip has been made.
pub fn prepare(
    id: &str,
    path: &Path,
    meta: &Meta,
    cache: &Path,
    strip_ready: impl FnOnce(PathBuf) + Send + 'static,
) -> Result<EditorData> {
    let lock = lock_for(id);
    let _held = lock.lock();
    let dir = cache.join("editor").join(id);
    std::fs::create_dir_all(&dir)?;

    let probed_path = dir.join("info.json");
    let info = match std::fs::read(&probed_path).ok().and_then(|b| serde_json::from_slice::<Probed>(&b).ok()) {
        Some(p) => p,
        None => {
            let i = ffmpeg::probe(path)?;
            let p = Probed { duration: i.duration, width: i.width, height: i.height, titles: i.audio_titles };
            let _ = std::fs::write(&probed_path, serde_json::to_vec(&p)?);
            p
        }
    };
    let count = info.titles.len();
    let audio: Vec<PathBuf> = (0..count).map(|i| dir.join(format!("a{i}.m4a"))).collect();
    let strip = dir.join("strip.jpg");

    // The filmstrip decodes keyframes only, and in the background: the editor opens
    // without it and shows it once it's ready.
    if !strip.exists() && info.duration > 0.0 && pending().lock().insert(id.to_string()) {
        let (id, input, out, rate) = (id.to_string(), path.to_path_buf(), strip.clone(), FILMSTRIP_FRAMES as f64 / info.duration);
        std::thread::spawn(move || {
            let args: Vec<String> = vec![
                "-skip_frame".into(),
                "nokey".into(),
                "-i".into(),
                ffmpeg::path_arg(&input),
                "-an".into(),
                "-vf".into(),
                format!("fps={rate:.5},scale=-2:96,tile={FILMSTRIP_FRAMES}x1"),
                "-frames:v".into(),
                "1".into(),
                "-q:v".into(),
                "5".into(),
                "-y".into(),
                ffmpeg::path_arg(&out),
            ];
            let made = ffmpeg::run(&args);
            pending().lock().remove(&id);
            match made {
                Ok(()) => strip_ready(out),
                Err(e) => log::warn!("filmstrip failed: {e}"),
            }
        });
    }
    if count > 0 && audio.iter().any(|p| !p.exists()) {
        let mut args: Vec<String> = vec!["-i".into(), ffmpeg::path_arg(path)];
        for (i, out) in audio.iter().enumerate() {
            args.extend(["-map".into(), format!("0:a:{i}"), "-c".into(), "copy".into(), "-y".into(), ffmpeg::path_arg(out)]);
        }
        ffmpeg::run(&args).context("could not split the audio tracks")?;
    }

    let tracks = (0..count)
        .map(|i| EditorTrack {
            meta: meta.tracks.get(i).cloned().unwrap_or(TrackMeta {
                name: info.titles[i].clone().unwrap_or_else(|| format!("Track {}", i + 1)),
                kind: crate::audio::TrackKind::App,
            }),
            audio: audio[i].to_string_lossy().into_owned(),
        })
        .collect();

    Ok(EditorData {
        id: id.to_string(),
        path: path.to_string_lossy().into_owned(),
        title: meta.title.clone(),
        game: meta.game.clone(),
        created: meta.created,
        duration: info.duration,
        width: info.width,
        height: info.height,
        tracks,
        filmstrip: strip.exists().then(|| strip.to_string_lossy().into_owned()),
        draft: meta.draft.clone(),
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Preset {
    /// Full quality.
    Original,
    /// Small enough to upload to Discord without Nitro.
    Discord,
    /// A silent, looping GIF.
    Gif,
}

impl Preset {
    pub fn extension(self) -> &'static str {
        match self {
            Preset::Gif => "gif",
            _ => "mp4",
        }
    }
}

fn centre() -> f64 {
    0.5
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportRequest {
    pub trim_start: f64,
    pub trim_end: f64,
    pub tracks: Vec<TrackEdit>,
    pub preset: Preset,
    /// Parts taken out between start and end, in seconds of the original.
    #[serde(default)]
    pub cuts: Vec<[f64; 2]>,
    /// 9:16 for TikTok, Shorts and Reels.
    #[serde(default)]
    pub vertical: bool,
    /// Where the 9:16 frame sits: 0 at the left edge, 1 at the right.
    #[serde(default = "centre")]
    pub crop_x: f64,
}

const DISCORD_BYTES: f64 = 10.0 * 1024.0 * 1024.0;

/// The parts that stay, in seconds from the trim start.
fn kept(request: &ExportRequest) -> Vec<(f64, f64)> {
    let start = request.trim_start.max(0.0);
    let end = request.trim_end.max(start + 0.1);
    let mut cuts: Vec<(f64, f64)> =
        request.cuts.iter().map(|c| (c[0].max(start), c[1].min(end))).filter(|(a, b)| b > a).collect();
    cuts.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut parts = Vec::new();
    let mut at = start;
    for (a, b) in cuts {
        if a > at {
            parts.push((at - start, a - start));
        }
        at = at.max(b);
    }
    if end > at {
        parts.push((at - start, end - start));
    }
    parts
}

pub fn export(
    source: &Path,
    request: &ExportRequest,
    encoder: &Encoder,
    out: &Path,
    on_progress: impl Fn(f64),
) -> Result<()> {
    let start = request.trim_start.max(0.0);
    let span = (request.trim_end - start).max(0.1);
    let parts = kept(request);
    if parts.is_empty() {
        bail!("Nothing is left after the cuts.");
    }
    let length: f64 = parts.iter().map(|(a, b)| b - a).sum();
    // With cuts, keep only the frames and samples inside the parts and close the gaps.
    let pick = (parts.len() > 1)
        .then(|| parts.iter().map(|(a, b)| format!("between(t,{a:.3},{b:.3})")).collect::<Vec<_>>().join("+"));
    let mut args: Vec<String> = vec![
        "-hide_banner".into(),
        "-loglevel".into(),
        "error".into(),
        "-nostdin".into(),
        "-progress".into(),
        "pipe:1".into(),
        "-ss".into(),
        format!("{start:.3}"),
        "-t".into(),
        format!("{span:.3}"),
        "-i".into(),
        ffmpeg::path_arg(source),
    ];

    let gif = request.preset == Preset::Gif;
    let enabled: Vec<(usize, f32)> = if gif {
        Vec::new()
    } else {
        request
            .tracks
            .iter()
            .enumerate()
            .filter(|(_, t)| t.enabled && t.volume > 0.0)
            .map(|(i, t)| (i, t.volume))
            .collect()
    };

    let (video_kbps, audio_kbps) = match request.preset {
        Preset::Original | Preset::Gif => (16_000, 192),
        Preset::Discord => {
            let audio = if enabled.is_empty() { 0.0 } else { 96.0 };
            let total_kbps = DISCORD_BYTES * 8.0 * 0.9 / 1000.0 / length;
            (((total_kbps - audio).max(250.0)) as u32, 96)
        }
    };

    let mut video: Vec<String> = Vec::new();
    if let Some(p) = &pick {
        video.push(format!("select='{p}',setpts=N/FRAME_RATE/TB"));
    }
    if request.vertical {
        video.push(format!("crop=w='min(iw,ih*9/16)':h=ih:x='(iw-ow)*{:.3}':y=0", request.crop_x.clamp(0.0, 1.0)));
    }
    match (request.preset, request.vertical) {
        (Preset::Original, true) => video.push("scale=1080:1920:flags=lanczos".into()),
        (Preset::Original, false) => {}
        (Preset::Discord, true) => video.push("scale=720:1280".into()),
        (Preset::Discord, false) => video.push("scale=-2:'min(720,ih)'".into()),
        (Preset::Gif, true) => video.push("fps=15,scale=360:640:flags=lanczos".into()),
        (Preset::Gif, false) => video.push("fps=15,scale=480:-2:flags=lanczos".into()),
    }
    if request.preset == Preset::Discord && video_kbps < 2500 {
        video.push("fps=30".into());
    }

    let mut filters: Vec<String> = Vec::new();
    if gif {
        // A palette of its own keeps the colours clean within 256 of them.
        video.push("split[g0][g1]".into());
        filters.push(format!("[0:v]{}", video.join(",")));
        filters.push("[g0]palettegen=stats_mode=diff[pal]".into());
        filters.push("[g1][pal]paletteuse=dither=bayer:bayer_scale=4[v]".into());
    } else {
        video.push(format!("format={}", encoder.export_pix_fmt));
        filters.push(format!("[0:v]{}[v]", video.join(",")));
    }
    if !enabled.is_empty() {
        let mut inputs = String::new();
        for (k, (i, volume)) in enabled.iter().enumerate() {
            filters.push(format!("[0:a:{i}]volume={volume:.3}[a{k}]"));
            inputs.push_str(&format!("[a{k}]"));
        }
        if enabled.len() == 1 {
            filters.push(format!("{inputs}anull[mix]"));
        } else {
            filters.push(format!("{inputs}amix=inputs={}:normalize=0:dropout_transition=0[mix]", enabled.len()));
        }
        match &pick {
            Some(p) => filters.push(format!("[mix]aselect='{p}',asetpts=N/SR/TB[aout]")),
            None => filters.push("[mix]anull[aout]".into()),
        }
    }
    args.extend(["-filter_complex".into(), filters.join(";"), "-map".into(), "[v]".into()]);
    if enabled.is_empty() {
        args.push("-an".into());
    } else {
        args.extend(["-map".into(), "[aout]".into(), "-c:a".into(), "aac".into(), "-b:a".into(), format!("{audio_kbps}k")]);
    }
    if gif {
        args.extend(["-loop".into(), "0".into()]);
    } else {
        args.extend(crate::encoder::args(encoder, video_kbps));
        args.extend(["-movflags".into(), "+faststart".into()]);
    }
    args.extend(["-y".into(), ffmpeg::path_arg(out)]);

    let mut child = std::process::Command::new(ffmpeg::exe())
        .creation_flags(0x0800_0000)
        .args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("FFmpeg could not be started")?;
    let stderr = child.stderr.take().unwrap();
    let errors = std::thread::spawn(move || {
        let mut text = String::new();
        let _ = std::io::Read::read_to_string(&mut BufReader::new(stderr), &mut text);
        text
    });
    if let Some(stdout) = child.stdout.take() {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if let Some(us) = line.strip_prefix("out_time_us=").and_then(|v| v.parse::<f64>().ok()) {
                on_progress((us / 1_000_000.0 / length).clamp(0.0, 1.0));
            }
        }
    }
    let status = child.wait()?;
    let text = errors.join().unwrap_or_default();
    if !status.success() {
        let _ = std::fs::remove_file(out);
        bail!("{}", ffmpeg::last_lines(&text, 3));
    }
    on_progress(1.0);
    Ok(())
}

pub fn save_frame(source: &Path, time: f64, out: &Path) -> Result<()> {
    let args: Vec<String> = vec![
        "-ss".into(),
        format!("{:.3}", time.max(0.0)),
        "-i".into(),
        ffmpeg::path_arg(source),
        "-frames:v".into(),
        "1".into(),
        "-update".into(),
        "1".into(),
        "-y".into(),
        ffmpeg::path_arg(out),
    ];
    ffmpeg::run(&args)
}
