//! Windows: WASAPI. Process loopback captures one app's process tree on its own.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};

use wasapi::{AudioClient, DeviceEnumerator, DeviceState, Direction, SampleType, SessionState, StreamMode, WaveFormat};

use super::{InputDevice, Source, CHANNELS, RATE};

/// Sets the calling thread up for audio (COM in the multithreaded apartment).
pub fn init_thread() {
    let _ = wasapi::initialize_mta();
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

/// Runs a capture stream until `stop`, handing each batch of samples to `sink`.
pub fn stream(source: &Source, stop: &AtomicBool, mut sink: impl FnMut(&mut Vec<f32>)) -> anyhow::Result<()> {
    init_thread();
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
        // Usually contiguous already, so this neither moves nor allocates.
        floats.extend(bytes.make_contiguous()[..whole].as_chunks::<4>().0.iter().map(|c| f32::from_le_bytes(*c)));
        bytes.drain(..whole);
        sink(&mut floats);
    }
    let _ = client.stop_stream();
    Ok(())
}

/// Every active microphone, as Windows names it.
pub fn input_devices() -> Vec<InputDevice> {
    init_thread();
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

/// (pid, playing) for every audio session on every output device.
pub fn audio_sessions() -> Vec<(u32, bool)> {
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
