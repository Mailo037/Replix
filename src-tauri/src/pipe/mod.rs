//! Pipes that feed raw audio into FFmpeg: named pipes on Windows, FIFOs elsewhere. Both
//! work the same way: `create`, `connect` (blocks until FFmpeg opens it), `write_all`, and
//! `unblock` to wake a `connect` that waits for an FFmpeg that never came.

#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

#[cfg(unix)]
pub use self::unix::PipeServer;
#[cfg(windows)]
pub use self::windows::PipeServer;
