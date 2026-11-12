//! A short sound that confirms a key press while a game is in front, where the corner note
//! may be hidden. Made here rather than shipped as a file: two soft sine notes.

use std::path::Path;
use std::sync::OnceLock;

use tauri::{AppHandle, Manager};

use crate::{platform, AppState};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Chime {
    /// A clip, screenshot or marker was taken, or a session started or stopped: two notes up.
    Done,
    /// The key did nothing (Replix is not recording): one low note.
    Refused,
}

const RATE: u32 = 44_100;

/// Plays the chime if the settings want sounds. Never waits for it.
pub fn play(app: &AppHandle, chime: Chime) {
    if !app.state::<AppState>().settings.lock().save_sound {
        return;
    }
    let (wav, name) = match chime {
        Chime::Done => (done(), "replix-done.wav"),
        Chime::Refused => (refused(), "replix-refused.wav"),
    };
    // Systems that play from a file get one in the cache, written once.
    let file = app.path().app_cache_dir().unwrap_or_else(|_| std::env::temp_dir()).join(name);
    platform::play_sound(wav, &file);
}

fn done() -> &'static [u8] {
    static WAV: OnceLock<Vec<u8>> = OnceLock::new();
    WAV.get_or_init(|| wav(&[(784.0, 0.0, 0.09), (1175.0, 0.07, 0.16)]))
}

fn refused() -> &'static [u8] {
    static WAV: OnceLock<Vec<u8>> = OnceLock::new();
    WAV.get_or_init(|| wav(&[(330.0, 0.0, 0.18)]))
}

/// A 16-bit mono WAV of sine notes (frequency, start and length in seconds), each fading in
/// and out quickly so nothing clicks, quiet enough to sit under a game's sound.
fn wav(notes: &[(f32, f32, f32)]) -> Vec<u8> {
    let end = notes.iter().map(|n| n.1 + n.2).fold(0.0, f32::max) + 0.02;
    let samples = (end * RATE as f32) as usize;
    let mut mix = vec![0.0f32; samples];
    for &(freq, start, length) in notes {
        let first = (start * RATE as f32) as usize;
        let count = (length * RATE as f32) as usize;
        for i in 0..count.min(samples.saturating_sub(first)) {
            let t = i as f32 / RATE as f32;
            // Quick rise, gentle fall.
            let envelope = (t / 0.008).min(1.0) * (1.0 - t / length).max(0.0).powf(1.6);
            mix[first + i] += (t * freq * std::f32::consts::TAU).sin() * envelope * 0.22;
        }
    }
    let data_len = (samples * 2) as u32;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&RATE.to_le_bytes());
    out.extend_from_slice(&(RATE * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for s in mix {
        out.extend_from_slice(&((s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16).to_le_bytes());
    }
    out
}

/// Writes the sound to `file` unless it is there already with the same size.
pub(crate) fn ensure_file(wav: &[u8], file: &Path) -> bool {
    if std::fs::metadata(file).is_ok_and(|m| m.len() == wav.len() as u64) {
        return true;
    }
    if let Some(dir) = file.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    std::fs::write(file, wav).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chimes_are_short_valid_wavs() {
        for wav in [done(), refused()] {
            assert_eq!(&wav[0..4], b"RIFF");
            assert_eq!(&wav[8..16], b"WAVEfmt ");
            let data = u32::from_le_bytes(wav[40..44].try_into().unwrap()) as usize;
            assert_eq!(wav.len(), 44 + data);
            // Under a quarter of a second.
            assert!(data / 2 < RATE as usize / 4);
            // Never louder than a quarter of full scale.
            let loudest = wav[44..].chunks(2).map(|c| i16::from_le_bytes([c[0], c[1]]).unsigned_abs()).max().unwrap();
            assert!(loudest > 1000 && loudest < i16::MAX as u16 / 3);
        }
    }
}
