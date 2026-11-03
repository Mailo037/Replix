//! Getting clips out of Replix: onto the clipboard as files, or behind a link.

use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};
use windows::Win32::Foundation::{GlobalFree, HANDLE};
use windows::Win32::System::DataExchange::{CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};

const CF_UNICODETEXT: u32 = 13;
const CF_HDROP: u32 = 15;
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
/// Litterbox refuses larger files.
const LINK_LIMIT: u64 = 1024 * 1024 * 1024;

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

/// Uploads to Litterbox, catbox.moe's temporary host. Needs no account; the link stops
/// working after 72 hours. Uses the curl that ships with Windows 10 and 11.
pub fn upload(path: &Path) -> Result<String> {
    let size = std::fs::metadata(path).context("the file is gone")?.len();
    if size > LINK_LIMIT {
        bail!("Links work up to 1 GB. Export a shorter version first.");
    }
    // Inside quotes curl reads backslashes as escapes.
    let quoted = path.to_string_lossy().replace('\\', "\\\\").replace('"', "\\\"");
    let output = Command::new("curl.exe")
        .creation_flags(CREATE_NO_WINDOW)
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
