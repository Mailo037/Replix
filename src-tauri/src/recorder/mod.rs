//! The replay buffer.
//!
//! FFmpeg captures the target continuously and cuts the stream into two-second
//! MPEG-TS segments with one audio stream per slot. Saving a clip joins the newest
//! segments without re-encoding; a session keeps every segment from its start.

mod health;
mod pipeline;
mod save;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender};
use parking_lot::Mutex;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::audio;
use crate::encoder::{self, Encoder};
use crate::ffmpeg;
use crate::i18n::tr;
use crate::library::{self, Kind, Meta};
use crate::settings::{CaptureSource, Settings};
use crate::platform;
use health::Health;
use pipeline::{private_masks, segment_name, Pipeline, Segment, Session, Target, SEGMENT_SECONDS};
use save::{Placeholder, SaveJob};

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
    Marker,
    Apply(Box<Settings>),
}

struct PendingClip {
    /// Stream time of the key press, where the clip ends.
    end: f64,
    pressed: u64,
    deadline: Instant,
    pin: Arc<u64>,
    placeholder: Placeholder,
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
            health: Health::default(),
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
    health: Health,
}

impl Worker {
    fn run(mut self, rx: Receiver<Command>) {
        audio::init_thread();
        // Nothing records before FFmpeg is on the PC; on first start it is still downloading.
        crate::install::wait_ready();
        let (found, cached) = encoder::detect_cached(&self.encoder_cache());
        *self.encoders.lock() = found;
        self.encoders_cached = cached;
        if self.encoders.lock().is_empty() {
            self.set_status(RecState::Error, None, Some(tr("No video encoder works on this PC.")));
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

    /// The chosen encoder, or the best one; one that keeps failing at start is passed over
    /// for the next that works, so a broken driver costs a few seconds, not the recording.
    fn encoder(&self) -> Option<&'static Encoder> {
        let found = self.encoders.lock();
        let usable = |e: &&&'static Encoder| self.health.usable(e.id);
        if self.settings.encoder != "auto" {
            let chosen = found.iter().filter(usable).find(|e| e.id == self.settings.encoder || e.codec == self.settings.encoder);
            if let Some(e) = chosen {
                return Some(e);
            }
        }
        found.iter().find(usable).or(found.first()).copied()
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
            Command::Marker => self.add_marker(),
        }
    }

    fn desired_target(&self) -> Option<Target> {
        if !self.settings.enabled {
            return None;
        }
        let current = self.pipeline.as_ref().map(|p| &p.target);
        match self.settings.source {
            CaptureSource::Screen => {
                // A display that was unplugged falls back to the main one.
                let index = self.settings.monitor;
                let index = if (index as usize) < platform::monitors().len() { index } else { 0 };
                Some(Target::Monitor { index })
            }
            CaptureSource::Game => {
                let ignored = |exe: &str| self.settings.profile(exe).is_some_and(|p| p.ignore) || self.settings.is_excluded(exe);
                if let Some(t @ Target::Window { hwnd, exe, .. }) = current {
                    if platform::is_window(*hwnd) && !ignored(exe) {
                        return Some(t.clone());
                    }
                }
                let w = platform::foreground()?;
                if self.settings.is_excluded(&w.exe) {
                    return None;
                }
                // A program with its own settings counts as a game, even one Replix would not spot.
                let wanted = match self.settings.profile(&w.exe) {
                    Some(p) => !p.ignore,
                    None => platform::looks_like_game(&w),
                };
                if !wanted {
                    return None;
                }
                Some(Target::Window { hwnd: w.hwnd, pid: w.pid, name: platform::game_name(&w), exe: w.exe })
            }
            CaptureSource::App => {
                let exe = self.settings.app_exe.as_ref()?.to_lowercase();
                if self.settings.is_excluded(&exe) {
                    return None;
                }
                if let Some(t @ Target::Window { hwnd, exe: e, .. }) = current {
                    if platform::is_window(*hwnd) && e.to_lowercase() == exe {
                        return Some(t.clone());
                    }
                }
                let w = platform::list_windows().into_iter().find(|w| w.exe.to_lowercase() == exe)?;
                Some(Target::Window { hwnd: w.hwnd, pid: w.pid, name: platform::app_name(&w.exe_path), exe: w.exe })
            }
        }
    }

    fn tick(&mut self) {
        if matches!(self.status.lock().state, RecState::Starting) && self.encoders.lock().is_empty() {
            return;
        }
        // Nothing can be recorded at all here, e.g. a Linux session without X11.
        if self.settings.enabled {
            if let Some(problem) = platform::capture_unavailable(matches!(self.settings.source, CaptureSource::Screen)) {
                if let Some(mut p) = self.pipeline.take() {
                    p.stop();
                    Self::finish_saves(&self.app, &self.settings, &mut p);
                }
                self.set_status(RecState::Error, None, Some(problem));
                return;
            }
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

        let excluded = self.settings.excluded_exes();
        let mut failed = None;
        if let Some(p) = self.pipeline.as_mut() {
            if !p.alive() {
                failed = Some(p.last_error());
            } else {
                Self::finish_clips(&self.app, &self.settings, p, false);
                p.trim(p.settings.buffer_seconds);
                p.watch_privacy(&excluded);
                if p.last_poll.elapsed() > Duration::from_secs(2) {
                    p.router.poll(&excluded);
                    p.last_poll = Instant::now();
                }
                if p.stalled() {
                    failed = Some(tr("The capture stopped delivering pictures."));
                } else if p.started.elapsed() > health::HEALTHY {
                    self.health.ran_well(p.encoder.id);
                }
            }
        }
        if let Some(error) = failed {
            log::warn!("capture stopped: {error}");
            if let Some(mut p) = self.pipeline.take() {
                p.stop();
                Self::finish_saves(&self.app, &self.settings, &mut p);
                if self.health.failed(p.encoder.id, p.started.elapsed(), p.target.exists()) {
                    log::warn!("{} keeps failing at start; trying another encoder", p.encoder.id);
                }
            }
            // The remembered encoder may no longer work, e.g. after a driver change Windows hid.
            if self.encoders_cached {
                self.encoders_cached = false;
                let _ = std::fs::remove_file(self.encoder_cache());
                let (found, _) = encoder::detect_cached(&self.encoder_cache());
                *self.encoders.lock() = found;
            }
            self.retry_at = Some(Instant::now() + self.health.wait());
            let message = if error.is_empty() { tr("The capture stopped unexpectedly.") } else { error };
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
                self.health.failed(encoder.id, Duration::ZERO, target.exists());
                self.retry_at = Some(Instant::now() + self.health.wait());
                self.set_status(RecState::Error, Some(target.label()), Some(format!("{e:#}")));
            }
        }
    }

    fn set_status(&self, state: RecState, target: Option<String>, error: Option<String>) {
        self.set_status_full(state, target, None, error);
    }

    fn set_status_full(&self, state: RecState, target: Option<String>, session_since: Option<u64>, error: Option<String>) {
        let encoder = self.pipeline.as_ref().map(|p| p.encoder).or_else(|| self.encoder()).map(|e| e.label.to_string());
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
            crate::toast(&self.app, &tr("Nothing to save"), &tr("Replix is not recording right now."));
            return;
        };
        let seconds = p.settings.buffer_seconds as f64;
        // The last second or so before the key press is still in the encoder, and the segment
        // holding it is not complete yet. The clip waits for it rather than ending early.
        // Stream time starts a little after launch, so this end lands just after the press.
        let end = p.now();
        let (done, current) = p.segments();
        let Some(first) = done.iter().find(|s| s.end > end - seconds).map(|s| s.index).or(current) else {
            let (title, detail) = save::nothing_yet();
            crate::toast(&self.app, &title, &detail);
            return;
        };
        // No preview from the buffer while a kept-out program may be in it.
        let preview = (!p.private_near(end - seconds, end)).then(|| p.dir.0.join(segment_name(first)));
        let placeholder = Placeholder::announce(&self.app, Kind::Clip, &p.target.label(), seconds, preview);
        let pending = PendingClip {
            end,
            pressed: audio::now_ms(),
            deadline: Instant::now() + Duration::from_secs_f64(SEGMENT_SECONDS + 4.0),
            pin: p.pin(first),
            placeholder,
        };
        p.pending.push(pending);
        crate::toast(&self.app, &tr("Saving clip…"), &p.target.label());
    }

    /// Saves the clips whose end has been written, or all of them once the capture stopped.
    fn finish_clips(app: &AppHandle, settings: &Settings, p: &mut Pipeline, all: bool) {
        let seconds = p.settings.buffer_seconds as f64;
        let (done, _) = p.segments();
        let written = done.back().map_or(f64::MIN, |s| s.end);
        let now = Instant::now();
        let stream_now = p.now();
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
                let (title, detail) = save::nothing_yet();
                crate::toast(app, &title, &detail);
                clip.placeholder.finish(None);
                continue;
            };
            let skip = (start - first.start).max(0.0);
            let length = end - first.start - skip;
            let trim = skip > 0.1 || last.end - end > 0.1;
            let (origin, until) = (first.start, last.end);
            let job = SaveJob {
                app: app.clone(),
                dir: p.dir.clone(),
                segments: picked.iter().map(|s| s.index).collect(),
                tracks: p.router.tracks_between(clip.pressed.saturating_sub((seconds as u64 + 2) * 1000), clip.pressed),
                kind: Kind::Clip,
                name: p.target.label(),
                trim_to: trim.then_some((skip, length)),
                duration: until - origin,
                fps: p.settings.fps,
                clips_dir: settings.clips_dir.clone(),
                limit_gb: settings.storage_limit_gb,
                origin,
                markers: p.markers.iter().copied().filter(|m| *m >= origin && *m <= until).collect(),
                private: private_masks(&p.private, origin, until, stream_now),
                encoder: p.encoder,
                kbps: p.settings.quality.video_kbps(),
                _pin: clip.pin,
                placeholder: clip.placeholder,
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
            crate::toast(&self.app, &tr("Can't start a session"), &tr("Replix is not recording right now."));
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
            crate::toast(&self.app, &tr("Session started"), &tr("Press the session key again to stop."));
        }
    }

    fn finish_session(app: &AppHandle, settings: &Settings, p: &mut Pipeline) {
        let Some(Session { first, since }) = p.session else { return };
        let stream_now = p.now();
        let (done, _) = p.segments();
        let segments: Vec<Segment> = done.iter().filter(|s| s.index >= first).cloned().collect();
        let duration = segments.iter().map(Segment::duration).sum();
        let (Some(head), Some(tail)) = (segments.first(), segments.last()) else {
            crate::toast(app, &tr("Nothing to save yet"), &tr("The session has no completed recording segments."));
            return;
        };
        let (origin, until, first) = (head.start, tail.end, head.index);
        let tracks = p.router.tracks_between(since, audio::now_ms());
        let name = p.target.label();
        let private = private_masks(&p.private, origin, until, stream_now);
        let preview = private.is_empty().then(|| p.dir.0.join(segment_name(first)));
        let placeholder = Placeholder::announce(app, Kind::Session, &name, duration, preview);
        let job = SaveJob {
            app: app.clone(),
            dir: p.dir.clone(),
            segments: segments.iter().map(|s| s.index).collect(),
            tracks,
            kind: Kind::Session,
            name,
            trim_to: None,
            duration,
            fps: p.settings.fps,
            clips_dir: settings.clips_dir.clone(),
            limit_gb: settings.storage_limit_gb,
            origin,
            markers: p.markers.iter().copied().filter(|m| *m >= origin && *m <= until + SEGMENT_SECONDS).collect(),
            private,
            encoder: p.encoder,
            kbps: p.settings.quality.video_kbps(),
            _pin: p.pin(first),
            placeholder,
        };
        crate::toast(app, &tr("Saving session…"), &job.name);
        job.start();
    }

    /// Marks this moment; the editor shows it on the timeline of every clip or session that
    /// covers it.
    fn add_marker(&mut self) {
        let Some(p) = self.pipeline.as_mut() else {
            crate::toast(&self.app, &tr("Can't set a marker"), &tr("Replix is not recording right now."));
            return;
        };
        let now = p.now();
        p.markers.push(now);
        let detail = match &p.session {
            Some(s) => save::into_session(audio::now_ms().saturating_sub(s.since) as f64 / 1000.0),
            None => p.target.label(),
        };
        crate::toast(&self.app, &tr("Marker set"), &detail);
    }

    fn screenshot(&self) {
        let target = self.pipeline.as_ref().map(|p| p.target.clone()).or_else(|| {
            let w = platform::foreground()?;
            (!self.settings.is_excluded(&w.exe)).then(|| Target::Window { hwnd: w.hwnd, pid: w.pid, name: platform::game_name(&w), exe: w.exe })
        });
        let Some(target) = target else { return };
        // On a picture of the whole screen, windows of programs kept out of recordings are black.
        let hide: String = match target.screen() {
            Some(index) => {
                platform::excluded_areas(index, &self.settings.excluded_exes(), &mut platform::WindowMemo::default()).iter().map(|(_, a)| format!(",{}", save::black_box(a))).collect()
            }
            None => String::new(),
        };
        let app = self.app.clone();
        let clips_dir = self.settings.clips_dir.clone();
        let cursor = self.settings.capture_cursor;
        std::thread::spawn(move || {
            let name = target.label();
            let path = library::new_path(&clips_dir, &name, "png", None);
            let capture = target.capture(30, cursor, &format!("{}{hide}", platform::DOWNLOAD));
            let mut args: Vec<String> = capture.input;
            if let Some(filter) = capture.filter {
                args.extend(["-filter:v".into(), filter]);
            }
            args.extend([
                "-frames:v".into(),
                "1".into(),
                "-update".into(),
                "1".into(),
                "-y".into(),
                ffmpeg::path_arg(&path),
            ]);
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
                    crate::toast(&app, &tr("Screenshot saved"), &name);
                }
                Err(e) => {
                    log::warn!("screenshot failed: {e}");
                    crate::toast(&app, &tr("Screenshot failed"), &tr("The picture could not be taken."));
                }
            }
        });
    }
}
