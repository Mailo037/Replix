//! Getting clips out of Replix: onto the clipboard as files, or behind a link.

use std::path::Path;
use std::process::Command;

use anyhow::{bail, Context, Result};

/// Litterbox refuses larger files.
const LINK_LIMIT: u64 = 1024 * 1024 * 1024;

#[cfg(windows)]
mod clipboard {
    use std::path::PathBuf;

    use anyhow::{bail, Context, Result};
    use windows::Win32::Foundation::{GlobalFree, HANDLE};
    use windows::Win32::System::DataExchange::{CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData};
    use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};

    const CF_UNICODETEXT: u32 = 13;
    const CF_HDROP: u32 = 15;

    /// DROPFILES from shlobj_core.h: the header of a file list on the clipboard.
    #[repr(C)]
    struct DropFiles {
        files_offset: u32,
        x: i32,
        y: i32,
        non_client: i32,
        wide: i32,
    }

    fn put_on_clipboard(format: u32, bytes: &[u8]) -> Result<()> {
        unsafe {
            let memory = GlobalAlloc(GMEM_MOVEABLE, bytes.len())?;
            let target = GlobalLock(memory) as *mut u8;
            if target.is_null() {
                let _ = GlobalFree(Some(memory));
                bail!("no memory for the clipboard");
            }
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), target, bytes.len());
            let _ = GlobalUnlock(memory);
            OpenClipboard(None).context("the clipboard is busy")?;
            let _ = EmptyClipboard();
            // On success the clipboard owns the memory.
            let placed = SetClipboardData(format, Some(HANDLE(memory.0)));
            let _ = CloseClipboard();
            if placed.is_err() {
                let _ = GlobalFree(Some(memory));
            }
            placed?;
        }
        Ok(())
    }

    fn wide(text: &str) -> Vec<u8> {
        text.encode_utf16().flat_map(u16::to_le_bytes).collect()
    }

    /// The same thing Explorer's Copy does, so pasting into Discord, a chat or a folder works.
    pub fn copy_files(paths: &[PathBuf]) -> Result<()> {
        let header = DropFiles { files_offset: std::mem::size_of::<DropFiles>() as u32, x: 0, y: 0, non_client: 0, wide: 1 };
        let mut bytes = Vec::new();
        // SAFETY: DropFiles is plain old data.
        bytes.extend_from_slice(unsafe {
            std::slice::from_raw_parts(&header as *const DropFiles as *const u8, std::mem::size_of::<DropFiles>())
        });
        for path in paths {
            bytes.extend(wide(&path.to_string_lossy()));
            bytes.extend([0, 0]);
        }
        bytes.extend([0, 0]);
        put_on_clipboard(CF_HDROP, &bytes)
    }

    pub fn copy_text(text: &str) -> Result<()> {
        let mut bytes = wide(text);
        bytes.extend([0, 0]);
        put_on_clipboard(CF_UNICODETEXT, &bytes)
    }
}

#[cfg(target_os = "linux")]
mod clipboard {
    use std::io::Write;
    use std::path::PathBuf;
    use std::process::{Command, Stdio};

    use anyhow::{bail, Context, Result};

    /// Hands `bytes` to wl-copy (Wayland) or xclip (X11). Both stay in the background to
    /// serve the clipboard, so only their start is waited for, not their end.
    fn put(mime: &str, bytes: &[u8]) -> Result<()> {
        let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some();
        let mut cmd = if wayland {
            let mut c = Command::new("wl-copy");
            c.args(["--type", mime]);
            c
        } else {
            let mut c = Command::new("xclip");
            c.args(["-selection", "clipboard", "-t", mime, "-i"]);
            c
        };
        let mut child = cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .with_context(|| crate::i18n::tr("Copying needs wl-clipboard (Wayland) or xclip (X11). Install it with your package manager."))?;
        child.stdin.take().context("no input")?.write_all(bytes)?;
        let status = child.wait()?;
        if !status.success() {
            bail!("the clipboard did not take it");
        }
        Ok(())
    }

    /// file:///… with the characters a URI may not hold escaped.
    fn uri(path: &std::path::Path) -> String {
        let mut out = String::from("file://");
        for b in path.to_string_lossy().bytes() {
            match b {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
                _ => out.push_str(&format!("%{b:02X}")),
            }
        }
        out
    }

    /// As a file manager copies files, so pasting into Discord, a chat or a folder works.
    pub fn copy_files(paths: &[PathBuf]) -> Result<()> {
        let list: String = paths.iter().map(|p| uri(p) + "\r\n").collect();
        put("text/uri-list", list.as_bytes())
    }

    pub fn copy_text(text: &str) -> Result<()> {
        put("text/plain;charset=utf-8", text.as_bytes())
    }

    #[cfg(test)]
    mod tests {
        #[test]
        fn paths_become_file_uris() {
            assert_eq!(super::uri(std::path::Path::new("/home/a/Videos/My clip #1.mp4")), "file:///home/a/Videos/My%20clip%20%231.mp4");
        }
    }
}

#[cfg(target_os = "macos")]
mod clipboard {
    use std::io::Write;
    use std::path::PathBuf;
    use std::process::{Command, Stdio};

    use anyhow::{bail, Context, Result};

    /// As Finder copies files, so pasting into Discord, a chat or a folder works.
    pub fn copy_files(paths: &[PathBuf]) -> Result<()> {
        let list = serde_json::to_string(&paths.iter().map(|p| p.to_string_lossy().into_owned()).collect::<Vec<_>>())?;
        let script = format!(
            "ObjC.import('AppKit'); const pb = $.NSPasteboard.generalPasteboard; pb.clearContents; \
             pb.writeObjects($({list}.map(p => $.NSURL.fileURLWithPath(p))));"
        );
        let out = Command::new("osascript").args(["-l", "JavaScript", "-e", &script]).output().context("osascript is missing")?;
        if !out.status.success() {
            bail!("the clipboard did not take it: {}", String::from_utf8_lossy(&out.stderr).trim());
        }
        Ok(())
    }

    pub fn copy_text(text: &str) -> Result<()> {
        let mut child = Command::new("pbcopy").stdin(Stdio::piped()).spawn().context("pbcopy is missing")?;
        child.stdin.take().context("no input")?.write_all(text.as_bytes())?;
        if !child.wait()?.success() {
            bail!("the clipboard did not take it");
        }
        Ok(())
    }
}

pub use clipboard::{copy_files, copy_text};

/// Uploads to Litterbox, catbox.moe's temporary host. Needs no account; the link stops
/// working after 72 hours. Uses curl, which ships with Windows 10 and 11, macOS and nearly
/// every Linux.
pub fn upload(path: &Path) -> Result<String> {
    let size = std::fs::metadata(path).context("the file is gone")?.len();
    if size > LINK_LIMIT {
        bail!("Links work up to 1 GB. Export a shorter version first.");
    }
    // Inside quotes curl reads backslashes as escapes.
    let quoted = path.to_string_lossy().replace('\\', "\\\\").replace('"', "\\\"");
    let mut cmd = Command::new(format!("curl{}", std::env::consts::EXE_SUFFIX));
    crate::platform::background(&mut cmd, false);
    let output = cmd
        .args(["-sS", "--fail", "--max-time", "900", "-F", "reqtype=fileupload", "-F", "time=72h", "-F"])
        .arg(format!("fileToUpload=@\"{quoted}\""))
        .arg("https://litterbox.catbox.moe/resources/internals/api.php")
        .output()
        .context("curl is missing on this PC")?;
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if !output.status.success() || !text.starts_with("https://") {
        let error = String::from_utf8_lossy(&output.stderr).trim().to_string();
        log::warn!("upload failed: {error} {text}");
        bail!("The upload didn't go through. Check your connection and try again.");
    }
    Ok(text)
}
