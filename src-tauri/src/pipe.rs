//! Named pipes that feed raw audio into FFmpeg.

use anyhow::{bail, Result};
use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, ERROR_PIPE_CONNECTED, GENERIC_READ, HANDLE, INVALID_HANDLE_VALUE};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, WriteFile, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_NONE, OPEN_EXISTING, PIPE_ACCESS_OUTBOUND,
};
use windows::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_WAIT,
};

pub struct PipeServer {
    handle: HANDLE,
    pub path: String,
}

// The handle is only used from one thread at a time.
unsafe impl Send for PipeServer {}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

impl PipeServer {
    pub fn create(name: &str) -> Result<Self> {
        let path = format!(r"\\.\pipe\{name}");
        let w = wide(&path);
        let handle = unsafe {
            CreateNamedPipeW(
                PCWSTR(w.as_ptr()),
                PIPE_ACCESS_OUTBOUND,
                PIPE_TYPE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                1,
                // Small buffer: data that waits here is audio arriving late.
                64 * 1024,
                0,
                0,
                None,
            )
        };
        if handle == INVALID_HANDLE_VALUE || handle.is_invalid() {
            bail!("could not create pipe {path}");
        }
        Ok(Self { handle, path })
    }

    /// Blocks until FFmpeg opens the pipe.
    pub fn connect(&self) -> Result<()> {
        match unsafe { ConnectNamedPipe(self.handle, None) } {
            Ok(()) => Ok(()),
            Err(e) if e.code() == ERROR_PIPE_CONNECTED.to_hresult() => Ok(()),
            Err(e) => Err(e.into()),
        }
    }

    pub fn write_all(&self, mut data: &[u8]) -> Result<()> {
        while !data.is_empty() {
            let mut written = 0u32;
            unsafe { WriteFile(self.handle, Some(data), Some(&mut written), None)? };
            if written == 0 {
                bail!("pipe closed");
            }
            data = &data[written as usize..];
        }
        Ok(())
    }

    /// Wakes a thread stuck in `connect` by connecting to the pipe ourselves.
    pub fn unblock(path: &str) {
        let w = wide(path);
        unsafe {
            if let Ok(h) = CreateFileW(
                PCWSTR(w.as_ptr()),
                GENERIC_READ.0,
                FILE_SHARE_NONE,
                None,
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL,
                None,
            ) {
                let _ = CloseHandle(h);
            }
        }
    }
}

impl Drop for PipeServer {
    fn drop(&mut self) {
        unsafe {
            let _ = DisconnectNamedPipe(self.handle);
            let _ = CloseHandle(self.handle);
        }
    }
}
