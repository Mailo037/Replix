//! The replay buffer.
//!
//! FFmpeg captures the target continuously and cuts the stream into two-second
//! MPEG-TS segments with one audio stream per slot. Saving a clip joins the newest
//! segments without re-encoding; a session keeps every segment from its start.

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Seek, SeekFrom, Write};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use crossbeam_channel::{Receiver, RecvTimeoutError, Sender};
use parking_lot::Mutex;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::audio::{self, Router, Track};
use crate::encoder::{self, Encoder};
use crate::ffmpeg;
use crate::library::{self, Kind, Meta, TrackMeta};
use crate::settings::{CaptureSource, Settings};
use crate::win;

const SEGMENT_SECONDS: f64 = 2.0;
static SAVE_ID: AtomicU64 = AtomicU64::new(0);
static PUBLISH_LOCK: Mutex<()> = parking_lot::const_mutex(());
static STORAGE_LIMIT_LOCK: Mutex<()> = parking_lot::const_mutex(());

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RecState {
    Starting,
    Off,
    Waiting,
    Recording,
    Error,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub state: RecState,
    pub target: Option<String>,
    pub session_since: Option<u64>,
    pub encoder: Option<String>,
    pub error: Option<String>,
    /// Clip length for what is being recorded; a game's own setting can differ from the general one.
    pub clip_seconds: Option<u32>,
}

pub enum Command {
    SaveClip,
    ToggleSession,
    Screenshot,
    Apply(Box<Settings>),
}

#[derive(Debug, Clone, PartialEq)]
enum Target {
    Window { hwnd: u64, pid: u32, exe: String, name: String },
    Monitor { index: u32 },
}

impl Target {
    fn exe(&self) -> Option<&str> {
        match self {
            Target::Window { exe, .. } => Some(exe),
            Target::Monitor { .. } => None,
        }
    }

    fn label(&self) -> String {
        match self {
            Target::Window { name, .. } => name.clone(),
            Target::Monitor { index: 0 } => "Desktop".into(),
            Target::Monitor { index } => format!("Desktop {}", index + 1),
        }
    }

    fn capture_source(&self, fps: u32, cursor: bool) -> String {
        let what = match self {
            Target::Window { hwnd, .. } => format!("hwnd={hwnd}"),
            Target::Monitor { index } => {
                // Graphics Capture sends nothing while the screen is still, which stalls the
                // buffer. Desktop Duplication repeats the last frame, so prefer it for screens.
                if let Some(output) = win::dxgi_output(*index) {
                    return format!("ddagrab=output_idx={output}:framerate={fps}:draw_mouse={}", u8::from(cursor));
                }
                format!("monitor_idx={index}")
            }
        };
        // Even sizes: several encoders reject odd widths.
        format!("gfxcapture={what}:max_framerate={fps}:capture_cursor={}:width=-2:height=-2", u8::from(cursor))
    }
}

struct Session {
    first: u64,
    since: u64,
}

struct BufferDir(PathBuf);

impl Drop for BufferDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct Pipeline {
    target: Target,
    dir: Arc<BufferDir>,
    child: Child,
    stdin: Option<ChildStdin>,
    router: Router,
    log: Arc<Mutex<VecDeque<String>>>,
    session: Option<Session>,
    /// The settings this capture runs with, including the target's own ones.
    settings: Settings,
    last_poll: Instant,
    segments: VecDeque<Segment>,
    list_offset: u64,
    /// Each save owns its pin until it finishes reading the buffer.
    pins: Vec<Weak<u64>>,
    /// When FFmpeg was launched; stream time counts from just after it.
    started: Instant,
    /// Clips whose last moment is still on its way through the encoder.
    pending: Vec<PendingClip>,
}

#[derive(Clone)]
struct Segment {
    index: u64,
    /// Stream time in seconds.
    start: f64,
    end: f64,
}

impl Segment {
    fn duration(&self) -> f64 {
        (self.end - self.start).max(0.0)
    }
}

struct PendingClip {
    /// Stream time of the key press, where the clip ends.
    end: f64,
    pressed: u64,
    deadline: Instant,
    pin: Arc<u64>,
}

impl Pipeline {
    fn start(dir: PathBuf, target: Target, settings: &Settings, encoder: &Encoder) -> Result<Self> {
        std::fs::create_dir_all(&dir)?;
        let buffer = Arc::new(BufferDir(dir.clone()));
        let run = dir.file_name().and_then(|n| n.to_str()).unwrap_or("run").to_string();
        let keep = Duration::from_secs(settings.buffer_seconds as u64 + 15);
        let (mut router, pipes) = Router::new(&run, settings.record_mic, settings.mic_device.clone(), keep)?;
        match &target {
            Target::Window { pid, exe, name, .. } => {
                let root = win::root_of(*pid, &win::processes());
                router.start_main(Some((root, exe.clone(), name.clone())));
            }
            Target::Monitor { .. } => router.start_main(None),
        }

        let mut args: Vec<String> = vec![
            "-hide_banner".into(),
            "-loglevel".into(),
            "warning".into(),
            "-f".into(),
            "lavfi".into(),
            "-i".into(),
            format!(
                "{},fps={}{}",
                target.capture_source(settings.fps, settings.capture_cursor),
                settings.fps,
                encoder.filters
            ),
        ];
        for pipe in &pipes {
            args.extend(
                [
                    "-thread_queue_size", "1024", "-probesize", "32", "-analyzeduration", "0", "-f", "f32le", "-ar",
                    "48000", "-ac", "2", "-i",
                ]
                .map(String::from),
            );
            args.push(pipe.clone());
        }
        args.extend(["-map".into(), "0:v".into()]);
        for i in 1..=pipes.len() {
            args.extend(["-map".into(), format!("{i}:a")]);
        }
        args.extend(encoder::args(encoder, settings.quality.video_kbps()));
        args.extend(
            [
                "-force_key_frames", "expr:gte(t,n_forced*2)", "-c:a", "aac", "-b:a", "160k", "-max_muxing_queue_size",
                // Timestamps run on across segments, so a save can read them back to back as one stream.
                "1024", "-f", "segment", "-segment_time", "2", "-segment_format", "mpegts", "-segment_list_type",
                "csv", "-segment_list",
            ]
            .map(String::from),
        );
        args.push(ffmpeg::path_arg(&dir.join("list.csv")));
        args.push(ffmpeg::path_arg(&dir.join("seg%06d.ts")));

        log::info!("starting capture of {} with {}", target.label(), encoder.id);
        let started = Instant::now();
        let mut child = std::process::Command::new(ffmpeg::exe())
            .creation_flags(0x0800_0000)
            .args(&args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .context("FFmpeg could not be started")?;
        let stdin = child.stdin.take();
        let log_lines = Arc::new(Mutex::new(VecDeque::new()));
        if let Some(stderr) = child.stderr.take() {
            let sink = log_lines.clone();
            std::thread::spawn(move || {
                for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                    log::debug!("ffmpeg: {line}");
                    let mut q = sink.lock();
                    q.push_back(line);
                    if q.len() > 40 {
                        q.pop_front();
                    }
                }
            });
        }
        Ok(Self {
            target,
            dir: buffer,
            child,
            stdin,
            router,
            log: log_lines,
            session: None,
            settings: settings.clone(),
            last_poll: Instant::now(),
            segments: VecDeque::new(),
            list_offset: 0,
            pins: Vec::new(),
            started,
            pending: Vec::new(),
        })
    }

    fn alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    fn last_error(&self) -> String {
        let q = self.log.lock();
        let text: Vec<&str> = q.iter().map(String::as_str).collect();
        ffmpeg::last_lines(&text.join("\n"), 3)
    }

    fn stop(&mut self) {
        if let Some(mut stdin) = self.stdin.take() {
            let _ = stdin.write_all(b"q");
        }
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline {
            if !matches!(self.child.try_wait(), Ok(None)) {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
        self.router.stop();
    }

    /// Completed segments in order, plus the index of the one being written.
    fn segments(&mut self) -> (&VecDeque<Segment>, Option<u64>) {
        if let Ok(mut file) = std::fs::File::open(self.dir.0.join("list.csv")) {
            if file.seek(SeekFrom::Start(self.list_offset)).is_ok() {
                self.read_segments(BufReader::new(file));
            }
        }
        let next = self.segments.back().map(|s| s.index + 1).unwrap_or(0);
        let current = self.dir.0.join(segment_name(next)).exists().then_some(next);
        (&self.segments, current)
    }

    fn read_segments(&mut self, mut reader: impl BufRead) {
        let mut line = String::new();
        while let Ok(bytes) = reader.read_line(&mut line) {
            if !line.ends_with('\n') {
                break;
            }
            self.list_offset += bytes as u64;
            {
                let line = line.trim_end();
                let mut parts = line.split(',');
                if let (Some(name), Some(start), Some(end)) = (parts.next(), parts.next(), parts.next()) {
                    if let (Some(index), Ok(start), Ok(end)) =
                        (segment_index(name), start.parse::<f64>(), end.parse::<f64>())
                    {
                        self.segments.push_back(Segment { index, start, end });
                    }
                }
            }
            line.clear();
        }
    }

    /// Deletes segments that fell out of the buffer and are not kept by a session or a save.
    fn trim(&mut self, buffer_seconds: u32) {
        self.pins.retain(|pin| pin.strong_count() > 0);
        let (done, _) = self.segments();
        let keep = ((buffer_seconds as f64 + 6.0) / SEGMENT_SECONDS).ceil() as usize;
        if done.len() <= keep {
            return;
        }
        let mut floor = done[done.len() - keep].index;
        if let Some(s) = &self.session {
            floor = floor.min(s.first);
        }
        if let Some(p) = self.pins.iter().filter_map(Weak::upgrade).map(|pin| *pin).min() {
            floor = floor.min(p);
        }
        while let Some(segment) = self.segments.front().filter(|s| s.index < floor) {
            match std::fs::remove_file(self.dir.0.join(segment_name(segment.index))) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(_) => break,
            }
            self.segments.pop_front();
        }
    }

    fn pin(&mut self, first: u64) -> Arc<u64> {
        let pin = Arc::new(first);
        self.pins.push(Arc::downgrade(&pin));
        pin
    }
}

fn segment_name(index: u64) -> String {
    format!("seg{index:06}.ts")
}

fn segment_index(name: &str) -> Option<u64> {
    name.strip_prefix("seg")?.strip_suffix(".ts")?.parse().ok()
}

pub struct Recorder {
    tx: Sender<Command>,
    pub status: Arc<Mutex<Status>>,
    pub encoders: Arc<Mutex<Vec<&'static Encoder>>>,
}

impl Recorder {
    pub fn start(app: AppHandle, settings: Settings) -> Self {
        let (tx, rx) = crossbeam_channel::unbounded();
        let status = Arc::new(Mutex::new(Status {
            state: RecState::Starting,
            target: None,
            session_since: None,
            encoder: None,
            error: None,
            clip_seconds: None,
        }));
        let encoders = Arc::new(Mutex::new(Vec::new()));
        let worker = Worker {
            app,
            settings,
            status: status.clone(),
            encoders: encoders.clone(),
            encoders_cached: false,
            pipeline: None,
            retry_at: None,
        };
        std::thread::Builder::new().name("recorder".into()).spawn(move || worker.run(rx)).expect("thread");
        Self { tx, status, encoders }
    }

    pub fn send(&self, command: Command) {
        let _ = self.tx.send(command);
    }
}

struct Worker {
    app: AppHandle,
    settings: Settings,
    status: Arc<Mutex<Status>>,
    encoders: Arc<Mutex<Vec<&'static Encoder>>>,
    /// The encoder list came from an earlier test and may be out of date.
    encoders_cached: bool,
    pipeline: Option<Pipeline>,
    retry_at: Option<Instant>,
}

impl Worker {
    fn run(mut self, rx: Receiver<Command>) {
        let _ = wasapi::initialize_mta();
        let (found, cached) = encoder::detect_cached(&self.encoder_cache());
        *self.encoders.lock() = found;
        self.encoders_cached = cached;
        if self.encoders.lock().is_empty() {
            self.set_status(RecState::Error, None, Some("No video encoder works on this PC.".into()));
        }
        let buffer_root = self.buffer_root();
        let _ = std::fs::remove_dir_all(&buffer_root);
        loop {
            match rx.recv_timeout(Duration::from_millis(500)) {
                Ok(command) => self.handle(command),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
            self.tick();
        }
        if let Some(mut p) = self.pipeline.take() {
            p.stop();
            Self::finish_saves(&self.app, &self.settings, &mut p);
        }
    }

    fn encoder_cache(&self) -> PathBuf {
        let dir = self.app.path().app_cache_dir().unwrap_or_else(|_| std::env::temp_dir());
        let _ = std::fs::create_dir_all(&dir);
        dir.join("encoders.json")
    }

    fn buffer_root(&self) -> PathBuf {
        self.app.path().app_local_data_dir().unwrap_or_else(|_| std::env::temp_dir()).join("buffer")
    }

    fn encoder(&self) -> Option<&'static Encoder> {
        let found = self.encoders.lock();
        if self.settings.encoder != "auto" {
            if let Some(e) = found.iter().find(|e| e.id == self.settings.encoder || e.codec == self.settings.encoder) {
                return Some(e);
            }
        }
        found.first().copied()
    }

    fn handle(&mut self, command: Command) {
        match command {
            Command::Apply(settings) => {
                let restart = match &self.pipeline {
                    Some(p) => settings.for_exe(p.target.exe()).capture_differs(&p.settings),
                    None => settings.capture_differs(&self.settings),
                };
                self.settings = *settings;
                if restart {
                    if let Some(mut p) = self.pipeline.take() {
                        p.stop();
                        Self::finish_saves(&self.app, &self.settings, &mut p);
                    }
                    self.retry_at = None;
                } else if let Some(p) = self.pipeline.as_mut() {
                    // Clip length and the like apply to the running capture as they are.
                    p.settings = self.settings.for_exe(p.target.exe());
                }
            }
            Command::SaveClip => self.save_clip(),
            Command::ToggleSession => self.toggle_session(),
            Command::Screenshot => self.screenshot(),
        }
    }

    fn desired_target(&self) -> Option<Target> {
        if !self.settings.enabled {
            return None;
        }
        let current = self.pipeline.as_ref().map(|p| &p.target);
        match self.settings.source {
            CaptureSource::Screen => Some(Target::Monitor { index: self.settings.monitor }),
            CaptureSource::Game => {
                let ignored = |exe: &str| self.settings.profile(exe).is_some_and(|p| p.ignore);
                if let Some(t @ Target::Window { hwnd, exe, .. }) = current {
                    if win::is_window(*hwnd) && !ignored(exe) {
                        return Some(t.clone());
                    }
                }
                let w = win::foreground()?;
                // A program with its own settings counts as a game, even one Replix would not spot.
                let wanted = match self.settings.profile(&w.exe) {
                    Some(p) => !p.ignore,
                    None => win::looks_like_game(&w),
                };
                if !wanted {
                    return None;
                }
                Some(Target::Window { hwnd: w.hwnd, pid: w.pid, name: win::game_name(&w), exe: w.exe })
            }
            CaptureSource::App => {
                let exe = self.settings.app_exe.as_ref()?.to_lowercase();
                if let Some(t @ Target::Window { hwnd, exe: e, .. }) = current {
                    if win::is_window(*hwnd) && e.to_lowercase() == exe {
                        return Some(t.clone());
                    }
                }
                let w = win::list_windows().into_iter().find(|w| w.exe.to_lowercase() == exe)?;
                Some(Target::Window { hwnd: w.hwnd, pid: w.pid, name: win::app_name(&w.exe_path), exe: w.exe })
            }
        }
    }

    fn tick(&mut self) {
        if matches!(self.status.lock().state, RecState::Starting) && self.encoders.lock().is_empty() {
            return;
        }
        let desired = self.desired_target();
        let running = self.pipeline.as_ref().map(|p| p.target.clone());
        if desired != running {
            if let Some(mut p) = self.pipeline.take() {
                // A session in progress is saved rather than lost.
                p.stop();
                Self::finish_saves(&self.app, &self.settings, &mut p);
            }
            if let Some(target) = desired.clone() {
                if self.retry_at.is_none_or(|t| Instant::now() >= t) {
                    self.launch(target);
                }
            }
        }

        let mut failed = None;
        if let Some(p) = self.pipeline.as_mut() {
            if !p.alive() {
                failed = Some(p.last_error());
            } else {
                Self::finish_clips(&self.app, &self.settings, p, false);
                p.trim(p.settings.buffer_seconds);
                if p.last_poll.elapsed() > Duration::from_secs(2) {
                    p.router.poll();
                    p.last_poll = Instant::now();
                }
            }
        }
        if let Some(error) = failed {
            log::warn!("capture stopped: {error}");
            if let Some(mut p) = self.pipeline.take() {
                p.stop();
                Self::finish_saves(&self.app, &self.settings, &mut p);
            }
            // The remembered encoder may no longer work, e.g. after a driver change Windows hid.
            if self.encoders_cached {
                self.encoders_cached = false;
                let _ = std::fs::remove_file(self.encoder_cache());
                let (found, _) = encoder::detect_cached(&self.encoder_cache());
                *self.encoders.lock() = found;
            }
            self.retry_at = Some(Instant::now() + Duration::from_secs(5));
            let message = if error.is_empty() { "The capture stopped unexpectedly.".to_string() } else { error };
            self.set_status(RecState::Error, desired.map(|t| t.label()), Some(message));
            return;
        }

        match &self.pipeline {
            Some(p) => {
                let since = p.session.as_ref().map(|s| s.since);
                let label = p.target.label();
                self.set_status_full(RecState::Recording, Some(label), since, None);
            }
            None if !self.settings.enabled => self.set_status(RecState::Off, None, None),
            None if self.status.lock().state == RecState::Error && self.retry_at.is_some_and(|t| Instant::now() < t) => {}
            None => self.set_status(RecState::Waiting, None, None),
        }
    }

    fn launch(&mut self, target: Target) {
        let Some(encoder) = self.encoder() else { return };
        let dir = self.buffer_root().join(audio::now_ms().to_string());
        let settings = self.settings.for_exe(target.exe());
        match Pipeline::start(dir, target.clone(), &settings, encoder) {
            Ok(p) => {
                self.pipeline = Some(p);
                self.retry_at = None;
            }
            Err(e) => {
                log::error!("capture failed to start: {e:#}");
                self.retry_at = Some(Instant::now() + Duration::from_secs(5));
                self.set_status(RecState::Error, Some(target.label()), Some(format!("{e:#}")));
            }
        }
    }

    fn set_status(&self, state: RecState, target: Option<String>, error: Option<String>) {
        self.set_status_full(state, target, None, error);
    }

    fn set_status_full(&self, state: RecState, target: Option<String>, session_since: Option<u64>, error: Option<String>) {
        let encoder = self.encoder().map(|e| e.label.to_string());
        let clip_seconds = self.pipeline.as_ref().map(|p| p.settings.buffer_seconds);
        let next = Status { state, target, session_since, encoder, error, clip_seconds };
        let mut current = self.status.lock();
        if *current != next {
            *current = next.clone();
            drop(current);
            let _ = self.app.emit("status", next);
        }
    }

    fn save_clip(&mut self) {
        let Some(p) = self.pipeline.as_mut() else {
            crate::toast(&self.app, "Nothing to save", "Replix is not recording right now.");
            return;
        };
        let seconds = p.settings.buffer_seconds as f64;
        // The last second or so before the key press is still in the encoder, and the segment
        // holding it is not complete yet. The clip waits for it rather than ending early.
        // Stream time starts a little after launch, so this end lands just after the press.
        let end = p.started.elapsed().as_secs_f64();
        let (done, current) = p.segments();
        let Some(first) = done.iter().find(|s| s.end > end - seconds).map(|s| s.index).or(current) else {
            crate::toast(&self.app, "Nothing to save yet", "The buffer is still filling up.");
            return;
        };
        let pending = PendingClip {
            end,
            pressed: audio::now_ms(),
            deadline: Instant::now() + Duration::from_secs_f64(SEGMENT_SECONDS + 4.0),
            pin: p.pin(first),
        };
        p.pending.push(pending);
        crate::toast(&self.app, "Saving clip…", &p.target.label());
    }

    /// Saves the clips whose end has been written, or all of them once the capture stopped.
    fn finish_clips(app: &AppHandle, settings: &Settings, p: &mut Pipeline, all: bool) {
        let seconds = p.settings.buffer_seconds as f64;
        let (done, _) = p.segments();
        let written = done.back().map_or(f64::MIN, |s| s.end);
        let now = Instant::now();
        let (ready, waiting) = std::mem::take(&mut p.pending)
            .into_iter()
            .partition::<Vec<_>, _>(|c| all || written >= c.end || now >= c.deadline);
        p.pending = waiting;
        for clip in ready {
            // Without the segment holding the press, the clip ends with what was written.
            let end = clip.end.min(written);
            let start = end - seconds;
            let picked: Vec<&Segment> =
                p.segments.iter().filter(|s| s.index >= *clip.pin && s.end > start && s.start < end).collect();
            let (Some(first), Some(last)) = (picked.first(), picked.last()) else {
                crate::toast(app, "Nothing to save yet", "The buffer is still filling up.");
                continue;
            };
            let skip = (start - first.start).max(0.0);
            let length = end - first.start - skip;
            let trim = skip > 0.1 || last.end - end > 0.1;
            let job = SaveJob {
                app: app.clone(),
                dir: p.dir.clone(),
                segments: picked.iter().map(|s| s.index).collect(),
                tracks: p.router.tracks_between(clip.pressed.saturating_sub((seconds as u64 + 2) * 1000), clip.pressed),
                kind: Kind::Clip,
                name: p.target.label(),
                trim_to: trim.then_some((skip, length)),
                duration: last.end - first.start,
                fps: p.settings.fps,
                clips_dir: settings.clips_dir.clone(),
                limit_gb: settings.storage_limit_gb,
                _pin: clip.pin,
            };
            job.start();
        }
    }

    /// Saves what a capture that just stopped still owes.
    fn finish_saves(app: &AppHandle, settings: &Settings, p: &mut Pipeline) {
        Self::finish_clips(app, settings, p, true);
        Self::finish_session(app, settings, p);
    }

    fn toggle_session(&mut self) {
        let Some(p) = self.pipeline.as_mut() else {
            crate::toast(&self.app, "Can't start a session", "Replix is not recording right now.");
            return;
        };
        if p.session.is_some() {
            Self::finish_session(&self.app, &self.settings, p);
            p.session = None;
            p.router.hold = false;
        } else {
            let (done, current) = p.segments();
            let first = current.unwrap_or_else(|| done.back().map(|s| s.index + 1).unwrap_or(0));
            p.session = Some(Session { first, since: audio::now_ms() });
            p.router.hold = true;
            crate::toast(&self.app, "Session started", "Press the session key again to stop.");
        }
    }

    fn finish_session(app: &AppHandle, settings: &Settings, p: &mut Pipeline) {
        let Some(Session { first, since }) = p.session else { return };
        let (done, _) = p.segments();
        let segments = done.iter().filter(|s| s.index >= first);
        let duration = segments.clone().map(Segment::duration).sum();
        let picked: Vec<u64> = segments.map(|s| s.index).collect();
        let Some(&first) = picked.first() else {
            crate::toast(app, "Nothing to save yet", "The session has no completed recording segments.");
            return;
        };
        let tracks = p.router.tracks_between(since, audio::now_ms());
        let job = SaveJob {
            app: app.clone(),
            dir: p.dir.clone(),
            segments: picked,
            tracks,
            kind: Kind::Session,
            name: p.target.label(),
            trim_to: None,
            duration,
            fps: p.settings.fps,
            clips_dir: settings.clips_dir.clone(),
            limit_gb: settings.storage_limit_gb,
            _pin: p.pin(first),
        };
        crate::toast(app, "Saving session…", &job.name);
        job.start();
    }

    fn screenshot(&self) {
        let target = self.pipeline.as_ref().map(|p| p.target.clone()).or_else(|| {
            win::foreground().map(|w| Target::Window { hwnd: w.hwnd, pid: w.pid, name: win::game_name(&w), exe: w.exe })
        });
        let Some(target) = target else { return };
        let app = self.app.clone();
        let clips_dir = self.settings.clips_dir.clone();
        let cursor = self.settings.capture_cursor;
        std::thread::spawn(move || {
            let name = target.label();
            let path = library::new_path(&clips_dir, &name, "png", None);
            let source = format!("{},hwdownload,format=bgra", target.capture_source(30, cursor));
            let args: Vec<String> = vec![
                "-f".into(),
                "lavfi".into(),
                "-i".into(),
                source,
                "-frames:v".into(),
                "1".into(),
                "-update".into(),
                "1".into(),
                "-y".into(),
                ffmpeg::path_arg(&path),
            ];
            match ffmpeg::run_with_timeout(&args, Duration::from_secs(10)) {
                Ok(()) => {
                    let info = ffmpeg::probe(&path).unwrap_or_default();
                    let meta = Meta {
                        kind: Kind::Screenshot,
                        title: "Screenshot".into(),
                        game: name.clone(),
                        created: audio::now_ms(),
                        width: info.width,
                        height: info.height,
                        ..Default::default()
                    };
                    let _ = library::write_meta(&path, &meta);
                    let _ = app.emit("library-changed", ());
                    crate::toast(&app, "Screenshot saved", &name);
                }
                Err(e) => {
                    log::warn!("screenshot failed: {e}");
                    crate::toast(&app, "Screenshot failed", "The picture could not be taken.");
                }
            }
        });
    }
}

struct SaveJob {
    app: AppHandle,
    dir: Arc<BufferDir>,
    segments: Vec<u64>,
    tracks: Vec<Track>,
    kind: Kind,
    name: String,
    /// Seconds to skip at the start of the joined segments, and the length to keep.
    trim_to: Option<(f64, f64)>,
    duration: f64,
    fps: u32,
    clips_dir: PathBuf,
    limit_gb: u32,
    _pin: Arc<u64>,
}

impl SaveJob {
    fn start(self) {
        std::thread::spawn(move || self.run());
    }

    fn run(self) {
        let result = self.write();
        drop(self._pin);
        match result {
            Ok(meta) => {
                let what = if self.kind == Kind::Session { "Session saved" } else { "Clip saved" };
                crate::toast(&self.app, what, &format!("{} · {}", self.name, library::format_duration(meta.duration)));
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
                crate::toast(&self.app, "Saving failed", &format!("{e}"));
            }
        }
    }

    fn write(&self) -> Result<Meta> {
        let id = SAVE_ID.fetch_add(1, Ordering::Relaxed);
        let stamp = format!("{}-{}-{id}", audio::now_ms(), std::process::id());
        let list = self.dir.0.join(format!("save-{stamp}.txt"));
        let label = if self.kind == Kind::Session { Some("Session") } else { None };
        let folder = library::new_path(&self.clips_dir, &self.name, "mp4", label);
        let joined = folder.with_file_name(format!(".replix-{stamp}.part"));
        let trimmed = folder.with_file_name(format!(".replix-{stamp}-trimmed.part"));
        let result = (|| {
            let (source, length) = self.render(&list, &joined, &trimmed)?;
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
                tracks: self.tracks.iter().map(|t| TrackMeta { name: t.name.clone(), kind: t.kind }).collect(),
                unread,
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
                return Err(e).context("Could not move the recording into the library");
            }
            Ok(meta)
        })();
        for path in [list, joined, trimmed] {
            let _ = std::fs::remove_file(path);
        }
        result
    }

    /// The finished file and its length: trimmed if that worked, whole if not.
    fn render<'a>(&self, list: &Path, joined: &'a Path, trimmed: &'a Path) -> Result<(&'a Path, f64)> {
        // The segments share one timeline, so they are read back to back as a single stream.
        // That skips opening and probing every segment on its own. Names in the list are
        // relative to the buffer folder, which FFmpeg runs in.
        let text: String = self.segments.iter().map(|i| segment_name(*i) + "\n").collect();
        std::fs::write(list, text)?;
        let tracks: Vec<(usize, &str)> = self.tracks.iter().map(|t| (t.slot, t.name.as_str())).collect();
        let input = |trim| joined_input(list, &tracks, trim);
        if let Some(trim) = self.trim_to {
            match self.write_mp4(&input(Some(trim)), trimmed, trim.1) {
                Ok(()) => return Ok((trimmed, trim.1)),
                Err(e) => {
                    log::warn!("trimming the clip failed, keeping it whole: {e:#}");
                    // Whatever the failed pass wrote is no clip; it must not take up room.
                    let _ = std::fs::remove_file(trimmed);
                }
            }
        }
        self.write_mp4(&input(None), joined, self.duration)?;
        Ok((joined, self.duration))
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
}

/// Reads the segments in `list` as one stream with the chosen audio streams and their titles.
/// With `trim` (seconds to skip, seconds to keep), the clip is cut in the same pass: the
/// first segment starts on a keyframe, and shifting the stream back by the part before the
/// clip leaves that keyframe at a negative time, which the MP4 hides with an edit list.
/// Writing stops at the key press. No joined copy is written first.
fn joined_input(list: &Path, tracks: &[(usize, &str)], trim: Option<(f64, f64)>) -> Vec<String> {
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

    #[test]
    fn moov_room_grows_with_length() {
        assert!(moov_room(60.0, 60, 6) > moov_room(30.0, 60, 6));
        assert!(moov_room(-1.0, 60, 0) >= 64 * 1024);
    }
}
