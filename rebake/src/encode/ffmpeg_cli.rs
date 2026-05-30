use std::process::Command;

use crate::core::stage::StageError;

pub fn ensure_ffmpeg_cli_available() -> Result<(), StageError> {
    let output = Command::new("ffmpeg")
        .arg("-version")
        .output()
        .map_err(|e| StageError::io("failed to execute ffmpeg CLI", e))?;

    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    Err(StageError::external(
        format!("ffmpeg CLI is not available or failed to start: {stderr}"),
        std::io::Error::other("ffmpeg CLI availability check failed"),
    ))
}

pub fn ensure_ffmpeg_cli_encoder_available(encoder_name: &str) -> Result<(), StageError> {
    ensure_ffmpeg_cli_available()?;

    let output = Command::new("ffmpeg")
        .arg("-hide_banner")
        .arg("-h")
        .arg(format!("encoder={encoder_name}"))
        .output()
        .map_err(|e| {
            StageError::io(
                format!("failed to query ffmpeg CLI encoder {encoder_name}"),
                e,
            )
        })?;

    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    Err(StageError::external(
        format!(
            "ffmpeg CLI does not report encoder '{encoder_name}'. \
             This only checks the ffmpeg binary; GPU driver/runtime failures can still occur when encoding starts. \
             stdout: {stdout}; stderr: {stderr}"
        ),
        std::io::Error::other(format!("ffmpeg encoder {encoder_name} unavailable")),
    ))
}
