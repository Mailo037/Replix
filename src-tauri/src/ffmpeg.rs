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

/// Runs FFmpeg to completion, retaining stderr so callers can identify the actual failure.
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
        Err(anyhow!(text.trim().to_owned()))
    }
}

/// "6.1.1" from "ffmpeg version 6.1.1-3ubuntu5 Copyright …", or "7.1" from "ffmpeg version n7.1".
/// Builds from Git name a date or commit instead ("N-118000-g…" gives "N").
pub fn parse_version(text: &str) -> Option<String> {
    let word = text.lines().next()?.strip_prefix("ffmpeg version ")?.split_whitespace().next()?;
    let word = word.strip_prefix('n').filter(|w| w.starts_with(|c: char| c.is_ascii_digit())).unwrap_or(word);
    Some(word.split(['-', '+', '~']).next().unwrap_or(word).to_string())
}

/// The major version of the FFmpeg Replix runs. A build from Git counts as the newest.
fn major_version() -> u32 {
    static MAJOR: std::sync::OnceLock<u32> = std::sync::OnceLock::new();
    *MAJOR.get_or_init(|| {
        let mut cmd = Command::new(exe());
        crate::platform::background(&mut cmd, false);
        let text = cmd.arg("-version").stdin(Stdio::null()).output().map(|o| String::from_utf8_lossy(&o.stdout).into_owned());
        let number = text.ok().as_deref().and_then(parse_version).unwrap_or_default();
        match number.split('.').next().and_then(|m| m.parse().ok()) {
            Some(major) => major,
            None if !number.is_empty() => u32::MAX,
            None => 0,
        }
    })
}

/// The option that reads a filter graph from a file. FFmpeg 7 replaced `-filter_complex_script`
/// with `-/filter_complex`; Linux and macOS may still have FFmpeg 6.
pub fn graph_file_option() -> &'static str {
    if major_version() >= 7 {
        "-/filter_complex"
    } else {
        "-filter_complex_script"
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
    /// Frames a second of the picture; 0 when unknown.
    pub fps: f64,
}

#[derive(Deserialize)]
struct ProbeOut {
    #[serde(default)]
    streams: Vec<ProbeStream>,
    format: Option<ProbeFormat>,
    error: Option<serde_json::Value>,
}

#[derive(Deserialize)]
struct ProbeStream {
    codec_type: Option<String>,
    codec_name: Option<String>,
    nb_read_packets: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
    channels: Option<u32>,
    avg_frame_rate: Option<String>,
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
    cmd.args(["-v", "error", "-show_entries", "format=duration:stream=codec_type,width,height,channels,avg_frame_rate:stream_tags=title,name,handler_name", "-of", "json"])
        .arg(path);
    let out = probe_output(cmd, PROBE_LIMIT, keep_going)?;
    let parsed: ProbeOut = serde_json::from_slice(&out)?;
    let mut info = MediaInfo {
        duration: parsed
            .format
            .and_then(|f| f.duration)
            .and_then(|d| d.parse().ok())
            .unwrap_or(0.0),
        ..Default::default()
    };
    for stream in parsed.streams {
        match stream.codec_type.as_deref() {
            Some("video") if info.width == 0 => {
                info.width = stream.width.unwrap_or(0);
                info.height = stream.height.unwrap_or(0);
                info.fps = stream.avg_frame_rate.as_deref().and_then(frame_rate).unwrap_or(0.0);
            }
            Some("audio") => {
                info.audio_titles.push(track_title(&stream.tags));
                info.audio_channels.push(stream.channels.unwrap_or(2));
            }
            _ => {}
        }
    }
    Ok(info)
}

/// "60000/1001" as 59.94; "0/0" (unknown) as None.
fn frame_rate(text: &str) -> Option<f64> {
    let (num, den) = text.split_once('/').unwrap_or((text, "1"));
    let (num, den): (f64, f64) = (num.parse().ok()?, den.parse().ok()?);
    (num > 0.0 && den > 0.0).then(|| num / den)
}

/// Counts packets through the entire MPEG-TS selection, not just the initial probe window.
/// An AAC entry in the PMT without any packets cannot supply MP4's audio parameters.
/// Streams that do carry packets are kept even if probing their header found no parameters.
/// A timeout or read error is not evidence that a track is empty.
pub fn empty_aac_slots(dir: &Path, list: &Path) -> Result<Vec<usize>> {
    let mut cmd = Command::new(tool("ffprobe"));
    cmd.current_dir(dir)
        .args([
            "-v",
            "error",
            "-f",
            "mpegts",
            "-probesize",
            "32",
            "-analyzeduration",
            "0",
            "-count_packets",
            "-show_error",
            "-show_entries",
            "stream=codec_type,codec_name,nb_read_packets:error=code,string",
            "-of",
            "json",
        ])
        .arg(format!("concatf:{}", path_arg(list)));
    empty_aac(cmd)
}

/// The same for one file that holds the whole recording, such as a session's own file.
pub fn empty_aac_streams(file: &Path) -> Result<Vec<usize>> {
    let mut cmd = Command::new(tool("ffprobe"));
    cmd.args([
        "-v",
        "error",
        "-count_packets",
        "-show_error",
        "-show_entries",
        "stream=codec_type,codec_name,nb_read_packets:error=code,string",
        "-of",
        "json",
    ])
    .arg(file);
    empty_aac(cmd)
}

fn empty_aac(cmd: Command) -> Result<Vec<usize>> {
    // Unlike a header probe, this reads the whole clip and may take as long as a remux.
    let out = probe_output(cmd, Duration::from_secs(600), &|| true)?;
    let parsed: ProbeOut = serde_json::from_slice(&out)?;
    if let Some(error) = parsed.error {
        bail!("could not scan all recording segments: {error}");
    }
    if parsed.streams.is_empty() {
        bail!("ffprobe found no streams in the recording segments");
    }
    let mut empty = Vec::new();
    for (slot, stream) in parsed
        .streams
        .into_iter()
        .filter(|s| s.codec_type.as_deref() == Some("audio"))
        .enumerate()
    {
        let packets = stream
            .nb_read_packets
            .as_deref()
            .map(str::parse::<u64>)
            .transpose()?;
        if stream.codec_name.as_deref() == Some("aac") && packets.unwrap_or(0) == 0 {
            empty.push(slot);
        }
    }
    Ok(empty)
}

fn probe_output(
    mut cmd: Command,
    timeout: Duration,
    keep_going: &(dyn Fn() -> bool + Sync),
) -> Result<Vec<u8>> {
    crate::platform::background(&mut cmd, true);
    let mut child = cmd
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
        if started.elapsed() > timeout {
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
    Ok(out.stdout)
}

/// Options naming audio track `k` of the output. MP4 keeps the title in its "name" box,
/// which FFmpeg before 7 does not read back; the handler name it does, on every version.
pub fn name_track(k: usize, name: &str) -> [String; 4] {
    let stream = format!("-metadata:s:a:{k}");
    [stream.clone(), format!("title={name}"), stream, format!("handler_name={name}")]
}

/// A track's name from its tags: the title, else the handler name when a person gave it.
fn track_title(tags: &std::collections::HashMap<String, String>) -> Option<String> {
    // What muxers and phones write when nobody named the track.
    const DEFAULTS: &[&str] = &["SoundHandler", "VideoHandler", "Core Media Audio", "Core Media Video", ""];
    tags.get("title")
        .or(tags.get("name"))
        .or(tags.get("handler_name").filter(|h| !DEFAULTS.contains(&h.trim()) && !h.starts_with("ISO Media")))
        .cloned()
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
