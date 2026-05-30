use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::core::stage::StageError;

pub const DEFAULT_NVENC_AV1_QP: u32 = 130;
pub const DEFAULT_NVENC_AV1_PRESET: NvencPreset = NvencPreset::P7;
pub const DEFAULT_NVENC_H264_QP: u32 = 26;
pub const DEFAULT_NVENC_H264_PRESET: NvencPreset = NvencPreset::P5;
pub const DEFAULT_NVENC_H264_TUNE: NvencTune = NvencTune::Hq;
pub const DEFAULT_NVENC_H264_PROFILE: &str = "high";
pub const DEFAULT_NVENC_H264_B_FRAMES: u32 = 1;
pub const DEFAULT_NVENC_H264_RC_LOOKAHEAD: u32 = 32;
pub const DEFAULT_NVENC_B_FRAMES: u32 = 0;
pub const MAX_NVENC_B_FRAMES: u32 = 7;
pub const MAX_NVENC_RC_LOOKAHEAD: u32 = 120;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NvencPreset {
    #[serde(rename = "P1", alias = "p1")]
    P1,
    #[serde(rename = "P2", alias = "p2")]
    P2,
    #[serde(rename = "P3", alias = "p3")]
    P3,
    #[serde(rename = "P4", alias = "p4")]
    P4,
    #[serde(rename = "P5", alias = "p5")]
    P5,
    #[serde(rename = "P6", alias = "p6")]
    P6,
    #[serde(rename = "P7", alias = "p7")]
    P7,
}

impl Default for NvencPreset {
    fn default() -> Self {
        Self::P4
    }
}

impl NvencPreset {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::P1 => "p1",
            Self::P2 => "p2",
            Self::P3 => "p3",
            Self::P4 => "p4",
            Self::P5 => "p5",
            Self::P6 => "p6",
            Self::P7 => "p7",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NvencTune {
    #[serde(rename = "Hq", alias = "hq")]
    Hq,
    #[serde(rename = "Ll", alias = "ll")]
    Ll,
    #[serde(rename = "Ull", alias = "ull")]
    Ull,
}

impl NvencTune {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Hq => "hq",
            Self::Ll => "ll",
            Self::Ull => "ull",
        }
    }
}

pub fn validate_nvenc_b_frames(codec: &str, b_frames: u32) -> Result<(), StageError> {
    if b_frames > MAX_NVENC_B_FRAMES {
        return Err(StageError::invalid(format!(
            "{codec} b_frames must be between 0 and {MAX_NVENC_B_FRAMES}"
        )));
    }
    Ok(())
}

pub fn validate_nvenc_rc_lookahead(
    codec: &str,
    rc_lookahead: Option<u32>,
) -> Result<(), StageError> {
    if let Some(value) = rc_lookahead
        && value > MAX_NVENC_RC_LOOKAHEAD
    {
        return Err(StageError::invalid(format!(
            "{codec} rc_lookahead must be between 0 and {MAX_NVENC_RC_LOOKAHEAD}"
        )));
    }
    Ok(())
}

pub fn is_nvenc_device_visible() -> bool {
    Path::new("/dev/nvidiactl").exists()
        && (Path::new("/dev/nvidia0").exists() || Path::new("/dev/nvidia-caps").exists())
}

pub fn ensure_nvenc_device_visible() -> Result<(), StageError> {
    if is_nvenc_device_visible() {
        return Ok(());
    }

    Err(StageError::external(
        "NVIDIA device files are not visible. NVENC requires a container/runtime with NVIDIA GPU access (for example Docker Compose device reservations).",
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "NVIDIA device files not found",
        ),
    ))
}
