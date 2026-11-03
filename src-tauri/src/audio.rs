//! Audio capture: every app that plays sound gets its own track.
//!
//! FFmpeg reads a fixed number of audio inputs (named pipes), one per slot. A slot
//! is silent until an app starts playing, then that app's process tree is captured
//! into it with WASAPI process loopback. The game always has slot 0, the microphone
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
use wasapi::{AudioClient, DeviceEnumerator, DeviceState, Direction, SampleType, SessionState, StreamMode, WaveFormat};

use crate::pipe::PipeServer;
use crate::win;

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

    /// Takes `n` samples, padding with silence when the source had nothing.
    fn take(&self, n: usize, out: &mut Vec<f32>) {
        out.clear();
        let mut q = self.samples.lock();
        // A source that runs ahead of the clock is pulled back to a short lag.
        let ahead = RATE * CHANNELS / 4;
        if q.len() > n + ahead {
            let excess = q.len() - n - RATE * CHANNELS / 50;
            q.drain(..excess);
        }
        let k = n.min(q.len());
        out.extend(q.drain(..k));
        out.resize(n, 0.0);
    }

    fn clear(&self) {
        self.samples.lock().clear();
    }
}

#[derive(Debug, Clone)]
pub enum Source {
    Process(u32),
    /// A capture device by id; `None` follows the Windows default.
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

fn open(source: &Source) -> anyhow::Result<AudioClient> {
    Ok(match source {
        Source::Process(pid) => AudioClient::new_application_loopback_client(*pid, true)?,
        Source::Mic(id) => {
            let devices = DeviceEnumerator::new()?;
            // A microphone that was unplugged falls back to the default one.
            let chosen = id
                .as_deref()
                .and_then(|id| devices.get_device(id).ok())
                .filter(|d| matches!(d.get_state(), Ok(DeviceState::Active)));
            match chosen {
                Some(device) => device,
                None => devices.get_default_device(&Direction::Capture)?,
            }
            .get_iaudioclient()?
        }
        // Capturing from a render device is loopback.
        Source::Desktop => DeviceEnumerator::new()?.get_default_device(&Direction::Render)?.get_iaudioclient()?,
    })
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

/// Runs a capture stream until `stop`, handing each batch of samples to `sink`.
fn stream(source: &Source, stop: &AtomicBool, mut sink: impl FnMut(&mut Vec<f32>)) -> anyhow::Result<()> {
    let _ = wasapi::initialize_mta();
    let format = WaveFormat::new(32, 32, &SampleType::Float, RATE, CHANNELS, None);
    let mut client = open(source)?;
    let mode = StreamMode::EventsShared { autoconvert: true, buffer_duration_hns: 0 };
    client.initialize_client(&format, &Direction::Capture, &mode)?;
    let event = client.set_get_eventhandle()?;
    let reader = client.get_audiocaptureclient()?;
    client.start_stream()?;
    let mut bytes: VecDeque<u8> = VecDeque::new();
    let mut floats: Vec<f32> = Vec::new();
    while !stop.load(Ordering::Relaxed) {
        // A silent process sends no events; the pipe writer fills the gap.
        if event.wait_for_event(100).is_err() {
            continue;
        }
        while let Ok(Some(frames)) = reader.get_next_packet_size() {
            if frames == 0 {
                break;
            }
            reader.read_from_device_to_deque(&mut bytes)?;
        }
        let whole = bytes.len() / 4 * 4;
        floats.clear();
        let (a, b) = bytes.as_slices();
        let mut tmp: Vec<u8> = Vec::with_capacity(whole);
        tmp.extend_from_slice(a);
        tmp.extend_from_slice(b);
        tmp.truncate(whole);
        floats.extend(tmp.as_chunks::<4>().0.iter().map(|c| f32::from_le_bytes(*c)));
        bytes.drain(..whole);
        sink(&mut floats);
    }
    let _ = client.stop_stream();
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InputDevice {
    pub id: String,
    pub name: String,
    pub default: bool,
}

/// Every active microphone, as Windows names it.
pub fn input_devices() -> Vec<InputDevice> {
    let _ = wasapi::initialize_mta();
    let Ok(enumerator) = DeviceEnumerator::new() else { return Vec::new() };
    let default = enumerator.get_default_device(&Direction::Capture).and_then(|d| d.get_id()).ok();
    let Ok(devices) = enumerator.get_device_collection(&Direction::Capture) else { return Vec::new() };
    (&devices)
        .into_iter()
        .flatten()
        .filter_map(|d| {
            let id = d.get_id().ok()?;
            let name = d.get_friendlyname().unwrap_or_else(|_| "Microphone".into());
            Some(InputDevice { default: default.as_deref() == Some(id.as_str()), id, name })
        })
        .collect()
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
            if pipe.connect().is_err() || stop.load(Ordering::Relaxed) {
                return;
            }
            ring.clear();
            let start = Instant::now();
            let mut sent: u64 = 0;
            let mut samples: Vec<f32> = Vec::new();
            let mut bytes: Vec<u8> = Vec::new();
            while !stop.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(10));
                let due = (start.elapsed().as_secs_f64() * RATE as f64) as u64;
                if due <= sent {
                    continue;
                }
                let frames = (due - sent) as usize;
                ring.take(frames * CHANNELS, &mut samples);
                bytes.clear();
                for s in &samples {
                    bytes.extend_from_slice(&s.to_le_bytes());
                }
                if pipe.write_all(&bytes).is_err() {
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Track {
    /// Index among the recording's audio streams.
    pub slot: usize,
    pub name: String,
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

    fn assign(&mut self, slot: usize, key: String, name: String, kind: TrackKind, source: Source) {
        self.slots[slot].release();
        let stop = Arc::new(AtomicBool::new(false));
        let handle = spawn_capture(source, self.slots[slot].ring.clone(), stop.clone());
        self.slots[slot].current = Some(Assignment { key, stop, handle: Some(handle), gone_since: None });
        self.slots[slot].history.push(Span { name, kind, start: now_ms(), end: None });
    }

    /// Slot 0: the game's process tree, or the whole desktop when recording a screen.
    pub fn start_main(&mut self, game: Option<(u32, String, String)>) {
        match game {
            Some((pid, exe, name)) => {
                let key = exe.to_lowercase();
                self.game_key = Some(key.clone());
                self.assign(0, key, name, TrackKind::Game, Source::Process(pid));
            }
            None => {
                self.game_key = None;
                self.assign(0, "desktop".into(), "Desktop".into(), TrackKind::Desktop, Source::Desktop);
            }
        }
        if let Some(mic) = self.mic {
            let source = Source::Mic(self.mic_device.clone());
            self.assign(mic, "mic".into(), "Microphone".into(), TrackKind::Mic, source);
        }
    }

    /// Looks for apps that started playing sound and frees slots of apps that are gone.
    pub fn poll(&mut self) {
        // In screen mode slot 0 already holds every sound, so separate app tracks would
        // only duplicate it. They still help there: muting Discord needs its own track.
        let processes = win::processes();
        let mut seen: HashSet<String> = HashSet::new();
        let own = std::process::id();
        let children: HashSet<u32> = processes.iter().filter(|p| p.parent == own).map(|p| p.pid).collect();
        for (pid, active) in audio_sessions() {
            if pid == 0 || pid == own || children.contains(&pid) {
                continue;
            }
            let root = win::root_of(pid, &processes);
            // Replix's own previews play through its WebView.
            if root == own {
                continue;
            }
            let Some(entry) = processes.iter().find(|p| p.pid == root) else { continue };
            let key = entry.exe.to_lowercase();
            if Some(&key) == self.game_key.as_ref() || key == "audiodg.exe" {
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
            let name = win::process_path(root).map(|p| win::app_name(&p)).unwrap_or_else(|| entry.exe.clone());
            log::info!("audio track {slot}: {name} ({key}, pid {root})");
            self.assign(slot, key, name, TrackKind::App, Source::Process(root));
        }
        for i in 1..=APP_SLOTS {
            let keep_for = self.keep_for;
            let hold = self.hold;
            let slot = &mut self.slots[i];
            let Some(a) = slot.current.as_mut() else { continue };
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
                out.push(Track { slot: i, name: span.name.clone(), kind: span.kind });
            }
        }
        out
    }

    pub fn stop(&mut self) {
        for slot in &mut self.slots {
            slot.release();
        }
        self.writer_stop.store(true, Ordering::Relaxed);
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

/// (pid, playing) for every audio session on every output device.
fn audio_sessions() -> Vec<(u32, bool)> {
    let mut out = Vec::new();
    let Ok(enumerator) = DeviceEnumerator::new() else { return out };
    let Ok(devices) = enumerator.get_device_collection(&Direction::Render) else { return out };
    let count = devices.get_nbr_devices().unwrap_or(0);
    for i in 0..count {
        let Ok(device) = devices.get_device_at_index(i) else { continue };
        let Ok(manager) = device.get_iaudiosessionmanager() else { continue };
        let Ok(sessions) = manager.get_audiosessionenumerator() else { continue };
        for j in 0..sessions.get_count().unwrap_or(0) {
            let Ok(session) = sessions.get_session(j) else { continue };
            let state = session.get_state().ok();
            if matches!(state, Some(SessionState::Expired) | None) {
                continue;
            }
            if let Ok(pid) = session.get_process_id() {
                out.push((pid, matches!(state, Some(SessionState::Active))));
            }
        }
    }
    out
}
