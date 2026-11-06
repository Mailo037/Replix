//! Getting FFmpeg onto the PC.
//!
//! Windows: the installer does not carry it. Replix downloads one pinned build on first
//! start, checks it against the hashes below and keeps it in a folder of its own version. A
//! new Replix that pins a newer FFmpeg fetches it into a new folder and drops the old one,
//! so a file that is in use is never overwritten.
//!
//! Linux and macOS: FFmpeg comes from the system's package manager (apt, dnf, pacman,
//! Homebrew), which keeps it up to date and built for the machine. Replix finds it and
//! checks it is new enough and can grab the screen.

use std::io::Read;
#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
#[cfg(windows)]
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
#[cfg(windows)]
use std::time::Duration;

use anyhow::{bail, Result};
#[cfg(windows)]
use anyhow::Context;
use parking_lot::{Condvar, Mutex};
use serde::Serialize;
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter};
#[cfg(windows)]
use tauri::Manager;

#[cfg(windows)]
use crate::ffmpeg::SpawnTied;

#[cfg(windows)]
pub const VERSION: &str = "9.0.2";
#[cfg(windows)]
const URL: &str = "https://github.com/GyanD/codexffmpeg/releases/download/9.0.2/ffmpeg-9.0.2-essentials_build.zip";
#[cfg(windows)]
const ARCHIVE_SIZE: u64 = 114_768_076;
#[cfg(windows)]
const ARCHIVE_SHA256: &str = "60f467265b1e312373dbcd92200c2618a74850f98d3d078e94296bb3fa2047ba";
/// Inside the archive, under this folder's `bin`.
#[cfg(windows)]
const ARCHIVE_ROOT: &str = "ffmpeg-9.0.2-essentials_build";
#[cfg(windows)]
const TOOLS: [(&str, &str); 2] = [
    ("ffmpeg.exe", "3256173f3f8bffd7df12227c68adf68025edb1832273a9530688a7bb1ed8edec"),
    ("ffprobe.exe", "f0d36ecbbdd3bcfac3efa078c96c7271c2e68b3810595552ac3b7f17e9a65c52"),
];
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// The oldest FFmpeg found on the system that Replix works with.
#[cfg(unix)]
const MIN_MAJOR: u32 = 6;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "state", rename_all = "camelCase")]
// Only Windows downloads; the other systems go from checking straight to ready or failed.
#[cfg_attr(unix, allow(dead_code))]
pub enum Setup {
    Checking,
    Downloading { done: u64, total: u64 },
    Unpacking,
    Ready,
    Failed { error: String },
}

struct Shared {
    state: Mutex<Setup>,
    changed: Condvar,
}

fn shared() -> &'static Shared {
    static SHARED: OnceLock<Shared> = OnceLock::new();
    SHARED.get_or_init(|| Shared { state: Mutex::new(Setup::Checking), changed: Condvar::new() })
}

/// The folder FFmpeg lives in once it is ready.
static READY: OnceLock<PathBuf> = OnceLock::new();

pub fn ready_dir() -> Option<&'static Path> {
    READY.get().map(PathBuf::as_path)
}

pub fn state() -> Setup {
    shared().state.lock().clone()
}

/// Blocks until FFmpeg is ready; a failed download waits for a retry.
pub fn wait_ready() {
    let shared = shared();
    let mut state = shared.state.lock();
    while *state != Setup::Ready {
        shared.changed.wait(&mut state);
    }
}

fn set(app: &AppHandle, next: Setup) {
    let shared = shared();
    *shared.state.lock() = next.clone();
    shared.changed.notify_all();
    let _ = app.emit("ffmpeg-setup", next);
}

/// Starts getting FFmpeg ready, unless that is under way or done. Also the retry after a
/// failed download.
pub fn start(app: &AppHandle) {
    static RUNNING: AtomicBool = AtomicBool::new(false);
    if READY.get().is_some() || RUNNING.swap(true, Ordering::SeqCst) {
        return;
    }
    set(app, Setup::Checking);
    let app = app.clone();
    std::thread::Builder::new()
        .name("ffmpeg-setup".into())
        .spawn(move || {
            #[cfg(windows)]
            let result = app.path().app_local_data_dir().context("no data folder").and_then(|data| prepare(&|step| set(&app, step), &data.join("ffmpeg")));
            #[cfg(unix)]
            let result = find_system();
            match result {
                Ok(dir) => {
                    let _ = READY.set(dir);
                    set(&app, Setup::Ready);
                    // Thumbnails and details that failed without FFmpeg are worth another try.
                    let _ = app.emit("library-changed", ());
                }
                Err(e) => {
                    log::error!("FFmpeg setup failed: {e:#}");
                    set(&app, Setup::Failed { error: format!("{e:#}") });
                }
            }
            RUNNING.store(false, Ordering::SeqCst);
        })
        .expect("thread");
}

/// The folder with the system's ffmpeg and ffprobe, once one is good enough.
#[cfg(unix)]
fn find_system() -> Result<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
    // An app started from the Dock or a desktop menu may not get the shell's PATH.
    dirs.extend(["/opt/homebrew/bin", "/usr/local/bin", "/opt/local/bin", "/usr/bin", "/snap/bin"].map(PathBuf::from));
    let mut problem = None;
    for dir in dirs {
        if !(dir.join("ffmpeg").is_file() && dir.join("ffprobe").is_file()) {
            continue;
        }
        match check_system(&dir) {
            Ok(()) => {
                log::info!("using FFmpeg in {}", dir.display());
                return Ok(dir);
            }
            Err(e) => problem = problem.or(Some(e)),
        }
    }
    Err(problem.unwrap_or_else(|| anyhow::anyhow!(install_hint())))
}

#[cfg(target_os = "linux")]
fn install_hint() -> String {
    crate::i18n::tr("Replix needs FFmpeg 6 or newer. Install it with your package manager, e.g. sudo apt install ffmpeg, then try again.")
}

#[cfg(target_os = "macos")]
fn install_hint() -> String {
    crate::i18n::tr("Replix needs FFmpeg 6 or newer. Install it with Homebrew (brew install ffmpeg), then try again.")
}

/// The screen grabber this system's FFmpeg must have.
#[cfg(target_os = "linux")]
const GRABBER: &str = "x11grab";
#[cfg(target_os = "macos")]
const GRABBER: &str = "avfoundation";

#[cfg(unix)]
fn check_system(dir: &Path) -> Result<()> {
    let run = |args: &[&str]| -> Result<String> {
        let out = Command::new(dir.join("ffmpeg")).args(args).stdin(std::process::Stdio::null()).output()?;
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    };
    let version = run(&["-version"])?;
    let number = parse_version(&version).unwrap_or_default();
    let major: u32 = number.split('.').next().and_then(|m| m.parse().ok()).unwrap_or(0);
    // Builds from Git name a date or commit instead of a version; those are new.
    let from_git = !number.is_empty() && !number.chars().next().is_some_and(|c| c.is_ascii_digit());
    if major < MIN_MAJOR && !from_git {
        bail!(crate::i18n::trf("The FFmpeg on this computer is version {version}. Replix needs 6 or newer.", &[("version", &number)]));
    }
    let devices = run(&["-hide_banner", "-devices"])?;
    if !devices.split_whitespace().any(|w| w == GRABBER) {
        bail!(crate::i18n::trf("This FFmpeg can't record the screen: it was built without {grabber}.", &[("grabber", &GRABBER)]));
    }
    Ok(())
}

/// "6.1.1" from "ffmpeg version 6.1.1-3ubuntu5 Copyright …", or "7.1" from "ffmpeg version n7.1".
#[cfg(unix)]
fn parse_version(text: &str) -> Option<String> {
    let word = text.lines().next()?.strip_prefix("ffmpeg version ")?.split_whitespace().next()?;
    let word = word.strip_prefix('n').filter(|w| w.starts_with(|c: char| c.is_ascii_digit())).unwrap_or(word);
    Some(word.split(['-', '+', '~']).next().unwrap_or(word).to_string())
}

#[cfg(windows)]
fn prepare(report: &dyn Fn(Setup), root: &Path) -> Result<PathBuf> {
    let dir = root.join(VERSION);
    if TOOLS.iter().all(|(name, _)| dir.join(name).is_file()) {
        remove_others(root);
        return Ok(dir);
    }
    std::fs::create_dir_all(root).context("the FFmpeg folder could not be made")?;
    let part = root.join(format!("{VERSION}.part"));
    let _ = std::fs::remove_dir_all(&part);
    std::fs::create_dir_all(&part)?;
    if !adopt_bundled(&part) {
        let archive = root.join("download.zip");
        download(report, &archive)?;
        report(Setup::Unpacking);
        let unpacked = unpack(&archive, &part);
        let _ = std::fs::remove_file(&archive);
        unpacked?;
    }
    for (name, sha) in TOOLS {
        if hash_file(&part.join(name))? != sha {
            bail!("{name} is not the expected build");
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::rename(&part, &dir).context("FFmpeg could not be put in place")?;
    remove_others(root);
    Ok(dir)
}

/// Replix up to 0.1.5 installed FFmpeg next to itself. When that copy is the pinned build
/// it is moved over instead of downloaded again.
#[cfg(windows)]
fn adopt_bundled(part: &Path) -> bool {
    let Some(exe_dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf)) else {
        return false;
    };
    let matches = TOOLS.iter().all(|(name, sha)| hash_file(&exe_dir.join(name)).is_ok_and(|h| h == *sha));
    if !matches {
        return false;
    }
    for (name, _) in TOOLS {
        let (from, to) = (exe_dir.join(name), part.join(name));
        if std::fs::rename(&from, &to).is_err() {
            if std::fs::copy(&from, &to).is_err() {
                return false;
            }
            let _ = std::fs::remove_file(&from);
        }
    }
    log::info!("FFmpeg {VERSION} moved over from the install folder");
    true
}

#[cfg(windows)]
fn system_tool(name: &str) -> PathBuf {
    let root = std::env::var_os("SystemRoot").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
    root.join("System32").join(name)
}

#[cfg(windows)]
fn download(report: &dyn Fn(Setup), archive: &Path) -> Result<()> {
    let _ = std::fs::remove_file(archive);
    log::info!("downloading FFmpeg {VERSION}");
    report(Setup::Downloading { done: 0, total: ARCHIVE_SIZE });
    let mut child = Command::new(system_tool("curl.exe"))
        .creation_flags(CREATE_NO_WINDOW)
        .args(["-sSLf", "--retry", "3", "--connect-timeout", "20", "-o"])
        .arg(archive)
        .arg(URL)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn_tied()
        .context("the download could not be started")?;
    let mut stderr = child.stderr.take().unwrap();
    let errors = std::thread::spawn(move || {
        let mut text = String::new();
        let _ = stderr.read_to_string(&mut text);
        text
    });
    let mut shown = 0;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        let done = std::fs::metadata(archive).map(|m| m.len()).unwrap_or(0);
        // About every megabyte, so the page is not flooded.
        if done >= shown + (1 << 20) {
            shown = done;
            report(Setup::Downloading { done, total: ARCHIVE_SIZE });
        }
        std::thread::sleep(Duration::from_millis(200));
    };
    let errors = errors.join().unwrap_or_default();
    if !status.success() {
        let _ = std::fs::remove_file(archive);
        let detail = errors.trim().trim_start_matches("curl: ").to_string();
        bail!(crate::i18n::trf("FFmpeg could not be downloaded. Check the internet connection. ({detail})", &[("detail", &detail)]));
    }
    let size = std::fs::metadata(archive)?.len();
    if size != ARCHIVE_SIZE || hash_file(archive)? != ARCHIVE_SHA256 {
        let _ = std::fs::remove_file(archive);
        bail!(crate::i18n::tr("The FFmpeg download was damaged."));
    }
    report(Setup::Downloading { done: size, total: ARCHIVE_SIZE });
    Ok(())
}

#[cfg(windows)]
fn unpack(archive: &Path, into: &Path) -> Result<()> {
    let members: Vec<String> = TOOLS.iter().map(|(name, _)| format!("{ARCHIVE_ROOT}/bin/{name}")).collect();
    let out = Command::new(system_tool("tar.exe"))
        .creation_flags(CREATE_NO_WINDOW)
        .arg("-xf")
        .arg(archive)
        .arg("-C")
        .arg(into)
        .args(["--strip-components", "2"])
        .args(&members)
        .stdin(Stdio::null())
        .output()
        .context("the FFmpeg download could not be unpacked")?;
    if !out.status.success() {
        bail!("the FFmpeg download could not be unpacked: {}", String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(())
}

/// Older FFmpeg versions and leftovers of an interrupted setup.
#[cfg(windows)]
fn remove_others(root: &Path) {
    let Ok(entries) = std::fs::read_dir(root) else { return };
    for entry in entries.flatten() {
        if entry.file_name() != VERSION {
            let path = entry.path();
            let _ = if path.is_dir() { std::fs::remove_dir_all(&path) } else { std::fs::remove_file(&path) };
        }
    }
}

#[cfg_attr(unix, allow(dead_code))]
fn hash_file(path: &Path) -> Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_hash_is_lowercase_hex() {
        let dir = std::env::temp_dir().join(format!("replix-hash-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("abc.txt");
        std::fs::write(&file, b"abc").unwrap();
        assert_eq!(hash_file(&file).unwrap(), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn system_versions_are_read() {
        assert_eq!(parse_version("ffmpeg version 6.1.1-3ubuntu5 Copyright (c) 2000-2023").as_deref(), Some("6.1.1"));
        assert_eq!(parse_version("ffmpeg version n7.1 Copyright").as_deref(), Some("7.1"));
        assert_eq!(parse_version("ffmpeg version 8.0 Copyright").as_deref(), Some("8.0"));
        assert_eq!(parse_version("ffmpeg version N-118000-gabc Copyright").as_deref(), Some("N"));
        assert_eq!(parse_version("something else"), None);
    }

    /// Only the current version's folder stays.
    #[cfg(windows)]
    #[test]
    fn old_versions_and_leftovers_are_removed() {
        let root = std::env::temp_dir().join(format!("replix-ffmpeg-{}", std::process::id()));
        for d in [VERSION, "8.0.1", &format!("{VERSION}.part")] {
            std::fs::create_dir_all(root.join(d)).unwrap();
        }
        std::fs::write(root.join("download.zip"), b"x").unwrap();
        remove_others(&root);
        let left: Vec<_> = std::fs::read_dir(&root).unwrap().flatten().map(|e| e.file_name()).collect();
        assert_eq!(left, vec![std::ffi::OsString::from(VERSION)]);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The real thing: downloads the pinned build (about 110 MB), checks and unpacks it.
    /// `cargo test --lib install -- --ignored`
    #[cfg(windows)]
    #[test]
    #[ignore]
    fn the_pinned_build_downloads_and_unpacks() {
        let root = std::env::temp_dir().join(format!("replix-setup-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("8.0.0")).unwrap();
        let steps = Mutex::new(Vec::new());
        let dir = prepare(&|s| steps.lock().push(s), &root).unwrap();
        assert_eq!(dir, root.join(VERSION));
        for (name, _) in TOOLS {
            assert!(dir.join(name).is_file(), "{name}");
        }
        let version = Command::new(dir.join("ffmpeg.exe")).arg("-version").output().unwrap();
        assert!(String::from_utf8_lossy(&version.stdout).starts_with(&format!("ffmpeg version {VERSION}")));
        let left: Vec<_> = std::fs::read_dir(&root).unwrap().flatten().map(|e| e.file_name()).collect();
        assert_eq!(left, vec![std::ffi::OsString::from(VERSION)], "the old version and the download are gone");
        let steps = steps.into_inner();
        assert!(steps.iter().any(|s| matches!(s, Setup::Downloading { done, .. } if *done == ARCHIVE_SIZE)));
        assert!(steps.contains(&Setup::Unpacking));
        // A second start finds it in place and downloads nothing.
        let again = Mutex::new(Vec::new());
        assert_eq!(prepare(&|s| again.lock().push(s), &root).unwrap(), dir);
        assert!(again.into_inner().is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }
}
