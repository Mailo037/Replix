//! macOS: microphones through AVFoundation (an FFmpeg helper writes the samples).
//!
//! macOS gives no app another app's sound without a loopback device. When one is installed
//! (BlackHole, Loopback, Soundflower), the game's track records it, so the whole system
//! sound is kept with the clip; separate tracks per app are not available.

use std::process::Command;
use std::sync::atomic::AtomicBool;

use anyhow::bail;

use super::process::stream_from;
use super::{InputDevice, Source, CHANNELS, RATE};
use crate::ffmpeg;

/// Nothing to set up per thread.
pub fn init_thread() {}

/// No app's sound can be told apart from the others.
pub fn audio_sessions() -> Vec<(u32, bool)> {
    Vec::new()
}

/// Every microphone, by the name AVFoundation gives it.
pub fn input_devices() -> Vec<InputDevice> {
    let out = Command::new(ffmpeg::exe())
        .args(["-hide_banner", "-nostdin", "-f", "avfoundation", "-list_devices", "true", "-i", ""])
        .output();
    let Ok(out) = out else { return Vec::new() };
    parse_devices(&String::from_utf8_lossy(&out.stderr))
}

fn parse_devices(text: &str) -> Vec<InputDevice> {
    let mut audio = false;
    let mut out = Vec::new();
    for line in text.lines() {
        if line.contains("AVFoundation audio devices") {
            audio = true;
            continue;
        }
        if line.contains("AVFoundation video devices") {
            audio = false;
            continue;
        }
        if !audio {
            continue;
        }
        // "[AVFoundation indev @ 0x…] [0] MacBook Pro Microphone"
        let Some(rest) = line.split("] [").nth(1) else { continue };
        let Some((_, name)) = rest.split_once("] ") else { continue };
        let name = name.trim().to_string();
        out.push(InputDevice { id: name.clone(), name, default: false });
    }
    out
}

fn is_loopback(name: &str) -> bool {
    let name = name.to_lowercase();
    ["blackhole", "loopback", "soundflower"].iter().any(|n| name.contains(n))
}

fn record(device: &str) -> Command {
    let mut cmd = Command::new(ffmpeg::exe());
    cmd.args(["-hide_banner", "-loglevel", "error", "-nostdin", "-f", "avfoundation", "-i"]);
    cmd.arg(format!(":{device}"));
    cmd.args(["-f", "f32le", "-ar", &RATE.to_string(), "-ac", &CHANNELS.to_string(), "pipe:1"]);
    cmd
}

/// Runs a capture stream until `stop`, handing each batch of samples to `sink`.
pub fn stream(source: &Source, stop: &AtomicBool, mut sink: impl FnMut(&mut Vec<f32>)) -> anyhow::Result<()> {
    let devices = input_devices();
    let device = match source {
        // A microphone that was unplugged falls back to the default one.
        Source::Mic(id) => id.clone().filter(|id| devices.iter().any(|d| d.id == *id)).unwrap_or_else(|| "default".into()),
        Source::Desktop | Source::Process(_) => match devices.iter().find(|d| is_loopback(&d.name)) {
            Some(d) => d.id.clone(),
            None => bail!("no loopback device for the system sound"),
        },
    };
    stream_from(record(&device), stop, &mut sink)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn microphones_are_read_from_the_device_list() {
        let text = "[AVFoundation indev @ 0x1] AVFoundation video devices:\n\
                    [AVFoundation indev @ 0x1] [0] FaceTime HD Camera\n\
                    [AVFoundation indev @ 0x1] [1] Capture screen 0\n\
                    [AVFoundation indev @ 0x1] AVFoundation audio devices:\n\
                    [AVFoundation indev @ 0x1] [0] MacBook Pro Microphone\n\
                    [AVFoundation indev @ 0x1] [1] BlackHole 2ch\n";
        let found = parse_devices(text);
        assert_eq!(found.iter().map(|d| d.name.as_str()).collect::<Vec<_>>(), ["MacBook Pro Microphone", "BlackHole 2ch"]);
        assert!(is_loopback(&found[1].name));
    }
}
