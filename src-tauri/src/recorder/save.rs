//! Turning part of the buffer into a clip or session in the library.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use parking_lot::Mutex;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use super::pipeline::{area_key, segment_name, BufferDir, Mask};
use crate::audio::{self, Track, TrackKind};
use crate::encoder::{self, Encoder};
use crate::ffmpeg;
use crate::i18n::{tr, trf};
use crate::library::{self, Kind, Meta, TrackMeta};
use crate::platform::Area;

static SAVE_ID: AtomicU64 = AtomicU64::new(0);
static PUBLISH_LOCK: Mutex<()> = parking_lot::const_mutex(());
static STORAGE_LIMIT_LOCK: Mutex<()> = parking_lot::const_mutex(());

struct Rendered<'a> {
    source: &'a Path,
    length: f64,
    skipped: f64,
    tracks: Vec<Track>,
}

/// A save on its way to the library. The library shows a tile for it at once, with a
/// picture from the buffer, and swaps in the real clip once it is written.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SaveStarted {
    id: String,
    kind: Kind,
    title: String,
    game: String,
    created: u64,
    duration: f64,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SaveFinished {
    id: String,
    /// The library item it became; none when the save failed.
    item: Option<String>,
}

pub(super) struct Placeholder {
    app: AppHandle,
    id: String,
    preview: PathBuf,
    /// Set once the save is over; the preview is then moved or removed, and not made any more.
    finished: Arc<Mutex<bool>>,
}

impl Placeholder {
    /// Announces the save and makes its preview from `segment`, a piece of the buffer the
    /// save holds on to. Like thumbnails, the preview waits while Replix is in the
    /// background: then a game is likely running, and nobody is looking at the library.
    /// Without a segment there is no preview: the buffer may show a program kept out of
    /// recordings, and only the finished clip has it hidden.
    pub fn announce(app: &AppHandle, kind: Kind, game: &str, duration: f64, segment: Option<PathBuf>) -> Self {
        let id = format!("save-{}-{}", audio::now_ms(), SAVE_ID.fetch_add(1, Ordering::Relaxed));
        let cache = app.path().app_cache_dir().unwrap_or_else(|_| std::env::temp_dir());
        let preview = cache.join("previews").join(format!("{id}.jpg"));
        let finished = Arc::new(Mutex::new(false));
        let started = SaveStarted {
            id: id.clone(),
            kind,
            title: library::default_title(kind, game),
            game: game.to_string(),
            created: audio::now_ms(),
            duration,
        };
        let _ = app.emit("save-started", started);
        if let Some(segment) = segment {
            let (app2, id2, out, done) = (app.clone(), id.clone(), preview.clone(), finished.clone());
            std::thread::spawn(move || {
                let over = || *done.lock();
                while !over() && !(crate::page_visible() && crate::app_focused()) {
                    std::thread::sleep(Duration::from_millis(300));
                }
                if over() {
                    return;
                }
                let go = || !over() && crate::page_visible();
                if let Err(e) = library::make_thumb(&segment, Kind::Clip, 0.0, &out, &go) {
                    log::debug!("no preview for a save in progress: {e:#}");
                    return;
                }
                let done = done.lock();
                if *done {
                    let _ = std::fs::remove_file(&out);
                } else {
                    let _ = app2.emit("save-preview", (id2, out.to_string_lossy().into_owned()));
                }
            });
        }
        Self { app: app.clone(), id, preview, finished }
    }

    /// Ends the placeholder. A saved clip takes over its preview as its thumbnail, so its
    /// tile has a picture right away.
    pub fn finish(&self, saved: Option<&Path>) {
        let mut finished = self.finished.lock();
        if *finished {
            return;
        }
        *finished = true;
        let item = saved.map(library::id_for);
        let mut moved = false;
        if let Some(id) = &item {
            if self.preview.exists() {
                let dir = self.preview.parent().and_then(Path::parent).map(|c| c.join("thumbs"));
                if let Some(dir) = dir {
                    let _ = std::fs::create_dir_all(&dir);
                    let thumb = dir.join(format!("{id}.jpg"));
                    moved = thumb.exists() || std::fs::rename(&self.preview, &thumb).is_ok();
                }
            }
        }
        if !moved {
            let _ = std::fs::remove_file(&self.preview);
        }
        let _ = self.app.emit("save-finished", SaveFinished { id: self.id.clone(), item });
    }
}

impl Drop for Placeholder {
    fn drop(&mut self) {
        self.finish(None);
    }
}

pub(super) struct SaveJob {
    pub app: AppHandle,
    pub dir: Arc<BufferDir>,
    pub segments: Vec<u64>,
    pub tracks: Vec<Track>,
    pub kind: Kind,
    pub name: String,
    /// Seconds to skip at the start of the joined segments, and the length to keep.
    pub trim_to: Option<(f64, f64)>,
    pub duration: f64,
    pub fps: u32,
    pub clips_dir: PathBuf,
    pub limit_gb: u32,
    /// Stream time where the first segment starts, the joined recording's zero.
    pub origin: f64,
    /// Marked moments, in stream time.
    pub markers: Vec<f64>,
    /// Parts of the picture to black out, in stream time: windows of programs kept out of
    /// recordings.
    pub private: Vec<Mask>,
    /// For blacking out, which means encoding the picture again.
    pub encoder: &'static Encoder,
    pub kbps: u32,
    pub _pin: Arc<u64>,
    pub placeholder: Placeholder,
}

impl SaveJob {
    pub fn start(self) {
        std::thread::spawn(move || self.run());
    }

    fn run(self) {
        let result = self.write();
        drop(self._pin);
        self.placeholder
            .finish(result.as_ref().ok().map(|(_, out)| out.as_path()));
        match result {
            Ok((meta, _)) => {
                let what = if self.kind == Kind::Session {
                    tr("Session saved")
                } else {
                    tr("Clip saved")
                };
                let mut detail = format!(
                    "{} · {}",
                    self.name,
                    library::format_duration(meta.duration)
                );
                if meta.tracks.len() < self.tracks.len() {
                    let warning = if meta.tracks.is_empty() {
                        tr("Audio was unavailable. This recording was saved without sound.")
                    } else {
                        tr("Some audio tracks were unavailable and could not be saved.")
                    };
                    detail.push_str(&format!("\n{warning}"));
                }
                crate::toast(&self.app, &what, &detail);
                let _ = self.app.emit("library-changed", ());
                drop(self.dir);
                if self.limit_gb > 0 {
                    let _cleanup = STORAGE_LIMIT_LOCK.lock();
                    if library::enforce_limit(
                        &self.clips_dir,
                        self.limit_gb as u64 * 1024 * 1024 * 1024,
                    ) {
                        let _ = self.app.emit("library-changed", ());
                    }
                }
            }
            Err(e) => {
                log::error!("saving failed: {e:#}");
                crate::toast(&self.app, &tr("Saving failed"), &format!("{e}"));
            }
        }
    }

    fn write(&self) -> Result<(Meta, PathBuf)> {
        let id = SAVE_ID.fetch_add(1, Ordering::Relaxed);
        let stamp = format!("{}-{}-{id}", audio::now_ms(), std::process::id());
        let list = self.dir.0.join(format!("save-{stamp}.txt"));
        let label = if self.kind == Kind::Session {
            Some("Session")
        } else {
            None
        };
        let folder = library::new_path(&self.clips_dir, &self.name, "mp4", label);
        let joined = folder.with_file_name(format!(".replix-{stamp}.part"));
        let trimmed = folder.with_file_name(format!(".replix-{stamp}-trimmed.part"));
        let hidden = folder.with_file_name(format!(".replix-{stamp}-hidden.part"));
        let graph = self.dir.0.join(format!("hide-{stamp}.txt"));
        let result = (|| {
            let Rendered {
                mut source,
                length,
                skipped,
                tracks,
            } = self.render(&list, &joined, &trimmed)?;
            // Markers and private stretches move with the clip's own start.
            let zero = self.origin + skipped;
            let hide = relative_masks(&self.private, zero, length);
            if !hide.is_empty() {
                self.black_out(source, &hidden, &graph, &hide, length, &tracks).context(tr("Could not hide a program kept out of recordings, so the clip was not saved"))?;
                source = &hidden;
            }
            // The recording is finished; a probe that fails (a busy drive) only costs the
            // details it would add, never the clip. A later scan fills them in.
            let probed = ffmpeg::probe(source);
            let unread = probed.is_err();
            let info = probed.unwrap_or_else(|e| {
                log::warn!("could not read the saved recording back: {e:#}");
                ffmpeg::MediaInfo {
                    duration: length,
                    ..Default::default()
                }
            });
            let _publish = PUBLISH_LOCK.lock();
            let out = library::new_path(&self.clips_dir, &self.name, "mp4", label);
            let meta = Meta {
                kind: self.kind,
                title: library::default_title(self.kind, &self.name),
                game: self.name.clone(),
                created: audio::now_ms(),
                duration: info.duration,
                width: info.width,
                height: info.height,
                tracks: tracks
                    .iter()
                    .map(|t| TrackMeta {
                        name: t.name.clone(),
                        kind: t.kind,
                        exe: t.exe.clone(),
                    })
                    .collect(),
                unread,
                markers: library::tidy_markers(
                    self.markers
                        .iter()
                        .map(|m| library::Marker::at(m - zero))
                        .collect(),
                    length,
                ),
                ..Default::default()
            };
            // The sidecar comes first: a library scan that finds the clip then never has to
            // guess what it is.
            if let Err(e) = library::write_meta(&out, &meta) {
                // The clip still goes in; a scan then describes it from the file.
                log::warn!("could not write the details of {}: {e}", out.display());
            }
            if let Err(e) = std::fs::rename(source, &out) {
                library::remove_meta(&out);
                return Err(e).context(tr("Could not move the recording into the library"));
            }
            Ok((meta, out))
        })();
        for path in [list, joined, trimmed, hidden, graph] {
            let _ = std::fs::remove_file(path);
        }
        result
    }

    /// The finished file, its length, and how much of the joined segments it skips: trimmed
    /// if that worked, whole if not.
    fn render<'a>(&self, list: &Path, joined: &'a Path, trimmed: &'a Path) -> Result<Rendered<'a>> {
        // The segments share one timeline, so they are read back to back as a single stream.
        // That skips opening and probing every segment on its own. Names in the list are
        // relative to the buffer folder, which FFmpeg runs in.
        let text: String = self
            .segments
            .iter()
            .map(|i| segment_name(*i) + "\n")
            .collect();
        std::fs::write(list, text)?;
        let tracks: Vec<(usize, &str)> = self
            .tracks
            .iter()
            .map(|t| (t.slot, t.name.as_str()))
            .collect();
        let remux = |out, trim| {
            remux_segments(
                &self.dir.0,
                list,
                &tracks,
                trim,
                out,
                self.duration,
                self.fps,
            )
        };
        if let Some(trim) = self.trim_to {
            match remux(trimmed, Some(trim)) {
                Ok(slots) => {
                    return Ok(Rendered {
                        source: trimmed,
                        length: trim.1,
                        skipped: trim.0,
                        tracks: retained_tracks(&self.tracks, &slots),
                    })
                }
                Err(e) => {
                    log::warn!("trimming the clip failed, keeping it whole: {e:#}");
                    // Whatever the failed pass wrote is no clip; it must not take up room.
                    let _ = std::fs::remove_file(trimmed);
                }
            }
        }
        let slots = remux(joined, None)?;
        Ok(Rendered {
            source: joined,
            length: self.duration,
            skipped: 0.0,
            tracks: retained_tracks(&self.tracks, &slots),
        })
    }

    /// Writes `source` again with the windows of kept-out programs black, and the desktop's
    /// sound silent while one was on screen. With the capture's encoder first; software if
    /// that one refuses frames from a file. The filter graph goes into the file `graph`: with
    /// many boxes it would not fit on a command line.
    fn black_out(
        &self,
        source: &Path,
        out: &Path,
        graph: &Path,
        hide: &[Mask],
        length: f64,
        tracks: &[Track],
    ) -> Result<()> {
        let (desktop, names) = audio_layout(tracks);
        let timeout = Duration::from_secs_f64(120.0 + length * 3.0);
        let mut last = None;
        for encoder in [self.encoder.for_export(), encoder::software()] {
            std::fs::write(graph, black_out_graph(hide, desktop, encoder))?;
            let args = black_out_args(source, out, graph, desktop, &names, encoder, self.kbps);
            match ffmpeg::run_with_timeout(&args, timeout) {
                Ok(()) => return Ok(()),
                Err(e) => {
                    log::warn!("blacking out with {} failed: {e:#}", encoder.id);
                    let _ = std::fs::remove_file(out);
                    last = Some(e);
                }
            }
            if encoder.codec == "libx264" {
                break;
            }
        }
        Err(last.unwrap_or_else(|| anyhow::anyhow!("no encoder")))
    }
}

fn retained_tracks(tracks: &[Track], slots: &[usize]) -> Vec<Track> {
    tracks
        .iter()
        .filter(|t| slots.contains(&t.slot))
        .cloned()
        .collect()
}

fn audio_layout(tracks: &[Track]) -> (Option<usize>, Vec<&str>) {
    (
        tracks.iter().position(|t| t.kind == TrackKind::Desktop),
        tracks.iter().map(|t| t.name.as_str()).collect(),
    )
}

/// Remuxes without decoding audio. Recovery is only for MP4's missing sample-rate error;
/// only AAC slots proven empty across the whole selection may be removed.
fn remux_segments(
    dir: &Path,
    list: &Path,
    tracks: &[(usize, &str)],
    trim: Option<(f64, f64)>,
    out: &Path,
    seconds: f64,
    fps: u32,
) -> Result<Vec<usize>> {
    let length = trim.map_or(seconds, |t| t.1);
    let input = joined_input(list, tracks, trim);
    match write_mp4(dir, &input, out, moov_room(length, fps, tracks.len())) {
        Ok(()) => return Ok(tracks.iter().map(|t| t.0).collect()),
        Err(e) if e.to_string().contains("sample rate not set") => {
            log::warn!(
                "audio parameters missing while saving; checking the entire selection: {e:#}"
            );
        }
        Err(e) => return Err(e),
    }
    let empty = ffmpeg::empty_aac_slots(dir, list)?;
    let kept: Vec<_> = tracks
        .iter()
        .copied()
        .filter(|t| !empty.contains(&t.0))
        .collect();
    let removed: Vec<_> = tracks.iter().filter(|t| empty.contains(&t.0)).collect();
    log::warn!("saving without packetless AAC tracks: {removed:?}");
    // A retained track may first play beyond FFmpeg's normal five-second/5 MB probe.
    // Let this exceptional retry find its header anywhere in the selection too.
    let list_path = if list.is_absolute() {
        list.to_path_buf()
    } else {
        dir.join(list)
    };
    let bytes = std::fs::read_to_string(list_path)?
        .lines()
        .try_fold(0u64, |total, name| {
            std::fs::metadata(dir.join(name)).map(|m| total.saturating_add(m.len()))
        })?;
    let mut input: Vec<String> = [
        "-probesize".into(),
        bytes.max(32).to_string(),
        "-analyzeduration".into(),
        i64::MAX.to_string(),
    ]
    .into();
    input.extend(joined_input(list, &kept, trim));
    write_mp4(dir, &input, out, moov_room(length, fps, kept.len()))?;
    Ok(kept.iter().map(|t| t.0).collect())
}

/// Reserves room for the MP4 index at the front, avoiding a second complete write.
/// Only an actual undersized index reservation warrants the +faststart fallback.
fn write_mp4(dir: &Path, input: &[String], out: &Path, room: u64) -> Result<()> {
    let mut args = input.to_vec();
    args.extend([
        "-c".into(),
        "copy".into(),
        "-moov_size".into(),
        room.to_string(),
    ]);
    args.extend(["-f", "mp4", "-y"].map(String::from));
    args.push(ffmpeg::path_arg(out));
    match ffmpeg::run_in(dir, &args) {
        Ok(()) => return Ok(()),
        Err(e) if e.to_string().contains("reserved_moov_size is too small") => {
            log::warn!("the reserved MP4 index room was too small, writing it again: {e:#}");
        }
        Err(e) => return Err(e),
    }
    let mut args = input.to_vec();
    args.extend(["-c", "copy", "-movflags", "+faststart", "-f", "mp4", "-y"].map(String::from));
    args.push(ffmpeg::path_arg(out));
    ffmpeg::run_in(dir, &args)
}

/// Masks in stream time as seconds of a clip that starts at `zero` and runs `length`.
fn relative_masks(masks: &[Mask], zero: f64, length: f64) -> Vec<Mask> {
    masks
        .iter()
        .map(|m| Mask { start: (m.start - zero).max(0.0), end: (m.end - zero).min(length), ..*m })
        .filter(|m| m.end > m.start)
        .collect()
}

/// One black box per distinct rectangle, enabled for each of its intervals without filling
/// pauses. The box is a little bigger so no edge shows; desktop sound is silent during any mask.
pub(super) fn black_out_graph(hide: &[Mask], desktop: Option<usize>, encoder: &Encoder) -> String {
    let between = |m: &Mask| format!("between(t,{:.3},{:.3})", m.start, m.end);
    // Repeated appearances need more intervals, not more filters over every video frame.
    let mut by_area: BTreeMap<[u32; 4], Vec<&Mask>> = BTreeMap::new();
    for m in hide {
        by_area.entry(area_key(&m.area)).or_default().push(m);
    }
    let mut graph = String::from("[0:v]");
    for masks in by_area.values() {
        let when = masks
            .iter()
            .map(|m| between(m))
            .collect::<Vec<_>>()
            .join("+");
        graph.push_str(&format!("{}:enable='{when}',", black_box(&masks[0].area)));
    }
    graph.push_str(&format!("format={}[v]", encoder.export_pix_fmt));
    if let Some(d) = desktop {
        let when = hide.iter().map(between).collect::<Vec<_>>().join("+");
        graph.push_str(&format!(";[0:a:{d}]volume=0:enable='{when}'[desk]"));
    }
    graph
}

/// A filter that fills this part of the picture black, whatever size the picture is.
pub(crate) fn black_box(a: &Area) -> String {
    format!(
        "drawbox=x='floor(iw*{:.5})-2':y='floor(ih*{:.5})-2':w='ceil(iw*{:.5})+4':h='ceil(ih*{:.5})+4':color=black:t=fill",
        a.left,
        a.top,
        a.right - a.left,
        a.bottom - a.top
    )
}

fn black_out_args(
    source: &Path,
    out: &Path,
    graph: &Path,
    desktop: Option<usize>,
    names: &[&str],
    encoder: &Encoder,
    kbps: u32,
) -> Vec<String> {
    let mut args: Vec<String> =
        vec!["-y".into(), "-i".into(), ffmpeg::path_arg(source), ffmpeg::graph_file_option().into(), ffmpeg::path_arg(graph)];
    args.extend(["-map".into(), "[v]".into()]);
    for (k, name) in names.iter().enumerate() {
        args.push("-map".into());
        args.push(if Some(k) == desktop { "[desk]".into() } else { format!("0:a:{k}") });
        args.extend(ffmpeg::name_track(k, name));
    }
    args.extend(encoder::args(encoder, kbps));
    args.extend(["-c:a".into(), "copy".into()]);
    if let Some(d) = desktop {
        args.extend([format!("-c:a:{d}"), "aac".into(), format!("-b:a:{d}"), "160k".into()]);
    }
    args.extend(["-movflags".into(), "+faststart".into(), "-f".into(), "mp4".into(), ffmpeg::path_arg(out)]);
    args
}

/// Reads the segments in `list` as one stream with the chosen audio streams and their titles.
/// With `trim` (seconds to skip, seconds to keep), the clip is cut in the same pass: the
/// first segment starts on a keyframe, and shifting the stream back by the part before the
/// clip leaves that keyframe at a negative time, which the MP4 hides with an edit list.
/// Writing stops at the key press. No joined copy is written first.
pub(super) fn joined_input(list: &Path, tracks: &[(usize, &str)], trim: Option<(f64, f64)>) -> Vec<String> {
    let mut args: Vec<String> = Vec::new();
    if let Some((skip, _)) = trim {
        args.extend(["-itsoffset".into(), format!("{:.3}", -skip)]);
    }
    args.extend(["-i".into(), format!("concatf:{}", ffmpeg::path_arg(list)), "-map".into(), "0:v:0".into()]);
    for (k, (slot, name)) in tracks.iter().enumerate() {
        args.extend(["-map".into(), format!("0:a:{slot}")]);
        args.extend(ffmpeg::name_track(k, name));
    }
    if let Some((_, seconds)) = trim {
        args.extend(["-t".into(), format!("{seconds:.3}")]);
    }
    args
}

/// Bytes an MP4 index needs for this much video, with plenty of margin: a few bytes for
/// each video frame and audio packet. Unused room stays behind as padding.
fn moov_room(seconds: f64, fps: u32, tracks: usize) -> u64 {
    let per_second = fps as u64 * 32 + tracks as u64 * 47 * 16;
    64 * 1024 + (seconds.max(0.0) as u64 + 4) * per_second
}

/// The toast for a clip that cannot be saved because nothing is in the buffer yet.
pub(super) fn nothing_yet() -> (String, String) {
    (tr("Nothing to save yet"), tr("The buffer is still filling up."))
}

/// "12:34 into the session", for the marker toast.
pub(super) fn into_session(seconds: f64) -> String {
    trf("{time} into the session", &[("time", &library::format_duration(seconds))])
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    /// FFmpeg as used by the tests: the one on PATH. Tests that need it skip without it.
    fn run(dir: &Path, args: &[&str]) {
        let ok = Command::new("ffmpeg").current_dir(dir).args(["-hide_banner", "-loglevel", "error", "-y"]).args(args).status().unwrap();
        assert!(ok.success(), "ffmpeg {args:?}");
    }

    /// Every frame and packet with its timestamp, so timing differences show up too.
    fn frames(dir: &Path, file: &str) -> String {
        let out = Command::new("ffmpeg").current_dir(dir).args(["-hide_banner", "-loglevel", "error", "-i", file, "-map", "0", "-f", "framemd5", "-"]).output().unwrap();
        assert!(out.status.success() && out.stderr.is_empty(), "decoding {file}: {}", String::from_utf8_lossy(&out.stderr));
        // Headers included: they carry each stream's time base, which timestamps depend on.
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    /// The one-pass trim must give exactly what joining the segments and then trimming the
    /// joined file gave: same frames, same samples, same timestamps.
    #[test]
    fn one_pass_trim_matches_join_then_trim() {
        if ffmpeg::missing_for_test() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("replix-trim-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // Like the recorder: keyframes every two seconds, two-second MPEG-TS segments,
        // several audio streams. B-frames on, as hardware encoders may use them.
        run(
            &dir,
            &[
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=320x180:rate=60",
                "-f",
                "lavfi",
                "-i",
                "sine=f=440:r=48000",
                "-f",
                "lavfi",
                "-i",
                "sine=f=880:r=48000",
                "-t",
                "10",
                "-map",
                "0:v",
                "-map",
                "1:a",
                "-map",
                "2:a",
                "-c:v",
                "libx264",
                "-preset",
                "ultrafast",
                "-bf",
                "2",
                "-force_key_frames",
                "expr:gte(t,n_forced*2)",
                "-c:a",
                "aac",
                "-f",
                "segment",
                "-segment_time",
                "2",
                "-segment_format",
                "mpegts",
                "seg%06d.ts",
            ],
        );
        std::fs::write(
            dir.join("list.txt"),
            "seg000001.ts\nseg000002.ts\nseg000003.ts\n",
        )
        .unwrap();
        let tracks = [(0, "Game"), (1, "Microphone")];
        let list = Path::new("list.txt");
        // Skips near the start, in the middle and just before the next keyframe.
        for (skip, length) in [(0.02, 5.0), (0.979, 4.5), (1.98, 3.9)] {
            // The old way: join, then trim the joined file.
            let mut old = joined_input(list, &tracks, None);
            old.extend(["-c", "copy", "-f", "mp4", "joined.mp4"].map(String::from));
            run(&dir, &old.iter().map(String::as_str).collect::<Vec<_>>());
            let (ss, t) = (format!("{skip:.3}"), format!("{length:.3}"));
            run(
                &dir,
                &[
                    "-ss",
                    &ss,
                    "-t",
                    &t,
                    "-i",
                    "joined.mp4",
                    "-map",
                    "0",
                    "-c",
                    "copy",
                    "-f",
                    "mp4",
                    "old.mp4",
                ],
            );
            // What the recorder runs now.
            let slots = remux_segments(
                &dir,
                list,
                &tracks,
                Some((skip, length)),
                Path::new("new.mp4"),
                6.0,
                60,
            )
            .unwrap();
            assert_eq!(slots, vec![0, 1]);
            let (a, b) = (frames(&dir, "old.mp4"), frames(&dir, "new.mp4"));
            assert!(!a.is_empty());
            assert_eq!(a, b, "skip {skip}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn audio_fixture(label: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("replix-recovery-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        run(
            &dir,
            &[
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=160x90:rate=30",
                "-f",
                "lavfi",
                "-i",
                "sine=f=440:r=48000",
                "-f",
                "lavfi",
                "-i",
                "sine=f=660:r=48000",
                "-f",
                "lavfi",
                "-i",
                "sine=f=880:r=48000",
                "-f",
                "lavfi",
                "-i",
                "sine=f=1100:r=48000",
                "-t",
                "12",
                "-map",
                "0:v",
                "-map",
                "1:a",
                "-map",
                "2:a",
                "-map",
                "3:a",
                "-map",
                "4:a",
                "-c:v",
                "libx264",
                "-preset",
                "ultrafast",
                "-threads:v",
                "1",
                "-bf",
                "0",
                "-g",
                "60",
                "-keyint_min",
                "60",
                "-sc_threshold",
                "0",
                "-c:a",
                "aac",
                "-f",
                "segment",
                "-segment_time",
                "2",
                "-segment_format",
                "mpegts",
                "seg%06d.ts",
            ],
        );
        dir
    }

    /// Leaves PAT/PMT declarations intact, like a capture whose audio inputs ended early.
    fn remove_audio_packets(dir: &Path, segment: usize, slots: &[usize]) {
        let path = dir.join(segment_name(segment as u64));
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(bytes.len() % 188, 0);
        let kept: Vec<u8> = bytes
            .as_chunks::<188>()
            .0
            .iter()
            .filter(|packet| {
                assert_eq!(packet[0], 0x47);
                let pid = ((packet[1] as usize & 31) << 8) | packet[2] as usize;
                !slots.iter().any(|slot| pid == 0x101 + slot)
            })
            .flatten()
            .copied()
            .collect();
        std::fs::write(path, kept).unwrap();
    }

    #[test]
    fn packetless_aac_recovers_video_and_its_trim() {
        if ffmpeg::missing_for_test() {
            return;
        }
        let dir = audio_fixture("empty");
        for segment in 2..6 {
            remove_audio_packets(&dir, segment, &[0, 1, 2, 3]);
        }
        std::fs::write(
            dir.join("list.txt"),
            "seg000002.ts\nseg000003.ts\nseg000004.ts\nseg000005.ts\n",
        )
        .unwrap();
        let list = Path::new("list.txt");
        let tracks = [(0, "Desktop"), (1, "Game"), (2, "App"), (3, "Microphone")];
        let err = write_mp4(
            &dir,
            &joined_input(list, &tracks, None),
            Path::new("broken.mp4"),
            200000,
        )
        .unwrap_err();
        assert!(err.to_string().contains("sample rate not set"), "{err:#}");
        assert_eq!(
            ffmpeg::empty_aac_slots(&dir, list).unwrap(),
            vec![0, 1, 2, 3]
        );
        for (name, trim) in [("whole.mp4", None), ("trim.mp4", Some((0.979, 5.0)))] {
            let slots =
                remux_segments(&dir, list, &tracks, trim, Path::new(name), 8.0, 30).unwrap();
            assert!(slots.is_empty());
            write_mp4(
                &dir,
                &joined_input(list, &[], trim),
                Path::new("expected.mp4"),
                200000,
            )
            .unwrap();
            assert_eq!(frames(&dir, name), frames(&dir, "expected.mp4"));
            assert!(ffmpeg::probe(&dir.join(name))
                .unwrap()
                .audio_titles
                .is_empty());
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn recovery_keeps_late_audio_and_remaps_desktop_privacy() {
        if ffmpeg::missing_for_test() {
            return;
        }
        let dir = audio_fixture("partial");
        for segment in 0..6 {
            remove_audio_packets(
                &dir,
                segment,
                if segment < 4 { &[0, 2, 3] } else { &[0, 2] },
            );
        }
        std::fs::write(
            dir.join("list.txt"),
            (0..6).map(|i| segment_name(i) + "\n").collect::<String>(),
        )
        .unwrap();
        std::fs::write(dir.join("early.txt"), "seg000000.ts\n").unwrap();
        assert_eq!(
            ffmpeg::empty_aac_slots(&dir, Path::new("early.txt")).unwrap(),
            vec![0, 2, 3]
        );
        let tracks = vec![
            Track {
                slot: 0,
                name: "Game".into(),
                exe: Some("game".into()),
                kind: TrackKind::Game,
            },
            Track {
                slot: 1,
                name: "Desktop".into(),
                exe: None,
                kind: TrackKind::Desktop,
            },
            Track {
                slot: 2,
                name: "App".into(),
                exe: Some("app".into()),
                kind: TrackKind::App,
            },
            Track {
                slot: 3,
                name: "Microphone".into(),
                exe: None,
                kind: TrackKind::Mic,
            },
        ];
        let names: Vec<_> = tracks.iter().map(|t| (t.slot, t.name.as_str())).collect();
        let slots = remux_segments(
            &dir,
            Path::new("list.txt"),
            &names,
            None,
            Path::new("saved.mp4"),
            12.0,
            30,
        )
        .unwrap();
        assert_eq!(slots, vec![1, 3]);
        let kept = retained_tracks(&tracks, &slots);
        assert_eq!(
            kept.iter()
                .map(|t| (t.slot, t.kind, t.exe.as_deref()))
                .collect::<Vec<_>>(),
            vec![(1, TrackKind::Desktop, None), (3, TrackKind::Mic, None)]
        );
        let expected = vec![Some("Desktop".into()), Some("Microphone".into())];
        assert_eq!(
            ffmpeg::probe(&dir.join("saved.mp4")).unwrap().audio_titles,
            expected
        );
        let (desktop, names) = audio_layout(&kept);
        assert_eq!(desktop, Some(0));
        let masks = [Mask {
            area: Area {
                left: 0.0,
                top: 0.0,
                right: 0.5,
                bottom: 1.0,
            },
            start: 1.0,
            end: 2.0,
        }];
        let graph = dir.join("graph.txt");
        std::fs::write(
            &graph,
            black_out_graph(&masks, desktop, encoder::software()),
        )
        .unwrap();
        let args = black_out_args(
            &dir.join("saved.mp4"),
            &dir.join("hidden.mp4"),
            &graph,
            desktop,
            &names,
            encoder::software(),
            1000,
        );
        ffmpeg::run(&args).unwrap();
        assert_eq!(
            ffmpeg::probe(&dir.join("hidden.mp4")).unwrap().audio_titles,
            expected
        );
        let samples = Command::new("ffmpeg")
            .current_dir(&dir)
            .args([
                "-v",
                "error",
                "-ss",
                "1.3",
                "-i",
                "hidden.mp4",
                "-t",
                "0.2",
                "-map",
                "0:a:0",
                "-f",
                "f32le",
                "-",
            ])
            .output()
            .unwrap();
        assert!(samples.status.success() && !samples.stdout.is_empty());
        assert!(samples
            .stdout
            .as_chunks::<4>()
            .0
            .iter()
            .all(|s| f32::from_le_bytes(*s).abs() < 0.001));
        // Losing the desktop must never mute the microphone that took its output index.
        let mic = retained_tracks(&tracks, &[3]);
        assert_eq!(audio_layout(&mic), (None, vec!["Microphone"]));
        assert!(!black_out_graph(&masks, None, encoder::software()).contains("[desk]"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_small_moov_reservation_retries_without_losing_tracks() {
        if ffmpeg::missing_for_test() {
            return;
        }
        let dir = audio_fixture("moov");
        std::fs::write(dir.join("list.txt"), "seg000000.ts\nseg000001.ts\n").unwrap();
        let input = joined_input(
            Path::new("list.txt"),
            &[(1, "Desktop"), (3, "Microphone")],
            None,
        );
        write_mp4(&dir, &input, Path::new("saved.mp4"), 8).unwrap();
        let info = ffmpeg::probe(&dir.join("saved.mp4")).unwrap();
        assert_eq!(
            info.audio_titles,
            vec![Some("Desktop".into()), Some("Microphone".into())]
        );
        let error = write_mp4(
            &dir,
            &input,
            &Path::new("missing").join("saved.mp4"),
            200000,
        )
        .unwrap_err();
        assert!(
            error.to_string().contains("No such file or directory"),
            "{error:#}"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    /// Blacking out turns only the window's part of the picture black, only while it is
    /// shown, silences the desktop's sound then, and keeps every track with its name.
    #[test]
    fn only_the_private_window_is_black() {
        if ffmpeg::missing_for_test() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("replix-private-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        run(&dir, &[
            "-f", "lavfi", "-i", "color=c=white:size=320x180:rate=30", "-f", "lavfi", "-i", "sine=f=440:r=48000",
            "-f", "lavfi", "-i", "sine=f=880:r=48000", "-t", "4", "-map", "0:v", "-map", "1:a", "-map", "2:a",
            "-c:v", "libx264", "-preset", "ultrafast", "-c:a", "aac", "in.mp4",
        ]);
        let window = Area { left: 0.0, top: 0.0, right: 0.4, bottom: 1.0 };
        let hide = [Mask { area: window, start: 1.0, end: 2.0 }];
        let graph = dir.join("graph.txt");
        std::fs::write(&graph, black_out_graph(&hide, Some(0), encoder::software())).unwrap();
        let args = black_out_args(&dir.join("in.mp4"), &dir.join("out.mp4"), &graph, Some(0), &["Desktop", "Microphone"], encoder::software(), 2000);
        let ok = Command::new("ffmpeg").args(["-hide_banner", "-loglevel", "error"]).args(&args).status().unwrap();
        assert!(ok.success());
        // How bright a part of the picture is: `crop` picks it.
        let level = |crop: &str, at: &str| {
            let out = Command::new("ffmpeg")
                .args(["-hide_banner", "-ss", at, "-t", "0.5", "-i"])
                .arg(dir.join("out.mp4"))
                .args(["-vf", &format!("{crop},signalstats,metadata=print:key=lavfi.signalstats.YAVG"), "-f", "null", "-"])
                .output()
                .unwrap();
            let log = String::from_utf8_lossy(&out.stderr).into_owned();
            log.lines().filter_map(|l| l.split("YAVG=").nth(1)).filter_map(|v| v.trim().parse::<f64>().ok()).fold(0.0, f64::max)
        };
        let (left, right) = ("crop=iw*0.3:ih:0:0", "crop=iw*0.5:ih:iw*0.5:0");
        assert!(level(left, "1.2") < 20.0, "the window is black while shown");
        assert!(level(right, "1.2") > 200.0, "the rest of the screen stays");
        assert!(level(left, "3.0") > 200.0, "untouched after it");
        let info = ffmpeg::probe(&dir.join("out.mp4")).unwrap();
        assert_eq!(
            info.audio_titles,
            vec![Some("Desktop".to_string()), Some("Microphone".to_string())]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn identical_rectangles_share_a_filter_without_joining_time_gaps() {
        let area = Area {
            left: 0.1,
            top: 0.1,
            right: 0.4,
            bottom: 0.4,
        };
        let other = Area {
            left: 0.6,
            top: 0.6,
            right: 0.9,
            bottom: 0.9,
        };
        let hide = [
            Mask {
                area,
                start: 1.0,
                end: 2.0,
            },
            Mask {
                area: other,
                start: 1.0,
                end: 2.0,
            },
            Mask {
                area,
                start: 3.0,
                end: 4.0,
            },
        ];
        let graph = black_out_graph(&hide, Some(0), encoder::software());
        assert_eq!(graph.matches("drawbox=").count(), 2);
        assert!(graph.contains(&format!(
            "{}:enable='between(t,1.000,2.000)+between(t,3.000,4.000)'",
            black_box(&area)
        )));
        assert!(graph.contains(";[0:a:0]volume=0:enable='between(t,1.000,2.000)+between(t,1.000,2.000)+between(t,3.000,4.000)'[desk]"));
    }

    #[test]
    fn graph_groups_only_bit_identical_rectangles() {
        let area = Area {
            left: 0.1,
            top: 0.1,
            right: 0.4,
            bottom: 0.4,
        };
        let other = Area {
            left: f32::from_bits(area.left.to_bits() + 1),
            ..area
        };
        let hide = [
            Mask {
                area,
                start: 1.0,
                end: 2.0,
            },
            Mask {
                area: other,
                start: 1.0,
                end: 2.0,
            },
        ];
        let graph = black_out_graph(&hide, None, encoder::software());
        assert_eq!(graph.matches("drawbox=").count(), 2);
    }

    #[test]
    fn a_long_graph_keeps_every_interval_for_a_repeated_rectangle() {
        let area = Area {
            left: 0.1,
            top: 0.1,
            right: 0.4,
            bottom: 0.4,
        };
        let hide: Vec<_> = (0..200)
            .map(|i| Mask {
                area,
                start: i as f64 * 3.0,
                end: i as f64 * 3.0 + 1.0,
            })
            .collect();
        let graph = black_out_graph(&hide, None, encoder::software());
        assert_eq!(graph.matches("drawbox=").count(), 1);
        assert_eq!(graph.matches("between(t,").count(), hide.len());
        for m in hide {
            assert!(graph.contains(&format!("between(t,{:.3},{:.3})", m.start, m.end)));
        }
    }

    #[test]
    fn four_private_fragments_leave_the_foreground_opening_visible() {
        if ffmpeg::missing_for_test() {
            return;
        }
        let dir =
            std::env::temp_dir().join(format!("replix-private-opening-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        run(
            &dir,
            &[
                "-f",
                "lavfi",
                "-i",
                "color=c=white:size=320x180:rate=30",
                "-t",
                "5",
                "-c:v",
                "libx264",
                "-preset",
                "ultrafast",
                "in.mp4",
            ],
        );
        let areas = [
            Area {
                left: 0.0,
                top: 0.0,
                right: 1.0,
                bottom: 0.3,
            },
            Area {
                left: 0.0,
                top: 0.7,
                right: 1.0,
                bottom: 1.0,
            },
            Area {
                left: 0.0,
                top: 0.3,
                right: 0.3,
                bottom: 0.7,
            },
            Area {
                left: 0.7,
                top: 0.3,
                right: 1.0,
                bottom: 0.7,
            },
        ];
        let hide: Vec<_> = areas
            .iter()
            .flat_map(|&area| {
                [(0.5, 1.5), (2.5, 3.5)].map(|(start, end)| Mask { area, start, end })
            })
            .collect();
        let graph = dir.join("graph.txt");
        std::fs::write(&graph, black_out_graph(&hide, None, encoder::software())).unwrap();
        let args = black_out_args(
            &dir.join("in.mp4"),
            &dir.join("out.mp4"),
            &graph,
            None,
            &[],
            encoder::software(),
            2000,
        );
        let ok = Command::new("ffmpeg")
            .args(["-hide_banner", "-loglevel", "error"])
            .args(&args)
            .status()
            .unwrap();
        assert!(ok.success());
        let level = |crop: &str, at: &str| {
            let out = Command::new("ffmpeg")
                .args(["-hide_banner", "-loglevel", "error", "-ss", at, "-i"])
                .arg(dir.join("out.mp4"))
                .args([
                    "-frames:v",
                    "1",
                    "-vf",
                    crop,
                    "-pix_fmt",
                    "gray",
                    "-f",
                    "rawvideo",
                    "-",
                ])
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "decoding {crop} at {at}: {}",
                String::from_utf8_lossy(&out.stderr)
            );
            assert!(!out.stdout.is_empty(), "no frame for {crop} at {at}");
            out.stdout.iter().map(|&v| v as f64).sum::<f64>() / out.stdout.len() as f64
        };
        let surround = [
            "crop=40:20:140:10",
            "crop=40:20:140:150",
            "crop=40:20:10:80",
            "crop=40:20:270:80",
        ];
        for at in ["0.8", "2.8"] {
            assert!(
                level("crop=40:20:140:80", at) > 200.0,
                "the foreground opening stays visible"
            );
            for crop in surround {
                assert!(
                    level(crop, at) < 20.0,
                    "the surrounding private fragments are black"
                );
            }
        }
        for at in ["0.1", "2.0", "4.0"] {
            for crop in surround {
                assert!(level(crop, at) > 200.0, "the time gaps stay unmasked");
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn masks_follow_the_clip_start() {
        let area = Area { left: 0.0, top: 0.0, right: 0.5, bottom: 0.5 };
        let masks = [Mask { area, start: 9.0, end: 12.0 }, Mask { area, start: 40.0, end: 50.0 }];
        assert_eq!(relative_masks(&masks, 10.0, 20.0), vec![Mask { area, start: 0.0, end: 2.0 }]);
        assert!(relative_masks(&masks[..1], 30.0, 20.0).is_empty());
    }

    #[test]
    fn moov_room_grows_with_length() {
        assert!(moov_room(60.0, 60, 6) > moov_room(30.0, 60, 6));
        assert!(moov_room(-1.0, 60, 0) >= 64 * 1024);
    }
}
