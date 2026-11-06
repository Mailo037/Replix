//! Linux: PulseAudio, or PipeWire through its PulseAudio server (the default on current
//! distributions). `pactl` lists who plays sound, `parec` records a microphone, the whole
//! output, or a single app's stream, so every app still gets its own track.

use std::process::Command;
use std::sync::atomic::AtomicBool;

use serde::Deserialize;

use super::process::{pause, stream_from, Ended};
use super::{InputDevice, Source, CHANNELS, RATE};
use crate::platform;

/// Nothing to set up per thread.
pub fn init_thread() {}

fn pactl(args: &[&str]) -> Option<Vec<u8>> {
    let mut cmd = Command::new("pactl");
    cmd.args(args);
    platform::background(&mut cmd, false);
    let out = cmd.output().ok()?;
    out.status.success().then_some(out.stdout)
}

#[derive(Deserialize)]
struct SinkInput {
    index: u32,
    #[serde(default)]
    corked: bool,
    #[serde(default)]
    properties: std::collections::HashMap<String, serde_json::Value>,
}

impl SinkInput {
    fn pid(&self) -> Option<u32> {
        match self.properties.get("application.process.id")? {
            serde_json::Value::String(s) => s.parse().ok(),
            serde_json::Value::Number(n) => n.as_u64().and_then(|n| u32::try_from(n).ok()),
            _ => None,
        }
    }
}

/// Every stream playing into an output, with its process. Needs pactl 16 or newer for JSON.
fn sink_inputs() -> Vec<SinkInput> {
    pactl(&["-f", "json", "list", "sink-inputs"]).and_then(|out| serde_json::from_slice(&out).ok()).unwrap_or_default()
}

/// (pid, playing) for every app that has a stream open.
pub fn audio_sessions() -> Vec<(u32, bool)> {
    sink_inputs().into_iter().filter_map(|s| Some((s.pid()?, !s.corked))).collect()
}

#[derive(Deserialize)]
struct PaSource {
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    properties: std::collections::HashMap<String, serde_json::Value>,
}

/// Every microphone, as the sound server names it. Monitors of outputs are left out.
pub fn input_devices() -> Vec<InputDevice> {
    let sources: Vec<PaSource> =
        pactl(&["-f", "json", "list", "sources"]).and_then(|out| serde_json::from_slice(&out).ok()).unwrap_or_default();
    let default = pactl(&["get-default-source"]).map(|o| String::from_utf8_lossy(&o).trim().to_string());
    sources
        .into_iter()
        .filter(|s| {
            let class = s.properties.get("device.class").and_then(|v| v.as_str());
            !s.name.ends_with(".monitor") && class != Some("monitor")
        })
        .map(|s| InputDevice {
            default: default.as_deref() == Some(s.name.as_str()),
            name: if s.description.is_empty() { s.name.clone() } else { s.description },
            id: s.name,
        })
        .collect()
}

fn parec() -> Command {
    let mut cmd = Command::new("parec");
    cmd.args(["--raw", "--format=float32le", "--latency-msec=20", "--client-name=Replix"]);
    cmd.arg(format!("--rate={RATE}")).arg(format!("--channels={CHANNELS}"));
    cmd
}

/// The stream the app (a process tree under `root`) plays, preferring one that is playing.
fn stream_of(root: u32) -> Option<u32> {
    let processes = platform::processes();
    let mut found: Vec<SinkInput> =
        sink_inputs().into_iter().filter(|s| s.pid().is_some_and(|pid| platform::root_of(pid, &processes) == root)).collect();
    found.sort_by_key(|s| s.corked);
    found.first().map(|s| s.index)
}

/// Runs a capture stream until `stop`, handing each batch of samples to `sink`.
pub fn stream(source: &Source, stop: &AtomicBool, mut sink: impl FnMut(&mut Vec<f32>)) -> anyhow::Result<()> {
    match source {
        Source::Mic(id) => {
            let mut cmd = parec();
            // A microphone that was unplugged falls back to the default one.
            if let Some(id) = id.as_deref().filter(|id| input_devices().iter().any(|d| d.id == *id)) {
                cmd.arg(format!("--device={id}"));
            }
            stream_from(cmd, stop, &mut sink)?;
        }
        Source::Desktop => {
            let mut cmd = parec();
            cmd.arg("--device=@DEFAULT_MONITOR@");
            stream_from(cmd, stop, &mut sink)?;
        }
        // An app opens and closes streams as it plays: each one is followed while it lasts.
        Source::Process(root) => loop {
            let Some(index) = stream_of(*root) else {
                if !pause(stop, 500) {
                    return Ok(());
                }
                continue;
            };
            let mut cmd = parec();
            cmd.arg(format!("--monitor-stream={index}"));
            if let Ended::Stopped = stream_from(cmd, stop, &mut sink)? {
                return Ok(());
            }
            if !pause(stop, 200) {
                return Ok(());
            }
        },
    }
    Ok(())
}
