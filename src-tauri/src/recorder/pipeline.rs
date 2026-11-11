//! One running capture: FFmpeg writing the buffer, the audio router feeding it, and what
//! happened while it ran (markers, moments a kept-out program was on screen).

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Stdio};
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use parking_lot::Mutex;

use super::session::{Hidden, Journal, Live};
use super::PendingClip;
use crate::audio::Router;
use crate::encoder::{self, Encoder};
use crate::ffmpeg::{self, SpawnTied};
use crate::settings::Settings;
use crate::platform::{self, Area, Capture, Grab};

pub(super) const SEGMENT_SECONDS: f64 = 2.0;

/// Added on both sides of a stretch a kept-out program was on screen: it is looked for
/// twice a second, and a window can open between two looks.
pub(super) const PRIVACY_MARGIN: f64 = 1.0;

/// How long the buffer may go without a new segment before the capture counts as stuck.
const STALL: Duration = Duration::from_secs(20);

#[derive(Debug, Clone, PartialEq)]
pub(super) enum Target {
    Window { hwnd: u64, pid: u32, exe: String, name: String },
    Monitor { index: u32 },
}

impl Target {
    pub fn exe(&self) -> Option<&str> {
        match self {
            Target::Window { exe, .. } => Some(exe),
            Target::Monitor { .. } => None,
        }
    }

    /// Also the folder and game name clips are filed under, so it stays the same in every
    /// language; the page translates "Desktop" where it shows it.
    pub fn label(&self) -> String {
        match self {
            Target::Window { name, .. } => name.clone(),
            Target::Monitor { index: 0 } => "Desktop".into(),
            Target::Monitor { index } => format!("Desktop {}", index + 1),
        }
    }

    /// Whether what is recorded is still there; a closed game is no fault of the encoder.
    pub fn exists(&self) -> bool {
        match self {
            Target::Window { hwnd, .. } => platform::is_window(*hwnd),
            Target::Monitor { .. } => true,
        }
    }

    /// The FFmpeg input for what is recorded; `chain` is added to its filters.
    pub fn capture(&self, fps: u32, cursor: bool, chain: &str) -> Capture {
        let grab = match self {
            Target::Window { hwnd, .. } => Grab::Window(*hwnd),
            Target::Monitor { index } => Grab::Monitor(*index),
        };
        platform::capture(grab, fps, cursor, chain)
    }

    /// The monitor whose picture is recorded, when it is a whole one: a screen, or on macOS
    /// the display a game is on. Kept-out programs are looked for there.
    pub fn screen(&self) -> Option<u32> {
        match self {
            Target::Monitor { index } => Some(*index),
            Target::Window { hwnd, .. } => platform::screen_of_window(*hwnd),
        }
    }
}

pub(super) struct Session {
    pub first: u64,
    pub since: u64,
    /// Writes the session into the clips folder while it runs. None when that could not
    /// start, or stopped: the buffer then keeps the session's segments until it ends.
    pub live: Option<Live>,
    /// When a picture of the session was last made for the library, and how many so far.
    pub pictured: Option<Instant>,
    pub pictures: u32,
}

pub(super) struct BufferDir(pub PathBuf);

impl Drop for BufferDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[derive(Clone)]
pub(super) struct Segment {
    pub index: u64,
    /// Stream time in seconds.
    pub start: f64,
    pub end: f64,
}

impl Segment {
    pub fn duration(&self) -> f64 {
        (self.end - self.start).max(0.0)
    }
}

/// Where the windows of programs kept out of recordings were on the recorded screen, from
/// one look to the last look that found them the same. Empty once they are gone.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Look {
    pub from: f64,
    pub to: f64,
    pub shown: Vec<(u64, Area)>,
}

/// A part of the picture to black out, in stream time.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Mask {
    pub area: Area,
    pub start: f64,
    pub end: f64,
}

pub(super) struct Pipeline {
    pub target: Target,
    pub dir: Arc<BufferDir>,
    child: Child,
    stdin: Option<ChildStdin>,
    pub router: Router,
    log: Arc<Mutex<VecDeque<String>>>,
    pub session: Option<Session>,
    /// The settings this capture runs with, including the target's own ones.
    pub settings: Settings,
    pub encoder: &'static Encoder,
    pub last_poll: Instant,
    pub segments: VecDeque<Segment>,
    list_offset: u64,
    /// Each save owns its pin until it finishes reading the buffer.
    pins: Vec<Weak<u64>>,
    /// When FFmpeg was launched; stream time counts from just after it.
    pub started: Instant,
    /// Clips whose last moment is still on its way through the encoder.
    pub pending: Vec<PendingClip>,
    /// Moments marked with the marker key, in stream time.
    pub markers: Vec<f64>,
    pub private: Vec<Look>,
    memo: platform::WindowMemo,
    /// The newest segment seen, and when it first showed up.
    newest: Option<u64>,
    grew: Instant,
    /// Pictures keep coming whatever is on screen: Desktop Duplication repeats the last one,
    /// and games draw all the time. Graphics Capture sends nothing while an app's window
    /// stays still, so a quiet buffer means nothing there.
    steady: bool,
}

impl Pipeline {
    pub fn start(dir: PathBuf, target: Target, settings: &Settings, encoder: &'static Encoder) -> Result<Self> {
        std::fs::create_dir_all(&dir)?;
        let buffer = Arc::new(BufferDir(dir.clone()));
        let run = dir.file_name().and_then(|n| n.to_str()).unwrap_or("run").to_string();
        let keep = Duration::from_secs(settings.buffer_seconds as u64 + 15);
        let (mut router, pipes) = Router::new(&run, settings.record_mic, settings.mic_device.clone(), keep)?;
        match &target {
            Target::Window { pid, exe, name, .. } => {
                let root = platform::root_of(*pid, &platform::processes());
                router.start_main(Some((root, exe.clone(), name.clone())));
            }
            Target::Monitor { .. } => router.start_main(None),
        }

        let steady = match &target {
            Target::Monitor { index } => platform::steady_screen(*index),
            Target::Window { .. } => settings.source == crate::settings::CaptureSource::Game,
        };
        let capture = target.capture(settings.fps, settings.capture_cursor, &format!(",fps={}{}", settings.fps, encoder.filters));
        // A failed audio input must end the capture too, so the worker can restart it.
        let mut args: Vec<String> = vec!["-hide_banner".into(), "-loglevel".into(), "warning".into(), "-xerror".into()];
        args.extend(encoder.device.iter().map(|a| a.to_string()));
        args.extend(capture.input);
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
        if let Some(filter) = capture.filter {
            args.extend(["-filter:v".into(), filter]);
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
        let mut cmd = std::process::Command::new(ffmpeg::exe());
        platform::background(&mut cmd, false);
        let mut child = cmd
            .args(&args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn_tied()
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
            encoder,
            last_poll: Instant::now(),
            segments: VecDeque::new(),
            list_offset: 0,
            pins: Vec::new(),
            started,
            pending: Vec::new(),
            markers: Vec::new(),
            private: Vec::new(),
            memo: platform::WindowMemo::default(),
            newest: None,
            grew: started,
            steady,
        })
    }

    pub fn alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None)) && self.router.alive()
    }

    /// Stream time now, in seconds.
    pub fn now(&self) -> f64 {
        self.started.elapsed().as_secs_f64()
    }

    pub fn last_error(&self) -> String {
        let q = self.log.lock();
        let text: Vec<&str> = q.iter().map(String::as_str).collect();
        ffmpeg::last_lines(&text.join("\n"), 3)
    }

    pub fn stop(&mut self) {
        self.router.prepare_stop();
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
    pub fn segments(&mut self) -> (&VecDeque<Segment>, Option<u64>) {
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

    /// Deletes segments that fell out of the buffer and are not kept by a session or a save,
    /// and forgets markers and private stretches from before what is left.
    pub fn trim(&mut self, buffer_seconds: u32) {
        self.pins.retain(|pin| pin.strong_count() > 0);
        let (done, _) = self.segments();
        let keep = ((buffer_seconds as f64 + 6.0) / SEGMENT_SECONDS).ceil() as usize;
        if done.len() > keep {
            let mut floor = done[done.len() - keep].index;
            if let Some(s) = &self.session {
                // What is in the session's file no longer has to stay in the buffer.
                floor = floor.min(s.live.as_ref().map_or(s.first, Live::written));
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
        // A session's file reaches back further than the buffer does.
        let session_start = self.session.as_ref().and_then(|s| s.live.as_ref()).and_then(Live::span).map(|s| s.0);
        let oldest = self.segments.front().map(|s| session_start.map_or(s.start, |start| start.min(s.start)) - PRIVACY_MARGIN);
        if let Some(oldest) = oldest {
            self.markers.retain(|m| *m >= oldest);
            // The newest look stays: it tells whether the windows are still there.
            let last = self.private.len().saturating_sub(1);
            let mut i = 0;
            self.private.retain(|p| {
                i += 1;
                i - 1 == last || p.to >= oldest
            });
        }
    }

    pub fn pin(&mut self, first: u64) -> Arc<u64> {
        let pin = Arc::new(first);
        self.pins.push(Arc::downgrade(&pin));
        pin
    }

    /// Notes where the windows of programs kept out of recordings are on the recorded screen
    /// now. A window capture shows only its own window, so only screens need watching.
    pub fn watch_privacy(&mut self, excluded: &[String]) {
        let Some(index) = self.target.screen() else { return };
        let shown = platform::excluded_areas(index, excluded, &mut self.memo);
        let now = self.now();
        note_private(&mut self.private, shown, now);
    }

    /// Hands the session's file the segments completed since the last look, and keeps its
    /// journal up to date. False once the file can take no more.
    pub fn feed_session(&mut self) -> bool {
        self.segments();
        let now = self.now();
        let Some(session) = self.session.as_mut() else { return true };
        let Some(live) = session.live.as_mut() else { return true };
        live.feed(&self.segments);
        if live.failed() {
            return false;
        }
        let origin = live.span().map(|s| s.0);
        let from = origin.unwrap_or(now);
        let journal = Journal {
            game: self.target.label(),
            since: session.since,
            fps: self.settings.fps,
            kbps: self.settings.quality.video_kbps(),
            origin,
            tracks: self.router.tracks_between(session.since, crate::audio::now_ms()),
            markers: self.markers.iter().copied().filter(|m| *m >= from).collect(),
            // Whatever is on screen now may still be there when the session breaks off.
            private: private_masks(&self.private, from, f64::INFINITY, f64::INFINITY).iter().map(Hidden::from).collect(),
            done: false,
        };
        live.note(journal);
        true
    }

    /// Whether a kept-out program was on screen between two stream times, or is now.
    pub fn private_near(&self, from: f64, to: f64) -> bool {
        !private_masks(&self.private, from, to, self.now()).is_empty()
    }

    /// A capture that stopped delivering pictures while FFmpeg kept running: a lost graphics
    /// device, say. A minimised game sends none on purpose and is left to come back.
    pub fn stalled(&mut self) -> bool {
        let newest = self.segments.back().map(|s| s.index);
        if newest != self.newest {
            self.newest = newest;
            self.grew = Instant::now();
        }
        if let Target::Window { hwnd, .. } = self.target {
            if platform::is_minimized(hwnd) {
                self.grew = Instant::now();
            }
        }
        self.steady && self.grew.elapsed() > STALL
    }
}

pub(super) fn segment_name(index: u64) -> String {
    format!("seg{index:06}.ts")
}

fn segment_index(name: &str) -> Option<u64> {
    name.strip_prefix("seg")?.strip_suffix(".ts")?.parse().ok()
}

pub(super) fn area_key(area: &Area) -> [u32; 4] {
    [area.left, area.top, area.right, area.bottom].map(f32::to_bits)
}

/// Adds what a look at the screen found: the last look lasts longer when nothing changed.
pub(super) fn note_private(looks: &mut Vec<Look>, mut shown: Vec<(u64, Area)>, now: f64) {
    shown.sort_unstable_by_key(|(hwnd, area)| (*hwnd, area_key(area)));
    match looks.last_mut() {
        Some(last) if last.shown == shown => last.to = now,
        None if shown.is_empty() => {}
        _ => looks.push(Look { from: now, to: now, shown }),
    }
}

/// Samples are sorted by handle, with all the visible fragments of a window together.
fn areas_of(look: &Look, hwnd: u64) -> &[(u64, Area)] {
    let from = look.shown.partition_point(|(h, _)| *h < hwnd);
    let to = look.shown.partition_point(|(h, _)| *h <= hwnd);
    &look.shown[from..to]
}

/// The boxes to black out in `from..to`, in stream time. A window that shows up or goes away
/// is hidden for the margin longer on that side. Changed geometry is bounded between looks;
/// unchanged fragments stay separate so foreground windows remain visible through the gaps.
/// The newest look lasts until `now`.
pub(super) fn private_masks(looks: &[Look], from: f64, to: f64, now: f64) -> Vec<Mask> {
    let mut raw = Vec::new();
    for (i, look) in looks.iter().enumerate() {
        let prev = i.checked_sub(1).map(|p| &looks[p]);
        let next = looks.get(i + 1);
        let until = if next.is_none() {
            now.max(look.to)
        } else {
            look.to
        };
        for window in look.shown.chunk_by(|(a, _), (b, _)| a == b) {
            let hwnd = window[0].0;
            let before = prev.is_some_and(|p| !areas_of(p, hwnd).is_empty());
            let after = next.map(|n| areas_of(n, hwnd)).unwrap_or(&[]);
            let start = if before {
                look.from
            } else {
                look.from - PRIVACY_MARGIN
            };
            let end = if !after.is_empty() {
                until
            } else {
                next.map_or(until, |n| n.from) + PRIVACY_MARGIN
            };
            raw.extend(window.iter().map(|&(_, area)| Mask { area, start, end }));
            if let Some(n) = next.filter(|_| !after.is_empty()) {
                let unchanged = window
                    .iter()
                    .map(|(_, a)| area_key(a))
                    .eq(after.iter().map(|(_, a)| area_key(a)));
                if unchanged {
                    raw.extend(window.iter().map(|&(_, area)| Mask {
                        area,
                        start: look.to,
                        end: n.from,
                    }));
                } else {
                    // Bounding all endpoint fragments keeps movement private, but only in
                    // the gap between samples: never join sibling fragments at rest.
                    let area = window
                        .iter()
                        .chain(after)
                        .map(|(_, a)| *a)
                        .reduce(Area::union)
                        .unwrap();
                    raw.push(Mask {
                        area,
                        start: look.to,
                        end: n.from,
                    });
                }
            }
        }
    }
    let mut masks: Vec<Mask> = raw
        .into_iter()
        .map(|m| Mask { start: m.start.max(from), end: m.end.min(to), ..m })
        .filter(|m| m.end > m.start)
        .collect();
    // The same box over touching stretches is one box.
    masks.sort_by(|a, b| {
        area_key(&a.area)
            .cmp(&area_key(&b.area))
            .then(a.start.total_cmp(&b.start))
    });
    let mut joined: Vec<Mask> = Vec::new();
    for m in masks {
        match joined.last_mut() {
            Some(last) if last.area == m.area && m.start <= last.end => {
                last.end = last.end.max(m.end)
            }
            _ => joined.push(m),
        }
    }
    joined.sort_by(|a, b| a.start.total_cmp(&b.start));
    joined
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: Area = Area { left: 0.1, top: 0.1, right: 0.3, bottom: 0.4 };
    const B: Area = Area { left: 0.5, top: 0.2, right: 0.7, bottom: 0.5 };

    fn mask(area: Area, start: f64, end: f64) -> Mask {
        Mask { area, start, end }
    }

    fn hidden_at(masks: &[Mask], at: f64, x: f32, y: f32) -> bool {
        masks.iter().any(|m| {
            m.start <= at
                && at <= m.end
                && m.area.left <= x
                && x < m.area.right
                && m.area.top <= y
                && y < m.area.bottom
        })
    }

    #[test]
    fn reordered_samples_extend_the_same_look() {
        let mut looks = Vec::new();
        note_private(&mut looks, vec![(2, B), (1, B), (1, A)], 10.0);
        note_private(&mut looks, vec![(1, A), (2, B), (1, B)], 10.5);
        assert_eq!(
            looks,
            vec![Look {
                from: 10.0,
                to: 10.5,
                shown: vec![(1, A), (1, B), (2, B)]
            }]
        );
    }

    #[test]
    fn stable_fragments_keep_the_gap_when_another_window_changes() {
        let other = Area {
            left: 0.8,
            top: 0.0,
            right: 1.0,
            bottom: 0.2,
        };
        let mut looks = Vec::new();
        note_private(&mut looks, vec![(1, B), (1, A)], 10.0);
        note_private(&mut looks, vec![(1, A), (1, B)], 11.0);
        note_private(&mut looks, vec![(2, other), (1, B), (1, A)], 11.5);
        let masks = private_masks(&looks, 0.0, 30.0, 12.0);
        assert_eq!(looks.len(), 2);
        assert_eq!(masks.len(), 3);
        assert!(masks.contains(&mask(A, 9.0, 13.0)));
        assert!(masks.contains(&mask(B, 9.0, 13.0)));
        assert!(hidden_at(&masks, 11.25, 0.2, 0.3));
        assert!(hidden_at(&masks, 11.25, 0.6, 0.3));
        assert!(!hidden_at(&masks, 11.25, 0.4, 0.3));
    }

    #[test]
    fn changed_fragment_sets_cover_all_endpoints_only_between_samples() {
        let c = Area {
            left: 0.0,
            top: 0.0,
            right: 0.2,
            bottom: 0.2,
        };
        let d = Area {
            left: 0.8,
            top: 0.7,
            right: 1.0,
            bottom: 1.0,
        };
        let mut looks = Vec::new();
        note_private(&mut looks, vec![(1, A), (1, B)], 10.0);
        note_private(&mut looks, vec![(1, B), (1, A)], 11.0);
        note_private(&mut looks, vec![(1, d), (1, c)], 11.5);
        note_private(&mut looks, vec![(1, c), (1, d)], 12.0);
        note_private(&mut looks, vec![], 13.0);
        let masks = private_masks(&looks, 0.0, 30.0, 20.0);
        assert_eq!(masks.len(), 5);
        assert!(masks.contains(&mask(A, 9.0, 11.0)));
        assert!(masks.contains(&mask(B, 9.0, 11.0)));
        assert!(masks.contains(&mask(c, 11.5, 14.0)));
        assert!(masks.contains(&mask(d, 11.5, 14.0)));
        assert!(masks.contains(&mask(A.union(B).union(c).union(d), 11.0, 11.5)));
        for area in [A, B, c, d] {
            assert!(hidden_at(
                &masks,
                11.25,
                (area.left + area.right) / 2.0,
                (area.top + area.bottom) / 2.0
            ));
        }
        assert!(!hidden_at(&masks, 10.5, 0.9, 0.8));
        assert!(!hidden_at(&masks, 12.0, 0.4, 0.6));
    }

    #[test]
    fn a_window_that_comes_and_goes_is_hidden_with_margins() {
        let mut looks = Vec::new();
        note_private(&mut looks, vec![], 1.0);
        assert!(looks.is_empty());
        note_private(&mut looks, vec![(1, A)], 10.0);
        note_private(&mut looks, vec![(1, A)], 10.5);
        note_private(&mut looks, vec![], 12.0);
        note_private(&mut looks, vec![(1, A)], 13.5);
        assert_eq!(looks.len(), 3);
        assert_eq!(looks[0], Look { from: 10.0, to: 10.5, shown: vec![(1, A)] });
        // The margins make the two touch, so they come out as one; the newest runs to now.
        assert_eq!(private_masks(&looks, 0.0, 30.0, 20.0), vec![mask(A, 9.0, 21.0)]);
        // Cut to the clip.
        assert_eq!(private_masks(&looks, 11.0, 15.0, 20.0), vec![mask(A, 11.0, 15.0)]);
        assert!(private_masks(&looks, 22.0, 30.0, 20.0).is_empty());
    }

    #[test]
    fn only_the_window_is_hidden_and_a_move_covers_both_places() {
        let mut looks = Vec::new();
        note_private(&mut looks, vec![(1, A)], 10.0);
        note_private(&mut looks, vec![(1, A)], 11.0);
        note_private(&mut looks, vec![(1, B)], 11.5);
        note_private(&mut looks, vec![], 13.0);
        let masks = private_masks(&looks, 0.0, 30.0, 20.0);
        assert_eq!(
            masks,
            vec![mask(A, 9.0, 11.0), mask(A.union(B), 11.0, 11.5), mask(B, 11.5, 14.0)]
        );
    }

    #[test]
    fn more_than_120_fragments_keep_the_foreground_opening() {
        let shown: Vec<_> = (0..160)
            .map(|i| {
                let (left, right) = if i % 2 == 0 { (0.0, 0.25) } else { (0.75, 1.0) };
                (
                    1,
                    Area {
                        left,
                        top: i as f32 / 160.0,
                        right,
                        bottom: (i + 1) as f32 / 160.0,
                    },
                )
            })
            .collect();
        let mut looks = Vec::new();
        note_private(&mut looks, shown.clone(), 10.0);
        note_private(&mut looks, shown.clone(), 10.5);
        let masks = private_masks(&looks, 0.0, 30.0, 20.0);
        assert_eq!(masks.len(), 160);
        for (_, area) in shown {
            assert!(masks.contains(&mask(area, 9.0, 21.0)));
        }
        assert!(!hidden_at(&masks, 15.0, 0.5, 0.5));
    }

    /// Even short pauses stay open once a window has appeared many times in a long session.
    #[test]
    fn more_than_120_appearances_keep_every_time_gap() {
        let mut looks = Vec::new();
        let mut shown_at = Vec::new();
        let mut t = 0.0;
        for i in 0..1000 {
            note_private(&mut looks, vec![(1, A)], t);
            shown_at.push(t);
            note_private(&mut looks, vec![], t + 0.5);
            // Every tenth pause is long.
            t += if i % 10 == 0 { 60.0 } else { 5.0 };
        }
        let masks = private_masks(&looks, 0.0, t, t);
        assert_eq!(masks.len(), shown_at.len());
        assert!(shown_at
            .iter()
            .all(|s| masks.iter().any(|m| m.start <= *s && m.end >= s + 0.5)));
        assert!(shown_at
            .iter()
            .all(|s| !hidden_at(&masks, s + 3.0, 0.2, 0.3)));
        assert!(
            masks.windows(2).all(|w| w[1].start > w[0].end),
            "every pause stays open"
        );
    }

    #[cfg(windows)]
    #[test]
    fn a_resized_game_is_scaled_not_cut() {
        let window = Target::Window { hwnd: 1, pid: 1, exe: "game.exe".into(), name: "Game".into() };
        assert!(window.capture(60, false, "").input.last().unwrap().contains("resize_mode=scale_aspect"));
    }

    #[cfg(unix)]
    #[test]
    fn a_capture_has_even_sizes_and_the_chain_after_them() {
        let screen = Target::Monitor { index: 0 };
        let capture = screen.capture(60, true, ",fps=60,format=yuv420p");
        assert!(capture.input.contains(&"-i".to_string()));
        assert_eq!(capture.filter.as_deref(), Some("scale=trunc(iw/2)*2:trunc(ih/2)*2,fps=60,format=yuv420p"));
    }
}
