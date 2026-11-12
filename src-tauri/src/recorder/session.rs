//! A session written out while it runs.
//!
//! As each of the buffer's segments completes, it is handed to a second FFmpeg that copies
//! it into one Matroska file in the clips folder. Matroska needs no index to be read, so
//! what was recorded up to a crash or a power cut is still there, and the next start turns
//! it into a session in the library. A long session also no longer piles up in the buffer:
//! a segment that is in the file and out of the replay buffer is deleted.

use std::collections::VecDeque;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, UNIX_EPOCH};

use anyhow::{Context, Result};
use crossbeam_channel::{Receiver, RecvTimeoutError, Sender};
use serde::{Deserialize, Serialize};

use super::pipeline::{segment_name, BufferDir, Mask, Segment};
use crate::audio::{Track, TrackKind};
use crate::encoder::Codec;
use crate::ffmpeg::{self, SpawnTied};
use crate::platform::{self, Area};

/// What a session's file is called while it is being written. The library leaves files
/// with this ending alone.
pub(super) const LIVE_EXT: &str = "recording.mkv";

/// How often what was written is pushed from the system's cache onto the disk, so a power
/// cut costs seconds rather than whatever the system had not got round to.
const SYNC_EVERY: Duration = Duration::from_secs(10);

/// The end of a mask that is still on screen: far beyond any recording, and a number JSON
/// can hold.
const OPEN_END: f64 = 1e9;

pub(super) fn is_live(path: &Path) -> bool {
    path.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.to_lowercase().ends_with(&format!(".{LIVE_EXT}")))
}

fn journal_of(file: &Path) -> PathBuf {
    file.with_extension("json")
}

/// A part of the picture to black out, as the journal keeps it.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub(super) struct Hidden {
    area: [f32; 4],
    start: f64,
    end: f64,
}

impl From<&Mask> for Hidden {
    fn from(m: &Mask) -> Self {
        Self { area: [m.area.left, m.area.top, m.area.right, m.area.bottom], start: m.start, end: m.end.min(OPEN_END) }
    }
}

impl From<&Hidden> for Mask {
    fn from(h: &Hidden) -> Self {
        let [left, top, right, bottom] = h.area;
        Self { area: Area { left, top, right, bottom }, start: h.start, end: h.end }
    }
}

/// What the file itself cannot say, kept next to it while the session runs: enough to
/// finish the session after a crash, with its tracks named, its markers in place and the
/// programs kept out of recordings still hidden.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(super) struct Journal {
    pub game: String,
    /// Epoch milliseconds.
    pub since: u64,
    pub fps: u32,
    pub kbps: u32,
    /// The picture's format; sessions from before HEVC are H.264.
    #[serde(default)]
    pub format: Codec,
    /// Stream time where the file starts; none until its first segment is in.
    pub origin: Option<f64>,
    pub tracks: Vec<Track>,
    /// Marked moments, in stream time.
    pub markers: Vec<f64>,
    /// In stream time; one still on screen runs on without an end.
    pub private: Vec<Hidden>,
    /// The session is in the library; the file is only still here because it could not
    /// be deleted.
    pub done: bool,
}

impl Journal {
    pub fn read(file: &Path) -> Option<Self> {
        serde_json::from_str(&std::fs::read_to_string(journal_of(file)).ok()?).ok()
    }

    /// Written next to its old self and renamed over it, so a crash in between leaves one
    /// of the two whole.
    pub fn write(&self, file: &Path) -> std::io::Result<()> {
        let path = journal_of(file);
        let temp = path.with_extension("json.tmp");
        // One left by a crash is hidden, and a hidden file cannot be written over in place.
        let _ = std::fs::remove_file(&temp);
        std::fs::write(&temp, serde_json::to_string(self)?)?;
        platform::hide_file(&temp);
        std::fs::rename(&temp, &path).inspect_err(|_| {
            let _ = std::fs::remove_file(&temp);
        })
    }

    /// Where the file starts in stream time, and the parts of the picture to black out.
    /// A journal from before the first segment went in does not know the start; whatever
    /// it lists is then hidden throughout rather than not at all.
    pub fn masks_from_start(&self) -> (f64, Vec<Mask>) {
        let masks = self.private.iter().map(Mask::from);
        match self.origin {
            Some(origin) => (origin, masks.collect()),
            None => (0.0, masks.map(|m| Mask { start: 0.0, end: OPEN_END, ..m }).collect()),
        }
    }
}

/// The name of the n-th picture of the session that began at `since`.
pub(super) fn picture_name(since: u64, n: u32) -> String {
    format!("live-{since}-{n}.jpg")
}

/// Removes pictures of sessions an earlier run left behind.
pub(super) fn clear_pictures(previews: &Path) {
    let Ok(entries) = std::fs::read_dir(previews) else { return };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with("live-") && name.ends_with(".jpg") {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// The audio tracks of a file nothing else describes: all of them, numbered.
pub(super) fn tracks_in(file: &Path) -> Vec<Track> {
    let count = ffmpeg::probe(file).map_or(0, |info| info.audio_titles.len());
    (0..count).map(|slot| Track { slot, name: format!("Track {}", slot + 1), exe: None, kind: TrackKind::App }).collect()
}

/// Removes a session's file and journal once it is in the library. A file that cannot be
/// deleted now (a virus scanner reading it, say) is marked done, so the next start removes
/// it instead of saving the session a second time.
pub(super) fn discard(file: &Path) {
    for _ in 0..20 {
        match std::fs::remove_file(file) {
            Ok(()) => break,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => break,
            Err(_) => std::thread::sleep(Duration::from_millis(100)),
        }
    }
    if file.exists() {
        log::warn!("{} could not be deleted; the next start tries again", file.display());
        let mut journal = Journal::read(file).unwrap_or_default();
        journal.done = true;
        let _ = journal.write(file);
    } else {
        let _ = std::fs::remove_file(journal_of(file));
    }
}

enum Step {
    Segment(u64),
    /// Close the file, then run this with whether FFmpeg ended cleanly.
    Finish(Box<dyn FnOnce(bool) + Send>),
}

struct Shared {
    /// Segments before this one are in the file.
    written: AtomicU64,
    failed: AtomicBool,
}

pub(super) struct Live {
    pub file: PathBuf,
    steps: Sender<Step>,
    /// The next segment to hand over.
    next: u64,
    shared: Arc<Shared>,
    /// Stream time of what was handed over so far.
    span: Option<(f64, f64)>,
    seconds: f64,
    /// The newest segment handed over, for a picture of the session.
    pub newest: Option<u64>,
    /// The journal as last written.
    noted: Option<Journal>,
}

impl Live {
    /// Starts writing `file` from segment `first` of the buffer in `dir`.
    pub fn start(dir: Arc<BufferDir>, file: PathBuf, first: u64) -> Result<Self> {
        let mut cmd = ffmpeg::command();
        // Segments arrive on stdin as one MPEG-TS stream; every audio slot is kept, since
        // only the end of the session shows which ones carried something.
        cmd.args(["-f", "mpegts", "-i", "pipe:0", "-map", "0", "-c", "copy"]);
        // A cluster is written out once it is complete; short ones leave little in memory.
        cmd.args(["-flush_packets", "1", "-cluster_time_limit", "1000", "-f", "matroska", "-y"]);
        cmd.arg(&file);
        let mut child = cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn_tied()
            .context("FFmpeg could not be started")?;
        // Read to the end so FFmpeg never waits on a full pipe; the last lines explain a failure.
        let errors = child.stderr.take().map(|mut stderr| {
            std::thread::spawn(move || {
                let mut text = String::new();
                let _ = stderr.read_to_string(&mut text);
                ffmpeg::last_lines(&text, 3)
            })
        });
        let shared = Arc::new(Shared { written: AtomicU64::new(first), failed: AtomicBool::new(false) });
        let (steps, rx) = crossbeam_channel::unbounded();
        let writer = Writer { dir, file: file.clone(), child, shared: shared.clone(), errors };
        std::thread::Builder::new().name("session".into()).spawn(move || writer.run(rx))?;
        Ok(Self { file, steps, next: first, shared, span: None, seconds: 0.0, newest: None, noted: None })
    }

    /// Hands over the segments completed since the last call.
    pub fn feed(&mut self, done: &VecDeque<Segment>) {
        for segment in done {
            if segment.index < self.next {
                continue;
            }
            if self.steps.send(Step::Segment(segment.index)).is_err() {
                self.shared.failed.store(true, Ordering::Relaxed);
                return;
            }
            self.next = segment.index + 1;
            self.newest = Some(segment.index);
            self.span = Some((self.span.map_or(segment.start, |s| s.0), segment.end));
            self.seconds += segment.duration();
        }
    }

    /// The first segment the buffer still has to keep for the file.
    pub fn written(&self) -> u64 {
        self.shared.written.load(Ordering::Relaxed)
    }

    /// The file can take no more: the drive is full or gone, or FFmpeg ended.
    pub fn failed(&self) -> bool {
        self.shared.failed.load(Ordering::Relaxed)
    }

    /// Stream time where the file starts and ends, once something is in it.
    pub fn span(&self) -> Option<(f64, f64)> {
        self.span
    }

    pub fn seconds(&self) -> f64 {
        self.seconds
    }

    /// Keeps the journal up to date; written only when something in it changed.
    pub fn note(&mut self, journal: Journal) {
        if self.noted.as_ref() == Some(&journal) {
            return;
        }
        match journal.write(&self.file) {
            Ok(()) => self.noted = Some(journal),
            Err(e) => log::warn!("could not note the session's details: {e}"),
        }
    }

    /// Closes the file once everything handed over is in it, then runs `then` on the
    /// writer's thread with whether FFmpeg ended cleanly. The file is worth reading
    /// either way.
    pub fn finish(self, then: impl FnOnce(bool) + Send + 'static) {
        let _ = self.steps.send(Step::Finish(Box::new(then)));
    }
}

struct Writer {
    dir: Arc<BufferDir>,
    file: PathBuf,
    child: Child,
    shared: Arc<Shared>,
    errors: Option<std::thread::JoinHandle<String>>,
}

impl Writer {
    fn run(mut self, steps: Receiver<Step>) {
        let mut stdin = self.child.stdin.take();
        let mut synced = Instant::now();
        let then = loop {
            match steps.recv_timeout(SYNC_EVERY) {
                Ok(Step::Segment(index)) => {
                    let copied = match stdin.as_mut() {
                        Some(pipe) => copy_segment(&self.dir.0.join(segment_name(index)), pipe),
                        None => continue,
                    };
                    match copied {
                        Ok(()) => self.shared.written.store(index + 1, Ordering::Relaxed),
                        Err(e) => {
                            log::warn!("the session's file stopped taking data: {e}");
                            // Closing the pipe lets FFmpeg finish the file with what it has.
                            stdin = None;
                            self.shared.failed.store(true, Ordering::Relaxed);
                        }
                    }
                }
                Ok(Step::Finish(then)) => break Some(then),
                Err(RecvTimeoutError::Timeout) => {}
                // Replix is ending without having stopped the session.
                Err(RecvTimeoutError::Disconnected) => break None,
            }
            if synced.elapsed() >= SYNC_EVERY {
                sync(&self.file);
                synced = Instant::now();
            }
        };
        drop(stdin);
        let clean = self.close();
        sync(&self.file);
        if let Some(then) = then {
            then(clean);
        }
    }

    /// Waits for FFmpeg to write the end of the file; one that hangs is stopped.
    fn close(&mut self) -> bool {
        let deadline = Instant::now() + Duration::from_secs(60);
        let status = loop {
            match self.child.try_wait() {
                Ok(Some(status)) => break Some(status),
                Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
                _ => {
                    let _ = self.child.kill();
                    let _ = self.child.wait();
                    break None;
                }
            }
        };
        let errors = self.errors.take().and_then(|e| e.join().ok()).unwrap_or_default();
        let clean = status.is_some_and(|s| s.success());
        if !clean {
            log::warn!("the session's file was not closed cleanly: {errors}");
        }
        clean
    }
}

fn copy_segment(segment: &Path, pipe: &mut impl Write) -> std::io::Result<()> {
    let mut file = std::fs::File::open(segment)?;
    let mut block = vec![0u8; 256 * 1024];
    loop {
        match file.read(&mut block)? {
            0 => return pipe.flush(),
            n => pipe.write_all(&block[..n])?,
        }
    }
}

/// Pushes what FFmpeg wrote so far from the system's cache onto the disk. The cache is
/// the file's, not the handle's, so a second handle does it.
fn sync(file: &Path) {
    if let Ok(handle) = std::fs::OpenOptions::new().write(true).open(file) {
        let _ = handle.sync_data();
    }
}

/// A session a crash, a power cut or a quit in the middle left behind.
pub(super) struct Leftover {
    pub file: PathBuf,
    pub journal: Journal,
    /// When the file was last written to, epoch milliseconds: about when the session broke off.
    pub ended: u64,
}

/// Sessions left behind in the clips folder and the game folders in it. Files that are
/// already in the library, or hold nothing, are removed here.
pub(super) fn leftovers(root: &Path) -> Vec<Leftover> {
    let mut found = Vec::new();
    let Ok(entries) = std::fs::read_dir(root) else { return found };
    let mut files: Vec<PathBuf> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Ok(inner) = std::fs::read_dir(&path) {
                files.extend(inner.flatten().map(|e| e.path()));
            }
        } else {
            files.push(path);
        }
    }
    for file in files {
        if !is_live(&file) {
            // A journal alone is left when its file never came to be.
            let orphan = file.to_string_lossy().to_lowercase().ends_with(".recording.json") && !file.with_extension("mkv").exists();
            if orphan {
                let _ = std::fs::remove_file(&file);
            }
            continue;
        }
        let journal = Journal::read(&file);
        let meta = std::fs::metadata(&file).ok();
        let empty = meta.as_ref().is_none_or(|m| m.len() == 0);
        if empty || journal.as_ref().is_some_and(|j| j.done) {
            let _ = std::fs::remove_file(&file);
            if !file.exists() {
                let _ = std::fs::remove_file(journal_of(&file));
            }
            continue;
        }
        let ended = meta
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_millis() as u64);
        // Without a journal the file still says most of it: the folder names the game.
        let journal = journal.unwrap_or_else(|| Journal {
            game: file
                .parent()
                .filter(|p| *p != root)
                .and_then(|p| p.file_name())
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "Desktop".into()),
            since: ended,
            ..Default::default()
        });
        found.push(Leftover { file, journal, ended });
    }
    found
}

/// Where a session's finished file goes: the name it was recorded under, without the
/// ending that marks it as still being written.
pub(super) fn finished_name(file: &Path) -> Option<PathBuf> {
    let name = file.file_name()?.to_str()?;
    let stem = name.get(..name.len().checked_sub(LIVE_EXT.len() + 1)?)?;
    Some(file.with_file_name(format!("{stem}.mp4")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("replix-session-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn live_files_are_told_apart_by_their_ending() {
        assert!(is_live(Path::new("Game Session 2026-11-03 21-12-04.recording.mkv")));
        assert!(is_live(Path::new("a.RECORDING.MKV")));
        assert!(!is_live(Path::new("Game Session.mkv")));
        assert!(!is_live(Path::new("recording.mkv")));
        assert_eq!(
            finished_name(Path::new("Game").join("Game Session 1.recording.mkv").as_path()),
            Some(Path::new("Game").join("Game Session 1.mp4"))
        );
        assert_eq!(finished_name(Path::new("recording.mkv")), None);
    }

    #[test]
    fn the_journal_comes_back_as_written_with_open_masks() {
        let dir = temp_dir("journal");
        let file = dir.join("Game Session.recording.mkv");
        let area = Area { left: 0.1, top: 0.2, right: 0.3, bottom: 0.4 };
        let open = Mask { area, start: 12.0, end: f64::INFINITY };
        let journal = Journal {
            game: "Game".into(),
            since: 5,
            fps: 60,
            kbps: 20000,
            format: Codec::H264,
            origin: Some(10.0),
            markers: vec![11.5],
            private: vec![Hidden::from(&open)],
            ..Default::default()
        };
        journal.write(&file).unwrap();
        // Written a second time over the first, as while a session runs.
        journal.write(&file).unwrap();
        let back = Journal::read(&file).unwrap();
        assert_eq!(back, journal);
        assert_eq!(back.masks_from_start(), (10.0, vec![Mask { area, start: 12.0, end: OPEN_END }]));
        // Without a start, what was on screen is hidden throughout.
        let early = Journal { origin: None, ..back };
        assert_eq!(early.masks_from_start(), (0.0, vec![Mask { area, start: 0.0, end: OPEN_END }]));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn leftovers_skip_finished_and_empty_files() {
        let dir = temp_dir("leftovers");
        std::fs::create_dir_all(dir.join("Game")).unwrap();
        let broken = dir.join("Game").join("Game Session 1.recording.mkv");
        let done = dir.join("Game").join("Game Session 2.recording.mkv");
        let empty = dir.join("Desktop Session 3.recording.mkv");
        let plain = dir.join("Game").join("clip.mkv");
        for file in [&broken, &done, &plain] {
            std::fs::write(file, b"x").unwrap();
        }
        std::fs::write(&empty, b"").unwrap();
        Journal { game: "Other".into(), done: true, ..Default::default() }.write(&done).unwrap();
        Journal::default().write(&empty).unwrap();
        let found = leftovers(&dir);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].file, broken);
        assert_eq!(found[0].journal.game, "Game", "named after its folder without a journal");
        assert!(found[0].ended > 0);
        assert!(!done.exists() && !journal_of(&done).exists(), "already in the library");
        assert!(!empty.exists() && !journal_of(&empty).exists());
        assert!(plain.exists(), "other videos are left alone");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_discarded_file_takes_its_journal_along() {
        let dir = temp_dir("discard");
        let file = dir.join("Game Session.recording.mkv");
        std::fs::write(&file, b"x").unwrap();
        Journal::default().write(&file).unwrap();
        discard(&file);
        assert!(!file.exists() && !journal_of(&file).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
