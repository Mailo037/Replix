//! Editing: preview material, drafts, exports and stills.

use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, OnceLock};

use anyhow::{bail, Context, Result};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::encoder::Encoder;
use crate::ffmpeg::SpawnTied;
use crate::{ffmpeg, media};
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

/// Filmstrips under way, by clip.
fn pending() -> &'static media::Pending {
    static PENDING: OnceLock<media::Pending> = OnceLock::new();
    PENDING.get_or_init(Default::default)
}

/// Gets a clip ready for the editor. `need` is the editor round asking, or `media::AHEAD`
/// when preparing on a hunch. `strip_ready` runs once a missing filmstrip has been made.
pub fn prepare(
    id: &str,
    path: &Path,
    meta: &Meta,
    cache: &Path,
    need: u64,
    strip_ready: impl FnOnce(PathBuf) + Send + 'static,
) -> Result<EditorData> {
    let lock = lock_for(id);
    // A warm-up of this clip may hold its lock; waiting as the editor makes it stop.
    let _held = if need == media::AHEAD { lock.lock() } else { media::wait_as_editor(|| lock.lock()) };
    let dir = cache.join("editor").join(id);
    std::fs::create_dir_all(&dir)?;

    let probed_path = dir.join("info.json");
    let info = match std::fs::read(&probed_path).ok().and_then(|b| serde_json::from_slice::<Probed>(&b).ok()) {
        Some(p) => p,
        None => {
            // Even reading the header waits on the disk; it takes its turn like the rest.
            // For the editor it waits while the window is hidden, like the rest of its work;
            // made ahead of time, it simply stops.
            let demand = media::Demand::new(need);
            let probe = |go: &(dyn Fn() -> bool + Sync)| ffmpeg::probe_while(path, go);
            let i = if need == media::AHEAD { media::run(&demand, probe) } else { media::run_needed(&demand, probe) }.result()?;
            let p = Probed { duration: i.duration, width: i.width, height: i.height, titles: i.audio_titles };
            let _ = std::fs::write(&probed_path, serde_json::to_vec(&p)?);
            p
        }
    };
    let count = info.titles.len();
    let audio: Vec<PathBuf> = (0..count).map(|i| dir.join(format!("a{i}.m4a"))).collect();
    let strip = dir.join("strip.jpg");

    // The tracks first: the editor needs them to open; the filmstrip can come later.
    if count > 0 && audio.iter().any(|p| !p.exists()) {
        split_tracks(path, &audio, need)?;
    }

    // The filmstrip decodes keyframes only, and in the background: the editor opens
    // without it and shows it once it's ready. A request for a clip whose filmstrip is
    // under way joins that job.
    // It gives way to waveforms of the same editor, which are quicker to want.
    let job = (!strip.exists() && info.duration > 0.0).then(|| pending().claim(id, need, true)).flatten();
    if let Some(demand) = job {
        let (id, input, out, rate) = (id.to_string(), path.to_path_buf(), strip.clone(), FILMSTRIP_FRAMES as f64 / info.duration);
        // Written under another name and renamed when whole: the strip's own name is how
        // the editor tells it is ready, so a half-made or stopped one must never carry it.
        let part = out.with_file_name("strip.part.jpg");
        // Says whether the filmstrip is settled: made, or failed for good.
        let worker = std::thread::spawn(move || -> bool {
            let args: Vec<String> = vec![
                // Two threads: it is background work beside a game, not a race.
                "-threads".into(),
                "2".into(),
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
                "-filter_threads".into(),
                "1".into(),
                "-threads".into(),
                "1".into(),
                "-y".into(),
                ffmpeg::path_arg(&part),
            ];
            loop {
                match media::run(&demand, |go| ffmpeg::run_while(&args, go)) {
                    media::Outcome::Done(()) => {
                        let published = std::fs::rename(&part, &out);
                        pending().finish(&id);
                        match published {
                            Ok(()) => strip_ready(out),
                            Err(e) => log::warn!("filmstrip could not be kept: {e}"),
                        }
                        return true;
                    }
                    media::Outcome::Stopped => {
                        let _ = std::fs::remove_file(&part);
                        // Gave way to other editor work: wait for it, then go on. Otherwise
                        // nobody needs it now; it ends unless a request came in meanwhile.
                        if demand.alive() && media::wait_until_wanted(&demand) {
                            continue;
                        }
                        if !pending().stopped(&id) {
                            return false;
                        }
                    }
                    media::Outcome::Failed(e) => {
                        let _ = std::fs::remove_file(&part);
                        pending().finish(&id);
                        log::warn!("filmstrip failed: {e}");
                        return true;
                    }
                }
            }
        });
        // Made ahead of time, the filmstrip belongs to the warm-up: it holds the warm-up's
        // slot until it ends, and one that stopped leaves the clip free to warm up again.
        if need == media::AHEAD && !worker.join().unwrap_or(true) {
            return Err(media::Stopped.into());
        }
    }
    let tracks = (0..count)
        .map(|i| EditorTrack {
            meta: meta.tracks.get(i).cloned().unwrap_or(TrackMeta {
                name: info.titles[i].clone().unwrap_or_else(|| format!("Track {}", i + 1)),
                kind: crate::audio::TrackKind::App,
                exe: None,
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

fn full() -> f64 {
    1.0
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
    /// Where the frame sits: 0 at the top, 1 at the bottom.
    #[serde(default = "centre")]
    pub crop_y: f64,
    /// Frame height as a share of the video's; 1 is full height.
    #[serde(default = "full")]
    pub crop_scale: f64,
    /// All sound together: 1 as recorded, up to 2 for twice as loud.
    #[serde(default = "full")]
    pub master_volume: f64,
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
    cancelled: &dyn Fn() -> bool,
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
        // Decoding and the filters share the export's thread budget too, not only encoding.
        "-threads".into(),
        export_threads().to_string(),
        "-filter_complex_threads".into(),
        export_threads().to_string(),
        "-ss".into(),
        format!("{start:.3}"),
        "-t".into(),
        format!("{span:.3}"),
        "-i".into(),
        ffmpeg::path_arg(source),
    ];

    let gif = request.preset == Preset::Gif;
    // Each track with where it is heard, in seconds of the input (which begins at `start`).
    type Heard = (usize, f32, Option<(f64, f64)>);
    let enabled: Vec<Heard> = if gif {
        Vec::new()
    } else {
        request
            .tracks
            .iter()
            .enumerate()
            .filter(|(_, t)| t.enabled && !t.removed && t.volume > 0.0 && request.master_volume > 0.0)
            .filter(|(_, t)| t.start.unwrap_or(0.0) < request.trim_end && t.end.unwrap_or(f64::MAX) > request.trim_start)
            .map(|(i, t)| {
                let t = &TrackEdit { volume: t.volume * request.master_volume.clamp(0.0, 2.0) as f32, ..t.clone() };
                let window = (t.start.is_some() || t.end.is_some())
                    .then(|| (t.start.unwrap_or(0.0) - start, t.end.unwrap_or(1e9) - start));
                (i, t.volume, window)
            })
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
        // Even sizes, as the encoders want them; the scale afterwards brings it to 1080x1920.
        video.push(format!(
            "crop=w='trunc(min(iw,ih*9/16)*{:.4}/2)*2':h='trunc(ow*16/9/2)*2':x='(iw-ow)*{:.3}':y='(ih-oh)*{:.3}'",
            request.crop_scale.clamp(0.2, 1.0),
            request.crop_x.clamp(0.0, 1.0),
            request.crop_y.clamp(0.0, 1.0)
        ));
    }
    match (request.preset, request.vertical) {
        (Preset::Original, true) => video.push("scale=1080:1920:flags=lanczos".into()),
        (Preset::Original, false) => {}
        (Preset::Discord, true) => video.push("scale=720:1280".into()),
        (Preset::Discord, false) => video.push("scale=-2:'min(720,ih)'".into()),
        (Preset::Gif, true) => video.push("fps=15,scale=360:640:flags=lanczos".into()),
        (Preset::Gif, false) => video.push("fps=15,scale=480:-2:flags=lanczos".into()),
    }
    if request.vertical {
        // A smaller frame rounds to sizes a hair off 9:16; the pixels stay square anyway.
        video.push("setsar=1".into());
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
        for (k, (i, volume, window)) in enabled.iter().enumerate() {
            let silence = window
                .map(|(a, b)| format!(",volume=0:enable='not(between(t,{a:.3},{b:.3}))'"))
                .unwrap_or_default();
            filters.push(format!("[0:a:{i}]volume={volume:.3}{silence}[a{k}]"));
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
        if encoder.codec == "libx264" {
            // The software encoder would otherwise take every core.
            args.extend(["-threads".into(), export_threads().to_string()]);
        }
        args.extend(["-movflags".into(), "+faststart".into()]);
    }
    args.extend(["-y".into(), ffmpeg::path_arg(out)]);

    // Below normal priority, like the other helper jobs: an export can run while a game does.
    let mut child = Reaped(std::process::Command::new(ffmpeg::exe())
        .creation_flags(0x0800_0000 | 0x0000_4000)
        .args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn_tied()
        .context("FFmpeg could not be started")?);
    let stderr = child.0.stderr.take().unwrap();
    let errors = std::thread::spawn(move || {
        let mut text = String::new();
        let _ = std::io::Read::read_to_string(&mut BufReader::new(stderr), &mut text);
        text
    });
    // Progress is read on a thread of its own, so an FFmpeg that stops moving (a stuck
    // decoder, a drive that went away) is noticed and ended: exports run one at a time,
    // and a stalled one would hold up every export after it.
    let (progress, moved) = std::sync::mpsc::channel();
    if let Some(stdout) = child.0.stdout.take() {
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if let Some(us) = line.strip_prefix("out_time_us=").and_then(|v| v.parse::<f64>().ok()) {
                    let _ = progress.send(us);
                }
            }
        });
    }
    let watched = watch_export(child, &moved, Watch::new(length, std::time::Instant::now()), cancelled, |us| {
        on_progress((us / 1_000_000.0 / length).clamp(0.0, 1.0));
    });
    let status = match watched {
        Ok(status) => status,
        Err(e) => {
            let _ = std::fs::remove_file(out);
            return Err(e);
        }
    };
    let text = errors.join().unwrap_or_default();
    if !status.success() {
        let _ = std::fs::remove_file(out);
        bail!("{}", ffmpeg::last_lines(&text, 3));
    }
    on_progress(1.0);
    Ok(())
}

/// How long an export may go without its time moving forward before it counts as stuck.
const EXPORT_STALL: std::time::Duration = std::time::Duration::from_secs(180);
/// The same once all of it is encoded: moving the index to the front rewrites the whole
/// file, which moves no time forward and can take a while for a long export on a slow drive.
const EXPORT_FINISH: std::time::Duration = std::time::Duration::from_secs(20 * 60);

/// CPU time that counts as an export still working while its time stands still: a GIF's
/// palette is made from the whole selection before the first frame is written.
const EXPORT_WORKING: std::time::Duration = std::time::Duration::from_millis(250);

/// Tells a running export from a stuck one by how long it has neither moved its time
/// forward nor used the processor.
struct Watch {
    length: f64,
    furthest: f64,
    cpu: std::time::Duration,
    since: std::time::Instant,
    stall: std::time::Duration,
    finish: std::time::Duration,
}

impl Watch {
    fn new(length: f64, now: std::time::Instant) -> Self {
        Self { length, furthest: f64::MIN, cpu: std::time::Duration::ZERO, since: now, stall: EXPORT_STALL, finish: EXPORT_FINISH }
    }

    fn saw(&mut self, us: f64, now: std::time::Instant) {
        if us > self.furthest {
            self.furthest = us;
            self.since = now;
        }
    }

    /// The process's CPU time so far.
    fn worked(&mut self, cpu: std::time::Duration, now: std::time::Instant) {
        if cpu >= self.cpu + EXPORT_WORKING {
            self.cpu = cpu;
            self.since = now;
        }
    }

    fn stuck(&self, now: std::time::Instant) -> bool {
        let finishing = self.furthest / 1_000_000.0 >= self.length - 0.5;
        now.duration_since(self.since) > if finishing { self.finish } else { self.stall }
    }
}

/// The error of an export someone cancelled.
pub const CANCELLED: &str = "Export cancelled";

/// Waits for an export's FFmpeg to end, passing on its progress, and kills it once it is
/// stuck or cancelled, also after it closed its output, until the process itself is gone.
/// A busy process counts as working, so cancelling is the way out of one that never ends.
fn watch_export(
    mut child: Reaped,
    moved: &std::sync::mpsc::Receiver<f64>,
    mut watch: Watch,
    cancelled: &dyn Fn() -> bool,
    mut on_progress: impl FnMut(f64),
) -> Result<std::process::ExitStatus> {
    let mut open = true;
    loop {
        if let Some(status) = child.0.try_wait()? {
            for us in moved.try_iter() {
                on_progress(us);
            }
            return Ok(status);
        }
        if open {
            match moved.recv_timeout(std::time::Duration::from_millis(500)) {
                Ok(us) => {
                    watch.saw(us, std::time::Instant::now());
                    on_progress(us);
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => open = false,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            }
        } else {
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
        if let Some(cpu) = crate::win::cpu_time(&child.0) {
            watch.worked(cpu, std::time::Instant::now());
        }
        if cancelled() {
            drop(child);
            bail!(CANCELLED);
        }
        // Checked on every pass: FFmpeg repeats its last time twice a second while stuck.
        if watch.stuck(std::time::Instant::now()) {
            drop(child);
            bail!("FFmpeg stopped making progress");
        }
    }
}

/// Threads an export may use at each stage (decoding, filters, encoding): half the cores,
/// at least one. Together with the export's lower priority this leaves a game running
/// beside it more room; it is no hard reservation.
fn export_threads() -> usize {
    std::thread::available_parallelism().map_or(1, |n| n.get() / 2).max(1)
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

/// Bars across a track's lane in the editor.
pub const PEAKS: usize = 1200;

/// Kills and reaps FFmpeg on every way out, an early return or error included.
struct Reaped(std::process::Child);

impl Drop for Reaped {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// A track's loudness over time, 0 to 1, made once and kept with its preview material.
/// FFmpeg streams the decoded track at 48 kHz, the rate Replix records at, with all of its
/// channels, and only the loudest sample of any channel per bar is kept: memory stays the
/// same for a one-hour session as for a short clip, and neither opposite channels nor high
/// notes are lost to mixing or resampling. The whole track is still decoded once, so it is made only
/// for the editor of `round`, in the shared media turn, and at a low priority. A reader
/// thread does the counting; this one watches it, so the round ending or a stuck decoder
/// ends the job even while no output arrives.
pub fn waveform(audio: &Path, duration: f64, round: u64) -> Result<Vec<f32>> {
    let cached = audio.with_extension("peaks");
    let read_cache = || {
        let bytes = std::fs::read(&cached).ok().filter(|b| b.len() == PEAKS * 4)?;
        Some(bytes.as_chunks::<4>().0.iter().map(|b| f32::from_le_bytes(*b)).collect::<Vec<f32>>())
    };
    if let Some(peaks) = read_cache() {
        return Ok(peaks);
    }
    match media::run_needed(&media::Demand::new(round.max(1)), |wanted| {
        // Made by another request while this one waited.
        if let Some(peaks) = read_cache() {
            return Ok(peaks);
        }
        // Every channel the track has is counted as it is, never mixed down first: in a
        // mix, opposite channels cancel and a loud passage would look quiet.
        let channels = ffmpeg::probe_while(audio, wanted)?.audio_channels.first().copied().unwrap_or(2);
        decode_peaks(audio, duration, channels.clamp(1, 16) as usize, wanted, &cached)
    }) {
        media::Outcome::Done(peaks) => Ok(peaks),
        media::Outcome::Stopped => Err(media::Stopped.into()),
        media::Outcome::Failed(e) => Err(e),
    }
}

fn decode_peaks(
    audio: &Path,
    duration: f64,
    channels: usize,
    wanted: &(dyn Fn() -> bool + Sync),
    cached: &Path,
) -> Result<Vec<f32>> {
    let mut child = Reaped(
        ffmpeg::command()
            .args(["-threads", "1", "-i"])
            .arg(ffmpeg::path_arg(audio))
            .args(["-vn", "-filter_threads", "1", "-ar", "48000", "-ac", &channels.to_string(), "-f", "s16le", "pipe:1"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn_tied()
            .context("FFmpeg could not be started")?,
    );
    let stdout = child.0.stdout.take().context("no output")?;
    let frames = (duration.max(0.1) * crate::audio::RATE as f64) as usize;
    let (done, result) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        crate::win::lower_thread_priority();
        let _ = done.send(count_peaks(stdout, frames, channels));
    });
    // Decoding AAC runs far faster than real time; this only catches a stuck decoder.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs_f64(60.0 + duration.max(0.0) / 4.0);
    let peaks = loop {
        match result.recv_timeout(std::time::Duration::from_millis(100)) {
            Ok(peaks) => break peaks?,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                // Dropping `child` kills FFmpeg, which ends the reader too.
                if !wanted() {
                    return Err(media::Stopped.into());
                }
                if std::time::Instant::now() > deadline {
                    bail!("the audio decoder stopped responding");
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => bail!("could not read the track"),
        }
    };
    // The output ended, so FFmpeg is exiting; its status says whether the track was whole.
    let exited = loop {
        if let Some(status) = child.0.try_wait()? {
            break status;
        }
        if !wanted() {
            return Err(media::Stopped.into());
        }
        if std::time::Instant::now() > deadline {
            bail!("the audio decoder stopped responding");
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    };
    if !exited.success() {
        bail!("could not read the track");
    }
    let loudest = peaks.iter().copied().max().filter(|&m| m > 0).unwrap_or(1) as f32;
    let peaks: Vec<f32> = peaks.into_iter().map(|v| (v as f32 / loudest).sqrt()).collect();
    let _ = std::fs::write(cached, peaks.iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<u8>>());
    Ok(peaks)
}

/// The loudest sample of any channel per bar, from interleaved 16-bit audio.
fn count_peaks(mut input: impl std::io::Read, frames: usize, channels: usize) -> std::io::Result<Vec<u16>> {
    let frame_bytes = 2 * channels;
    let mut peaks = vec![0u16; PEAKS];
    let mut buf = vec![0u8; 256 * 1024];
    let (mut n, mut used) = (0usize, 0usize);
    loop {
        let read = input.read(&mut buf[used..])?;
        if read == 0 {
            return Ok(peaks);
        }
        used += read;
        // A frame holds one sample per channel; a partial one waits for the next read.
        let whole = used / frame_bytes * frame_bytes;
        for frame in buf[..whole].chunks_exact(frame_bytes) {
            let loudest = frame.as_chunks::<2>().0.iter().map(|s| i16::from_le_bytes(*s).unsigned_abs()).max().unwrap_or(0);
            let bar = (n * PEAKS / frames.max(1)).min(PEAKS - 1);
            peaks[bar] = peaks[bar].max(loudest);
            n += 1;
        }
        buf.copy_within(whole..used, 0);
        used -= whole;
    }
}

/// Copies each audio track of a clip into a file of its own. They are written under other
/// names and renamed once all are whole: a track's own name is how a later open tells it
/// is there, so one cut short by a stop, a pause or Replix quitting must never carry it.
fn split_tracks(path: &Path, audio: &[PathBuf], need: u64) -> Result<()> {
    let part = |out: &PathBuf| out.with_extension("part.m4a");
    let mut args: Vec<String> = vec!["-i".into(), ffmpeg::path_arg(path)];
    for (i, out) in audio.iter().enumerate() {
        args.extend(["-map".into(), format!("0:a:{i}"), "-c".into(), "copy".into(), "-y".into(), ffmpeg::path_arg(&part(out))]);
    }
    let split = match media::run_needed(&media::Demand::new(need), |go| ffmpeg::run_while(&args, go)) {
        media::Outcome::Done(()) => audio.iter().try_for_each(|out| std::fs::rename(part(out), out)).map_err(|e| Some(e.into())),
        media::Outcome::Stopped => Err(None),
        media::Outcome::Failed(e) => Err(Some(e)),
    };
    let Err(failure) = split else { return Ok(()) };
    for out in audio {
        let _ = std::fs::remove_file(part(out));
        let _ = std::fs::remove_file(out);
    }
    Err(match failure {
        Some(e) => e.context("could not split the audio tracks"),
        // Not a failure: the editor closed or the window was hidden. It asks again.
        None => media::Stopped.into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An export whose time stands still is stuck, however often that time is repeated or
    /// if nothing arrives at all; finishing up is given longer.
    #[test]
    fn an_export_is_stuck_when_its_time_stands_still() {
        let start = std::time::Instant::now();
        let at = |s: u64| start + std::time::Duration::from_secs(s);
        let watch = Watch::new(60.0, start);
        assert!(!watch.stuck(at(10)));
        assert!(watch.stuck(at(EXPORT_STALL.as_secs() + 1)), "silence");

        let mut watch = Watch::new(60.0, start);
        watch.saw(10e6, at(1));
        for s in 2..=EXPORT_STALL.as_secs() + 2 {
            watch.saw(10e6, at(s));
        }
        assert!(watch.stuck(at(EXPORT_STALL.as_secs() + 2)), "the same time over and over");

        let mut watch = Watch::new(60.0, start);
        watch.saw(30e6, at(1));
        watch.saw(31e6, at(EXPORT_STALL.as_secs()));
        assert!(!watch.stuck(at(EXPORT_STALL.as_secs() + 60)), "moving on, even slowly");

        let mut watch = Watch::new(60.0, start);
        watch.saw(59.8e6, at(1));
        assert!(!watch.stuck(at(EXPORT_STALL.as_secs() + 60)), "rewriting the index at the end");
        assert!(watch.stuck(at(EXPORT_FINISH.as_secs() + 2)));

        // A GIF's palette: no time moves forward, but the processor is busy.
        let mut watch = Watch::new(60.0, start);
        for s in 1..=EXPORT_STALL.as_secs() + 60 {
            watch.worked(std::time::Duration::from_secs(s), at(s));
        }
        assert!(!watch.stuck(at(EXPORT_STALL.as_secs() + 60)), "busy without output");
        assert!(watch.stuck(at(2 * EXPORT_STALL.as_secs() + 61)), "then idle");
    }

    /// A process that closes its output and then hangs is still killed once it is stuck,
    /// and one that ends on its own is waited for.
    #[test]
    fn a_stuck_export_is_ended_after_its_output_closes() {
        let start = |script: &str| {
            Reaped(
                std::process::Command::new("powershell")
                    .args(["-NoProfile", "-Command", script])
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                    .unwrap(),
            )
        };
        let quick = |now| Watch { stall: std::time::Duration::from_secs(2), ..Watch::new(60.0, now) };
        // Its progress already ended (the sender is gone), the process lives on, idle.
        let (_, moved) = std::sync::mpsc::channel::<f64>();
        let begun = std::time::Instant::now();
        let hung = watch_export(start("Start-Sleep 60"), &moved, quick(begun), &|| false, |_| {});
        assert!(hung.is_err());
        let took = begun.elapsed();
        assert!(took >= std::time::Duration::from_secs(2) && took < std::time::Duration::from_secs(20), "{took:?}");

        let (_, moved) = std::sync::mpsc::channel::<f64>();
        let done = watch_export(start("exit 3"), &moved, quick(std::time::Instant::now()), &|| false, |_| {}).unwrap();
        assert_eq!(done.code(), Some(3));

        // Cancelled: ended at once, even while busy.
        let (_, moved) = std::sync::mpsc::channel::<f64>();
        let begun = std::time::Instant::now();
        let busy = start("while ($true) {}");
        let stop = watch_export(busy, &moved, Watch::new(60.0, begun), &|| begun.elapsed() > std::time::Duration::from_secs(1), |_| {});
        assert_eq!(stop.unwrap_err().to_string(), CANCELLED);
        assert!(begun.elapsed() < std::time::Duration::from_secs(10));
    }

    /// Split tracks appear under their own names only once whole; a stopped split leaves
    /// nothing a later open could take for them.
    #[test]
    fn split_tracks_appear_only_when_whole() {
        if ffmpeg::missing_for_test() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("replix-split-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let clip = dir.join("clip.mp4");
        let status = std::process::Command::new("ffmpeg")
            .args(["-hide_banner", "-loglevel", "error", "-f", "lavfi", "-i", "testsrc=s=64x64:d=1", "-f", "lavfi", "-i", "sine=d=1"])
            .args(["-f", "lavfi", "-i", "sine=f=880:d=1", "-map", "0:v", "-map", "1:a", "-map", "2:a", "-c:v", "libx264", "-c:a", "aac", "-y"])
            .arg(&clip)
            .status()
            .unwrap();
        assert!(status.success());
        let audio: Vec<PathBuf> = (0..2).map(|i| dir.join(format!("a{i}.m4a"))).collect();
        let names = || {
            let mut n: Vec<String> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
            n.sort();
            n
        };

        let (_rounds, round) = media::test_rounds();
        media::end_round(round);
        let stopped = split_tracks(&clip, &audio, round).unwrap_err();
        assert!(media::is_stopped(&stopped));
        assert_eq!(names(), ["clip.mp4"], "nothing left behind by a stopped split");

        let next = round + 1;
        split_tracks(&clip, &audio, next).unwrap();
        assert_eq!(names(), ["a0.m4a", "a1.m4a", "clip.mp4"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn waveform_has_one_bar_per_peak_and_follows_loudness() {
        if ffmpeg::missing_for_test() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("replix-wave-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let audio = dir.join("a0.m4a");
        // One second of silence, then one second of tone.
        let status = std::process::Command::new("ffmpeg")
            .args(["-hide_banner", "-loglevel", "error", "-f", "lavfi", "-i", "anullsrc=r=48000:cl=stereo:d=1", "-f", "lavfi", "-i", "sine=f=440:r=48000:d=1"])
            .args(["-filter_complex", "[0:a][1:a]concat=n=2:v=0:a=1", "-c:a", "aac", "-y"])
            .arg(&audio)
            .status()
            .unwrap();
        assert!(status.success());
        let (_rounds, round) = media::test_rounds();
        let peaks = waveform(&audio, 2.0, round).unwrap();
        assert_eq!(peaks.len(), PEAKS);
        let quiet = peaks[..PEAKS / 2 - 50].iter().copied().fold(0f32, f32::max);
        let loud = peaks[PEAKS / 2 + 50..].iter().copied().fold(1f32, f32::min);
        assert!(quiet < 0.1 && loud > 0.5, "quiet {quiet}, loud {loud}");
        // Opposite channels and a high tone still show as loud.
        let tricky = dir.join("a1.m4a");
        let status = std::process::Command::new("ffmpeg")
            .args(["-hide_banner", "-loglevel", "error", "-f", "lavfi", "-i", "aevalsrc=0.5*sin(2*PI*440*t)|-0.5*sin(2*PI*440*t):s=48000:d=1"])
            .args(["-f", "lavfi", "-i", "aevalsrc=0.5*sin(2*PI*9000*t)|0.5*sin(2*PI*9000*t):s=48000:d=1"])
            .args(["-filter_complex", "[0:a][1:a]concat=n=2:v=0:a=1", "-c:a", "aac", "-b:a", "192k", "-y"])
            .arg(&tricky)
            .status()
            .unwrap();
        assert!(status.success());
        let both = waveform(&tricky, 2.0, round).unwrap();
        let first = both[50..PEAKS / 2 - 50].iter().copied().fold(1f32, f32::min);
        let second = both[PEAKS / 2 + 50..PEAKS - 50].iter().copied().fold(1f32, f32::min);
        assert!(first > 0.8 && second > 0.8, "opposite channels {first}, high tone {second}");
        // The second call reads what the first one kept.
        assert_eq!(waveform(&audio, 2.0, round).unwrap(), peaks);

        // A 5.1 track whose channels oppose each other still shows as loud.
        let surround = dir.join("a3.m4a");
        let status = std::process::Command::new("ffmpeg")
            .args(["-hide_banner", "-loglevel", "error", "-f", "lavfi"])
            .args(["-i", "aevalsrc=0.5*sin(2*PI*440*t)|-0.5*sin(2*PI*440*t)|0.5*sin(2*PI*440*t)|0|-0.5*sin(2*PI*440*t)|0.5*sin(2*PI*440*t):s=48000:d=2:c=5.1"])
            .args(["-c:a", "aac", "-b:a", "384k", "-y"])
            .arg(&surround)
            .status()
            .unwrap();
        assert!(status.success());
        let wide = waveform(&surround, 2.0, round).unwrap();
        let low = wide[50..PEAKS - 50].iter().copied().fold(1f32, f32::min);
        // Silence would be near 0; a mixed-down 5.1 here would cancel to nothing.
        assert!(low > 0.6, "5.1 {low}");

        // A request stopped before it started never starts FFmpeg.
        let fresh = dir.join("a2.m4a");
        std::fs::copy(&audio, &fresh).unwrap();
        media::end_round(round);
        assert!(waveform(&fresh, 2.0, round).is_err_and(|e| media::is_stopped(&e)));
        assert!(!fresh.with_extension("peaks").exists());

        // A decoder that hangs without output is stopped by a stop all the same. The decoder
        // itself is started here, on a pipe nobody ever writes to, so the watch over the
        // decoding is what has to end it, not the header check before it.
        let round = round + 1;
        let name = format!("replix-test-stall-{}", std::process::id());
        let pipe = crate::pipe::PipeServer::create(&name).unwrap();
        let stall = PathBuf::from(&pipe.path);
        let wanted = || media::Demand::new(round).wanted();
        let stopper = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(800));
            media::end_round(round);
        });
        let started = std::time::Instant::now();
        let stalled = decode_peaks(&stall, 600.0, 2, &wanted, &dir.join("stall.peaks"));
        let took = started.elapsed();
        assert!(stalled.is_err_and(|e| media::is_stopped(&e)), "a stop, not a failure");
        assert!(took >= std::time::Duration::from_millis(700), "the decoder ran until the stop: {took:?}");
        assert!(took < std::time::Duration::from_secs(5), "took {took:?}");
        stopper.join().unwrap();
        drop(pipe);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
