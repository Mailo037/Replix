//! The replay buffer.
//!
//! FFmpeg captures the target continuously and cuts the stream into two-second
//! MPEG-TS segments with one audio stream per slot. Saving a clip joins the newest
//! segments without re-encoding; a session keeps every segment from its start.

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Write};
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
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

struct Pipeline {
    target: Target,
    dir: PathBuf,
    child: Child,
    stdin: Option<ChildStdin>,
    router: Router,
    log: Arc<Mutex<VecDeque<String>>>,
    session: Option<Session>,
    /// The settings this capture runs with, including the target's own ones.
    settings: Settings,
    last_poll: Instant,
    /// Lowest segment index a running save still needs.
    pinned: Arc<Mutex<Option<u64>>>,
}

#[derive(Clone)]
struct Segment {
    index: u64,
    duration: f64,
}

impl Pipeline {
    fn start(dir: PathBuf, target: Target, settings: &Settings, encoder: &Encoder) -> Result<Self> {
        std::fs::create_dir_all(&dir)?;
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
                "1024", "-f", "segment", "-segment_time", "2", "-segment_format", "mpegts", "-reset_timestamps", "1",
                "-segment_list_type", "csv", "-segment_list",
            ]
            .map(String::from),
        );
        args.push(ffmpeg::path_arg(&dir.join("list.csv")));
        args.push(ffmpeg::path_arg(&dir.join("seg%06d.ts")));

        log::info!("starting capture of {} with {}", target.label(), encoder.id);
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
            dir,
            child,
            stdin,
            router,
            log: log_lines,
            session: None,
            settings: settings.clone(),
            last_poll: Instant::now(),
            pinned: Arc::new(Mutex::new(None)),
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

    fn stop(mut self) {
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
    fn segments(&self) -> (Vec<Segment>, Option<u64>) {
        let mut done = Vec::new();
        if let Ok(text) = std::fs::read_to_string(self.dir.join("list.csv")) {
            for line in text.lines() {
                let mut parts = line.split(',');
                let (Some(name), Some(start), Some(end)) = (parts.next(), parts.next(), parts.next()) else { continue };
                let Some(index) = name.strip_prefix("seg").and_then(|s| s.strip_suffix(".ts")).and_then(|s| s.parse().ok())
                else {
                    continue;
                };
                let duration = end.parse::<f64>().unwrap_or(0.0) - start.parse::<f64>().unwrap_or(0.0);
                done.push(Segment { index, duration: duration.max(0.0) });
            }
        }
        let next = done.last().map(|s| s.index + 1).unwrap_or(0);
        let current = self.dir.join(segment_name(next)).exists().then_some(next);
        (done, current)
    }

    /// Deletes segments that fell out of the buffer and are not kept by a session or a save.
    fn trim(&self, buffer_seconds: u32) {
        let (done, _) = self.segments();
        let keep = ((buffer_seconds as f64 + 6.0) / SEGMENT_SECONDS).ceil() as usize;
        if done.len() <= keep {
            return;
        }
        let mut floor = done[done.len() - keep].index;
        if let Some(s) = &self.session {
            floor = floor.min(s.first);
        }
        if let Some(p) = *self.pinned.lock() {
            floor = floor.min(p);
        }
        let Ok(entries) = std::fs::read_dir(&self.dir) else { return };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let index = name.strip_prefix("seg").and_then(|s| s.strip_suffix(".ts")).and_then(|s| s.parse::<u64>().ok());
            if index.is_some_and(|i| i < floor) {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
}

fn segment_name(index: u64) -> String {
    format!("seg{index:06}.ts")
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
        if let Some(p) = self.pipeline.take() {
            p.stop();
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
                    if let Some(p) = self.pipeline.take() {
                        if p.session.is_some() {
                            self.finish_session(&p);
                        }
                        p.stop();
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
            if let Some(p) = self.pipeline.take() {
                // A session in progress is saved rather than lost.
                if p.session.is_some() {
                    self.finish_session(&p);
                }
                p.stop();
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
                p.trim(p.settings.buffer_seconds);
                if p.last_poll.elapsed() > Duration::from_secs(2) {
                    p.router.poll();
                    p.last_poll = Instant::now();
                }
            }
        }
        if let Some(error) = failed {
            log::warn!("capture stopped: {error}");
            if let Some(p) = self.pipeline.take() {
                p.stop();
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
        let Some(p) = self.pipeline.as_ref() else {
            crate::toast(&self.app, "Nothing to save", "Replix is not recording right now.");
            return;
        };
        let seconds = p.settings.buffer_seconds as f64;
        let (done, current) = p.segments();
        let mut picked: Vec<u64> = current.into_iter().collect();
        let mut total = 0.0;
        for s in done.iter().rev() {
            if total >= seconds {
                break;
            }
            picked.push(s.index);
            total += s.duration;
        }
        picked.reverse();
        if picked.is_empty() {
            crate::toast(&self.app, "Nothing to save yet", "The buffer is still filling up.");
            return;
        }
        let now = audio::now_ms();
        let tracks = p.router.tracks_between(now.saturating_sub((seconds as u64 + 2) * 1000), now);
        let job = SaveJob {
            app: self.app.clone(),
            dir: p.dir.clone(),
            segments: picked,
            tracks,
            kind: Kind::Clip,
            name: p.target.label(),
            keep_seconds: Some(seconds),
            clips_dir: self.settings.clips_dir.clone(),
            limit_gb: self.settings.storage_limit_gb,
            pinned: p.pinned.clone(),
        };
        std::thread::spawn(move || job.run());
    }

    fn toggle_session(&mut self) {
        let Some(p) = self.pipeline.as_mut() else {
            crate::toast(&self.app, "Can't start a session", "Replix is not recording right now.");
            return;
        };
        if p.session.is_some() {
            let p = self.pipeline.as_ref().unwrap();
            self.finish_session(p);
            let p = self.pipeline.as_mut().unwrap();
            p.session = None;
            p.router.hold = false;
        } else {
            let (done, current) = p.segments();
            let first = current.unwrap_or_else(|| done.last().map(|s| s.index + 1).unwrap_or(0));
            p.session = Some(Session { first, since: audio::now_ms() });
            p.router.hold = true;
            crate::toast(&self.app, "Session started", "Press the session key again to stop.");
        }
    }

    fn finish_session(&self, p: &Pipeline) {
        let Some(session) = &p.session else { return };
        let (done, current) = p.segments();
        let mut picked: Vec<u64> = done.iter().map(|s| s.index).filter(|&i| i >= session.first).collect();
        picked.extend(current.filter(|&i| i >= session.first));
        if picked.is_empty() {
            return;
        }
        let tracks = p.router.tracks_between(session.since, audio::now_ms());
        let job = SaveJob {
            app: self.app.clone(),
            dir: p.dir.clone(),
            segments: picked,
            tracks,
            kind: Kind::Session,
            name: p.target.label(),
            keep_seconds: None,
            clips_dir: self.settings.clips_dir.clone(),
            limit_gb: self.settings.storage_limit_gb,
            pinned: p.pinned.clone(),
        };
        std::thread::spawn(move || job.run());
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
                    library::write_meta(&path, &meta);
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
    dir: PathBuf,
    segments: Vec<u64>,
    tracks: Vec<Track>,
    kind: Kind,
    name: String,
    keep_seconds: Option<f64>,
    clips_dir: PathBuf,
    limit_gb: u32,
    pinned: Arc<Mutex<Option<u64>>>,
}

impl SaveJob {
    fn run(self) {
        *self.pinned.lock() = self.segments.first().copied();
        let result = self.write();
        *self.pinned.lock() = None;
        match result {
            Ok((path, meta)) => {
                library::write_meta(&path, &meta);
                if self.limit_gb > 0 {
                    library::enforce_limit(&self.clips_dir, self.limit_gb as u64 * 1024 * 1024 * 1024);
                }
                let _ = self.app.emit("library-changed", ());
                let what = if self.kind == Kind::Session { "Session saved" } else { "Clip saved" };
                crate::toast(&self.app, what, &format!("{} · {}", self.name, library::format_duration(meta.duration)));
            }
            Err(e) => {
                log::error!("saving failed: {e:#}");
                crate::toast(&self.app, "Saving failed", &format!("{e}"));
            }
        }
    }

    fn write(&self) -> Result<(PathBuf, Meta)> {
        let stamp = audio::now_ms();
        let list = self.dir.join(format!("save-{stamp}.txt"));
        let text: String = self.segments.iter().map(|i| format!("file '{}'\n", segment_name(*i))).collect();
        std::fs::write(&list, text)?;
        let label = if self.kind == Kind::Session { Some("Session") } else { None };
        let out = library::new_path(&self.clips_dir, &self.name, "mp4", label);
        let joined = if self.keep_seconds.is_some() { self.dir.join(format!("save-{stamp}.mp4")) } else { out.clone() };

        let mut args: Vec<String> = vec!["-f".into(), "concat".into(), "-safe".into(), "0".into(), "-i".into()];
        args.push(ffmpeg::path_arg(&list));
        args.extend(["-map".into(), "0:v:0".into()]);
        for t in &self.tracks {
            args.extend(["-map".into(), format!("0:a:{}", t.slot)]);
        }
        args.extend(["-c".into(), "copy".into()]);
        for (k, t) in self.tracks.iter().enumerate() {
            args.extend([format!("-metadata:s:a:{k}"), format!("title={}", t.name)]);
        }
        args.extend(["-movflags".into(), "+faststart".into(), "-y".into(), ffmpeg::path_arg(&joined)]);
        let joined_ok = ffmpeg::run(&args);
        let _ = std::fs::remove_file(&list);
        joined_ok?;

        if let Some(seconds) = self.keep_seconds {
            let duration = ffmpeg::probe(&joined).map(|i| i.duration).unwrap_or(0.0);
            if duration > seconds + 0.25 {
                let args: Vec<String> = vec![
                    "-sseof".into(),
                    format!("-{seconds:.3}"),
                    "-i".into(),
                    ffmpeg::path_arg(&joined),
                    "-map".into(),
                    "0".into(),
                    "-c".into(),
                    "copy".into(),
                    "-movflags".into(),
                    "+faststart".into(),
                    "-y".into(),
                    ffmpeg::path_arg(&out),
                ];
                let trimmed = ffmpeg::run(&args);
                if trimmed.is_err() {
                    std::fs::rename(&joined, &out)?;
                } else {
                    let _ = std::fs::remove_file(&joined);
                }
            } else {
                std::fs::rename(&joined, &out)?;
            }
        }
        if !out.exists() {
            bail!("the clip file is missing");
        }
        let info = ffmpeg::probe(&out).unwrap_or_default();
        let meta = Meta {
            kind: self.kind,
            title: library::default_title(self.kind, &self.name),
            game: self.name.clone(),
            created: audio::now_ms(),
            duration: info.duration,
            width: info.width,
            height: info.height,
            tracks: self.tracks.iter().map(|t| TrackMeta { name: t.name.clone(), kind: t.kind }).collect(),
            ..Default::default()
        };
        Ok((out, meta))
    }
}
