//! Linux and macOS: FIFOs in the temporary folder.

use std::ffi::CString;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;
use std::sync::OnceLock;

use anyhow::{bail, Result};

pub struct PipeServer {
    file: OnceLock<File>,
    pub path: String,
}

impl PipeServer {
    pub fn create(name: &str) -> Result<Self> {
        let path: PathBuf = std::env::temp_dir().join(format!("{name}.pipe"));
        let _ = std::fs::remove_file(&path);
        let c = CString::new(path.as_os_str().as_bytes())?;
        // SAFETY: the path is NUL-terminated; only this user may open the FIFO.
        if unsafe { libc::mkfifo(c.as_ptr(), 0o600) } != 0 {
            bail!("could not create pipe {}: {}", path.display(), std::io::Error::last_os_error());
        }
        Ok(Self { file: OnceLock::new(), path: path.to_string_lossy().into_owned() })
    }

    /// Blocks until FFmpeg opens the pipe.
    pub fn connect(&self) -> Result<()> {
        let file = OpenOptions::new().write(true).open(&self.path)?;
        let _ = self.file.set(file);
        Ok(())
    }

    pub fn write_all(&self, data: &[u8]) -> Result<()> {
        let Some(mut file) = self.file.get() else { bail!("pipe not open") };
        file.write_all(data)?;
        Ok(())
    }

    /// Wakes a thread stuck in `connect` by opening the other end ourselves: its open goes
    /// through, and its next write fails once this end is closed again, which ends it.
    pub fn unblock(path: &str) {
        let _ = OpenOptions::new().read(true).custom_flags(libc::O_NONBLOCK).open(path);
    }
}

impl Drop for PipeServer {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    #[test]
    fn what_is_written_comes_out_the_other_end() {
        let pipe = PipeServer::create(&format!("replix-test-{}", std::process::id())).unwrap();
        let path = pipe.path.clone();
        let reader = std::thread::spawn(move || {
            let mut text = String::new();
            File::open(path).unwrap().read_to_string(&mut text).unwrap();
            text
        });
        pipe.connect().unwrap();
        pipe.write_all(b"sound").unwrap();
        let path = pipe.path.clone();
        drop(pipe);
        assert_eq!(reader.join().unwrap(), "sound");
        assert!(!std::path::Path::new(&path).exists());
    }

    #[test]
    fn a_waiting_connect_can_be_woken() {
        let pipe = std::sync::Arc::new(PipeServer::create(&format!("replix-wake-{}", std::process::id())).unwrap());
        let waiting = pipe.clone();
        let connect = std::thread::spawn(move || waiting.connect());
        std::thread::sleep(std::time::Duration::from_millis(100));
        PipeServer::unblock(&pipe.path);
        assert!(connect.join().unwrap().is_ok());
    }
}
