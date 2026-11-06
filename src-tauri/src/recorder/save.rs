//! Turning part of the buffer into a clip or session in the library.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use parking_lot::Mutex;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use super::pipeline::{segment_name, BufferDir, Mask};
use crate::audio::{self, Track, TrackKind};
use crate::encoder::{self, Encoder};
use crate::ffmpeg;
use crate::i18n::{tr, trf};
use crate::library::{self, Kind, Meta, TrackMeta};
use crate::platform::Area;

static SAVE_ID: AtomicU64 = AtomicU64::new(0);
static PUBLISH_LOCK: Mutex<()> = parking_lot::const_mutex(());
static STORAGE_LIMIT_LOCK: Mutex<()> = parking_lot::const_mutex(());

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
        self.placeholder.finish(result.as_ref().ok().map(|(_, out)| out.as_path()));
        match result {
            Ok((meta, _)) => {
                let what = if self.kind == Kind::Session { tr("Session saved") } else { tr("Clip saved") };
                crate::toast(&self.app, &what, &format!("{} · {}", self.name, library::format_duration(meta.duration)));
                let _ = self.app.emit("library-changed", ());
                drop(self.dir);
                if self.limit_gb > 0 {
                    let _cleanup = STORAGE_LIMIT_LOCK.lock();
                    if library::enforce_limit(&self.clips_dir, self.limit_gb as u64 * 1024 * 1024 * 1024) {
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
        let label = if self.kind == Kind::Session { Some("Session") } else { None };
        let folder = library::new_path(&self.clips_dir, &self.name, "mp4", label);
        let joined = folder.with_file_name(format!(".replix-{stamp}.part"));
        let trimmed = folder.with_file_name(format!(".replix-{stamp}-trimmed.part"));
        let hidden = folder.with_file_name(format!(".replix-{stamp}-hidden.part"));
        let graph = self.dir.0.join(format!("hide-{stamp}.txt"));
        let result = (|| {
            let (mut source, length, skipped) = self.render(&list, &joined, &trimmed)?;
            // Markers and private stretches move with the clip's own start.
            let zero = self.origin + skipped;
            let hide = relative_masks(&self.private, zero, length);
            if !hide.is_empty() {
                self.black_out(source, &hidden, &graph, &hide, length).context(tr("Could not hide a program kept out of recordings, so the clip was not saved"))?;
                source = &hidden;
            }
            // The recording is finished; a probe that fails (a busy drive) only costs the
            // details it would add, never the clip. A later scan fills them in.
            let probed = ffmpeg::probe(source);
            let unread = probed.is_err();
            let info = probed.unwrap_or_else(|e| {
                log::warn!("could not read the saved recording back: {e:#}");
                ffmpeg::MediaInfo { duration: length, ..Default::default() }
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
                tracks: self.tracks.iter().map(|t| TrackMeta { name: t.name.clone(), kind: t.kind, exe: t.exe.clone() }).collect(),
                unread,
                markers: library::tidy_markers(self.markers.iter().map(|m| m - zero).collect(), length),
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
    fn render<'a>(&self, list: &Path, joined: &'a Path, trimmed: &'a Path) -> Result<(&'a Path, f64, f64)> {
        // The segments share one timeline, so they are read back to back as a single stream.
        // That skips opening and probing every segment on its own. Names in the list are
        // relative to the buffer folder, which FFmpeg runs in.
        let text: String = self.segments.iter().map(|i| segment_name(*i) + "\n").collect();
        std::fs::write(list, text)?;
        let tracks: Vec<(usize, &str)> = self.tracks.iter().map(|t| (t.slot, t.name.as_str())).collect();
        let input = |trim| joined_input(list, &tracks, trim);
        if let Some(trim) = self.trim_to {
            match self.write_mp4(&input(Some(trim)), trimmed, trim.1) {
                Ok(()) => return Ok((trimmed, trim.1, trim.0)),
                Err(e) => {
                    log::warn!("trimming the clip failed, keeping it whole: {e:#}");
                    // Whatever the failed pass wrote is no clip; it must not take up room.
                    let _ = std::fs::remove_file(trimmed);
                }
            }
        }
        self.write_mp4(&input(None), joined, self.duration)?;
        Ok((joined, self.duration, 0.0))
    }

    /// Writes an MP4 with its index at the front, so it plays before it has fully loaded,
    /// e.g. after an upload. Room for the index is reserved up front: `+faststart` would
    /// move it there afterwards by writing the whole file a second time.
    fn write_mp4(&self, input: &[String], out: &Path, seconds: f64) -> Result<()> {
        let room = moov_room(seconds, self.fps, self.tracks.len());
        let mut args = input.to_vec();
        args.extend(["-c".into(), "copy".into(), "-moov_size".into(), room.to_string()]);
        args.extend(["-f", "mp4", "-y"].map(String::from));
        args.push(ffmpeg::path_arg(out));
        if ffmpeg::run_in(&self.dir.0, &args).is_ok() {
            return Ok(());
        }
        log::warn!("the reserved MP4 index room was too small, writing it again");
        let mut args = input.to_vec();
        args.extend(["-c", "copy", "-movflags", "+faststart", "-f", "mp4", "-y"].map(String::from));
        args.push(ffmpeg::path_arg(out));
        ffmpeg::run_in(&self.dir.0, &args)
    }

    /// Writes `source` again with the windows of kept-out programs black, and the desktop's
    /// sound silent while one was on screen. With the capture's encoder first; software if
    /// that one refuses frames from a file. The filter graph goes into the file `graph`: with
    /// many boxes it would not fit on a command line.
    fn black_out(&self, source: &Path, out: &Path, graph: &Path, hide: &[Mask], length: f64) -> Result<()> {
        let desktop = self.tracks.iter().position(|t| t.kind == TrackKind::Desktop);
        let names: Vec<&str> = self.tracks.iter().map(|t| t.name.as_str()).collect();
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

/// Masks in stream time as seconds of a clip that starts at `zero` and runs `length`.
fn relative_masks(masks: &[Mask], zero: f64, length: f64) -> Vec<Mask> {
    masks
        .iter()
        .map(|m| Mask { start: (m.start - zero).max(0.0), end: (m.end - zero).min(length), ..*m })
        .filter(|m| m.end > m.start)
        .collect()
}

/// A black box over each mask while it lasts, a little bigger than the window so no edge
/// shows; the desktop's sound is silent while any is shown.
pub(super) fn black_out_graph(hide: &[Mask], desktop: Option<usize>, encoder: &Encoder) -> String {
    let between = |m: &Mask| format!("between(t,{:.3},{:.3})", m.start, m.end);
    let mut graph = String::from("[0:v]");
    for m in hide {
        graph.push_str(&format!("{}:enable='{}',", black_box(&m.area), between(m)));
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
        vec!["-y".into(), "-i".into(), ffmpeg::path_arg(source), "-/filter_complex".into(), ffmpeg::path_arg(graph)];
    args.extend(["-map".into(), "[v]".into()]);
    for (k, name) in names.iter().enumerate() {
        args.push("-map".into());
        args.push(if Some(k) == desktop { "[desk]".into() } else { format!("0:a:{k}") });
        args.extend([format!("-metadata:s:a:{k}"), format!("title={name}")]);
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
        args.extend([format!("-metadata:s:a:{k}"), format!("title={name}")]);
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
        run(&dir, &[
            "-f", "lavfi", "-i", "testsrc2=size=320x180:rate=60", "-f", "lavfi", "-i", "sine=f=440:r=48000",
            "-f", "lavfi", "-i", "sine=f=880:r=48000", "-t", "10", "-map", "0:v", "-map", "1:a", "-map", "2:a",
            "-c:v", "libx264", "-preset", "ultrafast", "-bf", "2", "-force_key_frames", "expr:gte(t,n_forced*2)",
            "-c:a", "aac", "-f", "segment", "-segment_time", "2", "-segment_format", "mpegts", "seg%06d.ts",
        ]);
        std::fs::write(dir.join("list.txt"), "seg000001.ts\nseg000002.ts\nseg000003.ts\n").unwrap();
        let tracks = [(0, "Game"), (1, "Microphone")];
        let list = Path::new("list.txt");
        // Skips near the start, in the middle and just before the next keyframe.
        for (skip, length) in [(0.02, 5.0), (0.979, 4.5), (1.98, 3.9)] {
            // The old way: join, then trim the joined file.
            let mut old = joined_input(list, &tracks, None);
            old.extend(["-c", "copy", "-f", "mp4", "joined.mp4"].map(String::from));
            run(&dir, &old.iter().map(String::as_str).collect::<Vec<_>>());
            let (ss, t) = (format!("{skip:.3}"), format!("{length:.3}"));
            run(&dir, &["-ss", &ss, "-t", &t, "-i", "joined.mp4", "-map", "0", "-c", "copy", "-f", "mp4", "old.mp4"]);
            // What the recorder runs now.
            let mut new = joined_input(list, &tracks, Some((skip, length)));
            new.extend(["-c", "copy", "-moov_size", "200000", "-f", "mp4", "new.mp4"].map(String::from));
            run(&dir, &new.iter().map(String::as_str).collect::<Vec<_>>());
            let (a, b) = (frames(&dir, "old.mp4"), frames(&dir, "new.mp4"));
            assert!(!a.is_empty());
            assert_eq!(a, b, "skip {skip}");
        }
        let _ = std::fs::remove_dir_all(&dir);
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
        assert_eq!(info.audio_titles, vec![Some("Desktop".to_string()), Some("Microphone".to_string())]);
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
