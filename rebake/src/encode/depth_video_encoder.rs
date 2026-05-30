//! Depth video encoding via FFmpeg subprocess.
//!
//! Encodes 16-bit depth frames (from ROS `compressedDepth`) into video files
//! using one of four supported codecs. For lossy codecs, depth values are
//! quantized via Q10Clip4 (16-bit → 10-bit) and packed into P010LE format.
//! FFV1 lossless encodes raw gray16le without quantization.
//!
//! # Supported Codecs
//!
//! | Codec | Type | Input Format | Container |
//! |-------|------|-------------|-----------|
//! | HEVC VAAPI | HW lossy | P010LE | MP4 |
//! | AV1 VAAPI | HW lossy | P010LE | MP4 |
//! | HEVC NVENC | HW lossy | P010LE | MP4 |
//! | AV1 NVENC | HW lossy | P010LE | MP4 |
//! | AV1 (SVT-AV1) | SW lossy | P010LE | MP4 |
//! | FFV1 | SW lossless | gray16le | MKV |
//!
//! VA-API codecs require `-color_range pc` (full range) to prevent studio range
//! scaling ([64, 940]) which would corrupt depth values that need full [0, 1023].
//!
//! # Usage
//!
//! As a pipeline stage via `rebake-cli run`:
//! ```yaml
//! stage_configs:
//!   - Rosbag2IngestorConfig: {}
//!   - DepthVideoConfig:
//!       depth_max_mm: 4092
//!       fps: 30
//!       codec_config:
//!         codec: AV1
//!         crf: 4
//!         preset: 4
//! ```

use std::collections::HashMap;
use std::path::Path;
use std::process::Command;

use camino::{Utf8Path, Utf8PathBuf};
use serde::{Deserialize, Serialize};

use crate::common::{DepthFrame, topic_name_to_flat_file_stem};
use crate::core::error::{OptionExt, PolarsExt, StageResult};
use crate::core::stage::{Context, Stage, StageConfig, StageError};
use crate::encode::compressed_depth::{DecodedDepthFrame, decode_depth_frame};
use crate::encode::depth_quantizer::{Q10ClipParams, q10_to_p010le, quantize_frame};
use crate::encode::ffmpeg_cli::ensure_ffmpeg_cli_encoder_available;
use crate::encode::ffmpeg_subprocess::FfmpegSubprocess;
use crate::encode::nvenc::{
    DEFAULT_NVENC_B_FRAMES, NvencPreset, NvencTune, ensure_nvenc_device_visible,
    validate_nvenc_b_frames, validate_nvenc_rc_lookahead,
};
use crate::encode::video_artifact::{VideoArtifact, VideoMetadata};

/// Default VA-API device path.
static DEFAULT_VAAPI_DEVICE: &str = "/dev/dri/renderD128";
pub const DEFAULT_DEPTH_NVENC_H265_QP: u32 = 10;
pub const DEFAULT_DEPTH_NVENC_AV1_QP: u32 = 20;
const DEFAULT_DEPTH_GOP_FRAMES: u32 = 30;

// ============================================================================
// Configuration Types
// ============================================================================

/// Depth video encoding configuration.
///
/// Controls how depth frames (16-bit grayscale) are compressed into video files.
/// For lossy codecs, depth values are quantized via Q10Clip4 (16-bit → 10-bit)
/// before encoding. FFV1 lossless encodes raw gray16le without quantization.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DepthVideoConfig {
    /// Maximum depth in millimeters for Q10Clip4 quantization.
    /// Pixels with depth > depth_max_mm are clipped to 0 (invalid).
    /// Ignored when codec is FFV1 (lossless).
    /// Default: 4092.
    #[serde(default = "default_depth_max_mm")]
    pub depth_max_mm: u16,

    /// Codec-specific configuration.
    /// Default: AV1 via SVT-AV1 (CRF=4, preset=4).
    #[serde(default)]
    pub codec_config: DepthCodecConfig,

    /// Frames per second. Default: 30.
    #[serde(default = "default_depth_fps")]
    pub fps: u32,
}

impl Default for DepthVideoConfig {
    fn default() -> Self {
        Self {
            depth_max_mm: default_depth_max_mm(),
            codec_config: DepthCodecConfig::default(),
            fps: default_depth_fps(),
        }
    }
}

impl DepthVideoConfig {
    /// Validate the configuration independent of machine capabilities.
    pub fn validate(&self) -> Result<(), StageError> {
        if self.fps == 0 {
            return Err(StageError::invalid(
                "depth video fps must be greater than 0",
            ));
        }
        if !self.codec_config.is_lossless() && self.depth_max_mm == 0 {
            return Err(StageError::invalid(
                "depth_max_mm must be greater than 0 for lossy depth codecs",
            ));
        }
        self.codec_config.validate()
    }

    /// Validate the configuration and ensure the current machine can execute it.
    pub fn preflight(&self) -> Result<(), StageError> {
        self.validate()?;

        if let Some(device) = self.codec_config.vaapi_device_path() {
            if !Path::new(device).exists() {
                return Err(StageError::invalid(format!(
                    "VA-API depth codec selected but {device} was not found. \
                     Mount /dev/dri or choose a software codec explicitly."
                )));
            }

            return ensure_ffmpeg_cli_encoder_available(self.codec_config.ffmpeg_encoder_name());
        }

        if self.codec_config.is_nvenc() {
            ensure_nvenc_device_visible()?;
        }

        ensure_ffmpeg_cli_encoder_available(self.codec_config.ffmpeg_encoder_name())
    }

    /// Build canonical metadata for an encoded depth video.
    pub fn video_metadata(&self, width: u32, height: u32) -> Result<VideoMetadata, StageError> {
        self.validate()?;

        if width == 0 || height == 0 {
            return Err(StageError::invalid(
                "video metadata requires positive width and height",
            ));
        }

        let (codec_family, encoder_name, pix_fmt) = match &self.codec_config {
            DepthCodecConfig::H265Vaapi { .. } => ("h265", "hevc_vaapi", "p010le"),
            DepthCodecConfig::Av1Vaapi { .. } => ("av1", "av1_vaapi", "p010le"),
            DepthCodecConfig::H265Nvenc { .. } => ("h265", "hevc_nvenc", "p010le"),
            DepthCodecConfig::Av1Nvenc { .. } => ("av1", "av1_nvenc", "p010le"),
            DepthCodecConfig::AV1 { .. } => ("av1", "libsvtav1", "p010le"),
            DepthCodecConfig::Ffv1 => ("ffv1", "ffv1", "gray16le"),
        };

        Ok(VideoMetadata {
            media_type: "depth".to_string(),
            codec_family: codec_family.to_string(),
            encoder_name: encoder_name.to_string(),
            pix_fmt: pix_fmt.to_string(),
            width,
            height,
            fps: self.fps,
            encoding_config_json: serde_json::to_string(self)
                .or_invalid("failed to serialize depth video config")?,
        })
    }

    /// Build a depth video artifact with canonical metadata.
    pub fn video_artifact(
        &self,
        video_path: impl Into<String>,
        width: u32,
        height: u32,
    ) -> Result<VideoArtifact, StageError> {
        Ok(VideoArtifact {
            video_path: video_path.into(),
            metadata: self.video_metadata(width, height)?,
        })
    }
}

/// Codec configuration for depth video encoding.
///
/// Uses the same `#[serde(tag = "codec")]` pattern as `CodecConfig` for RGB video.
/// Each variant exposes the codec's native parameters directly (no preset abstraction).
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(tag = "codec", deny_unknown_fields)]
pub enum DepthCodecConfig {
    /// HEVC via VA-API. Best balance of compression, speed, and quality.
    /// Input: Q10Clip4 quantized → P010LE.
    #[serde(rename = "H265_VAAPI", alias = "hevc_vaapi", alias = "h265_vaapi")]
    H265Vaapi {
        /// Quantization parameter (0-51, lower = better quality). Default: 18.
        #[serde(default = "default_depth_hevc_qp")]
        qp: u32,

        /// VA-API device path. Default: /dev/dri/renderD128.
        #[serde(default)]
        device: Option<String>,
    },

    /// AV1 via VA-API. Slightly better quality than HEVC, slower decode.
    /// Input: Q10Clip4 quantized → P010LE.
    /// NOTE: Uses -global_quality (not -qp) due to FFmpeg AV1 VAAPI limitation.
    #[serde(rename = "AV1_VAAPI", alias = "av1_vaapi")]
    Av1Vaapi {
        /// Global quality parameter (0-255, lower = better quality). Default: 35.
        /// Mapped to FFmpeg's -global_quality, not -qp.
        #[serde(default = "default_depth_av1_gq")]
        global_quality: u32,

        /// VA-API device path. Default: /dev/dri/renderD128.
        #[serde(default)]
        device: Option<String>,
    },

    /// AV1 via SVT-AV1 software encoder. No hardware required, but slow (~140 FPS).
    /// Input: Q10Clip4 quantized → P010LE.
    #[serde(rename = "AV1", alias = "av1", alias = "Av1")]
    AV1 {
        /// Constant Rate Factor (0-63, lower = better quality). Default: 4.
        #[serde(default = "default_depth_av1_crf")]
        crf: u32,

        /// SVT-AV1 preset (0-13, lower = better quality/slower). Default: 4.
        #[serde(default = "default_depth_av1_preset")]
        preset: u32,
    },

    /// HEVC via NVIDIA NVENC. Input: Q10Clip4 quantized -> P010LE.
    #[serde(rename = "H265_NVENC", alias = "hevc_nvenc", alias = "h265_nvenc")]
    H265Nvenc {
        /// Quantization parameter (0-51, lower = better quality). Default: 10.
        #[serde(default = "default_depth_nvenc_h265_qp")]
        qp: u32,

        /// NVIDIA GPU index passed to FFmpeg's -gpu option.
        #[serde(default)]
        gpu: Option<u32>,

        /// NVENC preset (P1 fastest, P7 slowest/best compression). Default: P4.
        #[serde(default)]
        preset: NvencPreset,

        /// NVENC tune. Omit to use FFmpeg/NVENC defaults.
        #[serde(default)]
        tune: Option<NvencTune>,

        /// Number of B-frames. Default: 0 for frame-indexed packaging.
        #[serde(default = "default_depth_nvenc_b_frames", alias = "b-frames")]
        b_frames: u32,

        /// Number of frames to look ahead for rate control (0-120).
        #[serde(default, alias = "rc-lookahead")]
        rc_lookahead: Option<u32>,
    },

    /// AV1 via NVIDIA NVENC. Input: Q10Clip4 quantized -> P010LE.
    #[serde(rename = "AV1_NVENC", alias = "av1_nvenc")]
    Av1Nvenc {
        /// Quantization parameter (0-255, lower = better quality). Default: 20.
        #[serde(default = "default_depth_nvenc_av1_qp")]
        qp: u32,

        /// NVIDIA GPU index passed to FFmpeg's -gpu option.
        #[serde(default)]
        gpu: Option<u32>,

        /// NVENC preset (P1 fastest, P7 slowest/best compression). Default: P4.
        #[serde(default)]
        preset: NvencPreset,

        /// NVENC tune. Omit to use FFmpeg/NVENC defaults.
        #[serde(default)]
        tune: Option<NvencTune>,

        /// Number of B-frames. Default: 0 for frame-indexed packaging.
        #[serde(default = "default_depth_nvenc_b_frames", alias = "b-frames")]
        b_frames: u32,

        /// Number of frames to look ahead for rate control (0-120).
        #[serde(default, alias = "rc-lookahead")]
        rc_lookahead: Option<u32>,
    },

    /// FFV1 lossless. Zero quality loss, but large files (~165 MB/min).
    /// Input: raw gray16le (no quantization).
    /// Container: MKV (FFV1 does not support MP4).
    #[serde(rename = "FFV1", alias = "ffv1")]
    Ffv1,
}

impl Default for DepthCodecConfig {
    fn default() -> Self {
        DepthCodecConfig::AV1 {
            crf: default_depth_av1_crf(),
            preset: default_depth_av1_preset(),
        }
    }
}

impl DepthCodecConfig {
    /// Returns the FFmpeg encoder library name.
    pub fn ffmpeg_encoder_name(&self) -> &'static str {
        match self {
            DepthCodecConfig::H265Vaapi { .. } => "hevc_vaapi",
            DepthCodecConfig::Av1Vaapi { .. } => "av1_vaapi",
            DepthCodecConfig::H265Nvenc { .. } => "hevc_nvenc",
            DepthCodecConfig::Av1Nvenc { .. } => "av1_nvenc",
            DepthCodecConfig::AV1 { .. } => "libsvtav1",
            DepthCodecConfig::Ffv1 => "ffv1",
        }
    }

    /// Returns true if this codec is lossless (FFV1).
    pub fn is_lossless(&self) -> bool {
        matches!(self, DepthCodecConfig::Ffv1)
    }

    /// Returns true if this codec uses VA-API hardware acceleration.
    pub fn is_vaapi(&self) -> bool {
        matches!(
            self,
            DepthCodecConfig::H265Vaapi { .. } | DepthCodecConfig::Av1Vaapi { .. }
        )
    }

    /// Returns true if this codec uses NVIDIA NVENC hardware acceleration.
    pub fn is_nvenc(&self) -> bool {
        matches!(
            self,
            DepthCodecConfig::H265Nvenc { .. } | DepthCodecConfig::Av1Nvenc { .. }
        )
    }

    fn vaapi_device_path(&self) -> Option<&str> {
        match self {
            DepthCodecConfig::H265Vaapi { device, .. }
            | DepthCodecConfig::Av1Vaapi { device, .. } => {
                Some(device.as_deref().unwrap_or(DEFAULT_VAAPI_DEVICE))
            }
            _ => None,
        }
    }

    /// Returns the video file extension for this codec.
    pub fn video_extension(&self) -> &'static str {
        match self {
            DepthCodecConfig::Ffv1 => "mkv",
            _ => "mp4",
        }
    }

    /// Validates codec-specific options.
    pub fn validate(&self) -> Result<(), StageError> {
        match self {
            DepthCodecConfig::H265Vaapi { qp, .. } => {
                if *qp > 51 {
                    return Err(StageError::invalid(
                        "H265_VAAPI depth qp must be between 0 and 51",
                    ));
                }
                Ok(())
            }
            DepthCodecConfig::Av1Vaapi { global_quality, .. } => {
                if *global_quality > 255 {
                    return Err(StageError::invalid(
                        "AV1_VAAPI depth global_quality must be between 0 and 255",
                    ));
                }
                Ok(())
            }
            DepthCodecConfig::AV1 { crf, preset } => {
                if *crf > 63 {
                    return Err(StageError::invalid(
                        "AV1 depth crf must be between 0 and 63",
                    ));
                }
                if *preset > 13 {
                    return Err(StageError::invalid(
                        "AV1 depth preset must be between 0 and 13",
                    ));
                }
                Ok(())
            }
            DepthCodecConfig::H265Nvenc {
                qp,
                b_frames,
                rc_lookahead,
                ..
            } => {
                if *qp > 51 {
                    return Err(StageError::invalid(
                        "H265_NVENC depth qp must be between 0 and 51",
                    ));
                }
                validate_nvenc_b_frames("H265_NVENC depth", *b_frames)?;
                validate_nvenc_rc_lookahead("H265_NVENC depth", *rc_lookahead)?;
                Ok(())
            }
            DepthCodecConfig::Av1Nvenc {
                qp,
                b_frames,
                rc_lookahead,
                ..
            } => {
                if *qp > 255 {
                    return Err(StageError::invalid(
                        "AV1_NVENC depth qp must be between 0 and 255",
                    ));
                }
                validate_nvenc_b_frames("AV1_NVENC depth", *b_frames)?;
                validate_nvenc_rc_lookahead("AV1_NVENC depth", *rc_lookahead)?;
                Ok(())
            }
            DepthCodecConfig::Ffv1 => Ok(()),
        }
    }
}

fn default_depth_max_mm() -> u16 {
    4092
}

fn default_depth_fps() -> u32 {
    30
}

fn default_depth_hevc_qp() -> u32 {
    18
}

fn default_depth_nvenc_h265_qp() -> u32 {
    DEFAULT_DEPTH_NVENC_H265_QP
}

fn default_depth_nvenc_av1_qp() -> u32 {
    DEFAULT_DEPTH_NVENC_AV1_QP
}

fn default_depth_av1_gq() -> u32 {
    35
}

fn default_depth_av1_crf() -> u32 {
    4
}

fn default_depth_av1_preset() -> u32 {
    4
}

fn default_depth_nvenc_b_frames() -> u32 {
    DEFAULT_NVENC_B_FRAMES
}

// ============================================================================
// Pipeline Stage
// ============================================================================

#[typetag::serde(name = "DepthVideoConfig")]
impl StageConfig for DepthVideoConfig {
    /// Builds a depth video encoder stage from this configuration.
    fn build(&self) -> Box<dyn Stage> {
        Box::new(DepthVideoEncoder::new(self.clone()))
    }
}

/// Pipeline stage that encodes depth frames into video files.
///
/// Wraps [`encode_depth_videos()`] as a [`Stage`] so it can be used in
/// `rebake-cli run` pipelines. Follows the same pattern as [`VideoEncoder`]:
/// reads `depth_data` from context, writes video files to `video_cache_dir/{uuid}/videos/`,
/// and stores typed artifacts in `context.video_registry`.
///
/// # Preconditions
///
/// - `airoa_metadata`: **Required** (for UUID-based output directory)
/// - `depth_data`: Conditional (if missing, stage returns early with no action)
///
/// # Postconditions
///
/// - `depth_data`: Preserved
/// - `video_cache_dir`: Set to `{base_video_cache_dir}/{uuid}`
/// - `bundle_root`: Set to `{base_video_cache_dir}/{uuid}`
/// - `video_registry`: Extended with depth video artifacts
///
/// [`VideoEncoder`]: crate::encode::video_encoder::VideoEncoder
pub struct DepthVideoEncoder {
    config: DepthVideoConfig,
}

impl DepthVideoEncoder {
    pub fn new(config: DepthVideoConfig) -> Self {
        Self { config }
    }
}

impl Stage for DepthVideoEncoder {
    /// Returns the stable stage name used in pipeline logs and configs.
    fn name(&self) -> &'static str {
        "depth_video_encoder"
    }

    /// Encodes any depth topics in the context and stores the output video artifacts.
    fn run(&mut self, mut context: Context) -> Result<Context, StageError> {
        // Determine base video_cache_dir from context (same as VideoEncoder)
        let base_video_cache_dir = context
            .video_cache_dir()
            .cloned()
            .unwrap_or_else(|| Utf8PathBuf::from("./video_cache"));

        let base_video_cache_dir = if base_video_cache_dir.is_relative() {
            let current_dir = std::env::current_dir()
                .map_err(|e| StageError::invalid_with("failed to get current directory", e))?;
            Utf8PathBuf::try_from(current_dir)
                .map_err(|e| StageError::invalid_with("current directory is not valid UTF-8", e))?
                .join(&base_video_cache_dir)
        } else {
            base_video_cache_dir
        };

        let uuid = context
            .airoa_metadata()
            .map(|m| m.uuid_string())
            .or_missing("airoa_metadata in context (did Rosbag2Ingestor load meta.json?)")?;

        let video_cache_dir = base_video_cache_dir.join(&uuid);

        let depth_data = match context.depth_data.take() {
            Some(data) => data,
            None => {
                context.set_video_cache_dir(video_cache_dir);
                return Ok(context);
            }
        };

        let videos_dir = video_cache_dir.join("videos");
        let depth_video_artifacts = encode_depth_videos(&depth_data, &videos_dir, &self.config)?;

        let mut video_registry = context.video_registry.take().unwrap_or_default();
        video_registry.extend(depth_video_artifacts);

        context.set_depth_data(depth_data);
        context.set_video_cache_dir(video_cache_dir.clone());
        context.set_bundle_root(video_cache_dir);
        if !video_registry.is_empty() {
            context.set_video_registry(video_registry);
        }

        Ok(context)
    }
}

// ============================================================================
// Encoding Pipeline
// ============================================================================

/// Encodes depth video files from `compressedDepth` frames.
///
/// For each topic in `depth_data`, decodes the ROS compressedDepth payload,
/// quantizes (lossy) or passes through (lossless), and pipes raw frames
/// to an FFmpeg subprocess.
///
/// # Returns
///
/// A mapping of topic name → video artifact with relative path (for example,
/// `"videos/depth.mp4"`).
///
/// # Errors
///
/// Returns an error if decoding, quantization, or FFmpeg encoding fails.
pub fn encode_depth_videos(
    depth_data: &HashMap<String, Vec<DepthFrame>>,
    videos_dir: &Utf8Path,
    config: &DepthVideoConfig,
) -> StageResult<HashMap<String, VideoArtifact>> {
    let mut video_artifacts = HashMap::new();

    for (topic_name, frames) in depth_data {
        if frames.is_empty() {
            continue;
        }

        let sanitized = topic_name_to_flat_file_stem(topic_name);
        let ext = config.codec_config.video_extension();
        let video_filename = format!("{sanitized}.{ext}");
        let output_path = videos_dir.join(&video_filename);

        tracing::debug!(
            "Encoding {} depth frames for topic {} to {}",
            frames.len(),
            topic_name,
            output_path
        );

        let (width, height) = encode_single_topic(frames, &output_path, config)?;

        let relative_path = format!("videos/{video_filename}");
        let artifact = config.video_artifact(relative_path, width, height)?;
        video_artifacts.insert(topic_name.clone(), artifact);
    }

    Ok(video_artifacts)
}

/// Encodes all frames for a single depth topic into a video file.
fn encode_single_topic(
    frames: &[DepthFrame],
    output_path: &Utf8Path,
    config: &DepthVideoConfig,
) -> StageResult<(u32, u32)> {
    config.preflight()?;

    // Decode first frame to get dimensions
    let first_frame = decode_depth_frame(&frames[0])?;
    let width = first_frame.width;
    let height = first_frame.height;

    let is_lossless = config.codec_config.is_lossless();
    let params = if is_lossless {
        None
    } else {
        Some(Q10ClipParams::new(config.depth_max_mm))
    };

    // Spawn FFmpeg subprocess
    let mut process = spawn_ffmpeg(width, height, config, output_path)?;

    // Process first frame
    write_frame(&mut process, &first_frame, is_lossless, params.as_ref())?;

    // Process remaining frames
    for frame in &frames[1..] {
        let decoded = decode_depth_frame(frame)?;
        if decoded.width != width || decoded.height != height {
            return Err(StageError::invalid(format!(
                "Depth frame dimension mismatch: expected {width}x{height}, got {}x{}",
                decoded.width, decoded.height
            )));
        }
        write_frame(&mut process, &decoded, is_lossless, params.as_ref())?;
    }

    process.finish()?;

    tracing::info!("Depth encoded {} frames to {}", frames.len(), output_path);

    Ok((width, height))
}

/// Writes a single decoded depth frame to the FFmpeg stdin pipe.
///
/// For lossy codecs: quantize to Q10 → convert to P010LE → write.
/// For FFV1 lossless: write raw gray16le bytes directly.
fn write_frame(
    process: &mut FfmpegSubprocess,
    frame: &DecodedDepthFrame,
    is_lossless: bool,
    params: Option<&Q10ClipParams>,
) -> StageResult<()> {
    if is_lossless {
        // FFV1: pipe raw gray16le directly
        let raw_gray16le = frame.gray16le_bytes();
        process.write_all(&raw_gray16le, "failed to write gray16le depth frame")?;
    } else {
        let params = params.ok_or_else(|| {
            StageError::invalid("internal error: lossy depth encoding requires Q10Clip parameters")
        })?;
        // Lossy: quantize → P010LE
        let pixel_count = frame.values_mm.len();
        let mut quantized = vec![0u16; pixel_count];
        let _clipped = quantize_frame(&frame.values_mm, params, &mut quantized);

        let p010_bytes = q10_to_p010le(&quantized, frame.width, frame.height);
        process.write_all(&p010_bytes, "failed to write P010LE depth frame")?;
    }
    Ok(())
}

/// Spawns an FFmpeg subprocess configured for the given depth codec.
fn spawn_ffmpeg(
    width: u32,
    height: u32,
    config: &DepthVideoConfig,
    output_path: &Utf8Path,
) -> StageResult<FfmpegSubprocess> {
    // Create output directory if needed
    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent.as_std_path()).map_err(|e| {
            StageError::io(format!("failed to create output directory: {parent}"), e)
        })?;
    }

    let size = format!("{width}x{height}");
    let fps_str = config.fps.to_string();
    let gop_str = DEFAULT_DEPTH_GOP_FRAMES.to_string();

    let mut cmd = Command::new("ffmpeg");
    cmd.args(["-hide_banner", "-nostats", "-loglevel", "warning", "-y"]);

    match &config.codec_config {
        DepthCodecConfig::H265Vaapi { qp, device } => {
            let device = device.as_deref().unwrap_or(DEFAULT_VAAPI_DEVICE);
            let qp_str = qp.to_string();
            cmd.args([
                // Input: raw P010LE depth frames via stdin.
                // -color_range pc forces full range [0, 1023] instead of limited [64, 940].
                // Without this, VA-API applies studio range scaling which corrupts depth values.
                "-color_range",
                "pc",
                "-f",
                "rawvideo",
                "-pix_fmt",
                "p010le",
                "-s",
                &size,
                "-framerate",
                &fps_str,
                "-i",
                "pipe:0",
                "-vaapi_device",
                device,
                "-vf",
                "format=p010,hwupload",
                "-c:v",
                "hevc_vaapi",
                "-profile:v",
                "main10",
                "-qp",
                &qp_str,
                "-g",
                &gop_str,
                "-rc_mode",
                "CQP",
                output_path.as_str(),
            ]);
        }
        DepthCodecConfig::Av1Vaapi {
            global_quality,
            device,
        } => {
            let device = device.as_deref().unwrap_or(DEFAULT_VAAPI_DEVICE);
            let gq_str = global_quality.to_string();
            cmd.args([
                // Input: raw P010LE depth frames via stdin.
                // -color_range pc forces full range [0, 1023] instead of limited [64, 940].
                // Without this, VA-API applies studio range scaling which corrupts depth values.
                "-color_range",
                "pc",
                "-f",
                "rawvideo",
                "-pix_fmt",
                "p010le",
                "-s",
                &size,
                "-framerate",
                &fps_str,
                "-i",
                "pipe:0",
                "-vaapi_device",
                device,
                "-vf",
                "format=p010,hwupload",
                "-c:v",
                "av1_vaapi",
                "-profile:v",
                "main",
                "-global_quality",
                &gq_str,
                "-g",
                &gop_str,
                "-rc_mode",
                "CQP",
                output_path.as_str(),
            ]);
        }
        DepthCodecConfig::H265Nvenc {
            qp,
            gpu,
            preset,
            tune,
            b_frames,
            rc_lookahead,
        } => {
            let qp_str = qp.to_string();
            let b_frames_str = b_frames.to_string();
            cmd.args([
                "-color_range",
                "pc",
                "-f",
                "rawvideo",
                "-pix_fmt",
                "p010le",
                "-s",
                &size,
                "-framerate",
                &fps_str,
                "-i",
                "pipe:0",
                "-c:v",
                "hevc_nvenc",
                "-profile:v",
                "main10",
                "-pix_fmt",
                "p010le",
                "-color_range",
                "pc",
                "-rc",
                "constqp",
                "-qp",
                &qp_str,
                "-g",
                &gop_str,
                "-bf",
                &b_frames_str,
                "-preset",
                preset.as_str(),
            ]);
            if *b_frames > 0 {
                cmd.args(["-b_ref_mode", "middle"]);
            }
            if let Some(rc_lookahead) = rc_lookahead {
                let rc_lookahead = rc_lookahead.to_string();
                cmd.args(["-rc-lookahead", &rc_lookahead]);
            }
            if let Some(tune) = tune {
                cmd.args(["-tune", tune.as_str()]);
            }
            if let Some(gpu) = gpu {
                cmd.args(["-gpu", &gpu.to_string()]);
            }
            cmd.arg(output_path.as_str());
        }
        DepthCodecConfig::Av1Nvenc {
            qp,
            gpu,
            preset,
            tune,
            b_frames,
            rc_lookahead,
        } => {
            let qp_str = qp.to_string();
            let b_frames_str = b_frames.to_string();
            cmd.args([
                "-color_range",
                "pc",
                "-f",
                "rawvideo",
                "-pix_fmt",
                "p010le",
                "-s",
                &size,
                "-framerate",
                &fps_str,
                "-i",
                "pipe:0",
                "-c:v",
                "av1_nvenc",
                "-pix_fmt",
                "p010le",
                "-color_range",
                "pc",
                "-rc",
                "constqp",
                "-qp",
                &qp_str,
                "-g",
                &gop_str,
                "-bf",
                &b_frames_str,
                "-preset",
                preset.as_str(),
            ]);
            if *b_frames > 0 {
                cmd.args(["-b_ref_mode", "middle"]);
            }
            if let Some(rc_lookahead) = rc_lookahead {
                let rc_lookahead = rc_lookahead.to_string();
                cmd.args(["-rc-lookahead", &rc_lookahead]);
            }
            if let Some(tune) = tune {
                cmd.args(["-tune", tune.as_str()]);
            }
            if let Some(gpu) = gpu {
                cmd.args(["-gpu", &gpu.to_string()]);
            }
            cmd.arg(output_path.as_str());
        }
        DepthCodecConfig::AV1 { crf, preset } => {
            let crf_str = crf.to_string();
            let preset_str = preset.to_string();
            cmd.args([
                "-f",
                "rawvideo",
                "-pix_fmt",
                "p010le",
                "-s",
                &size,
                "-framerate",
                &fps_str,
                "-i",
                "pipe:0",
                "-c:v",
                "libsvtav1",
                "-preset",
                &preset_str,
                "-crf",
                &crf_str,
                "-g",
                &gop_str,
                "-pix_fmt",
                "yuv420p10le",
                output_path.as_str(),
            ]);
        }
        DepthCodecConfig::Ffv1 => {
            cmd.args([
                "-f",
                "rawvideo",
                "-pix_fmt",
                "gray16le",
                "-s",
                &size,
                "-framerate",
                &fps_str,
                "-i",
                "pipe:0",
                "-c:v",
                "ffv1",
                "-level",
                "3",
                "-coder",
                "1",
                "-context",
                "1",
                "-g",
                "1",
                "-slicecrc",
                "1",
                output_path.as_str(),
            ]);
        }
    }

    FfmpegSubprocess::spawn(
        cmd,
        format!("depth {}", config.codec_config.ffmpeg_encoder_name()),
    )
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::encode::compressed_depth::test_support::make_png_depth_frame;
    use tempfile::tempdir;

    fn nvenc_smoke_tests_enabled() -> bool {
        std::env::var_os("REBAKE_RUN_NVENC_TESTS").is_some()
    }

    #[test]
    // Tests that the default codec config is AV1 with the expected values.
    fn depth_codec_config_default_is_av1() {
        let config = DepthCodecConfig::default();
        match config {
            DepthCodecConfig::AV1 { crf, preset } => {
                assert_eq!(crf, 4);
                assert_eq!(preset, 4);
            }
            _ => panic!("default should be AV1"),
        }
        assert!(!config.is_lossless());
    }

    #[test]
    // Tests that the default depth video config uses the documented defaults.
    fn depth_video_config_default_values() {
        let config = DepthVideoConfig::default();
        assert_eq!(config.depth_max_mm, 4092);
        assert_eq!(config.fps, 30);
        assert!(!config.codec_config.is_lossless());
    }

    #[test]
    // Tests that HEVC VA-API settings deserialize correctly from YAML.
    fn depth_codec_config_serde_hevc_vaapi() {
        let yaml = r#"
codec: H265_VAAPI
qp: 14
"#;
        let config: DepthCodecConfig = serde_yaml::from_str(yaml).unwrap();
        match config {
            DepthCodecConfig::H265Vaapi { qp, device } => {
                assert_eq!(qp, 14);
                assert!(device.is_none());
            }
            _ => panic!("expected H265Vaapi"),
        }
    }

    #[test]
    // Tests that AV1 VA-API settings deserialize correctly from YAML.
    fn depth_codec_config_serde_av1_vaapi() {
        let yaml = r#"
codec: AV1_VAAPI
global_quality: 30
"#;
        let config: DepthCodecConfig = serde_yaml::from_str(yaml).unwrap();
        match config {
            DepthCodecConfig::Av1Vaapi {
                global_quality,
                device,
            } => {
                assert_eq!(global_quality, 30);
                assert!(device.is_none());
            }
            _ => panic!("expected Av1Vaapi"),
        }
    }

    #[test]
    // Tests that depth NVENC settings deserialize and validate correctly.
    fn depth_codec_config_serde_nvenc() {
        let yaml = r#"
codec: H265_NVENC
qp: 18
preset: P4
tune: Hq
b_frames: 2
rc_lookahead: 16
"#;
        let config: DepthCodecConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(
            config,
            DepthCodecConfig::H265Nvenc {
                qp: 18,
                gpu: None,
                preset: NvencPreset::P4,
                tune: Some(NvencTune::Hq),
                b_frames: 2,
                rc_lookahead: Some(16),
            }
        );
        assert_eq!(config.ffmpeg_encoder_name(), "hevc_nvenc");
        assert!(config.is_nvenc());
        config.validate().unwrap();
    }

    #[test]
    fn depth_av1_nvenc_uses_measured_default_qp() {
        let config: DepthCodecConfig = serde_yaml::from_str("codec: AV1_NVENC\n").unwrap();
        match config {
            DepthCodecConfig::Av1Nvenc { qp, .. } => {
                assert_eq!(qp, DEFAULT_DEPTH_NVENC_AV1_QP);
            }
            other => panic!("expected AV1_NVENC depth codec config, got {other:?}"),
        }
    }

    #[test]
    fn depth_h265_nvenc_uses_measured_default_qp() {
        let config: DepthCodecConfig = serde_yaml::from_str("codec: H265_NVENC\n").unwrap();
        match config {
            DepthCodecConfig::H265Nvenc { qp, .. } => {
                assert_eq!(qp, DEFAULT_DEPTH_NVENC_H265_QP);
            }
            other => panic!("expected H265_NVENC depth codec config, got {other:?}"),
        }
    }

    #[test]
    #[ignore = "requires NVIDIA GPU, nvidia-container-toolkit/runtime access, and FFmpeg NVENC encoders"]
    fn depth_nvenc_smoke_hevc_and_av1() {
        if !nvenc_smoke_tests_enabled() {
            eprintln!("set REBAKE_RUN_NVENC_TESTS=1 to run NVENC smoke tests");
            return;
        }

        let cases = vec![
            (
                "hevc",
                DepthCodecConfig::H265Nvenc {
                    qp: DEFAULT_DEPTH_NVENC_H265_QP,
                    gpu: None,
                    preset: NvencPreset::P4,
                    tune: None,
                    b_frames: DEFAULT_NVENC_B_FRAMES,
                    rc_lookahead: None,
                },
            ),
            (
                "av1",
                DepthCodecConfig::Av1Nvenc {
                    qp: 80,
                    gpu: None,
                    preset: NvencPreset::P4,
                    tune: None,
                    b_frames: DEFAULT_NVENC_B_FRAMES,
                    rc_lookahead: None,
                },
            ),
        ];

        for (name, codec_config) in cases {
            let temp_dir = tempdir().unwrap();
            let videos_dir =
                Utf8PathBuf::from_path_buf(temp_dir.path().join(format!("videos_{name}"))).unwrap();
            let config = DepthVideoConfig {
                depth_max_mm: 4092,
                codec_config,
                fps: 30,
            };

            let width = 256;
            let height = 256;
            let values: Vec<u16> = (0..(width * height))
                .map(|i| {
                    let depth = (i % 4092) as u16;
                    if depth == 0 { 0 } else { depth }
                })
                .collect();
            let frames = vec![
                make_png_depth_frame(0, width, height, &values),
                make_png_depth_frame(1, width, height, &values),
            ];
            let mut depth_data = HashMap::new();
            depth_data.insert("/camera/depth/image_rect_raw".to_string(), frames);

            let artifacts = encode_depth_videos(&depth_data, &videos_dir, &config)
                .expect("depth NVENC encode should succeed");
            assert_eq!(artifacts.len(), 1);

            let mut outputs = std::fs::read_dir(videos_dir.as_std_path())
                .expect("depth NVENC output directory should exist");
            let output = outputs
                .next()
                .expect("depth NVENC should write one output file")
                .expect("depth NVENC output entry should be readable")
                .path();
            assert!(
                std::fs::metadata(&output).unwrap().len() > 0,
                "depth NVENC output should not be empty: {}",
                output.display()
            );
            assert!(
                outputs.next().is_none(),
                "depth NVENC smoke test should produce exactly one file"
            );
        }
    }

    #[test]
    // Tests that software AV1 settings deserialize correctly from YAML.
    fn depth_codec_config_serde_av1() {
        let yaml = r#"
codec: AV1
crf: 22
preset: 8
"#;
        let config: DepthCodecConfig = serde_yaml::from_str(yaml).unwrap();
        match config {
            DepthCodecConfig::AV1 { crf, preset } => {
                assert_eq!(crf, 22);
                assert_eq!(preset, 8);
            }
            _ => panic!("expected AV1"),
        }
    }

    #[test]
    // Tests that AV1 codec aliases deserialize to the same enum variant.
    fn depth_codec_config_serde_av1_alias() {
        // Verify aliases match RGB CodecConfig exactly
        let yaml = "codec: av1\ncrf: 10\npreset: 6\n";
        let config: DepthCodecConfig = serde_yaml::from_str(yaml).unwrap();
        assert!(matches!(config, DepthCodecConfig::AV1 { .. }));

        let yaml = "codec: Av1\ncrf: 10\npreset: 6\n";
        let config: DepthCodecConfig = serde_yaml::from_str(yaml).unwrap();
        assert!(matches!(config, DepthCodecConfig::AV1 { .. }));
    }

    #[test]
    // Tests that the lowercase HEVC VA-API alias is accepted.
    fn depth_codec_config_serde_h265_vaapi_alias() {
        // Verify h265_vaapi alias works (matching RGB)
        let yaml = "codec: h265_vaapi\nqp: 20\n";
        let config: DepthCodecConfig = serde_yaml::from_str(yaml).unwrap();
        assert!(matches!(config, DepthCodecConfig::H265Vaapi { .. }));
    }

    #[test]
    // Tests that the lowercase AV1 VA-API alias is accepted.
    fn depth_codec_config_serde_av1_vaapi_alias() {
        // Verify av1_vaapi lowercase alias works (matching RGB)
        let yaml = "codec: av1_vaapi\nglobal_quality: 30\n";
        let config: DepthCodecConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(
            config,
            DepthCodecConfig::Av1Vaapi {
                global_quality: 30,
                device: None
            }
        );
    }

    #[test]
    // Tests that FFV1 deserializes correctly from YAML.
    fn depth_codec_config_serde_ffv1() {
        let yaml = "codec: FFV1\n";
        let config: DepthCodecConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config, DepthCodecConfig::Ffv1);
    }

    #[test]
    // Tests that the lowercase FFV1 alias is accepted.
    fn depth_codec_config_serde_ffv1_alias() {
        // Verify ffv1 lowercase alias works
        let yaml = "codec: ffv1\n";
        let config: DepthCodecConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config, DepthCodecConfig::Ffv1);
    }

    #[test]
    // Tests that depth video config survives a YAML roundtrip.
    fn depth_video_config_serde_roundtrip() {
        let config = DepthVideoConfig {
            depth_max_mm: 5000,
            codec_config: DepthCodecConfig::AV1 { crf: 20, preset: 4 },
            fps: 15,
        };

        let yaml = serde_yaml::to_string(&config).unwrap();
        let restored: DepthVideoConfig = serde_yaml::from_str(&yaml).unwrap();

        assert_eq!(restored.depth_max_mm, 5000);
        assert_eq!(restored.fps, 15);
        match restored.codec_config {
            DepthCodecConfig::AV1 { crf, preset } => {
                assert_eq!(crf, 20);
                assert_eq!(preset, 4);
            }
            _ => panic!("expected AV1 after roundtrip"),
        }
    }

    #[test]
    // Tests that omitted config fields fall back to the documented defaults.
    fn depth_video_config_serde_defaults() {
        let yaml = "{}";
        let config: DepthVideoConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.depth_max_mm, 4092);
        assert_eq!(config.fps, 30);
        match config.codec_config {
            DepthCodecConfig::AV1 { crf, preset } => {
                assert_eq!(crf, 4);
                assert_eq!(preset, 4);
            }
            _ => panic!("default should be AV1"),
        }
    }

    #[test]
    // Tests that FFV1 is reported as a lossless codec.
    fn ffv1_is_lossless() {
        assert!(DepthCodecConfig::Ffv1.is_lossless());
        assert!(!DepthCodecConfig::default().is_lossless());
    }

    #[test]
    // Tests that lossy codecs are not reported as lossless.
    fn lossy_codecs_are_not_lossless() {
        assert!(
            !DepthCodecConfig::H265Vaapi {
                qp: 18,
                device: None
            }
            .is_lossless()
        );
        assert!(
            !DepthCodecConfig::Av1Vaapi {
                global_quality: 35,
                device: None
            }
            .is_lossless()
        );
        assert!(!DepthCodecConfig::AV1 { crf: 18, preset: 6 }.is_lossless());
    }

    #[test]
    // Tests that the default codec uses the MP4 file extension.
    fn video_extension_default_mp4() {
        assert_eq!(DepthCodecConfig::default().video_extension(), "mp4");
    }

    #[test]
    // Tests that every lossy codec uses the MP4 file extension.
    fn video_extension_lossy_mp4() {
        assert_eq!(
            DepthCodecConfig::AV1 { crf: 18, preset: 6 }.video_extension(),
            "mp4"
        );
        assert_eq!(
            DepthCodecConfig::H265Vaapi {
                qp: 18,
                device: None
            }
            .video_extension(),
            "mp4"
        );
        assert_eq!(
            DepthCodecConfig::Av1Vaapi {
                global_quality: 35,
                device: None
            }
            .video_extension(),
            "mp4"
        );
    }

    #[test]
    // Tests that FFV1 uses the MKV file extension.
    fn video_extension_ffv1_mkv() {
        assert_eq!(DepthCodecConfig::Ffv1.video_extension(), "mkv");
    }

    #[test]
    // Tests that metadata captures the main video properties.
    fn depth_video_metadata_captures_dimensions_and_codec_fields() {
        let config = DepthVideoConfig {
            depth_max_mm: 4092,
            codec_config: DepthCodecConfig::Av1Vaapi {
                global_quality: 35,
                device: None,
            },
            fps: 24,
        };

        let metadata = config.video_metadata(848, 480).unwrap();

        assert_eq!(metadata.media_type, "depth");
        assert_eq!(metadata.codec_family, "av1");
        assert_eq!(metadata.encoder_name, "av1_vaapi");
        assert_eq!(metadata.pix_fmt, "p010le");
        assert_eq!(metadata.width, 848);
        assert_eq!(metadata.height, 480);
        assert_eq!(metadata.fps, 24);
    }

    #[test]
    // Tests that a video artifact wraps the relative path and metadata together.
    fn depth_video_artifact_wraps_relative_path_and_metadata() {
        let config = DepthVideoConfig::default();

        let artifact = config.video_artifact("videos/depth.mp4", 640, 360).unwrap();

        assert_eq!(artifact.video_path, "videos/depth.mp4");
        assert_eq!(artifact.metadata.media_type, "depth");
        assert_eq!(artifact.metadata.width, 640);
        assert_eq!(artifact.metadata.height, 360);
    }

    #[test]
    // Tests that metadata creation rejects zero-sized dimensions.
    fn depth_video_metadata_rejects_zero_dimensions() {
        let config = DepthVideoConfig::default();

        let error = config.video_metadata(0, 480).unwrap_err();
        assert!(error.reason().contains("positive width and height"));
    }

    #[test]
    // Tests that topic names are converted into safe file names.
    fn sanitize_topic_name_removes_leading_slash() {
        assert_eq!(topic_name_to_flat_file_stem("/depth/image"), "depth__image");
        assert_eq!(topic_name_to_flat_file_stem("depth/image"), "depth__image");
        assert_eq!(topic_name_to_flat_file_stem("/depth"), "depth");
    }

    #[test]
    // Tests that empty depth topics are skipped without creating artifacts.
    fn encode_depth_videos_skips_empty_topics() {
        let mut depth_data = HashMap::new();
        depth_data.insert("/depth".to_string(), vec![]);

        let dir = Utf8Path::new("/tmp/test_depth");
        let config = DepthVideoConfig::default();
        let result = encode_depth_videos(&depth_data, dir, &config).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    // Tests that unsupported 32FC1 depth topics fail with a clear error.
    fn encode_depth_videos_rejects_unsupported_32fc1_topics() {
        let mut frame = DepthFrame::new(0u32, "bin", vec![1, 2, 3, 4]);
        frame.set_ros_format("32FC1; compressedDepth png");

        let mut depth_data = HashMap::new();
        depth_data.insert("/depth".to_string(), vec![frame]);

        let dir = Utf8Path::new("/tmp/test_depth");
        let config = DepthVideoConfig::default();
        let err = encode_depth_videos(&depth_data, dir, &config)
            .expect_err("32FC1 depth should fail with an explicit unsupported error");
        assert!(err.reason().contains("32FC1"));
    }
}
