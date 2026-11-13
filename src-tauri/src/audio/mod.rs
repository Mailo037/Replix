//! Audio capture: every app that plays sound gets its own track.
//!
//! FFmpeg reads a fixed number of audio inputs (named pipes), one per slot. A slot
//! is silent until an app starts playing, then that app's process tree is captured
//! into it on its own (WASAPI process loopback on Windows, the app's PulseAudio stream on
//! Linux). The game always has slot 0, the microphone
//! the last one. Which app sat in which slot, and when, is kept so a saved clip can
//! name its tracks and leave out the silent ones.

use std::collections::{HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use nnnoiseless::DenoiseState;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::pipe::PipeServer;
use crate::platform;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(unix)]
mod process;
#[cfg(windows)]
mod windows;

#[cfg(target_os = "linux")]
use self::linux as imp;
#[cfg(target_os = "macos")]
use self::macos as imp;
#[cfg(windows)]
use self::windows as imp;

use imp::{audio_sessions, stream};
pub use imp::{init_thread, input_devices};

pub const RATE: usize = 48_000;
pub const CHANNELS: usize = 2;
/// Slots for apps besides the game and the microphone.
pub const APP_SLOTS: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TrackKind {
    Game,
    App,
    Mic,
    Desktop,
}

pub fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

#[derive(Default)]
pub struct Ring {
    samples: Mutex<VecDeque<f32>>,
}

impl Ring {
    fn push(&self, data: &[f32]) {
        let mut q = self.samples.lock();
        q.extend(data.iter().copied());
        // Never hold more than a second: anything older is audio that missed its time.
        let max = RATE * CHANNELS;
        if q.len() > max {
            let excess = q.len() - max;
            q.drain(..excess);
        }
    }

    /// Takes `n` samples as FFmpeg reads them (f32, little-endian), padding with silence
    /// when the source had nothing. Written straight into `out`: this runs for every slot
    /// fifty times a second, mostly for slots that are silent.
    fn take_bytes(&self, n: usize, out: &mut Vec<u8>) {
        out.clear();
        out.reserve(n * 4);
        {
            let mut q = self.samples.lock();
            // A source that runs ahead of the clock is pulled back to a short lag.
            let ahead = RATE * CHANNELS / 4;
            if q.len() > n + ahead {
                let excess = q.len() - n - RATE * CHANNELS / 50;
                q.drain(..excess);
            }
            let k = n.min(q.len());
            let (front, back) = q.as_slices();
            let from_front = k.min(front.len());
            for s in front[..from_front].iter().chain(&back[..k - from_front]) {
                out.extend_from_slice(&s.to_le_bytes());
            }
            q.drain(..k);
        }
        // Silence is all zero bytes in f32.
        out.resize(n * 4, 0);
    }

    fn clear(&self) {
        self.samples.lock().clear();
    }
}

#[derive(Debug, Clone)]
pub enum Source {
    // macOS cannot capture one app's sound, so it never looks at the process.
    #[cfg_attr(target_os = "macos", allow(dead_code))]
    Process(u32),
    /// A capture device by id; `None` follows the system default.
    Mic(Option<String>),
    Desktop,
}

/// Microphone volume as f32 bits. Read per packet, so changes apply without a restart.
static MIC_GAIN: AtomicU32 = AtomicU32::new(0x3F80_0000);

pub fn set_mic_gain(gain: f32) {
    MIC_GAIN.store(gain.clamp(0.0, 4.0).to_bits(), Ordering::Relaxed);
}

fn apply_mic_gain(samples: &mut [f32]) {
    let gain = f32::from_bits(MIC_GAIN.load(Ordering::Relaxed));
    if gain != 1.0 {
        for s in samples {
            *s = (*s * gain).clamp(-1.0, 1.0);
        }
    }
}

/// Noise suppression on the microphone, switchable while recording.
static MIC_DENOISE: AtomicBool = AtomicBool::new(true);

pub fn set_mic_denoise(on: bool) {
    MIC_DENOISE.store(on, Ordering::Relaxed);
}

/// Removes steady background noise (fans, hum, keyboard clatter) with RNNoise. Works on
/// 10 ms mono frames, so the voice comes out one frame later.
struct Denoiser {
    state: Box<DenoiseState<'static>>,
    pending: Vec<f32>,
    frame: Vec<f32>,
    out: Vec<f32>,
}

impl Denoiser {
    fn new() -> Self {
        Self { state: DenoiseState::new(), pending: Vec::new(), frame: vec![0.0; DenoiseState::FRAME_SIZE], out: Vec::new() }
    }

    /// Takes interleaved stereo and leaves the cleaned-up voice in it, on both channels.
    fn process(&mut self, samples: &mut Vec<f32>) {
        // RNNoise expects 16-bit sample values.
        self.pending.extend(samples.as_chunks::<CHANNELS>().0.iter().map(|f| f.iter().sum::<f32>() / CHANNELS as f32 * 32767.0));
        let mut used = 0;
        while self.pending.len() - used >= DenoiseState::FRAME_SIZE {
            let input = &self.pending[used..used + DenoiseState::FRAME_SIZE];
            self.state.process_frame(&mut self.frame, input);
            self.out.extend(self.frame.iter().map(|s| (s / 32767.0).clamp(-1.0, 1.0)));
            used += DenoiseState::FRAME_SIZE;
        }
        self.pending.drain(..used);
        samples.clear();
        samples.extend(self.out.drain(..).flat_map(|s| [s; CHANNELS]));
    }
}

/// Everything the microphone goes through before it is recorded or measured.
struct MicChain {
    denoiser: Denoiser,
}

impl MicChain {
    fn new() -> Self {
        Self { denoiser: Denoiser::new() }
    }

    fn process(&mut self, samples: &mut Vec<f32>) {
        if MIC_DENOISE.load(Ordering::Relaxed) {
            self.denoiser.process(samples);
        }
        apply_mic_gain(samples);
    }
}

fn capture(source: Source, ring: Arc<Ring>, stop: Arc<AtomicBool>) -> anyhow::Result<()> {
    let mut mic = matches!(source, Source::Mic(_)).then(MicChain::new);
    stream(&source, &stop, |samples| {
        if let Some(mic) = mic.as_mut() {
            mic.process(samples);
        }
        ring.push(samples);
    })
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InputDevice {
    pub id: String,
    pub name: String,
    pub default: bool,
}

/// Listens to a microphone until `stop` and reports its peak level (0 to 1, after the
/// volume) about twenty times a second. Independent of recording, for testing the mic.
pub fn meter(device: Option<String>, stop: Arc<AtomicBool>, report: impl Fn(f32) + Send + 'static) {
    let spawned = std::thread::Builder::new().name("mic-meter".into()).spawn(move || {
        let mut peak = 0f32;
        let mut last = Instant::now();
        let mut chain = MicChain::new();
        let result = stream(&Source::Mic(device), &stop, |samples| {
            chain.process(samples);
            peak = samples.iter().fold(peak, |p, s| p.max(s.abs()));
            if last.elapsed() >= Duration::from_millis(50) {
                report(peak);
                peak = 0.0;
                last = Instant::now();
            }
        });
        if let Err(e) = result {
            log::warn!("microphone test stopped: {e}");
        }
    });
    if let Err(e) = spawned {
        log::warn!("microphone test could not start: {e}");
    }
}

fn spawn_capture(source: Source, ring: Arc<Ring>, stop: Arc<AtomicBool>) -> JoinHandle<()> {
    std::thread::Builder::new()
        .name("audio-capture".into())
        .spawn(move || {
            if let Err(e) = capture(source.clone(), ring, stop) {
                log::warn!("audio capture {source:?} stopped: {e}");
            }
        })
        .expect("thread")
}

/// Feeds one pipe at real-time pace: whatever the source captured, silence otherwise.
fn spawn_writer(pipe: PipeServer, ring: Arc<Ring>, stop: Arc<AtomicBool>) -> JoinHandle<()> {
    std::thread::Builder::new()
        .name("audio-pipe".into())
        .spawn(move || {
            if let Err(e) = pipe.connect() {
                if !stop.load(Ordering::Relaxed) {
                    log::warn!("could not connect audio pipe {}: {e:#}", pipe.path);
                }
                return;
            }
            if stop.load(Ordering::Relaxed) {
                return;
            }
            ring.clear();
            let start = Instant::now();
            let mut sent: u64 = 0;
            let mut bytes: Vec<u8> = Vec::new();
            while !stop.load(Ordering::Relaxed) {
                // Every slot has its own writer; 20 ms batches keep their wake-ups low
                // while staying far inside what FFmpeg buffers.
                std::thread::sleep(Duration::from_millis(20));
                let due = (start.elapsed().as_secs_f64() * RATE as f64) as u64;
                if due <= sent {
                    continue;
                }
                let frames = (due - sent) as usize;
                ring.take_bytes(frames * CHANNELS, &mut bytes);
                if let Err(e) = pipe.write_all(&bytes) {
                    if !stop.load(Ordering::Relaxed) {
                        log::warn!("audio pipe {} stopped: {e:#}", pipe.path);
                    }
                    break;
                }
                sent = due;
            }
        })
        .expect("thread")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Span {
    pub name: String,
    /// The program's full path, for its icon.
    pub exe: Option<String>,
    pub kind: TrackKind,
    pub start: u64,
    pub end: Option<u64>,
}

struct Assignment {
    key: String,
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
    gone_since: Option<Instant>,
}

struct Slot {
    ring: Arc<Ring>,
    current: Option<Assignment>,
    history: Vec<Span>,
}

impl Slot {
    fn release(&mut self) {
        if let Some(mut a) = self.current.take() {
            a.stop.store(true, Ordering::Relaxed);
            if let Some(h) = a.handle.take() {
                let _ = h.join();
            }
        }
        if let Some(span) = self.history.last_mut() {
            span.end.get_or_insert(now_ms());
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Track {
    /// Index among the recording's audio streams.
    pub slot: usize,
    pub name: String,
    pub exe: Option<String>,
    pub kind: TrackKind,
}

pub struct Router {
    slots: Vec<Slot>,
    mic: Option<usize>,
    mic_device: Option<String>,
    writers: Vec<(String, JoinHandle<()>)>,
    writer_stop: Arc<AtomicBool>,
    game_key: Option<String>,
    /// How long a slot stays with an app that went quiet for good.
    keep_for: Duration,
    pub hold: bool,
}

impl Router {
    /// Creates the pipes FFmpeg will read, in input order.
    pub fn new(
        run: &str,
        record_mic: bool,
        mic_device: Option<String>,
        keep_for: Duration,
    ) -> anyhow::Result<(Self, Vec<String>)> {
        let total = 1 + APP_SLOTS + usize::from(record_mic);
        let writer_stop = Arc::new(AtomicBool::new(false));
        let mut slots = Vec::new();
        let mut writers = Vec::new();
        let mut paths = Vec::new();
        for i in 0..total {
            let ring = Arc::new(Ring::default());
            let pipe = PipeServer::create(&format!("replix-{}-{run}-{i}", std::process::id()))?;
            paths.push(pipe.path.clone());
            let path = pipe.path.clone();
            writers.push((path, spawn_writer(pipe, ring.clone(), writer_stop.clone())));
            slots.push(Slot { ring, current: None, history: Vec::new() });
        }
        let mic = record_mic.then_some(total - 1);
        Ok((Self { slots, mic, mic_device, writers, writer_stop, game_key: None, keep_for, hold: false }, paths))
    }

    fn assign(&mut self, slot: usize, key: String, name: String, exe: Option<String>, kind: TrackKind, source: Source) {
        self.slots[slot].release();
        let stop = Arc::new(AtomicBool::new(false));
        let handle = spawn_capture(source, self.slots[slot].ring.clone(), stop.clone());
        self.slots[slot].current = Some(Assignment { key, stop, handle: Some(handle), gone_since: None });
        self.slots[slot].history.push(Span { name, exe, kind, start: now_ms(), end: None });
    }

    /// Slot 0: the game's process tree, or the whole desktop when recording a screen.
    pub fn start_main(&mut self, game: Option<(u32, String, String)>) {
        match game {
            Some((pid, exe, name)) => {
                let key = exe.to_lowercase();
                self.game_key = Some(key.clone());
                self.assign(0, key, name, platform::process_path(pid), TrackKind::Game, Source::Process(pid));
            }
            None => {
                self.game_key = None;
                self.assign(0, "desktop".into(), "Desktop".into(), None, TrackKind::Desktop, Source::Desktop);
            }
        }
        if let Some(mic) = self.mic {
            let source = Source::Mic(self.mic_device.clone());
            self.assign(mic, "mic".into(), "Microphone".into(), None, TrackKind::Mic, source);
        }
    }

    /// Looks for apps that started playing sound and frees slots of apps that are gone.
    /// Programs in `excluded` (lower-case executable names) never get a track, and lose
    /// the one they have.
    pub fn poll(&mut self, excluded: &[String]) {
        // In screen mode slot 0 already holds every sound, so separate app tracks would
        // only duplicate it. They still help there: muting Discord needs its own track.
        let processes = platform::processes();
        let mut seen: HashSet<String> = HashSet::new();
        let own = std::process::id();
        for (pid, active) in audio_sessions() {
            if pid == 0 || pid == own || processes.get(&pid).is_some_and(|p| p.parent == own) {
                continue;
            }
            let root = platform::root_of(pid, &processes);
            // Replix's own previews play through its WebView.
            if root == own {
                continue;
            }
            let Some(entry) = processes.get(&root) else { continue };
            let key = entry.exe_lower.clone();
            if Some(&key) == self.game_key.as_ref() || key == "audiodg.exe" || excluded.contains(&key) {
                continue;
            }
            seen.insert(key.clone());
            let taken = self.slots.iter().any(|s| s.current.as_ref().is_some_and(|a| a.key == key));
            if taken || !active {
                continue;
            }
            let free = (1..=APP_SLOTS).find(|&i| self.slots[i].current.is_none());
            let Some(slot) = free else {
                log::info!("no free audio slot for {key}");
                continue;
            };
            let path = platform::process_path(root);
            let name = path.as_deref().map(platform::app_name).unwrap_or_else(|| entry.exe.clone());
            log::info!("audio track {slot}: {name} ({key}, pid {root})");
            self.assign(slot, key, name, path, TrackKind::App, Source::Process(root));
        }
        for i in 1..=APP_SLOTS {
            let keep_for = self.keep_for;
            let hold = self.hold;
            let slot = &mut self.slots[i];
            let Some(a) = slot.current.as_mut() else { continue };
            if excluded.contains(&a.key) {
                slot.release();
                continue;
            }
            if seen.contains(&a.key) {
                a.gone_since = None;
                continue;
            }
            let since = *a.gone_since.get_or_insert_with(Instant::now);
            if !hold && since.elapsed() > keep_for {
                slot.release();
            }
        }
    }

    /// The tracks that carried something between two moments (epoch ms).
    pub fn tracks_between(&self, from: u64, to: u64) -> Vec<Track> {
        let mut out = Vec::new();
        for (i, slot) in self.slots.iter().enumerate() {
            let overlapping = slot.history.iter().rev().find(|s| s.start <= to && s.end.is_none_or(|e| e >= from));
            if let Some(span) = overlapping {
                out.push(Track { slot: i, name: span.name.clone(), exe: span.exe.clone(), kind: span.kind });
            }
        }
        out
    }

    /// A lost pipe cannot be reconnected after FFmpeg has ended its input. Restart the
    /// whole capture instead of silently continuing with picture-only segments.
    pub fn alive(&self) -> bool {
        self.writers.iter().all(|(_, handle)| !handle.is_finished())
    }

    /// Stop feeding before FFmpeg is asked to quit, so an expected pipe close is not an error.
    pub fn prepare_stop(&self) {
        self.writer_stop.store(true, Ordering::Relaxed);
    }

    pub fn stop(&mut self) {
        self.prepare_stop();
        for slot in &mut self.slots {
            slot.release();
        }
        for (path, _) in &self.writers {
            PipeServer::unblock(path);
        }
        for (_, handle) in self.writers.drain(..) {
            let _ = handle.join();
        }
    }
}

impl Drop for Router {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn floats(bytes: &[u8]) -> Vec<f32> {
        bytes.as_chunks::<4>().0.iter().map(|c| f32::from_le_bytes(*c)).collect()
    }

    #[test]
    fn a_ring_hands_out_what_it_has_then_silence() {
        let ring = Ring::default();
        let mut bytes = Vec::new();
        let samples: Vec<f32> = (1..=300).map(|i| i as f32 / 512.0).collect();
        ring.push(&samples);
        ring.take_bytes(1000, &mut bytes);
        let got = floats(&bytes);
        assert_eq!(got.len(), 1000);
        assert_eq!(got[..300], samples[..]);
        assert!(got[300..].iter().all(|s| *s == 0.0));
        assert!(ring.samples.lock().is_empty());
        // An empty ring gives silence of the asked length.
        ring.take_bytes(64, &mut bytes);
        assert_eq!(bytes, vec![0u8; 256]);
    }

    /// Taken in other amounts than pushed, so the queue wraps around inside and both of its
    /// halves are read.
    #[test]
    fn a_ring_keeps_the_order_it_was_given() {
        let ring = Ring::default();
        let mut bytes = Vec::new();
        let mut next = 0u32;
        let mut expected = 0u32;
        for _ in 0..50 {
            let batch: Vec<f32> = (next + 1..=next + 480).map(|i| i as f32).collect();
            next += 480;
            ring.push(&batch);
            ring.take_bytes(400, &mut bytes);
            for s in floats(&bytes) {
                expected += 1;
                assert_eq!(s, expected as f32);
            }
        }
    }

    #[test]
    fn one_failed_writer_makes_the_router_unhealthy() {
        let (finish, waiting) = std::sync::mpsc::channel::<()>();
        let (keep_running, other) = std::sync::mpsc::channel::<()>();
        let mut router = Router {
            slots: Vec::new(),
            mic: None,
            mic_device: None,
            writers: vec![
                (
                    String::new(),
                    std::thread::spawn(move || {
                        let _ = waiting.recv();
                    }),
                ),
                (
                    String::new(),
                    std::thread::spawn(move || {
                        let _ = other.recv();
                    }),
                ),
            ],
            writer_stop: Arc::new(AtomicBool::new(false)),
            game_key: None,
            keep_for: Duration::ZERO,
            hold: false,
        };
        assert!(router.alive());
        finish.send(()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !router.writers[0].1.is_finished() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(1));
        }
        let alive = router.alive();
        let other_alive = !router.writers[1].1.is_finished();
        keep_running.send(()).unwrap();
        // These test threads have no pipes to unblock during Router::drop.
        for (_, handle) in router.writers.drain(..) {
            handle.join().unwrap();
        }
        assert!(!alive, "one failed audio input must restart the capture");
        assert!(other_alive, "the other input was still healthy");
    }
}
