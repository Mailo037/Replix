//! Finding and running FFmpeg.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use serde::Deserialize;


/// FFmpeg where setup found it (downloaded on Windows, the system's elsewhere); until then
/// (and in tests) whatever is on the PATH.
fn tool(name: &str) -> PathBuf {
    match crate::install::ready_dir() {
        Some(dir) => dir.join(format!("{name}{}", std::env::consts::EXE_SUFFIX)),
        None => PathBuf::from(name),
    }
}

/// `spawn` for FFmpeg and ffprobe: the process is tied to Replix and ends with it.
pub trait SpawnTied {
    fn spawn_tied(&mut self) -> std::io::Result<std::process::Child>;
}

impl SpawnTied for Command {
    fn spawn_tied(&mut self) -> std::io::Result<std::process::Child> {
        crate::platform::prepare_tied(self);
        let child = self.spawn()?;
        crate::platform::tie_to_app(&child);
        Ok(child)
    }
}

pub fn exe() -> PathBuf {
    tool("ffmpeg")
}

/// FFmpeg for a helper job (saving, thumbnails, probing): these yield the CPU to the game.
/// The capture itself is started elsewhere and keeps normal priority so it never drops frames.
pub fn command() -> Command {
    let mut cmd = Command::new(exe());
    crate::platform::background(&mut cmd, true);
    cmd.arg("-hide_banner").args(["-loglevel", "error", "-nostdin"]);
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

/// Like `run`, stopped as soon as `keep_going` says no, e.g. work done ahead of time that
/// nobody needs once the window is hidden.
pub fn run_while(args: &[String], keep_going: impl Fn() -> bool) -> Result<()> {
    run_checked(command(), args, Duration::from_secs(600), keep_going)
}

fn run_command(cmd: Command, args: &[String], timeout: Duration) -> Result<()> {
    run_checked(cmd, args, timeout, || true)
}

fn run_checked(mut cmd: Command, args: &[String], timeout: Duration, keep_going: impl Fn() -> bool) -> Result<()> {
    let mut child = cmd
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn_tied()
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
        if !keep_going() {
            let _ = child.kill();
            let _ = child.wait();
            bail!("stopped");
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
    /// Channels of each audio stream, in the same order.
    pub audio_channels: Vec<u32>,
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
    channels: Option<u32>,
    #[serde(default)]
    tags: std::collections::HashMap<String, String>,
}

#[derive(Deserialize)]
struct ProbeFormat {
    duration: Option<String>,
}

/// Reading a file's header takes well under a second; a probe still running after this is
/// stuck, e.g. on a file that is still being written or a drive that stopped answering.
const PROBE_LIMIT: Duration = Duration::from_secs(20);

pub fn probe(path: &Path) -> Result<MediaInfo> {
    probe_while(path, &|| true)
}

/// Like `probe`, stopped as soon as `keep_going` says no.
pub fn probe_while(path: &Path, keep_going: &(dyn Fn() -> bool + Sync)) -> Result<MediaInfo> {
    let mut cmd = Command::new(tool("ffprobe"));
    crate::platform::background(&mut cmd, true);
    let mut child = cmd
        .args(["-v", "error", "-show_entries", "format=duration:stream=codec_type,width,height,channels:stream_tags=title,name", "-of", "json"])
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn_tied()
        .context("ffprobe could not be started")?;
    let (mut stdout, mut stderr) = (child.stdout.take().unwrap(), child.stderr.take().unwrap());
    let read_out = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = stdout.read_to_end(&mut bytes);
        bytes
    });
    let read_err = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = stderr.read_to_end(&mut bytes);
        bytes
    });
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if started.elapsed() > PROBE_LIMIT {
            let _ = child.kill();
            let _ = child.wait();
            bail!("ffprobe took too long");
        }
        if !keep_going() {
            let _ = child.kill();
            let _ = child.wait();
            bail!("stopped");
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let out = std::process::Output {
        status,
        stdout: read_out.join().unwrap_or_default(),
        stderr: read_err.join().unwrap_or_default(),
    };
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
            Some("audio") => {
                info.audio_titles.push(stream.tags.get("title").or(stream.tags.get("name")).cloned());
                info.audio_channels.push(stream.channels.unwrap_or(2));
            }
            _ => {}
        }
    }
    Ok(info)
}

pub fn path_arg(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// For tests that need FFmpeg on the PATH: whether it is missing, in which case they skip.
/// Where `REPLIX_TESTS_NEED_FFMPEG` is set (CI installs it on purpose), a missing FFmpeg
/// fails the test instead of passing it unseen.
#[cfg(test)]
pub fn missing_for_test() -> bool {
    let found = std::process::Command::new("ffmpeg").arg("-version").stdout(Stdio::null()).status().is_ok_and(|s| s.success());
    if !found && std::env::var_os("REPLIX_TESTS_NEED_FFMPEG").is_some() {
        panic!("FFmpeg is needed on the PATH for this test");
    }
    if !found {
        eprintln!("skipped: no ffmpeg on PATH");
    }
    !found
}
