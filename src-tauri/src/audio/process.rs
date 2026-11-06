//! Linux and macOS: sound comes from a helper program (parec, FFmpeg) that writes raw
//! 32-bit float stereo at 48 kHz to its output.

use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use anyhow::Context;

use crate::ffmpeg::SpawnTied;

/// How a helper's stream ended.
pub enum Ended {
    /// `stop` was set.
    Stopped,
    /// The helper finished by itself: its source went away.
    Gone,
}

/// Runs `cmd` until `stop` or until it ends, handing each batch of samples to `sink`.
pub fn stream_from(mut cmd: Command, stop: &AtomicBool, sink: &mut impl FnMut(&mut Vec<f32>)) -> anyhow::Result<Ended> {
    crate::platform::background(&mut cmd, false);
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn_tied()
        .context("the sound helper could not be started")?;
    let mut out = child.stdout.take().context("no output")?;
    let (tx, rx) = crossbeam_channel::bounded::<Vec<u8>>(64);
    // Reading blocks while the source is quiet; the loop below must still see `stop`.
    let reader = std::thread::Builder::new().name("audio-read".into()).spawn(move || {
        let mut buf = vec![0u8; 48_000 * 2 * 4 / 50];
        loop {
            match out.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if tx.send(buf[..n].to_vec()).is_err() {
                        break;
                    }
                }
            }
        }
    })?;
    let mut pending: Vec<u8> = Vec::new();
    let mut floats: Vec<f32> = Vec::new();
    let ended = loop {
        if stop.load(Ordering::Relaxed) {
            break Ended::Stopped;
        }
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(bytes) => {
                pending.extend_from_slice(&bytes);
                let whole = pending.len() / 4 * 4;
                floats.clear();
                floats.extend(pending[..whole].as_chunks::<4>().0.iter().map(|c| f32::from_le_bytes(*c)));
                pending.drain(..whole);
                sink(&mut floats);
            }
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break Ended::Gone,
        }
    };
    let _ = child.kill();
    let _ = child.wait();
    let _ = reader.join();
    Ok(ended)
}

/// Waits about `ms` milliseconds, or less once `stop` is set. Says whether to go on.
#[cfg(target_os = "linux")]
pub fn pause(stop: &AtomicBool, ms: u64) -> bool {
    for _ in 0..ms / 50 {
        if stop.load(Ordering::Relaxed) {
            return false;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    !stop.load(Ordering::Relaxed)
}
