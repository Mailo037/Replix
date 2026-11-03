//! Finding and running the bundled FFmpeg.

use std::io::Read;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use serde::Deserialize;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
/// Helper jobs (saving, thumbnails, probing) yield the CPU to the game. The capture
/// itself is started elsewhere and keeps normal priority so it never drops frames.
const BELOW_NORMAL_PRIORITY_CLASS: u32 = 0x0000_4000;

/// The sidecar sits next to the app executable; during development fall back to PATH.
fn tool(name: &str) -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let candidate = dir.join(format!("{name}.exe"));
            if candidate.exists() {
                return candidate;
            }
        }
    }
    PathBuf::from(name)
}

pub fn exe() -> PathBuf {
    tool("ffmpeg")
}

pub fn command() -> Command {
    let mut cmd = Command::new(exe());
    cmd.creation_flags(CREATE_NO_WINDOW | BELOW_NORMAL_PRIORITY_CLASS).arg("-hide_banner").args(["-loglevel", "error", "-nostdin"]);
    cmd
}

/// Runs FFmpeg to completion and turns a failure into its last error lines.
pub fn run(args: &[String]) -> Result<()> {
    run_with_timeout(args, Duration::from_secs(600))
}

/// Like `run`, with relative paths resolved against `dir`.
pub fn run_in(dir: &Path, args: &[String]) -> Result<()> {
    let mut cmd = command();
    cmd.current_dir(dir);
    run_command(cmd, args, Duration::from_secs(600))
}

pub fn run_with_timeout(args: &[String], timeout: Duration) -> Result<()> {
    run_command(command(), args, timeout)
}

fn run_command(mut cmd: Command, args: &[String], timeout: Duration) -> Result<()> {
    let mut child = cmd
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .context("FFmpeg could not be started")?;
    let mut stderr = child.stderr.take().unwrap();
    let reader = std::thread::spawn(move || {
        let mut text = String::new();
        let _ = stderr.read_to_string(&mut text);
        text
    });
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if started.elapsed() > timeout {
            let _ = child.kill();
            let _ = child.wait();
            bail!("FFmpeg took too long");
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let text = reader.join().unwrap_or_default();
    if status.success() {
        Ok(())
    } else {
        Err(anyhow!(last_lines(&text, 4)))
    }
}

pub fn last_lines(text: &str, n: usize) -> String {
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    lines[lines.len().saturating_sub(n)..].join("\n")
}

#[derive(Debug, Clone, Default)]
pub struct MediaInfo {
    pub duration: f64,
    pub width: u32,
    pub height: u32,
    pub audio_titles: Vec<Option<String>>,
}

#[derive(Deserialize)]
struct ProbeOut {
    #[serde(default)]
    streams: Vec<ProbeStream>,
    format: Option<ProbeFormat>,
}

#[derive(Deserialize)]
struct ProbeStream {
    codec_type: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
    #[serde(default)]
    tags: std::collections::HashMap<String, String>,
}

#[derive(Deserialize)]
struct ProbeFormat {
    duration: Option<String>,
}

pub fn probe(path: &Path) -> Result<MediaInfo> {
    let out = Command::new(tool("ffprobe"))
        .creation_flags(CREATE_NO_WINDOW | BELOW_NORMAL_PRIORITY_CLASS)
        .args(["-v", "error", "-show_entries", "format=duration:stream=codec_type,width,height:stream_tags=title,name", "-of", "json"])
        .arg(path)
        .stdin(Stdio::null())
        .output()
        .context("ffprobe could not be started")?;
    if !out.status.success() {
        bail!("{}", last_lines(&String::from_utf8_lossy(&out.stderr), 3));
    }
    let parsed: ProbeOut = serde_json::from_slice(&out.stdout)?;
    let mut info = MediaInfo {
        duration: parsed.format.and_then(|f| f.duration).and_then(|d| d.parse().ok()).unwrap_or(0.0),
        ..Default::default()
    };
    for stream in parsed.streams {
        match stream.codec_type.as_deref() {
            Some("video") if info.width == 0 => {
                info.width = stream.width.unwrap_or(0);
                info.height = stream.height.unwrap_or(0);
            }
            // MP4 keeps a track's title in its "name" tag.
            Some("audio") => info.audio_titles.push(stream.tags.get("title").or(stream.tags.get("name")).cloned()),
            _ => {}
        }
    }
    Ok(info)
}

pub fn path_arg(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}
