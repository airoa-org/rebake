use std::io::{Read, Write};
use std::process::{Child, ChildStderr, Command, Stdio};
use std::thread::{self, JoinHandle};

use crate::core::stage::StageError;

const MAX_CAPTURED_STDERR: usize = 64 * 1024;

/// Owns an FFmpeg child process while frames are written to its stdin.
///
/// FFmpeg stderr is connected through an OS pipe. A pipe has a finite buffer:
/// if FFmpeg writes enough diagnostics and the parent process does not read
/// them, FFmpeg can block and the parent can then hang while writing frames to
/// stdin. This wrapper starts a background thread that continuously reads
/// stderr and keeps only the latest bytes for error reporting.
pub struct FfmpegSubprocess {
    child: Option<Child>,
    stderr_handle: Option<JoinHandle<Vec<u8>>>,
    backend_name: String,
}

impl FfmpegSubprocess {
    pub fn spawn(
        mut command: Command,
        backend_name: impl Into<String>,
    ) -> Result<Self, StageError> {
        let backend_name = backend_name.into();
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());

        let mut child = command.spawn().map_err(|e| {
            StageError::io(
                format!("failed to spawn FFmpeg process for {backend_name}"),
                e,
            )
        })?;

        let stderr = child.stderr.take().ok_or_else(|| {
            StageError::external(
                format!("failed to capture FFmpeg stderr for {backend_name}"),
                std::io::Error::other("missing ffmpeg stderr pipe"),
            )
        })?;
        let stderr_handle = thread::spawn(move || read_stderr_with_limit(stderr));

        Ok(Self {
            child: Some(child),
            stderr_handle: Some(stderr_handle),
            backend_name,
        })
    }

    pub fn write_all(&mut self, data: &[u8], context: &str) -> Result<(), StageError> {
        let write_result = {
            let child = self.child.as_mut().ok_or_else(|| {
                StageError::external(
                    format!("FFmpeg process for {} is already closed", self.backend_name),
                    std::io::Error::new(std::io::ErrorKind::BrokenPipe, "ffmpeg process closed"),
                )
            })?;
            let stdin = child.stdin.as_mut().ok_or_else(|| {
                StageError::external(
                    format!("FFmpeg stdin for {} is already closed", self.backend_name),
                    std::io::Error::new(std::io::ErrorKind::BrokenPipe, "ffmpeg stdin closed"),
                )
            })?;
            stdin.write_all(data)
        };

        if let Err(e) = write_result {
            let stderr = self.terminate_after_write_error();
            return Err(StageError::external(
                format!(
                    "{context}: failed to write to FFmpeg stdin for {}: {e}.{}",
                    self.backend_name,
                    stderr_suffix(&stderr)
                ),
                std::io::Error::other(e.to_string()),
            ));
        }

        Ok(())
    }

    pub fn finish(&mut self) -> Result<(), StageError> {
        let mut child = self.child.take().ok_or_else(|| {
            StageError::external(
                format!("FFmpeg process for {} is already closed", self.backend_name),
                std::io::Error::new(std::io::ErrorKind::BrokenPipe, "ffmpeg process closed"),
            )
        })?;

        drop(child.stdin.take());
        let status = child.wait().map_err(|e| {
            StageError::io(
                format!(
                    "failed to wait for FFmpeg process for {}",
                    self.backend_name
                ),
                e,
            )
        })?;
        let stderr = self.join_stderr_lossy();

        if status.success() {
            return Ok(());
        }

        Err(StageError::external(
            format!(
                "FFmpeg process for {} failed with status {status}.{}",
                self.backend_name,
                stderr_suffix(&stderr)
            ),
            std::io::Error::other(format!("ffmpeg exited with status {status}")),
        ))
    }

    fn terminate_after_write_error(&mut self) -> String {
        if let Some(mut child) = self.child.take() {
            drop(child.stdin.take());
            match child.try_wait() {
                Ok(Some(_)) => {}
                Ok(None) => {
                    let _ = child.kill();
                    let _ = child.wait();
                }
                Err(_) => {
                    let _ = child.kill();
                    let _ = child.wait();
                }
            }
        }
        self.join_stderr_lossy()
    }

    fn join_stderr_lossy(&mut self) -> String {
        let Some(handle) = self.stderr_handle.take() else {
            return String::new();
        };

        match handle.join() {
            Ok(bytes) => String::from_utf8_lossy(&bytes).trim().to_string(),
            Err(_) => String::from("failed to join FFmpeg stderr reader thread"),
        }
    }
}

impl Drop for FfmpegSubprocess {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            drop(child.stdin.take());
            match child.try_wait() {
                Ok(Some(_)) => {}
                Ok(None) | Err(_) => {
                    let _ = child.kill();
                    let _ = child.wait();
                }
            }
        }

        if let Some(handle) = self.stderr_handle.take() {
            let _ = handle.join();
        }
    }
}

fn read_stderr_with_limit(mut stderr: ChildStderr) -> Vec<u8> {
    let mut captured = Vec::new();
    let mut buffer = [0_u8; 8192];

    // Keep reading until FFmpeg closes stderr. This prevents the stderr pipe
    // from filling while frame bytes are still being written to stdin.
    loop {
        match stderr.read(&mut buffer) {
            Ok(0) => break,
            Ok(n) => push_stderr_with_limit(&mut captured, &buffer[..n], MAX_CAPTURED_STDERR),
            Err(_) => break,
        }
    }

    captured
}

fn push_stderr_with_limit(captured: &mut Vec<u8>, incoming: &[u8], limit: usize) {
    if incoming.len() >= limit {
        captured.clear();
        captured.extend_from_slice(&incoming[incoming.len() - limit..]);
        return;
    }

    captured.extend_from_slice(incoming);
    if captured.len() > limit {
        let overflow = captured.len() - limit;
        // Vec::drain removes the oldest bytes from the front; the retained
        // stderr is intentionally the most recent output before failure.
        captured.drain(..overflow);
    }
}

fn stderr_suffix(stderr: &str) -> String {
    if stderr.is_empty() {
        String::new()
    } else {
        format!(" FFmpeg stderr: {stderr}")
    }
}
