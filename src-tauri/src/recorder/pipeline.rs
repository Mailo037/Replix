//! One running capture: FFmpeg writing the buffer, the audio router feeding it, and what
//! happened while it ran (markers, moments a kept-out program was on screen).

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Seek, SeekFrom, Write};
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Stdio};
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use parking_lot::Mutex;

use super::PendingClip;
use crate::audio::Router;
use crate::encoder::{self, Encoder};
use crate::ffmpeg::{self, SpawnTied};
use crate::settings::Settings;
use crate::win;

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
            Target::Window { hwnd, .. } => win::is_window(*hwnd),
            Target::Monitor { .. } => true,
        }
    }

    pub fn capture_source(&self, fps: u32, cursor: bool) -> String {
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
        // Even sizes: several encoders reject odd widths. A game that changes its resolution
        // or leaves full screen is scaled into the first size, never cut off.
        format!(
            "gfxcapture={what}:max_framerate={fps}:capture_cursor={}:width=-2:height=-2:resize_mode=scale_aspect",
            u8::from(cursor)
        )
    }
}

pub(super) struct Session {
    pub first: u64,
    pub since: u64,
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

/// A stretch of stream time in which a program kept out of recordings was on the
/// recorded screen; the newest may still be open.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Private {
    pub start: f64,
    pub end: Option<f64>,
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
    pub private: Vec<Private>,
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
                let root = win::root_of(*pid, &win::processes());
                router.start_main(Some((root, exe.clone(), name.clone())));
            }
            Target::Monitor { .. } => router.start_main(None),
        }

        let steady = match &target {
            Target::Monitor { index } => win::dxgi_output(*index).is_some(),
            Target::Window { .. } => settings.source == crate::settings::CaptureSource::Game,
        };
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
            newest: None,
            grew: started,
            steady,
        })
    }

    pub fn alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
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
        if let Some(oldest) = self.segments.front().map(|s| s.start - PRIVACY_MARGIN) {
            self.markers.retain(|m| *m >= oldest);
            self.private.retain(|p| p.end.is_none_or(|e| e >= oldest));
        }
    }

    pub fn pin(&mut self, first: u64) -> Arc<u64> {
        let pin = Arc::new(first);
        self.pins.push(Arc::downgrade(&pin));
        pin
    }

    /// Notes whether a program kept out of recordings is on the recorded screen now. A
    /// window capture shows only its own window, so only screens need watching.
    pub fn watch_privacy(&mut self, excluded: &[String]) {
        let Target::Monitor { index } = self.target else { return };
        let showing = win::excluded_on_monitor(index, excluded);
        let now = self.now();
        note_private(&mut self.private, showing.is_some(), now);
    }

    /// Whether a kept-out program was on screen between two stream times, or is now.
    pub fn private_near(&self, from: f64, to: f64) -> bool {
        !private_spans(&self.private, from, to, self.now()).is_empty()
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
            if win::is_minimized(hwnd) {
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

/// Opens a private stretch when a kept-out program shows up, and closes it once it is gone.
pub(super) fn note_private(spans: &mut Vec<Private>, showing: bool, now: f64) {
    let open = spans.last().is_some_and(|p| p.end.is_none());
    match (showing, open) {
        (true, false) => spans.push(Private { start: now, end: None }),
        (false, true) => {
            if let Some(p) = spans.last_mut() {
                p.end = Some(now);
            }
        }
        _ => {}
    }
}

/// The private stretches that touch `from..to`, widened by the margin, cut to that range and
/// joined where they overlap; in stream time. An open one lasts until `now`.
pub(super) fn private_spans(spans: &[Private], from: f64, to: f64, now: f64) -> Vec<(f64, f64)> {
    let mut out: Vec<(f64, f64)> = Vec::new();
    for p in spans {
        let start = (p.start - PRIVACY_MARGIN).max(from);
        let end = (p.end.unwrap_or(now) + PRIVACY_MARGIN).min(to);
        if end <= start {
            continue;
        }
        match out.last_mut() {
            Some(last) if start <= last.1 => last.1 = last.1.max(end),
            _ => out.push((start, end)),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_stretches_open_close_and_widen() {
        let mut spans = Vec::new();
        note_private(&mut spans, false, 1.0);
        assert!(spans.is_empty());
        note_private(&mut spans, true, 10.0);
        note_private(&mut spans, true, 10.5);
        note_private(&mut spans, false, 12.0);
        note_private(&mut spans, true, 13.5);
        assert_eq!(spans, vec![Private { start: 10.0, end: Some(12.0) }, Private { start: 13.5, end: None }]);
        // The margins make the two touch, so they come out as one; the open one runs to now.
        assert_eq!(private_spans(&spans, 0.0, 30.0, 20.0), vec![(9.0, 21.0)]);
        // Cut to the clip.
        assert_eq!(private_spans(&spans, 11.0, 15.0, 20.0), vec![(11.0, 15.0)]);
        assert!(private_spans(&spans, 22.0, 30.0, 20.0).is_empty());
    }

    #[test]
    fn a_resized_game_is_scaled_not_cut() {
        let window = Target::Window { hwnd: 1, pid: 1, exe: "game.exe".into(), name: "Game".into() };
        assert!(window.capture_source(60, false).contains("resize_mode=scale_aspect"));
    }
}
