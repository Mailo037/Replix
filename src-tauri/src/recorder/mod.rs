//! The replay buffer.
//!
//! FFmpeg captures the target continuously and cuts the stream into two-second
//! MPEG-TS segments with one audio stream per slot. Saving a clip joins the newest
//! segments without re-encoding; a session copies every segment from its start into a
//! file of its own as it goes (see `session`), so it survives a crash.

mod health;
mod pipeline;
mod save;
mod session;

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
use save::{Footage, Placeholder, SaveJob};
use session::Live;

/// How often the library's picture of a running session is renewed while someone looks.
const PICTURE_EVERY: Duration = Duration::from_secs(30);

/// Whether a file in the clips folder is a session still being recorded.
pub(crate) fn is_live_file(path: &std::path::Path) -> bool {
    session::is_live(path)
}

/// A locked screen or a sleeping graphics adapter can make every encoder test fail.
const ENCODER_RETRY: Duration = Duration::from_secs(60);

type EncoderResult = (Vec<&'static Encoder>, bool);
type EncoderProbe = std::thread::JoinHandle<EncoderResult>;

/// Slow graphics tests never hold up a pause or a change of source.
fn finish_encoder_probe(probe: &mut Option<EncoderProbe>) -> Option<EncoderResult> {
    let probe = probe.take_if(|probe| probe.is_finished())?;
    Some(probe.join().unwrap_or_else(|_| {
        log::error!("encoder test thread failed");
        (Vec::new(), false)
    }))
}

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

/// Waiting means there is no target, not that the recorder cannot encode it.
fn idle_status(
    enabled: bool,
    has_encoder: bool,
    retry_error: Option<String>,
) -> (RecState, Option<String>) {
    if !enabled {
        (RecState::Off, None)
    } else if !has_encoder {
        (
            RecState::Error,
            Some(tr("No video encoder works on this PC.")),
        )
    } else if let Some(error) = retry_error {
        (RecState::Error, Some(error))
    } else {
        (RecState::Waiting, None)
    }
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
            encoder_probe: None,
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
    encoder_probe: Option<EncoderProbe>,
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
            self.retry_at = Some(Instant::now() + ENCODER_RETRY);
            self.set_status(RecState::Error, None, Some(tr("No video encoder works on this PC.")));
        }
        let buffer_root = self.buffer_root();
        let _ = std::fs::remove_dir_all(&buffer_root);
        session::clear_pictures(&previews_dir(&self.app));
        self.recover_sessions();
        loop {
            match rx.recv_timeout(Duration::from_millis(500)) {
                Ok(command) => self.handle(command),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
            // Tests can finish between commands; apply all settings already handed over
            // before a recovered encoder starts recording again.
            for command in rx.try_iter() {
                self.handle(command);
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

    /// Sessions that broke off (a crash, a power cut, Replix quit in the middle) are still
    /// in the clips folder as they were recorded; each becomes a session in the library.
    /// They are listed before anything records, so a session started now is never taken
    /// for one, and saved one after another beside the recording.
    fn recover_sessions(&self) {
        let found = session::leftovers(&self.settings.clips_dir);
        if found.is_empty() {
            return;
        }
        let (app, settings) = (self.app.clone(), self.settings.clone());
        let encoder = self.encoder().unwrap_or_else(encoder::software);
        let work = move || {
            for left in found {
                log::info!("recovering the session in {}", left.file.display());
                let (origin, private) = left.journal.masks_from_start();
                let journal = left.journal;
                let seconds = left.ended.saturating_sub(journal.since) as f64 / 1000.0;
                let tracks = if journal.tracks.is_empty() { session::tracks_in(&left.file) } else { journal.tracks };
                let placeholder = Placeholder::announce(&app, Kind::Session, &journal.game, seconds, None);
                let job = SaveJob {
                    app: app.clone(),
                    footage: Footage::Live { file: left.file, _picture: None },
                    tracks,
                    kind: Kind::Session,
                    name: journal.game,
                    trim_to: None,
                    duration: seconds,
                    fps: if journal.fps > 0 { journal.fps } else { settings.fps },
                    clips_dir: settings.clips_dir.clone(),
                    limit_gb: settings.storage_limit_gb,
                    origin,
                    markers: journal.markers,
                    private,
                    encoder,
                    kbps: if journal.kbps > 0 { journal.kbps } else { settings.quality.video_kbps() },
                    recorded: Some(left.ended),
                    placeholder,
                };
                job.run();
            }
        };
        if let Err(e) = std::thread::Builder::new().name("recovery".into()).spawn(work) {
            log::error!("sessions left behind could not be recovered now: {e}");
        }
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

    fn refresh_encoders(&mut self) {
        if let Some((found, cached)) = finish_encoder_probe(&mut self.encoder_probe) {
            let empty = found.is_empty();
            *self.encoders.lock() = found;
            self.encoders_cached = cached;
            self.retry_at = empty.then(|| Instant::now() + ENCODER_RETRY);
        }
        if self.settings.enabled && self.encoders.lock().is_empty()
            && self.encoder_probe.is_none() && self.retry_at.is_none_or(|t| Instant::now() >= t)
        {
            let cache = self.encoder_cache();
            self.encoder_probe = Some(std::thread::spawn(move || encoder::detect_cached(&cache)));
        }
    }

    fn tick(&mut self) {
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
        self.refresh_encoders();
        if self.settings.enabled && self.encoder_probe.is_some() {
            self.set_status(RecState::Starting, None, None);
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

        let excluded = self.settings.excluded_exes();
        let mut failed = None;
        if let Some(p) = self.pipeline.as_mut() {
            if !p.alive() {
                failed = Some(p.last_error());
            } else {
                Self::finish_clips(&self.app, &self.settings, p, false);
                if !p.feed_session() {
                    // The session's file takes no more: a full or lost drive. What is in
                    // it is saved as it is, and the rest of the session stays in the
                    // buffer until it ends.
                    let rest = p.session.as_ref().map(|s| (s.since, s.live.as_ref().map_or(s.first, Live::written)));
                    Self::finish_session(&self.app, &self.settings, p);
                    if let Some((since, first)) = rest {
                        p.session = Some(Session { first, since, live: None, pictured: None, pictures: 0 });
                        p.router.hold = true;
                    }
                }
                Self::picture_session(&self.app, p);
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
                self.encoders.lock().clear();
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
            None => {
                let current = self.status.lock().clone();
                let retry_error = self
                    .retry_at
                    .filter(|t| Instant::now() < *t)
                    .and(current.error);
                let (state, error) =
                    idle_status(self.settings.enabled, self.encoder().is_some(), retry_error);
                let target = if state == RecState::Error {
                    current.target
                } else {
                    None
                };
                self.set_status(state, target, error);
            }
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
                footage: Footage::Buffer { dir: p.dir.clone(), segments: picked.iter().map(|s| s.index).collect(), _pin: clip.pin },
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
                recorded: None,
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
        } else {
            let (done, current) = p.segments();
            let first = current.unwrap_or_else(|| done.back().map(|s| s.index + 1).unwrap_or(0));
            // Written straight into the clips folder while it runs. If that cannot start,
            // the session is kept in the buffer and saved when it ends.
            let file = library::new_path(&self.settings.clips_dir, &p.target.label(), session::LIVE_EXT, Some("Session"));
            let live = Live::start(p.dir.clone(), file, first)
                .inspect_err(|e| log::warn!("the session is kept in the buffer; its own file could not be started: {e:#}"))
                .ok();
            p.session = Some(Session { first, since: audio::now_ms(), live, pictured: None, pictures: 0 });
            p.router.hold = true;
            p.feed_session();
            crate::toast(&self.app, &tr("Session started"), &tr("Press the session key again to stop."));
        }
    }

    /// A picture of the running session for its tile in the library, renewed now and then,
    /// and only while someone has Replix in front of them.
    fn picture_session(app: &AppHandle, p: &mut Pipeline) {
        if !(crate::page_visible() && crate::app_focused()) {
            return;
        }
        let Some(session) = p.session.as_ref() else { return };
        if session.pictured.is_some_and(|at| at.elapsed() < PICTURE_EVERY) {
            return;
        }
        let (since, first, n) = (session.since, session.first, session.pictures);
        let Some(segment) = p.segments.back().filter(|s| s.index >= first).cloned() else { return };
        // Never from a stretch that may show a program kept out of recordings.
        let private = p.private_near(segment.start, segment.end);
        if let Some(session) = p.session.as_mut() {
            session.pictured = Some(Instant::now());
            session.pictures += u32::from(!private);
        }
        if private {
            return;
        }
        let (dir, pin) = (p.dir.clone(), p.pin(segment.index));
        let previews = previews_dir(app);
        let app = app.clone();
        std::thread::spawn(move || {
            let out = previews.join(session::picture_name(since, n));
            let go = || crate::page_visible();
            if library::make_thumb(&dir.0.join(segment_name(*pin)), Kind::Clip, 0.0, &out, &go).is_ok() {
                let _ = app.emit("session-picture", (since, out.to_string_lossy().into_owned()));
                // The page has moved on to this one.
                if let Some(before) = n.checked_sub(1) {
                    let _ = std::fs::remove_file(previews.join(session::picture_name(since, before)));
                }
            }
        });
    }

    fn finish_session(app: &AppHandle, settings: &Settings, p: &mut Pipeline) {
        let Some(Session { first, since, live, pictures, .. }) = p.session.take() else { return };
        p.router.hold = false;
        let previews = previews_dir(app);
        for n in pictures.saturating_sub(2)..pictures {
            let _ = std::fs::remove_file(previews.join(session::picture_name(since, n)));
        }
        if let Some(live) = live {
            Self::finish_live(app, settings, p, since, live);
            return;
        }
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
            footage: Footage::Buffer { dir: p.dir.clone(), segments: segments.iter().map(|s| s.index).collect(), _pin: p.pin(first) },
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
            recorded: None,
            placeholder,
        };
        crate::toast(app, &tr("Saving session…"), &job.name);
        job.start();
    }

    /// Ends a session that wrote its own file: the last segments go in, the file is closed,
    /// and it becomes an MP4 in the library.
    fn finish_live(app: &AppHandle, settings: &Settings, p: &mut Pipeline, since: u64, mut live: Live) {
        let stream_now = p.now();
        p.segments();
        if !live.failed() {
            live.feed(&p.segments);
        }
        // What is handed over but not yet written stays in the buffer until it is.
        let waiting = p.pin(live.written());
        let Some((origin, until)) = live.span() else {
            crate::toast(app, &tr("Nothing to save yet"), &tr("The session has no completed recording segments."));
            let file = live.file.clone();
            live.finish(move |_| {
                drop(waiting);
                session::discard(&file);
            });
            return;
        };
        let name = p.target.label();
        let private = private_masks(&p.private, origin, until, stream_now);
        // The library's picture of the save comes from the buffer, never from a stretch
        // that may show a program kept out of recordings.
        let picture = live.newest.filter(|_| private.is_empty()).map(|index| (p.dir.clone(), p.pin(index)));
        let preview = picture.as_ref().map(|(dir, pin)| dir.0.join(segment_name(**pin)));
        let placeholder = Placeholder::announce(app, Kind::Session, &name, live.seconds(), preview);
        let job = SaveJob {
            app: app.clone(),
            footage: Footage::Live { file: live.file.clone(), _picture: picture },
            tracks: p.router.tracks_between(since, audio::now_ms()),
            kind: Kind::Session,
            name,
            trim_to: None,
            duration: live.seconds(),
            fps: p.settings.fps,
            clips_dir: settings.clips_dir.clone(),
            limit_gb: settings.storage_limit_gb,
            origin,
            markers: p.markers.iter().copied().filter(|m| *m >= origin && *m <= until + SEGMENT_SECONDS).collect(),
            private,
            encoder: p.encoder,
            kbps: p.settings.quality.video_kbps(),
            recorded: None,
            placeholder,
        };
        crate::toast(app, &tr("Saving session…"), &job.name);
        live.finish(move |_| {
            drop(waiting);
            job.run();
        });
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

/// Where pictures of saves in progress and of a running session are made.
fn previews_dir(app: &AppHandle) -> PathBuf {
    app.path().app_cache_dir().unwrap_or_else(|_| std::env::temp_dir()).join("previews")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unfinished_encoder_probe_leaves_the_recorder_free() {
        let (done, wait) = std::sync::mpsc::channel();
        let mut probe = Some(std::thread::spawn(move || {
            wait.recv().unwrap();
            (vec![encoder::software()], false)
        }));
        let result = finish_encoder_probe(&mut probe);
        // Release the test thread even if a later assertion fails.
        done.send(()).unwrap();
        assert!(result.is_none());
        assert!(probe.is_some(), "a running test stays available for the next tick");
        probe.take().unwrap().join().unwrap();
    }

    #[test]
    fn a_finished_encoder_probe_is_collected_once() {
        let mut probe = Some(std::thread::spawn(|| (vec![encoder::software()], false)));
        let deadline = Instant::now() + Duration::from_secs(2);
        while !probe.as_ref().unwrap().is_finished() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(1));
        }
        let (found, cached) = finish_encoder_probe(&mut probe).expect("test completed");
        assert_eq!(found, vec![encoder::software()]);
        assert!(!cached);
        assert!(probe.is_none());
        assert!(finish_encoder_probe(&mut probe).is_none());
    }

    #[test]
    fn missing_encoders_are_never_reported_as_waiting_for_a_target() {
        for retry_error in [None, Some("capture failed".to_string())] {
            let (state, error) = idle_status(true, false, retry_error);
            assert_eq!(state, RecState::Error);
            assert!(error.is_some_and(|e| !e.is_empty()));
        }
    }

    #[test]
    fn pausing_clears_encoder_and_capture_errors() {
        for has_encoder in [false, true] {
            assert_eq!(
                idle_status(false, has_encoder, Some("capture failed".into())),
                (RecState::Off, None)
            );
        }
    }

    #[test]
    fn capture_errors_stay_visible_until_the_next_attempt() {
        let error = "capture failed".to_string();
        assert_eq!(
            idle_status(true, true, Some(error.clone())),
            (RecState::Error, Some(error))
        );
    }

    #[test]
    fn working_encoders_without_a_target_wait_normally() {
        assert_eq!(idle_status(true, true, None), (RecState::Waiting, None));
    }
}
