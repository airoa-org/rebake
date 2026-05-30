use std::fs;
use std::path::Path;
use std::sync::OnceLock;

use crate::common::{ImageShape, resolve_or_infer_image_shape};
use crate::core::error::OptionExt;
use crate::core::stage::{Context, Stage, StageConfig, StageError};
use crate::encode::ffmpeg_cli::ensure_ffmpeg_cli_encoder_available;
use crate::encode::ffmpeg_subprocess::FfmpegSubprocess;
use crate::encode::nvenc::{
    DEFAULT_NVENC_AV1_PRESET, DEFAULT_NVENC_AV1_QP, DEFAULT_NVENC_B_FRAMES,
    DEFAULT_NVENC_H264_B_FRAMES, DEFAULT_NVENC_H264_PRESET, DEFAULT_NVENC_H264_PROFILE,
    DEFAULT_NVENC_H264_QP, DEFAULT_NVENC_H264_RC_LOOKAHEAD, DEFAULT_NVENC_H264_TUNE, NvencPreset,
    NvencTune, ensure_nvenc_device_visible, validate_nvenc_b_frames, validate_nvenc_rc_lookahead,
};
use crate::encode::video_artifact::{VideoArtifact, VideoMetadata};

use camino::{Utf8Path, Utf8PathBuf};
use ffmpeg_next as ffmpeg;
use ffmpeg_next::codec::Flags as CodecFlags;

/// Stores the result of FFmpeg initialization.
/// Uses `Result<(), String>` instead of `Result<(), ffmpeg::Error>` because `ffmpeg::Error`
/// does not implement `Clone`, making it difficult to handle in a `OnceLock`.
static FFMPEG_INIT: OnceLock<Result<(), String>> = OnceLock::new();

/// Returns the result of FFmpeg initialization.
///
/// FFmpeg is initialized exactly once, thread-safely. Subsequent calls return the cached result.
fn get_ffmpeg_init_result() -> &'static Result<(), String> {
    FFMPEG_INIT.get_or_init(|| ffmpeg::init().map_err(|e| e.to_string()))
}

fn ensure_ffmpeg_initialized() -> Result<(), StageError> {
    get_ffmpeg_init_result().as_ref().map_err(|msg| {
        StageError::external(
            format!(
                "FFmpeg initialization failed: {}. Ensure FFmpeg libraries are installed.",
                msg
            ),
            std::io::Error::other(msg.clone()),
        )
    })?;
    Ok(())
}

fn ensure_ffmpeg_encoder_available(encoder_name: &str) -> Result<(), StageError> {
    ensure_ffmpeg_initialized()?;
    ffmpeg::encoder::find_by_name(encoder_name).ok_or_else(|| {
        StageError::invalid(format!(
            "FFmpeg encoder '{}' not found. \
             Ensure the codec library is installed (e.g., libx264-dev for H.264, libx265-dev for H.265)",
            encoder_name
        ))
    })?;
    Ok(())
}

fn copy_rgb_image_to_frame(rgb_img: &image::RgbImage, frame: &mut Video) {
    let stride = frame.stride(0);
    let src_row_bytes = (rgb_img.width() * 3) as usize; // RGB24 = 3 bytes per pixel
    let src_data = rgb_img.as_raw();
    let dst_data = frame.data_mut(0);

    for y in 0..rgb_img.height() as usize {
        let src_start = y * src_row_bytes;
        let dst_start = y * stride;
        dst_data[dst_start..dst_start + src_row_bytes]
            .copy_from_slice(&src_data[src_start..src_start + src_row_bytes]);
    }
}

fn rgb_frame_from_image(rgb_img: &image::RgbImage) -> Video {
    let mut frame = Video::new(Pixel::RGB24, rgb_img.width(), rgb_img.height());
    copy_rgb_image_to_frame(rgb_img, &mut frame);
    frame
}

use ffmpeg_next::ffi::av_rescale_q;
use ffmpeg_next::format::Pixel;
use ffmpeg_next::format::context::Output as OutputContext;
use ffmpeg_next::software::scaling::Flags as ScalingFlags;
use ffmpeg_next::software::scaling::context::Context as ScalingContext;
use ffmpeg_next::util::error::EAGAIN;
use ffmpeg_next::util::frame::video::Video;
use ffmpeg_next::{Dictionary, Rational};
use image::DynamicImage;
use serde::{Deserialize, Serialize};

const DEFAULT_FPS: u32 = 100;
const DEFAULT_GOP: u32 = 20;
const DEFAULT_CRF: &str = "34";

/// Default VA-API device path.
static DEFAULT_VAAPI_DEVICE: &str = "/dev/dri/renderD128";

/// Check if VA-API hardware acceleration is available.
///
/// Returns true if the VA-API device exists on the system.
pub fn is_vaapi_available() -> bool {
    Path::new(DEFAULT_VAAPI_DEVICE).exists()
}

// ============================================================================
// Image Format Detection for JPEG Pipe Optimization
// ============================================================================

/// Input mode for the VA-API video encoder pipe.
///
/// Determined on the first frame based on image format.
#[derive(Clone, Debug, PartialEq)]
pub enum InputMode {
    /// Mode not yet determined (waiting for first frame).
    Unknown,
    /// Direct pipe mode for JPEG/PNG (uses image2pipe).
    /// Much faster as it avoids Rust-side image decoding.
    DirectPipe { format: ImageFormat },
    /// Raw RGB24 pipe mode (uses rawvideo).
    /// Fallback for unsupported formats.
    RgbPipe,
}

/// Supported image formats for direct pipe mode.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ImageFormat {
    Jpeg,
    Png,
}

impl ImageFormat {
    /// Detects image format from raw data using magic bytes.
    ///
    /// Returns `Some(format)` for supported formats, `None` otherwise.
    ///
    /// TODO: This function uses magic byte detection as a workaround. Ideally, the format
    /// should be passed from `ImageFrame.extension` (which is derived from ROS
    /// CompressedImage.format field) instead of detecting it from raw bytes. This would
    /// require changing `add_data(&[u8])` to `add_data(&[u8], &str)` and updating all
    /// call sites.
    pub fn detect(data: &[u8]) -> Option<Self> {
        if data.len() < 4 {
            return None;
        }

        // JPEG: starts with FF D8 FF
        if data.len() >= 3 && data[0] == 0xFF && data[1] == 0xD8 && data[2] == 0xFF {
            return Some(ImageFormat::Jpeg);
        }

        // PNG: starts with 89 50 4E 47 0D 0A 1A 0A
        if data.len() >= 8 && data[0..8] == [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A] {
            return Some(ImageFormat::Png);
        }

        None
    }

    /// Detects image format from file extension string.
    ///
    /// This is preferred when the extension is known from ROS metadata.
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext.to_lowercase().as_str() {
            "jpg" | "jpeg" => Some(ImageFormat::Jpeg),
            "png" => Some(ImageFormat::Png),
            _ => None,
        }
    }

    /// Returns the FFmpeg decoder name for image2pipe input.
    pub fn ffmpeg_decoder(&self) -> &'static str {
        match self {
            ImageFormat::Jpeg => "mjpeg",
            ImageFormat::Png => "png",
        }
    }
}

// ============================================================================
// Codec Configuration Types
// ============================================================================

/// Codec-specific configuration.
///
/// Each variant contains parameters specific to that codec.
/// Use `codec: "AV1"` (or `av1`, `Av1`) in YAML to select a codec.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(tag = "codec", deny_unknown_fields)]
pub enum CodecConfig {
    /// AV1 codec via SVT-AV1. Best compression, slowest software decode.
    #[serde(rename = "AV1", alias = "av1", alias = "Av1")]
    AV1 {
        /// SVT-AV1 level of parallelism (0=auto, 1-6=explicit level).
        #[serde(default)]
        lp: Option<u32>,

        /// SVT-AV1 CPU pinning (0=disabled, N=pin to first N cores).
        #[serde(default)]
        pin: Option<u32>,

        /// SVT-AV1 preset (0-13, lower=better quality/slower). Default: 10.
        #[serde(default = "default_av1_preset")]
        preset: u32,

        /// Film grain synthesis level (0=off, 1-50=synthesis level).
        /// Useful for restoring grain that was removed during encoding.
        /// Recommended: 8 for live-action, 4-6 for animation.
        #[serde(default, rename = "film-grain")]
        film_grain: Option<u32>,

        /// Apply denoising when film grain is enabled.
        /// When true, denoising level is set by film-grain parameter.
        #[serde(default, rename = "film-grain-denoise")]
        film_grain_denoise: Option<bool>,

        /// Number of frames to look ahead for encoding decisions (-1=auto, 0-120).
        /// Higher values improve quality but increase latency and memory.
        #[serde(default)]
        lookahead: Option<i32>,

        /// Fast decode optimization level (0=off, 1-2=optimization level).
        /// Level 2 yields faster decoder speed but may reduce quality.
        #[serde(default, rename = "fast-decode")]
        fast_decode: Option<u32>,
    },

    /// H.264/AVC codec via libx264. Fastest decode, good compatibility.
    #[serde(rename = "H264", alias = "h264", alias = "H.264")]
    H264 {
        /// Thread count (0=auto, N=limit to N threads).
        #[serde(default)]
        threads: Option<u32>,

        /// x264 preset (ultrafast to veryslow). Default: Medium.
        #[serde(default)]
        preset: X264Preset,

        /// Tuning options for specific content types or use cases.
        /// Multiple tunings can be specified, but only one PSY tuning is allowed.
        #[serde(default)]
        tune: Vec<X264Tune>,
    },

    /// H.265/HEVC codec via libx265. Good compression, medium decode speed.
    #[serde(rename = "H265", alias = "h265", alias = "H.265")]
    H265 {
        /// Thread count (0=auto, N=limit to N threads).
        #[serde(default)]
        threads: Option<u32>,

        /// x265 preset (ultrafast to veryslow). Default: Medium.
        #[serde(default)]
        preset: X264Preset,

        /// Tuning options for specific content types or use cases.
        /// Multiple tunings can be specified, but only one PSY tuning is allowed.
        #[serde(default)]
        tune: Vec<X265Tune>,

        /// Frame-level parallelism threads.
        /// If not specified, x265 decides automatically based on system resources.
        #[serde(default, rename = "frame-threads")]
        frame_threads: Option<u32>,
    },

    // =========================================================================
    // VA-API Hardware Encoders (AMD VCN / Intel QSV)
    // =========================================================================
    /// H.264/AVC codec via VA-API hardware acceleration.
    /// Requires VA-API compatible hardware (AMD VCN or Intel QSV).
    #[serde(rename = "H264_VAAPI", alias = "h264_vaapi")]
    H264Vaapi {
        /// Quantization parameter (0-51, lower = better quality). Default: 21.
        #[serde(default = "default_vaapi_qp")]
        qp: u32,

        /// VA-API device path. Default: /dev/dri/renderD128.
        #[serde(default)]
        device: Option<String>,

        /// Encoder profile (constrained_baseline, main, high). Default: high.
        #[serde(default = "default_h264_vaapi_profile")]
        profile: Option<String>,

        /// B-frame depth (0-7). Only supported on some hardware.
        #[serde(default, rename = "b-depth")]
        b_depth: Option<u32>,

        /// Async depth for parallel encoding (1-64). Default: 16.
        #[serde(default = "default_h264_vaapi_async_depth", rename = "async-depth")]
        async_depth: Option<u32>,
    },

    /// H.265/HEVC codec via VA-API hardware acceleration.
    /// Requires VA-API compatible hardware (AMD VCN or Intel QSV).
    /// Note: AMD VCN does NOT support B-frames for HEVC.
    #[serde(rename = "H265_VAAPI", alias = "hevc_vaapi", alias = "h265_vaapi")]
    H265Vaapi {
        /// Quantization parameter (0-51, lower = better quality). Default: 29.
        #[serde(default = "default_vaapi_hevc_qp")]
        qp: u32,

        /// VA-API device path. Default: /dev/dri/renderD128.
        #[serde(default)]
        device: Option<String>,

        /// Encoder profile (main, main10).
        #[serde(default)]
        profile: Option<String>,

        /// Async depth for parallel encoding (1-64).
        #[serde(default, rename = "async-depth")]
        async_depth: Option<u32>,
    },

    /// AV1 codec via VA-API hardware acceleration.
    /// Requires AMD VCN 4.0+ (RDNA 3) or Intel Arc.
    #[serde(rename = "AV1_VAAPI", alias = "av1_vaapi")]
    Av1Vaapi {
        /// Quantization parameter (0-255, lower = better quality). Default: 110.
        #[serde(default = "default_vaapi_av1_qp")]
        qp: u32,

        /// VA-API device path. Default: /dev/dri/renderD128.
        #[serde(default)]
        device: Option<String>,

        /// Encoder profile (main).
        #[serde(default)]
        profile: Option<String>,

        /// B-frame depth (0-7).
        #[serde(default, rename = "b-depth")]
        b_depth: Option<u32>,

        /// Async depth for parallel encoding (1-64).
        #[serde(default, rename = "async-depth")]
        async_depth: Option<u32>,
    },

    // =========================================================================
    // NVENC Hardware Encoders (NVIDIA)
    // =========================================================================
    /// H.264/AVC codec via NVIDIA NVENC hardware acceleration.
    #[serde(rename = "H264_NVENC", alias = "h264_nvenc")]
    H264Nvenc {
        /// Quantization parameter (0-51, lower = better quality). Default: 26.
        #[serde(default = "default_nvenc_h264_qp")]
        qp: u32,

        /// NVIDIA GPU index passed to FFmpeg's -gpu option.
        #[serde(default)]
        gpu: Option<u32>,

        /// NVENC preset (P1 fastest, P7 slowest/best compression). Default: P5.
        #[serde(default = "default_nvenc_h264_preset")]
        preset: NvencPreset,

        /// NVENC tune. Default: Hq.
        #[serde(default = "default_nvenc_h264_tune")]
        tune: Option<NvencTune>,

        /// Encoder profile (baseline, main, high). Default: high.
        #[serde(default = "default_nvenc_h264_profile")]
        profile: Option<String>,

        /// Number of B-frames. Default: 1 for the measured H.264 NVENC VMAF>=93 profile.
        #[serde(default = "default_nvenc_h264_b_frames", alias = "b-frames")]
        b_frames: u32,

        /// Number of frames to look ahead for rate control (0-120). Default: 32.
        #[serde(default = "default_nvenc_h264_rc_lookahead", alias = "rc-lookahead")]
        rc_lookahead: Option<u32>,
    },

    /// H.265/HEVC codec via NVIDIA NVENC hardware acceleration.
    #[serde(rename = "H265_NVENC", alias = "hevc_nvenc", alias = "h265_nvenc")]
    H265Nvenc {
        /// Quantization parameter (0-51, lower = better quality). Default: 25.
        #[serde(default = "default_nvenc_hevc_qp")]
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

        /// Encoder profile (main, main10, rext).
        #[serde(default)]
        profile: Option<String>,

        /// Number of B-frames. Default: 0 for frame-indexed packaging.
        #[serde(default = "default_nvenc_b_frames", alias = "b-frames")]
        b_frames: u32,

        /// Number of frames to look ahead for rate control (0-120).
        #[serde(default, alias = "rc-lookahead")]
        rc_lookahead: Option<u32>,
    },

    /// AV1 codec via NVIDIA NVENC hardware acceleration.
    ///
    /// Uses project-local benchmark defaults tuned for VMAF >= 93 on the
    /// UMI RGB dataset family.
    #[serde(rename = "AV1_NVENC", alias = "av1_nvenc")]
    Av1Nvenc {
        /// Quantization parameter (0-255, lower = better quality). Default: 130.
        #[serde(default = "default_nvenc_av1_qp")]
        qp: u32,

        /// NVIDIA GPU index passed to FFmpeg's -gpu option.
        #[serde(default)]
        gpu: Option<u32>,

        /// NVENC preset (P1 fastest, P7 slowest/best compression). Default: P7.
        #[serde(default = "default_nvenc_av1_preset")]
        preset: NvencPreset,

        /// NVENC tune. Omit to use FFmpeg/NVENC defaults.
        #[serde(default)]
        tune: Option<NvencTune>,

        /// Encoder profile (main).
        #[serde(default)]
        profile: Option<String>,

        /// Number of B-frames. Default: 0 for frame-indexed packaging.
        #[serde(default = "default_nvenc_b_frames", alias = "b-frames")]
        b_frames: u32,

        /// Number of frames to look ahead for rate control (0-120).
        #[serde(default, alias = "rc-lookahead")]
        rc_lookahead: Option<u32>,
    },
}

fn default_av1_preset() -> u32 {
    10
}

fn default_vaapi_qp() -> u32 {
    21
}

fn default_h264_vaapi_profile() -> Option<String> {
    Some("high".to_string())
}

fn default_h264_vaapi_async_depth() -> Option<u32> {
    Some(16)
}

fn default_vaapi_hevc_qp() -> u32 {
    29
}

fn default_vaapi_av1_qp() -> u32 {
    // QP=110 achieves VMAF >= 93 across all tested rosbags (min=93.25, mean=93.39)
    // while keeping file size reasonable. Lower values = higher quality but larger files.
    110
}

fn default_nvenc_h264_qp() -> u32 {
    DEFAULT_NVENC_H264_QP
}

fn default_nvenc_h264_preset() -> NvencPreset {
    DEFAULT_NVENC_H264_PRESET
}

fn default_nvenc_h264_tune() -> Option<NvencTune> {
    Some(DEFAULT_NVENC_H264_TUNE)
}

fn default_nvenc_h264_profile() -> Option<String> {
    Some(DEFAULT_NVENC_H264_PROFILE.to_string())
}

fn default_nvenc_h264_b_frames() -> u32 {
    DEFAULT_NVENC_H264_B_FRAMES
}

fn default_nvenc_h264_rc_lookahead() -> Option<u32> {
    Some(DEFAULT_NVENC_H264_RC_LOOKAHEAD)
}

fn default_nvenc_hevc_qp() -> u32 {
    25
}

fn default_nvenc_av1_qp() -> u32 {
    DEFAULT_NVENC_AV1_QP
}

fn default_nvenc_av1_preset() -> NvencPreset {
    DEFAULT_NVENC_AV1_PRESET
}

fn default_nvenc_b_frames() -> u32 {
    DEFAULT_NVENC_B_FRAMES
}

impl Default for CodecConfig {
    fn default() -> Self {
        CodecConfig::AV1 {
            lp: None,
            pin: None,
            preset: 10,
            film_grain: None,
            film_grain_denoise: None,
            lookahead: None,
            fast_decode: None,
        }
    }
}

impl CodecConfig {
    /// Returns the FFmpeg encoder library name.
    pub fn ffmpeg_encoder_name(&self) -> &'static str {
        match self {
            CodecConfig::AV1 { .. } => "libsvtav1",
            CodecConfig::H264 { .. } => "libx264",
            CodecConfig::H265 { .. } => "libx265",
            CodecConfig::H264Vaapi { .. } => "h264_vaapi",
            CodecConfig::H265Vaapi { .. } => "hevc_vaapi",
            CodecConfig::Av1Vaapi { .. } => "av1_vaapi",
            CodecConfig::H264Nvenc { .. } => "h264_nvenc",
            CodecConfig::H265Nvenc { .. } => "hevc_nvenc",
            CodecConfig::Av1Nvenc { .. } => "av1_nvenc",
        }
    }

    /// Returns true if this codec uses VA-API hardware acceleration.
    pub fn is_vaapi(&self) -> bool {
        matches!(
            self,
            CodecConfig::H264Vaapi { .. }
                | CodecConfig::H265Vaapi { .. }
                | CodecConfig::Av1Vaapi { .. }
        )
    }

    /// Returns true if this codec uses NVIDIA NVENC hardware acceleration.
    pub fn is_nvenc(&self) -> bool {
        matches!(
            self,
            CodecConfig::H264Nvenc { .. }
                | CodecConfig::H265Nvenc { .. }
                | CodecConfig::Av1Nvenc { .. }
        )
    }

    /// Returns true if this codec is encoded through an FFmpeg CLI subprocess.
    pub fn uses_ffmpeg_cli_encoder(&self) -> bool {
        self.is_vaapi() || self.is_nvenc()
    }

    /// Returns a codec family name suitable for canonical video metadata.
    pub fn codec_family_name(&self) -> &'static str {
        match self {
            CodecConfig::AV1 { .. }
            | CodecConfig::Av1Vaapi { .. }
            | CodecConfig::Av1Nvenc { .. } => "av1",
            CodecConfig::H264 { .. }
            | CodecConfig::H264Vaapi { .. }
            | CodecConfig::H264Nvenc { .. } => "h264",
            CodecConfig::H265 { .. }
            | CodecConfig::H265Vaapi { .. }
            | CodecConfig::H265Nvenc { .. } => "h265",
        }
    }

    fn vaapi_device_path(&self) -> Option<&str> {
        match self {
            CodecConfig::H264Vaapi { device, .. }
            | CodecConfig::H265Vaapi { device, .. }
            | CodecConfig::Av1Vaapi { device, .. } => {
                Some(device.as_deref().unwrap_or(DEFAULT_VAAPI_DEVICE))
            }
            _ => None,
        }
    }

    fn nvenc_b_frames(&self) -> Option<u32> {
        match self {
            CodecConfig::H264Nvenc { b_frames, .. }
            | CodecConfig::H265Nvenc { b_frames, .. }
            | CodecConfig::Av1Nvenc { b_frames, .. } => Some(*b_frames),
            _ => None,
        }
    }

    /// Validates the codec configuration.
    ///
    /// Checks for invalid combinations such as multiple PSY tunings.
    pub fn validate(&self) -> Result<(), StageError> {
        match self {
            CodecConfig::AV1 {
                film_grain,
                lookahead,
                fast_decode,
                ..
            } => {
                if let Some(fg) = film_grain
                    && *fg > 50
                {
                    return Err(StageError::invalid(
                        "AV1 film-grain must be between 0 and 50",
                    ));
                }
                if let Some(la) = lookahead
                    && (*la < -1 || *la > 120)
                {
                    return Err(StageError::invalid(
                        "AV1 lookahead must be -1 (auto) or between 0 and 120",
                    ));
                }
                if let Some(fd) = fast_decode
                    && *fd > 2
                {
                    return Err(StageError::invalid(
                        "AV1 fast-decode must be between 0 and 2",
                    ));
                }
                Ok(())
            }
            CodecConfig::H264 { tune, .. } => validate_x264_tune(tune),
            CodecConfig::H265 { tune, .. } => validate_x265_tune(tune),
            CodecConfig::H264Vaapi { qp, .. } => {
                if *qp > 51 {
                    return Err(StageError::invalid(
                        "H264_VAAPI qp must be between 0 and 51",
                    ));
                }
                Ok(())
            }
            CodecConfig::H265Vaapi { qp, .. } => {
                if *qp > 51 {
                    return Err(StageError::invalid(
                        "H265_VAAPI qp must be between 0 and 51",
                    ));
                }
                Ok(())
            }
            CodecConfig::Av1Vaapi { qp, .. } => {
                if *qp > 255 {
                    return Err(StageError::invalid(
                        "AV1_VAAPI qp must be between 0 and 255",
                    ));
                }
                Ok(())
            }
            CodecConfig::H264Nvenc {
                qp,
                b_frames,
                rc_lookahead,
                ..
            } => {
                if *qp > 51 {
                    return Err(StageError::invalid(
                        "H264_NVENC qp must be between 0 and 51",
                    ));
                }
                validate_nvenc_b_frames("H264_NVENC", *b_frames)?;
                validate_nvenc_rc_lookahead("H264_NVENC", *rc_lookahead)?;
                Ok(())
            }
            CodecConfig::H265Nvenc {
                qp,
                b_frames,
                rc_lookahead,
                ..
            } => {
                if *qp > 51 {
                    return Err(StageError::invalid(
                        "H265_NVENC qp must be between 0 and 51",
                    ));
                }
                validate_nvenc_b_frames("H265_NVENC", *b_frames)?;
                validate_nvenc_rc_lookahead("H265_NVENC", *rc_lookahead)?;
                Ok(())
            }
            CodecConfig::Av1Nvenc {
                qp,
                b_frames,
                rc_lookahead,
                ..
            } => {
                if *qp > 255 {
                    return Err(StageError::invalid(
                        "AV1_NVENC qp must be between 0 and 255",
                    ));
                }
                validate_nvenc_b_frames("AV1_NVENC", *b_frames)?;
                validate_nvenc_rc_lookahead("AV1_NVENC", *rc_lookahead)?;
                Ok(())
            }
        }
    }
}

/// Validates x264 tune combinations.
///
/// Only one PSY tuning (film, animation, grain, stillimage, psnr, ssim) is allowed.
/// Non-PSY tunings (fastdecode, zerolatency) can be combined with one PSY tuning.
fn validate_x264_tune(tunes: &[X264Tune]) -> Result<(), StageError> {
    let psy_count = tunes.iter().filter(|t| t.is_psy()).count();
    if psy_count > 1 {
        let psy_tunes: Vec<_> = tunes
            .iter()
            .filter(|t| t.is_psy())
            .map(|t| t.as_str())
            .collect();
        return Err(StageError::invalid(format!(
            "x264 allows only one PSY tuning, but {} were specified: [{}]. \
             PSY tunings are: film, animation, grain, stillimage, psnr, ssim. \
             Non-PSY tunings (fastdecode, zerolatency) can be combined with one PSY tuning.",
            psy_count,
            psy_tunes.join(", ")
        )));
    }
    Ok(())
}

/// Validates x265 tune combinations.
///
/// Only one PSY tuning (psnr, ssim, grain, animation) is allowed.
/// Non-PSY tunings (fastdecode, zerolatency) can be combined with one PSY tuning.
fn validate_x265_tune(tunes: &[X265Tune]) -> Result<(), StageError> {
    let psy_count = tunes.iter().filter(|t| t.is_psy()).count();
    if psy_count > 1 {
        let psy_tunes: Vec<_> = tunes
            .iter()
            .filter(|t| t.is_psy())
            .map(|t| t.as_str())
            .collect();
        return Err(StageError::invalid(format!(
            "x265 allows only one PSY tuning, but {} were specified: [{}]. \
             PSY tunings are: psnr, ssim, grain, animation. \
             Non-PSY tunings (fastdecode, zerolatency) can be combined with one PSY tuning.",
            psy_count,
            psy_tunes.join(", ")
        )));
    }
    Ok(())
}

/// Encoder preset for x264/x265 codecs.
///
/// Controls the speed vs compression tradeoff.
/// Faster presets encode quickly but produce larger files.
///
/// # Recommendations
///
/// - **Development/Testing**: `Fast` or `Veryfast` for quick iteration
/// - **Production**: `Medium` (default) for balanced results
/// - **Final Archive**: `Slow` or `Slower` for best compression
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum X264Preset {
    #[serde(alias = "Ultrafast")]
    Ultrafast,
    #[serde(alias = "Superfast")]
    Superfast,
    #[serde(alias = "Veryfast")]
    Veryfast,
    #[serde(alias = "Faster")]
    Faster,
    #[serde(alias = "Fast")]
    Fast,
    #[default]
    #[serde(alias = "Medium")]
    Medium,
    #[serde(alias = "Slow")]
    Slow,
    #[serde(alias = "Slower")]
    Slower,
    #[serde(alias = "Veryslow")]
    Veryslow,
}

impl X264Preset {
    /// Returns the FFmpeg preset string.
    pub fn as_str(&self) -> &'static str {
        match self {
            X264Preset::Ultrafast => "ultrafast",
            X264Preset::Superfast => "superfast",
            X264Preset::Veryfast => "veryfast",
            X264Preset::Faster => "faster",
            X264Preset::Fast => "fast",
            X264Preset::Medium => "medium",
            X264Preset::Slow => "slow",
            X264Preset::Slower => "slower",
            X264Preset::Veryslow => "veryslow",
        }
    }
}

/// Tuning options for x264 (H.264) encoder.
///
/// Tunes the encoder for specific content types or use cases.
/// Multiple tunings can be combined, but only one PSY tuning is allowed.
///
/// # PSY Tunings (mutually exclusive)
/// - `Film`, `Animation`, `Grain`, `StillImage`, `Psnr`, `Ssim`
///
/// # Non-PSY Tunings (can combine with one PSY tuning)
/// - `FastDecode`, `ZeroLatency`
///
/// # Example
/// ```yaml
/// tune: [Film, FastDecode]  # Valid: one PSY + one non-PSY
/// tune: [FastDecode]        # Valid: non-PSY only
/// tune: [Film, Grain]       # Invalid: two PSY tunings
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum X264Tune {
    /// High-quality movie content. Lowers deblocking.
    #[serde(alias = "Film")]
    Film,
    /// Cartoon/anime with large flat areas. Boosts deblocking, doubles ref frames.
    #[serde(alias = "Animation")]
    Animation,
    /// Preserve film grain. Disables DCT decimation, adjusts deadzone.
    #[serde(alias = "Grain")]
    Grain,
    /// Still image encoding. Lowers deblocking further.
    #[serde(rename = "stillimage", alias = "StillImage")]
    StillImage,
    /// Optimize for PSNR metric (benchmarking).
    #[serde(alias = "Psnr")]
    Psnr,
    /// Optimize for SSIM metric (benchmarking).
    #[serde(alias = "Ssim")]
    Ssim,
    /// Faster decoding. Disables CABAC, deblocking, and weighted prediction.
    #[serde(rename = "fastdecode", alias = "FastDecode")]
    FastDecode,
    /// Zero latency streaming. Disables B-frames, lookahead, and mbtree.
    #[serde(rename = "zerolatency", alias = "ZeroLatency")]
    ZeroLatency,
}

impl X264Tune {
    /// Returns the FFmpeg tune string.
    pub fn as_str(&self) -> &'static str {
        match self {
            X264Tune::Film => "film",
            X264Tune::Animation => "animation",
            X264Tune::Grain => "grain",
            X264Tune::StillImage => "stillimage",
            X264Tune::Psnr => "psnr",
            X264Tune::Ssim => "ssim",
            X264Tune::FastDecode => "fastdecode",
            X264Tune::ZeroLatency => "zerolatency",
        }
    }

    /// Returns true if this is a PSY (psychovisual) tuning.
    /// Only one PSY tuning can be used at a time.
    pub fn is_psy(&self) -> bool {
        matches!(
            self,
            X264Tune::Film
                | X264Tune::Animation
                | X264Tune::Grain
                | X264Tune::StillImage
                | X264Tune::Psnr
                | X264Tune::Ssim
        )
    }
}

/// Tuning options for x265 (H.265/HEVC) encoder.
///
/// Similar to x264 tunings but with some differences in available options.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum X265Tune {
    /// Optimize for PSNR metric. Disables adaptive quant, psy-rd, cutree.
    #[serde(alias = "Psnr")]
    Psnr,
    /// Optimize for SSIM metric. Enables adaptive quant auto-mode, disables psy-rd.
    #[serde(alias = "Ssim")]
    Ssim,
    /// Preserve film grain. Minimizes QP fluctuations, retains high-frequency components.
    #[serde(alias = "Grain")]
    Grain,
    /// Faster decoding. Disables loop filters, SAO, B-intra.
    #[serde(rename = "fastdecode", alias = "FastDecode")]
    FastDecode,
    /// Zero latency streaming. Removes B-frames and lookahead.
    #[serde(rename = "zerolatency", alias = "ZeroLatency")]
    ZeroLatency,
    /// Optimized for animated content. Adjusts psy parameters and deblocking.
    #[serde(alias = "Animation")]
    Animation,
}

impl X265Tune {
    /// Returns the FFmpeg tune string.
    pub fn as_str(&self) -> &'static str {
        match self {
            X265Tune::Psnr => "psnr",
            X265Tune::Ssim => "ssim",
            X265Tune::Grain => "grain",
            X265Tune::FastDecode => "fastdecode",
            X265Tune::ZeroLatency => "zerolatency",
            X265Tune::Animation => "animation",
        }
    }

    /// Returns true if this is a PSY (psychovisual) tuning.
    /// Only one PSY tuning can be used at a time.
    pub fn is_psy(&self) -> bool {
        matches!(
            self,
            X265Tune::Psnr | X265Tune::Ssim | X265Tune::Grain | X265Tune::Animation
        )
    }
}

// ============================================================================
// Scaling Flags
// ============================================================================

/// Scaling algorithm for video frame resizing.
///
/// These flags control the interpolation method used when scaling video frames.
/// Quality and performance vary significantly between algorithms.
///
/// # Common Choices
///
/// - [`Bilinear`](ScalingFlag::Bilinear) - Fast, moderate quality
/// - [`Bicubic`](ScalingFlag::Bicubic) - Good balance of speed and quality
/// - [`Lanczos`](ScalingFlag::Lanczos) - High quality, slower
///
/// # Example
///
/// ```rust
/// use rebake::encode::video_encoder::ScalingFlag;
///
/// let scaling = ScalingFlag::Bicubic;
/// ```
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub enum ScalingFlag {
    /// Fast bilinear (lowest quality, fastest)
    FastBilinear,
    /// Bilinear interpolation
    Bilinear,
    /// Bicubic interpolation (recommended default)
    Bicubic,
    /// Bicubic for luma, bilinear for chroma
    Bicublin,
    /// Gaussian blur
    Gauss,
    /// Sinc interpolation
    Sinc,
    /// Lanczos resampling (high quality)
    Lanczos,
    /// Spline interpolation
    Spline,
    /// Vertical chroma drop mask
    SrcVChrDropMask,
    /// Vertical chroma drop shift
    SrcVChrDropShift,
    /// Default parameter
    ParamDefault,
    /// Print scaling info
    PrintInfo,
    /// Full chroma H interpolation
    FullChrHInt,
    /// Full chroma H input
    FullChrHInp,
    /// Direct BGR output
    DirectBgr,
    /// Accurate rounding
    AccurateRnd,
    /// Bit-exact output
    BitExact,
    /// Error diffusion dithering
    ErrorDiffusion,
}

/// Exact output dimensions for encoded videos.
///
/// This deliberately keeps the resize surface small:
/// callers provide the final width and height directly, and rebake
/// always stretches frames to that size without preserving aspect ratio.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct ResizeConfig {
    /// Output width in pixels.
    pub width: u32,
    /// Output height in pixels.
    pub height: u32,
}

impl ResizeConfig {
    fn validate(&self) -> Result<(), StageError> {
        if self.width == 0 || self.height == 0 {
            return Err(StageError::invalid(
                "resize width and height must be greater than 0",
            ));
        }

        if self.width % 2 != 0 || self.height % 2 != 0 {
            return Err(StageError::invalid(
                "resize width and height must be even for yuv420p output",
            ));
        }

        Ok(())
    }

    fn image_shape(&self, channels: usize) -> ImageShape {
        ImageShape::new(self.height as usize, self.width as usize, channels)
    }
}

impl From<ScalingFlag> for ScalingFlags {
    fn from(flag: ScalingFlag) -> Self {
        match flag {
            ScalingFlag::FastBilinear => ScalingFlags::FAST_BILINEAR,
            ScalingFlag::Bilinear => ScalingFlags::BILINEAR,
            ScalingFlag::Bicubic => ScalingFlags::BICUBIC,
            ScalingFlag::Bicublin => ScalingFlags::BICUBLIN,
            ScalingFlag::Gauss => ScalingFlags::GAUSS,
            ScalingFlag::Sinc => ScalingFlags::SINC,
            ScalingFlag::Lanczos => ScalingFlags::LANCZOS,
            ScalingFlag::Spline => ScalingFlags::SPLINE,
            ScalingFlag::SrcVChrDropMask => ScalingFlags::SRC_V_CHR_DROP_MASK,
            ScalingFlag::SrcVChrDropShift => ScalingFlags::SRC_V_CHR_DROP_SHIFT,
            ScalingFlag::ParamDefault => ScalingFlags::PARAM_DEFAULT,
            ScalingFlag::PrintInfo => ScalingFlags::PRINT_INFO,
            ScalingFlag::FullChrHInt => ScalingFlags::FULL_CHR_H_INT,
            ScalingFlag::FullChrHInp => ScalingFlags::FULL_CHR_H_INP,
            ScalingFlag::DirectBgr => ScalingFlags::DIRECT_BGR,
            ScalingFlag::AccurateRnd => ScalingFlags::ACCURATE_RND,
            ScalingFlag::BitExact => ScalingFlags::BITEXACT,
            ScalingFlag::ErrorDiffusion => ScalingFlags::ERROR_DIFFUSION,
        }
    }
}

impl ScalingFlag {
    fn ffmpeg_cli_scale_flag(&self) -> Result<Option<&'static str>, StageError> {
        match self {
            ScalingFlag::FastBilinear => Ok(Some("fast_bilinear")),
            ScalingFlag::Bilinear => Ok(Some("bilinear")),
            ScalingFlag::Bicubic => Ok(Some("bicubic")),
            ScalingFlag::Bicublin => Ok(Some("bicublin")),
            ScalingFlag::Gauss => Ok(Some("gauss")),
            ScalingFlag::Sinc => Ok(Some("sinc")),
            ScalingFlag::Lanczos => Ok(Some("lanczos")),
            ScalingFlag::Spline => Ok(Some("spline")),
            ScalingFlag::ParamDefault => Ok(None),
            ScalingFlag::PrintInfo => Ok(Some("print_info")),
            ScalingFlag::FullChrHInt => Ok(Some("full_chroma_int")),
            ScalingFlag::FullChrHInp => Ok(Some("full_chroma_inp")),
            ScalingFlag::AccurateRnd => Ok(Some("accurate_rnd")),
            ScalingFlag::BitExact => Ok(Some("bitexact")),
            ScalingFlag::ErrorDiffusion => Ok(Some("error_diffusion")),
            ScalingFlag::SrcVChrDropMask
            | ScalingFlag::SrcVChrDropShift
            | ScalingFlag::DirectBgr => Err(StageError::invalid(format!(
                "scaling flag '{self:?}' is not supported by FFmpeg CLI resize filters"
            ))),
        }
    }

    fn validate_ffmpeg_cli_resize_support(&self) -> Result<(), StageError> {
        self.ffmpeg_cli_scale_flag().map(|_| ())
    }
}

/// Configuration for the `VideoEncoder` stage.
///
/// This configures the `VideoEncoder`, which encodes a sequence of images from a topic into a video file.
/// It allows setting the frame rate, Group of Pictures (GOP) size, Constant Rate Factor (CRF) for quality,
/// and the scaling algorithm.
///
/// # Video Output Location
///
/// The encoder saves videos to `{video_cache_dir}/{uuid}/{topic}.mp4`, where:
/// - `video_cache_dir` is obtained from `context.video_cache_dir()`, with fallback to
///   `./video_cache` (relative to current working directory) if not set
/// - `uuid` is obtained from `airoa_metadata` in the context
///
/// This ensures that videos from different rosbags do not conflict with each other.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields, default)]
pub struct VideoEncoderConfig {
    /// The frame rate of the output video.
    pub fps: u32,
    /// The Group of Pictures (GOP) size, which determines the keyframe interval.
    pub gop: u32,
    /// The Constant Rate Factor (CRF), controlling video quality (lower is better).
    ///
    /// Valid ranges differ by codec:
    /// - AV1: 0-63 (default: 34)
    /// - H.264: 0-51 (default: 23)
    /// - H.265: 0-51 (default: 28)
    pub crf: String,
    /// The scaling algorithm to use for resizing frames.
    pub scaling: ScalingFlag,
    /// Optional exact output dimensions for encoded videos.
    ///
    /// When omitted, rebake preserves the source frame size.
    #[serde(default)]
    pub resize: Option<ResizeConfig>,
    /// Codec-specific configuration.
    ///
    /// Defaults to AV1 with auto-detect settings.
    #[serde(default)]
    pub codec_config: CodecConfig,
}

impl VideoEncoderConfig {
    /// Creates a new `VideoEncoderConfig` with a specific frame rate.
    ///
    /// Uses the canonical software AV1 defaults for all other fields.
    pub fn new(fps: u32) -> Self {
        Self {
            fps,
            ..Self::default()
        }
    }

    /// Sets the Group of Pictures (GOP) size.
    pub fn set_gop(mut self, gop: u32) -> Self {
        self.gop = gop;
        self
    }

    /// Sets the Constant Rate Factor (CRF) for quality control.
    pub fn set_crf(mut self, crf: String) -> Self {
        self.crf = crf;
        self
    }

    /// Sets the scaling algorithm for resizing frames.
    pub fn set_scaling(mut self, scaling: ScalingFlag) -> Self {
        self.scaling = scaling;
        self
    }

    /// Sets exact output dimensions for encoded videos.
    pub fn set_resize(mut self, resize: Option<ResizeConfig>) -> Self {
        self.resize = resize;
        self
    }

    /// Sets the codec configuration.
    pub fn set_codec_config(mut self, codec_config: CodecConfig) -> Self {
        self.codec_config = codec_config;
        self
    }

    /// Validate the configuration independent of machine capabilities.
    pub fn validate(&self) -> Result<(), StageError> {
        if self.fps == 0 {
            return Err(StageError::invalid("fps must be greater than 0"));
        }
        if self.gop == 0 {
            return Err(StageError::invalid("gop must be greater than 0"));
        }
        if let Some(resize) = self.resize {
            resize.validate()?;
            if self.codec_config.uses_ffmpeg_cli_encoder() {
                self.scaling.validate_ffmpeg_cli_resize_support()?;
            }
        }
        self.codec_config.validate()?;
        if let Some(b_frames) = self.codec_config.nvenc_b_frames()
            && b_frames > 0
            && self.gop <= b_frames + 1
        {
            return Err(StageError::invalid(format!(
                "NVENC gop must be greater than b_frames + 1 when B-frames are enabled (gop={}, b_frames={})",
                self.gop, b_frames
            )));
        }
        Ok(())
    }

    /// Validate the configuration and ensure the current machine can execute it.
    pub fn preflight(&self) -> Result<(), StageError> {
        self.validate()?;

        if let Some(device) = self.codec_config.vaapi_device_path() {
            if !Path::new(device).exists() {
                return Err(StageError::invalid(format!(
                    "VA-API codec selected but {device} was not found. \
                     Mount /dev/dri or choose a software codec explicitly."
                )));
            }

            return ensure_ffmpeg_cli_encoder_available(self.codec_config.ffmpeg_encoder_name());
        }

        if self.codec_config.is_nvenc() {
            ensure_nvenc_device_visible()?;
            return ensure_ffmpeg_cli_encoder_available(self.codec_config.ffmpeg_encoder_name());
        }

        ensure_ffmpeg_encoder_available(self.codec_config.ffmpeg_encoder_name())
    }

    /// Build canonical metadata for an encoded RGB video.
    ///
    /// This keeps encoding metadata in the encoder module,
    /// so callers do not need to reverse-engineer codec names or pixel formats
    /// from config JSON elsewhere in the stack.
    pub fn video_metadata(&self, width: u32, height: u32) -> Result<VideoMetadata, StageError> {
        self.validate()?;

        if width == 0 || height == 0 {
            return Err(StageError::invalid(
                "video metadata requires positive width and height",
            ));
        }

        let encoding_config_json = serde_json::to_string(self).map_err(|error| {
            StageError::invalid(format!(
                "failed to serialize video config for video metadata: {error}"
            ))
        })?;

        Ok(VideoMetadata {
            media_type: "rgb".to_string(),
            codec_family: self.codec_config.codec_family_name().to_string(),
            encoder_name: self.codec_config.ffmpeg_encoder_name().to_string(),
            pix_fmt: "yuv420p".to_string(),
            width,
            height,
            fps: self.fps,
            encoding_config_json,
        })
    }

    /// Resolve the encoded output dimensions for a frame.
    pub fn output_dimensions(&self, width: u32, height: u32) -> Result<(u32, u32), StageError> {
        if width == 0 || height == 0 {
            return Err(StageError::invalid(
                "output dimensions require positive width and height",
            ));
        }

        if let Some(resize) = self.resize {
            resize.validate()?;
            Ok((resize.width, resize.height))
        } else {
            Ok((width, height))
        }
    }

    /// Resolve the encoded output shape for an RGB image topic.
    pub fn output_shape(&self, shape: ImageShape) -> Result<ImageShape, StageError> {
        let (width, height) = self.output_dimensions(shape.width as u32, shape.height as u32)?;
        Ok(ImageShape::new(
            height as usize,
            width as usize,
            shape.channels,
        ))
    }

    /// Returns the configured output shape when resize is explicit.
    pub fn configured_output_shape(&self, channels: usize) -> Option<ImageShape> {
        self.resize.map(|resize| resize.image_shape(channels))
    }

    /// Build a video artifact with canonical metadata for an encoded RGB video.
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

impl Default for VideoEncoderConfig {
    fn default() -> Self {
        Self {
            fps: DEFAULT_FPS,
            gop: DEFAULT_GOP,
            crf: DEFAULT_CRF.to_string(),
            scaling: ScalingFlag::Bicubic,
            resize: None,
            codec_config: CodecConfig::default(),
        }
    }
}

#[typetag::serde(name = "VideoEncoderConfig")]
impl StageConfig for VideoEncoderConfig {
    fn build(&self) -> Box<dyn Stage> {
        Box::new(VideoEncoder::new(self.clone()))
    }
}

/// A pipeline stage that encodes images from all image topics into MP4 videos.
///
/// This stage iterates through the image data provided in the `Context`, and for each topic,
/// it creates a single MP4 video file. The encoding parameters are specified by the
/// `VideoEncoderConfig`.
///
/// # Preconditions
///
/// - `airoa_metadata`: **Required** (for UUID to create subdirectory)
/// - `image_data`: Conditional (if missing, stage sets `video_cache_dir` and returns early)
/// - `video_cache_dir`: Optional (defaults to `./video_cache`)
///
/// # Postconditions
///
/// - `video_cache_dir`: **Guaranteed** (set to `{base_dir}/{uuid}`)
/// - `bundle_root`: **Guaranteed** (set to `{base_dir}/{uuid}`)
/// - `video_registry`: Conditional (set only if `image_data` is present)
/// - `image_data`: Conditional (preserved only if present)
///
/// # Errors
///
/// - [`StageError::MissingData`]: `airoa_metadata` not set in context
/// - [`StageError::InvalidData`]: Current directory not valid UTF-8, image decode failure
/// - [`StageError::Io`]: Directory creation failure
/// - [`StageError::External`]: FFmpeg initialization failure, encoder errors
pub struct VideoEncoder {
    config: VideoEncoderConfig,
}

impl VideoEncoder {
    pub fn new(config: VideoEncoderConfig) -> Self {
        Self { config }
    }
}

impl Stage for VideoEncoder {
    fn name(&self) -> &'static str {
        "video_encoder"
    }

    fn run(&mut self, mut context: Context) -> Result<Context, StageError> {
        // Determine base video_cache_dir from context
        // Priority: video_cache_dir > ./video_cache (default)
        let base_video_cache_dir = context
            .video_cache_dir()
            .cloned()
            .unwrap_or_else(|| Utf8PathBuf::from("./video_cache"));

        // Convert to absolute path for consistent access from any working directory
        let base_video_cache_dir = if base_video_cache_dir.is_relative() {
            let current_dir = std::env::current_dir()
                .map_err(|e| StageError::invalid_with("failed to get current directory", e))?;
            Utf8PathBuf::try_from(current_dir)
                .map_err(|e| StageError::invalid_with("current directory is not valid UTF-8", e))?
                .join(&base_video_cache_dir)
        } else {
            base_video_cache_dir
        };

        // Get UUID from airoa_metadata and create subdirectory
        let uuid = context
            .airoa_metadata()
            .map(|m| m.uuid_string())
            .or_missing("airoa_metadata in context (did Rosbag2Ingestor load meta.json?)")?;

        let video_cache_dir = base_video_cache_dir.join(&uuid);

        let image_data = match context.image_data.take() {
            Some(data) => data,
            None => {
                context.set_video_cache_dir(video_cache_dir);
                return Ok(context);
            }
        };

        let mut video_registry = context.video_registry.take().unwrap_or_default();

        for (topic_name, data) in image_data.iter() {
            let relative = topic_name.trim_start_matches('/');
            let output_path = video_cache_dir.join(format!("{}.mp4", relative));

            let mut encoder = VideoEncoderVariant::from_config(&output_path, self.config.clone());
            for frame in data.iter() {
                encoder.add_data(&frame.bytes)?;
            }
            encoder.finish()?;

            let shape = resolve_or_infer_image_shape(
                topic_name,
                data,
                context.image_topic_shapes.as_ref(),
                3,
            )
            .ok_or_else(|| {
                StageError::invalid(format!(
                    "missing image shape for encoded video topic: {}",
                    topic_name
                ))
            })?;

            let (output_width, output_height) = self
                .config
                .output_dimensions(shape.width as u32, shape.height as u32)?;

            let artifact = self.config.video_artifact(
                format!("{relative}.mp4"),
                output_width,
                output_height,
            )?;
            video_registry.insert(topic_name.clone(), artifact);
        }
        context.set_image_data(image_data);
        context.set_video_cache_dir(video_cache_dir.clone());
        context.set_bundle_root(video_cache_dir);
        if !video_registry.is_empty() {
            context.set_video_registry(video_registry);
        }

        Ok(context)
    }
}

// ============================================================================
// Video Encoder Variant (unified dispatch)
// ============================================================================

/// Unified video encoder that dispatches to software or hardware encoding.
///
/// Selects the appropriate encoder based on the codec configuration.
/// Use [`VideoEncoderVariant::from_config()`] to create an instance.
///
/// # Why enum instead of trait
///
/// Encoders have identical public APIs but fundamentally different internals
/// (in-process FFmpeg bindings vs FFmpeg subprocess pipe). An enum avoids dynamic
/// dispatch overhead and matches the codebase's existing closed-set dispatch style
/// (cf. [`TimeSynchronizerConfig`](crate::synchronize::TimeSynchronizerConfig)).
pub enum VideoEncoderVariant {
    /// Software encoding via in-process FFmpeg (ffmpeg-next bindings).
    Software(SoftwareVideoEncoder),
    /// Hardware-accelerated encoding via VA-API (FFmpeg subprocess pipe).
    Vaapi(VaapiVideoEncoder),
    /// Hardware-accelerated encoding via NVIDIA NVENC (FFmpeg subprocess pipe).
    Nvenc(NvencVideoEncoder),
}

impl VideoEncoderVariant {
    /// Creates a new encoder variant based on the codec configuration.
    ///
    /// Hardware codec variants create an FFmpeg subprocess encoder. Software codecs
    /// use in-process FFmpeg bindings.
    pub fn from_config<P: AsRef<Utf8Path>>(output_path: &P, config: VideoEncoderConfig) -> Self {
        if config.codec_config.is_vaapi() {
            if let Some(device) = config.codec_config.vaapi_device_path()
                && !Path::new(device).exists()
            {
                tracing::warn!(
                    "VA-API codec selected but {} not found. \
                     Encoding will likely fail. \
                     Ensure /dev/dri is mounted for hardware acceleration.",
                    device
                );
            }
            Self::Vaapi(VaapiVideoEncoder::new(output_path, config))
        } else if config.codec_config.is_nvenc() {
            Self::Nvenc(NvencVideoEncoder::new(output_path, config))
        } else {
            Self::Software(SoftwareVideoEncoder::new(output_path, config))
        }
    }

    /// Adds a single frame to the video from a [`DynamicImage`].
    ///
    /// On the first call, the underlying encoder is initialized lazily with the
    /// dimensions of the provided image.
    pub fn add_frame(&mut self, img: &DynamicImage) -> Result<(), StageError> {
        match self {
            Self::Software(enc) => enc.add_frame(img),
            Self::Vaapi(enc) => enc.add_frame(img),
            Self::Nvenc(enc) => enc.add_frame(img),
        }
    }

    /// Adds a single frame from raw image data (JPEG, PNG, etc.).
    ///
    /// The image format is auto-detected. For VA-API, JPEG/PNG may use a fast
    /// direct pipe path; other formats fall back to RGB decoding.
    pub fn add_data(&mut self, data: &[u8]) -> Result<(), StageError> {
        match self {
            Self::Software(enc) => enc.add_data(data),
            Self::Vaapi(enc) => enc.add_data(data),
            Self::Nvenc(enc) => enc.add_data(data),
        }
    }

    /// Finalizes the encoding process and writes the video file.
    ///
    /// For software encoding, flushes the encoder and writes the container trailer.
    /// For VA-API, closes the FFmpeg subprocess stdin and waits for completion.
    ///
    /// Must be called to produce a valid video file. If no frames were added,
    /// this is a no-op.
    pub fn finish(&mut self) -> Result<(), StageError> {
        match self {
            Self::Software(enc) => enc.finish(),
            Self::Vaapi(enc) => enc.finish(),
            Self::Nvenc(enc) => enc.finish(),
        }
    }
}

// ============================================================================
// VA-API Hardware Encoder
// ============================================================================

/// Encodes a sequence of images into a video file using VA-API hardware acceleration.
///
/// This encoder uses a pipe-based approach: FFmpeg is spawned with video input
/// from stdin. It supports two input modes:
///
/// 1. **Direct Pipe (image2pipe)**: For JPEG/PNG, compressed bytes are sent
///    directly to FFmpeg, which decodes them internally. This is ~2x faster as it
///    avoids Rust-side image decoding.
///
/// 2. **Raw Pipe (rawvideo)**: For other formats, images are decoded in Rust and
///    sent as RGB24 raw video data.
///
/// The pipe-based approach is necessary because:
/// 1. ffmpeg-next doesn't expose hardware context APIs (av_hwdevice_ctx_create)
/// 2. FFmpeg CLI with `-vaapi_device` properly initializes the hardware context
pub struct VaapiVideoEncoder {
    output_path: Utf8PathBuf,
    config: VideoEncoderConfig,
    /// The FFmpeg child process (spawned lazily on first frame)
    ffmpeg_process: Option<FfmpegSubprocess>,
    /// Source frame dimensions (set from first frame, only used for rawvideo mode)
    input_dimensions: Option<(u32, u32)>,
    frame_count: u32,
    /// Input mode (determined on first frame based on format)
    input_mode: InputMode,
}

impl VaapiVideoEncoder {
    /// Creates a new VA-API video encoder.
    ///
    /// # Arguments
    ///
    /// * `output_path` - The path to the output video file.
    /// * `config` - The `VideoEncoderConfig` specifying encoding parameters.
    pub fn new<P: AsRef<Utf8Path>>(output_path: &P, config: VideoEncoderConfig) -> Self {
        Self {
            output_path: output_path.as_ref().to_path_buf(),
            config,
            ffmpeg_process: None,
            input_dimensions: None,
            frame_count: 0,
            input_mode: InputMode::Unknown,
        }
    }

    fn ffmpeg_video_filter(&self) -> Result<String, StageError> {
        if let Some(resize) = self.config.resize {
            let mut filter = format!("scale={}:{}", resize.width, resize.height);
            if let Some(scale_flag) = self.config.scaling.ffmpeg_cli_scale_flag()? {
                filter.push_str(&format!(":flags={scale_flag}"));
            }
            filter.push_str(",format=nv12,hwupload");
            Ok(filter)
        } else {
            Ok("format=nv12,hwupload".to_string())
        }
    }

    /// Spawns the FFmpeg process for raw RGB24 video input.
    ///
    /// FFmpeg is configured to read raw RGB24 video from stdin and encode using VA-API.
    /// Used as fallback when image format is not JPEG/PNG.
    fn spawn_ffmpeg_rawvideo(&mut self, width: u32, height: u32) -> Result<(), StageError> {
        use std::process::Command;

        self.config.preflight()?;

        // Create output directory if it doesn't exist
        if let Some(parent) = self.output_path.parent() {
            fs::create_dir_all(parent).map_err(|e| {
                StageError::io(
                    format!("failed to create video output directory: {}", parent),
                    e,
                )
            })?;
        }

        // Get VA-API parameters from codec config
        let (encoder_name, qp, device, profile, b_depth, async_depth) =
            self.extract_vaapi_params()?;

        let device = device.unwrap_or_else(|| DEFAULT_VAAPI_DEVICE.to_string());
        let gop_frames = self.config.gop;
        let video_filter = self.ffmpeg_video_filter()?;

        // Build FFmpeg command for VA-API encoding with raw video input from pipe
        let mut cmd = Command::new("ffmpeg");
        cmd.args([
            "-hide_banner",
            "-nostats",
            "-loglevel",
            "warning",
            "-y", // Overwrite output
            // Input: raw RGB24 video from stdin
            "-f",
            "rawvideo",
            "-pix_fmt",
            "rgb24",
            "-s",
            &format!("{}x{}", width, height),
            "-framerate",
            &self.config.fps.to_string(),
            "-i",
            "pipe:0",
            // VA-API hardware context initialization
            "-vaapi_device",
            &device,
            // Filter: convert RGB to NV12 and upload to GPU
            //
            // Known issue (AMD VCN4 / RDNA3): The AV1 hardware encoder pads frames
            // to 64-pixel width alignment (e.g., 848x480 → 896x480) and does not set
            // the correct render_width/render_height in the AV1 bitstream. Decoders
            // (libdav1d, PyAV) return padded frames with black borders on the right/bottom.
            // This is a hardware limitation fixed in VCN5 (RDNA4).
            // See: https://gitlab.freedesktop.org/mesa/mesa/-/issues/9185
            "-vf",
            &video_filter,
            // Encoder settings
            "-c:v",
            encoder_name,
        ]);

        // Quality parameter: H.264/HEVC use -qp, AV1 uses -global_quality
        if encoder_name == "av1_vaapi" {
            tracing::debug!(
                "VA-API AV1: using -global_quality {} (0=best, 255=worst)",
                qp
            );
            cmd.args(["-global_quality", &qp.to_string()]);
        } else {
            tracing::debug!(
                "VA-API {}: using -qp {} (0=best, 51=worst)",
                encoder_name,
                qp
            );
            cmd.args(["-qp", &qp.to_string()]);
        }

        cmd.args(["-g", &gop_frames.to_string()]);

        // Explicitly set CQP rate control mode to ensure the QP value is respected.
        // Without this, some VA-API drivers may ignore the QP parameter.
        cmd.args(["-rc_mode", "CQP"]);

        // Add optional parameters: default to "main" profile for AV1 if not specified.
        let av1_default_profile;
        if let Some(p) = profile {
            cmd.args(["-profile:v", &p]);
        } else if encoder_name == "av1_vaapi" {
            av1_default_profile = "main".to_string();
            cmd.args(["-profile:v", &av1_default_profile]);
        }
        if let Some(bd) = b_depth {
            // B-frames only for H.264 and AV1 (HEVC doesn't support B-frames on AMD VCN)
            if encoder_name != "hevc_vaapi" {
                cmd.args(["-b_depth", &bd.to_string()]);
            }
        }
        if let Some(ad) = async_depth {
            cmd.args(["-async_depth", &ad.to_string()]);
        }

        cmd.arg(self.output_path.as_str());

        self.ffmpeg_process = Some(FfmpegSubprocess::spawn(
            cmd,
            format!("VA-API {}", encoder_name),
        )?);

        tracing::debug!(
            "Spawned FFmpeg VA-API encoder: {}x{} @ {} fps, codec={}",
            width,
            height,
            self.config.fps,
            encoder_name
        );

        Ok(())
    }

    /// Spawns the FFmpeg process for image2pipe input (JPEG/PNG direct pipe).
    ///
    /// This is ~2x faster than rawvideo mode because:
    /// 1. Image data is piped directly without Rust-side decoding
    /// 2. FFmpeg's libjpeg-turbo decoder is highly optimized
    /// 3. Pipe transfer size is 10-20x smaller (compressed vs raw RGB24)
    fn spawn_ffmpeg_image2pipe(&mut self, format: ImageFormat) -> Result<(), StageError> {
        use std::process::Command;

        self.config.preflight()?;

        // Create output directory if it doesn't exist
        if let Some(parent) = self.output_path.parent() {
            fs::create_dir_all(parent).map_err(|e| {
                StageError::io(
                    format!("failed to create video output directory: {}", parent),
                    e,
                )
            })?;
        }

        // Get VA-API parameters from codec config
        let (encoder_name, qp, device, profile, b_depth, async_depth) =
            self.extract_vaapi_params()?;

        let device = device.unwrap_or_else(|| DEFAULT_VAAPI_DEVICE.to_string());
        let gop_frames = self.config.gop;
        let video_filter = self.ffmpeg_video_filter()?;

        // Build FFmpeg command for VA-API encoding with image2pipe input
        let mut cmd = Command::new("ffmpeg");
        cmd.args([
            "-hide_banner",
            "-nostats",
            "-loglevel",
            "warning",
            "-y", // Overwrite output
            // Input: image sequence from stdin (JPEG/PNG)
            "-f",
            "image2pipe",
            "-c:v",
            format.ffmpeg_decoder(),
            "-framerate",
            &self.config.fps.to_string(),
            "-i",
            "pipe:0",
            // VA-API hardware context initialization
            "-vaapi_device",
            &device,
            // Filter: upload to GPU (format conversion handled by FFmpeg internally)
            // See spawn_ffmpeg_rawvideo() for AMD VCN4 AV1 padding issue note.
            "-vf",
            &video_filter,
            // Encoder settings
            "-c:v",
            encoder_name,
        ]);

        // Quality parameter: H.264/HEVC use -qp, AV1 uses -global_quality
        if encoder_name == "av1_vaapi" {
            tracing::debug!(
                "VA-API AV1 (image2pipe): using -global_quality {} (0=best, 255=worst)",
                qp
            );
            cmd.args(["-global_quality", &qp.to_string()]);
        } else {
            tracing::debug!(
                "VA-API {} (image2pipe): using -qp {} (0=best, 51=worst)",
                encoder_name,
                qp
            );
            cmd.args(["-qp", &qp.to_string()]);
        }

        cmd.args(["-g", &gop_frames.to_string()]);

        // Explicitly set CQP rate control mode to ensure the QP value is respected.
        // Without this, some VA-API drivers may ignore the QP parameter.
        cmd.args(["-rc_mode", "CQP"]);

        // Add optional parameters: default to "main" profile for AV1 if not specified.
        let av1_default_profile;
        if let Some(p) = profile {
            cmd.args(["-profile:v", &p]);
        } else if encoder_name == "av1_vaapi" {
            av1_default_profile = "main".to_string();
            cmd.args(["-profile:v", &av1_default_profile]);
        }
        if let Some(bd) = b_depth {
            // B-frames only for H.264 and AV1 (HEVC doesn't support B-frames on AMD VCN)
            if encoder_name != "hevc_vaapi" {
                cmd.args(["-b_depth", &bd.to_string()]);
            }
        }
        if let Some(ad) = async_depth {
            cmd.args(["-async_depth", &ad.to_string()]);
        }

        cmd.arg(self.output_path.as_str());

        self.ffmpeg_process = Some(FfmpegSubprocess::spawn(
            cmd,
            format!("VA-API {} image2pipe", encoder_name),
        )?);

        tracing::debug!(
            "Spawned FFmpeg VA-API encoder (image2pipe/{:?}): @ {} fps, codec={}",
            format,
            self.config.fps,
            encoder_name
        );

        Ok(())
    }

    /// Writes raw data directly to FFmpeg's stdin pipe.
    fn write_to_pipe(&mut self, data: &[u8]) -> Result<(), StageError> {
        let process = self
            .ffmpeg_process
            .as_mut()
            .ok_or_else(|| StageError::invalid("FFmpeg process not started"))?;
        process.write_all(data, "failed to write video frame")
    }

    /// Adds a single frame from raw image data.
    ///
    /// Automatically detects image format on first frame:
    /// - JPEG/PNG: Uses direct pipe mode (fast, ~2x speedup)
    /// - Other formats: Falls back to RGB pipe mode (slower, but universal)
    ///
    /// # Errors
    ///
    /// Returns an error if the image cannot be decoded or written.
    pub fn add_data(&mut self, data: &[u8]) -> Result<(), StageError> {
        match &self.input_mode {
            InputMode::Unknown => {
                // First frame: detect format and spawn appropriate FFmpeg pipeline
                if let Some(format) = ImageFormat::detect(data) {
                    // Direct pipe mode for JPEG/PNG
                    self.spawn_ffmpeg_image2pipe(format)?;
                    self.input_mode = InputMode::DirectPipe { format };
                    self.write_to_pipe(data)?;
                    self.frame_count += 1;
                } else {
                    // Fallback: decode and use rawvideo mode
                    let img = image::load_from_memory(data)
                        .map_err(|e| StageError::invalid_with("failed to decode image data", e))?;
                    self.add_frame(&img)?;
                }
            }
            InputMode::DirectPipe { format: expected } => {
                // Verify format consistency
                match ImageFormat::detect(data) {
                    Some(actual) if actual == *expected => {}
                    Some(actual) => {
                        return Err(StageError::invalid(format!(
                            "Mixed image formats in stream: expected {:?}, got {:?}",
                            expected, actual
                        )));
                    }
                    None => {
                        return Err(StageError::invalid(format!(
                            "Unsupported image format in direct pipe stream: expected {:?}",
                            expected
                        )));
                    }
                }
                self.write_to_pipe(data)?;
                self.frame_count += 1;
            }
            InputMode::RgbPipe => {
                // Already in RGB mode, continue decoding
                let img = image::load_from_memory(data)
                    .map_err(|e| StageError::invalid_with("failed to decode image data", e))?;
                self.add_frame(&img)?;
            }
        }
        Ok(())
    }

    /// Adds a single frame from a `DynamicImage`.
    ///
    /// On the first call, spawns FFmpeg with the frame dimensions.
    /// The frame is converted to RGB24 and written to FFmpeg's stdin.
    ///
    /// # Errors
    ///
    /// Returns an error if the frame cannot be written.
    pub fn add_frame(&mut self, img: &DynamicImage) -> Result<(), StageError> {
        let rgb_img = img.to_rgb8();
        let input_width = rgb_img.width();
        let input_height = rgb_img.height();

        // Lazily spawn FFmpeg on first frame (rawvideo mode)
        if self.ffmpeg_process.is_none() {
            self.spawn_ffmpeg_rawvideo(input_width, input_height)?;
            self.input_dimensions = Some((input_width, input_height));
            self.input_mode = InputMode::RgbPipe;
        }

        // Verify source dimensions match
        if let Some((w, h)) = self.input_dimensions
            && (w != input_width || h != input_height)
        {
            return Err(StageError::invalid(format!(
                "Frame dimensions mismatch: expected {}x{}, got {}x{}",
                w, h, input_width, input_height
            )));
        }

        self.write_to_pipe(rgb_img.as_raw())?;

        self.frame_count += 1;
        Ok(())
    }

    /// Finalizes the encoding process.
    ///
    /// This function:
    /// 1. Closes FFmpeg's stdin to signal end of input
    /// 2. Waits for FFmpeg to complete encoding
    /// 3. Checks the exit status for errors
    ///
    /// # Errors
    ///
    /// Returns an error if encoding fails.
    pub fn finish(&mut self) -> Result<(), StageError> {
        if self.frame_count == 0 {
            return Ok(());
        }

        let mut process = self.ffmpeg_process.take().ok_or_else(|| {
            StageError::invalid("VA-API encoder finish called but FFmpeg was not started")
        })?;

        process.finish()?;

        let encoder_name = match &self.config.codec_config {
            CodecConfig::H264Vaapi { .. } => "h264_vaapi",
            CodecConfig::H265Vaapi { .. } => "hevc_vaapi",
            CodecConfig::Av1Vaapi { .. } => "av1_vaapi",
            _ => "unknown",
        };

        tracing::info!(
            "VA-API encoded {} frames to {} with {}",
            self.frame_count,
            self.output_path,
            encoder_name
        );

        Ok(())
    }

    /// Extracts VA-API parameters from the codec config.
    #[allow(clippy::type_complexity)]
    fn extract_vaapi_params(
        &self,
    ) -> Result<
        (
            &'static str,
            u32,
            Option<String>,
            Option<String>,
            Option<u32>,
            Option<u32>,
        ),
        StageError,
    > {
        match &self.config.codec_config {
            CodecConfig::H264Vaapi {
                qp,
                device,
                profile,
                b_depth,
                async_depth,
            } => Ok((
                "h264_vaapi",
                *qp,
                device.clone(),
                profile.clone(),
                *b_depth,
                *async_depth,
            )),
            CodecConfig::H265Vaapi {
                qp,
                device,
                profile,
                async_depth,
            } => Ok((
                "hevc_vaapi",
                *qp,
                device.clone(),
                profile.clone(),
                None, // HEVC has no B-frames on AMD VCN
                *async_depth,
            )),
            CodecConfig::Av1Vaapi {
                qp,
                device,
                profile,
                b_depth,
                async_depth,
            } => Ok((
                "av1_vaapi",
                *qp,
                device.clone(),
                profile.clone(),
                *b_depth,
                *async_depth,
            )),
            _ => Err(StageError::invalid(
                "VaapiVideoEncoder requires a VA-API codec config",
            )),
        }
    }
}

// ============================================================================
// NVIDIA NVENC Hardware Encoder
// ============================================================================

/// Encodes a sequence of images into a video file using NVIDIA NVENC.
///
/// This mirrors [`VaapiVideoEncoder`] intentionally: both hardware paths use the
/// FFmpeg CLI subprocess, but their command-line options and preflight checks are
/// backend-specific enough that keeping the two implementations explicit is easier
/// to maintain than a premature abstraction.
pub struct NvencVideoEncoder {
    output_path: Utf8PathBuf,
    config: VideoEncoderConfig,
    ffmpeg_process: Option<FfmpegSubprocess>,
    input_dimensions: Option<(u32, u32)>,
    frame_count: u32,
    input_mode: InputMode,
}

struct NvencEncoderParams {
    encoder_name: &'static str,
    qp: u32,
    gpu: Option<u32>,
    preset: NvencPreset,
    tune: Option<NvencTune>,
    profile: Option<String>,
    b_frames: u32,
    rc_lookahead: Option<u32>,
}

impl NvencVideoEncoder {
    pub fn new<P: AsRef<Utf8Path>>(output_path: &P, config: VideoEncoderConfig) -> Self {
        Self {
            output_path: output_path.as_ref().to_path_buf(),
            config,
            ffmpeg_process: None,
            input_dimensions: None,
            frame_count: 0,
            input_mode: InputMode::Unknown,
        }
    }

    fn ffmpeg_video_filter(&self) -> Result<String, StageError> {
        if let Some(resize) = self.config.resize {
            let mut filter = format!("scale={}:{}", resize.width, resize.height);
            if let Some(scale_flag) = self.config.scaling.ffmpeg_cli_scale_flag()? {
                filter.push_str(&format!(":flags={scale_flag}"));
            }
            filter.push_str(",format=yuv420p");
            Ok(filter)
        } else {
            Ok("format=yuv420p".to_string())
        }
    }

    fn spawn_ffmpeg_rawvideo(&mut self, width: u32, height: u32) -> Result<(), StageError> {
        use std::process::Command;

        self.config.preflight()?;

        if let Some(parent) = self.output_path.parent() {
            fs::create_dir_all(parent).map_err(|e| {
                StageError::io(
                    format!("failed to create video output directory: {}", parent),
                    e,
                )
            })?;
        }

        let params = self.extract_nvenc_params()?;
        let video_filter = self.ffmpeg_video_filter()?;
        let size = format!("{}x{}", width, height);
        let fps = self.config.fps.to_string();
        let qp = params.qp.to_string();
        let gop = self.config.gop.to_string();
        let b_frames = params.b_frames.to_string();

        let mut cmd = Command::new("ffmpeg");
        cmd.args([
            "-hide_banner",
            "-nostats",
            "-loglevel",
            "warning",
            "-y",
            "-f",
            "rawvideo",
            "-pix_fmt",
            "rgb24",
            "-s",
            &size,
            "-framerate",
            &fps,
            "-i",
            "pipe:0",
            "-vf",
            &video_filter,
            "-c:v",
            params.encoder_name,
            "-rc",
            "constqp",
            "-qp",
            &qp,
            "-g",
            &gop,
            "-bf",
            &b_frames,
            "-preset",
            params.preset.as_str(),
        ]);

        if params.b_frames > 0 {
            cmd.args(["-b_ref_mode", "middle"]);
        }
        if let Some(rc_lookahead) = params.rc_lookahead {
            let rc_lookahead = rc_lookahead.to_string();
            cmd.args(["-rc-lookahead", &rc_lookahead]);
        }
        if let Some(tune) = params.tune {
            cmd.args(["-tune", tune.as_str()]);
        }
        if let Some(gpu) = params.gpu {
            cmd.args(["-gpu", &gpu.to_string()]);
        }
        if let Some(profile) = params.profile {
            cmd.args(["-profile:v", &profile]);
        }

        cmd.arg(self.output_path.as_str());

        self.ffmpeg_process = Some(FfmpegSubprocess::spawn(
            cmd,
            format!("NVENC {}", params.encoder_name),
        )?);

        tracing::debug!(
            "Spawned FFmpeg NVENC encoder: {}x{} @ {} fps, codec={}",
            width,
            height,
            self.config.fps,
            params.encoder_name
        );

        Ok(())
    }

    fn spawn_ffmpeg_image2pipe(&mut self, format: ImageFormat) -> Result<(), StageError> {
        use std::process::Command;

        self.config.preflight()?;

        if let Some(parent) = self.output_path.parent() {
            fs::create_dir_all(parent).map_err(|e| {
                StageError::io(
                    format!("failed to create video output directory: {}", parent),
                    e,
                )
            })?;
        }

        let params = self.extract_nvenc_params()?;
        let video_filter = self.ffmpeg_video_filter()?;
        let fps = self.config.fps.to_string();
        let qp = params.qp.to_string();
        let gop = self.config.gop.to_string();
        let b_frames = params.b_frames.to_string();

        let mut cmd = Command::new("ffmpeg");
        cmd.args([
            "-hide_banner",
            "-nostats",
            "-loglevel",
            "warning",
            "-y",
            "-f",
            "image2pipe",
            "-c:v",
            format.ffmpeg_decoder(),
            "-framerate",
            &fps,
            "-i",
            "pipe:0",
            "-vf",
            &video_filter,
            "-c:v",
            params.encoder_name,
            "-rc",
            "constqp",
            "-qp",
            &qp,
            "-g",
            &gop,
            "-bf",
            &b_frames,
            "-preset",
            params.preset.as_str(),
        ]);

        if params.b_frames > 0 {
            cmd.args(["-b_ref_mode", "middle"]);
        }
        if let Some(rc_lookahead) = params.rc_lookahead {
            let rc_lookahead = rc_lookahead.to_string();
            cmd.args(["-rc-lookahead", &rc_lookahead]);
        }
        if let Some(tune) = params.tune {
            cmd.args(["-tune", tune.as_str()]);
        }
        if let Some(gpu) = params.gpu {
            cmd.args(["-gpu", &gpu.to_string()]);
        }
        if let Some(profile) = params.profile {
            cmd.args(["-profile:v", &profile]);
        }

        cmd.arg(self.output_path.as_str());

        self.ffmpeg_process = Some(FfmpegSubprocess::spawn(
            cmd,
            format!("NVENC {} image2pipe", params.encoder_name),
        )?);

        tracing::debug!(
            "Spawned FFmpeg NVENC encoder (image2pipe/{:?}): @ {} fps, codec={}",
            format,
            self.config.fps,
            params.encoder_name
        );

        Ok(())
    }

    fn write_to_pipe(&mut self, data: &[u8]) -> Result<(), StageError> {
        let process = self
            .ffmpeg_process
            .as_mut()
            .ok_or_else(|| StageError::invalid("FFmpeg process not started"))?;
        process.write_all(data, "failed to write video frame")
    }

    pub fn add_data(&mut self, data: &[u8]) -> Result<(), StageError> {
        match &self.input_mode {
            InputMode::Unknown => {
                if let Some(format) = ImageFormat::detect(data) {
                    self.spawn_ffmpeg_image2pipe(format)?;
                    self.input_mode = InputMode::DirectPipe { format };
                    self.write_to_pipe(data)?;
                    self.frame_count += 1;
                } else {
                    let img = image::load_from_memory(data)
                        .map_err(|e| StageError::invalid_with("failed to decode image data", e))?;
                    self.add_frame(&img)?;
                }
            }
            InputMode::DirectPipe { format: expected } => {
                match ImageFormat::detect(data) {
                    Some(actual) if actual == *expected => {}
                    Some(actual) => {
                        return Err(StageError::invalid(format!(
                            "Mixed image formats in stream: expected {:?}, got {:?}",
                            expected, actual
                        )));
                    }
                    None => {
                        return Err(StageError::invalid(format!(
                            "Unsupported image format in direct pipe stream: expected {:?}",
                            expected
                        )));
                    }
                }
                self.write_to_pipe(data)?;
                self.frame_count += 1;
            }
            InputMode::RgbPipe => {
                let img = image::load_from_memory(data)
                    .map_err(|e| StageError::invalid_with("failed to decode image data", e))?;
                self.add_frame(&img)?;
            }
        }
        Ok(())
    }

    pub fn add_frame(&mut self, img: &DynamicImage) -> Result<(), StageError> {
        let rgb_img = img.to_rgb8();
        let input_width = rgb_img.width();
        let input_height = rgb_img.height();

        if self.ffmpeg_process.is_none() {
            self.spawn_ffmpeg_rawvideo(input_width, input_height)?;
            self.input_dimensions = Some((input_width, input_height));
            self.input_mode = InputMode::RgbPipe;
        }

        if let Some((w, h)) = self.input_dimensions
            && (w != input_width || h != input_height)
        {
            return Err(StageError::invalid(format!(
                "Frame dimensions mismatch: expected {}x{}, got {}x{}",
                w, h, input_width, input_height
            )));
        }

        self.write_to_pipe(rgb_img.as_raw())?;
        self.frame_count += 1;
        Ok(())
    }

    pub fn finish(&mut self) -> Result<(), StageError> {
        if self.frame_count == 0 {
            return Ok(());
        }

        let mut process = self.ffmpeg_process.take().ok_or_else(|| {
            StageError::invalid("NVENC encoder finish called but FFmpeg was not started")
        })?;

        process.finish()?;

        tracing::info!(
            "NVENC encoded {} frames to {} with {}",
            self.frame_count,
            self.output_path,
            self.config.codec_config.ffmpeg_encoder_name()
        );

        Ok(())
    }

    fn extract_nvenc_params(&self) -> Result<NvencEncoderParams, StageError> {
        match &self.config.codec_config {
            CodecConfig::H264Nvenc {
                qp,
                gpu,
                preset,
                tune,
                profile,
                b_frames,
                rc_lookahead,
            } => Ok(NvencEncoderParams {
                encoder_name: "h264_nvenc",
                qp: *qp,
                gpu: *gpu,
                preset: *preset,
                tune: *tune,
                profile: profile.clone(),
                b_frames: *b_frames,
                rc_lookahead: *rc_lookahead,
            }),
            CodecConfig::H265Nvenc {
                qp,
                gpu,
                preset,
                tune,
                profile,
                b_frames,
                rc_lookahead,
            } => Ok(NvencEncoderParams {
                encoder_name: "hevc_nvenc",
                qp: *qp,
                gpu: *gpu,
                preset: *preset,
                tune: *tune,
                profile: profile.clone(),
                b_frames: *b_frames,
                rc_lookahead: *rc_lookahead,
            }),
            CodecConfig::Av1Nvenc {
                qp,
                gpu,
                preset,
                tune,
                profile,
                b_frames,
                rc_lookahead,
            } => Ok(NvencEncoderParams {
                encoder_name: "av1_nvenc",
                qp: *qp,
                gpu: *gpu,
                preset: *preset,
                tune: *tune,
                profile: profile.clone(),
                b_frames: *b_frames,
                rc_lookahead: *rc_lookahead,
            }),
            _ => Err(StageError::invalid(
                "NvencVideoEncoder requires an NVENC codec config",
            )),
        }
    }
}

// ============================================================================
// Software Video Encoder
// ============================================================================

/// Encodes a sequence of images (provided as byte arrays) into a video file using FFmpeg.
///
/// This encoder is initialized lazily. The actual video file and FFmpeg context are
/// created only when the first frame is added via `add_data`.
pub struct SoftwareVideoEncoder {
    output_path: Utf8PathBuf,
    config: VideoEncoderConfig,
    next_pts: i64,
    encoder: Option<ffmpeg::encoder::video::Encoder>,
    scaler: Option<ScalingContext>,
    output: Option<OutputContext>,
    stream_index: Option<usize>,
    stream_time_base: Option<Rational>,
}

impl SoftwareVideoEncoder {
    /// Creates a new, uninitialized `VideoEncoder`.
    ///
    /// This sets up the configuration for the encoder but does not initialize any FFmpeg components.
    /// The parent directory for the output path will be created lazily when encoding starts.
    ///
    /// # Arguments
    ///
    /// * `output_path` - The path to the output video file.
    /// * `config` - The `VideoEncoderConfig` specifying encoding parameters.
    pub fn new<P: AsRef<Utf8Path>>(output_path: &P, config: VideoEncoderConfig) -> Self {
        Self {
            output_path: output_path.as_ref().to_path_buf(),
            config,
            next_pts: 0,
            encoder: None,
            scaler: None,
            output: None,
            stream_index: None,
            stream_time_base: None,
        }
    }

    /// Initializes the FFmpeg encoder, output context, and scaler.
    ///
    /// This function is called internally by `add_data` when the first frame is received.
    /// It sets up the video codec based on `codec_config`, output format, and scaling context
    /// based on the dimensions of the first frame. It also writes the video header to the output file.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - FFmpeg initialization failed (libraries not installed)
    /// - The parent directory cannot be created
    /// - The FFmpeg encoder is not found (codec not installed)
    /// - Output file cannot be created
    /// - Encoder fails to open (e.g., invalid options)
    fn init_encoder(&mut self, width: u32, height: u32) -> Result<(), StageError> {
        ensure_ffmpeg_initialized()?;

        // Create the parent directory if it doesn't exist
        if let Some(parent) = self.output_path.parent() {
            fs::create_dir_all(parent).map_err(|e| {
                StageError::io(
                    format!("failed to create video output directory: {}", parent),
                    e,
                )
            })?;
        }

        // Validate codec configuration before encoding
        self.config.preflight()?;
        let (output_width, output_height) = self.config.output_dimensions(width, height)?;

        let codec = ffmpeg::encoder::find_by_name(self.config.codec_config.ffmpeg_encoder_name())
            .ok_or_else(|| {
                StageError::invalid(format!(
                    "FFmpeg encoder '{}' not found. \
                     Ensure the codec library is installed (e.g., libx264-dev for H.264, libx265-dev for H.265)",
                    self.config.codec_config.ffmpeg_encoder_name()
                ))
            })?;

        let mut output = ffmpeg::format::output(self.output_path.as_str()).map_err(|e| {
            StageError::external(
                format!("failed to create output file: {}", self.output_path),
                e,
            )
        })?;

        let stream_frame_rate = Rational::new(self.config.fps as i32, 1);
        let stream_time_base = Rational::new(1, self.config.fps as i32);

        let (encoder, stream_index) = {
            let mut stream = output
                .add_stream(codec)
                .map_err(|e| StageError::external("failed to add video stream to output", e))?;
            let stream_index = stream.index();

            stream.set_time_base(stream_time_base);
            stream.set_rate(stream_frame_rate);

            // Use new_with_codec to properly initialize codec-specific parameters
            // This fixes "broken ffmpeg default settings detected" error with libx264 on FFmpeg 5+
            let encoder = {
                let context = ffmpeg::codec::context::Context::new_with_codec(codec);
                let mut encoder = context.encoder().video().map_err(|e| {
                    StageError::external("failed to create video encoder context", e)
                })?;
                encoder.set_width(output_width);
                encoder.set_height(output_height);
                encoder.set_format(Pixel::YUV420P);
                encoder.set_time_base(stream_time_base);
                encoder.set_frame_rate(Some(stream_frame_rate));
                encoder.set_gop(self.config.gop);
                encoder.set_bit_rate(0); // Not used when CRF is set
                encoder.set_flags(CodecFlags::GLOBAL_HEADER);
                encoder
            };

            let mut options = Dictionary::new();
            options.set("crf", &self.config.crf);
            options.set("g", &self.config.gop.to_string());

            match &self.config.codec_config {
                CodecConfig::AV1 {
                    lp,
                    pin,
                    preset,
                    film_grain,
                    film_grain_denoise,
                    lookahead,
                    fast_decode,
                } => {
                    let lp_value = match lp {
                        Some(n) if *n >= 1 && *n <= 6 => *n,
                        _ => 0, // Auto-detect
                    };
                    let pin_value = pin.unwrap_or(0);

                    // Warn if pin value exceeds available CPU cores
                    if pin_value > 0
                        && let Ok(available) = std::thread::available_parallelism()
                    {
                        let core_count = available.get() as u32;
                        if pin_value > core_count {
                            tracing::warn!(
                                pin = pin_value,
                                available_cores = core_count,
                                "SVT-AV1 pin value exceeds available CPU cores; \
                                 encoder may not behave as expected"
                            );
                        }
                    }

                    // Build svtav1-params string with all parameters
                    let mut svtav1_params =
                        format!("lp={}:pin={}:preset={}", lp_value, pin_value, preset);

                    if let Some(fg) = film_grain {
                        svtav1_params.push_str(&format!(":film-grain={}", fg));
                    }
                    if let Some(fgd) = film_grain_denoise {
                        // film-grain-denoise: 0=off, 1=on
                        let fgd_value = if *fgd { 1 } else { 0 };
                        svtav1_params.push_str(&format!(":film-grain-denoise={}", fgd_value));
                    }
                    if let Some(la) = lookahead {
                        svtav1_params.push_str(&format!(":lookahead={}", la));
                    }
                    if let Some(fd) = fast_decode {
                        svtav1_params.push_str(&format!(":fast-decode={}", fd));
                    }

                    options.set("svtav1-params", &svtav1_params);
                }
                CodecConfig::H264 {
                    threads,
                    preset,
                    tune,
                } => {
                    options.set("preset", preset.as_str());
                    if let Some(t) = threads {
                        options.set("threads", &t.to_string());
                    }
                    if !tune.is_empty() {
                        let tune_str = tune
                            .iter()
                            .map(|t| t.as_str())
                            .collect::<Vec<_>>()
                            .join(",");
                        options.set("tune", &tune_str);
                    }
                }
                CodecConfig::H265 {
                    threads,
                    preset,
                    tune,
                    frame_threads,
                } => {
                    options.set("preset", preset.as_str());

                    // Build x265-params string
                    let mut x265_params = Vec::new();
                    if let Some(t) = threads {
                        x265_params.push(format!("pools={}", t));
                    }
                    if !tune.is_empty() {
                        let tune_str = tune
                            .iter()
                            .map(|t| t.as_str())
                            .collect::<Vec<_>>()
                            .join(",");
                        x265_params.push(format!("tune={}", tune_str));
                    }
                    if let Some(ft) = frame_threads {
                        x265_params.push(format!("frame-threads={}", ft));
                    }
                    if !x265_params.is_empty() {
                        options.set("x265-params", &x265_params.join(":"));
                    }
                }
                // VA-API codecs should use VaapiVideoEncoder, not SoftwareVideoEncoder
                CodecConfig::H264Vaapi { .. }
                | CodecConfig::H265Vaapi { .. }
                | CodecConfig::Av1Vaapi { .. }
                | CodecConfig::H264Nvenc { .. }
                | CodecConfig::H265Nvenc { .. }
                | CodecConfig::Av1Nvenc { .. } => {
                    return Err(StageError::invalid(
                        "Hardware codecs must use a subprocess encoder, not SoftwareVideoEncoder. \
                         This is an internal error - please report it as a bug.",
                    ));
                }
            }

            let encoder = encoder.open_as_with(codec, options).map_err(|e| {
                StageError::external(
                    format!(
                        "failed to open {} encoder. Check codec options and ensure the codec is properly installed",
                        self.config.codec_config.ffmpeg_encoder_name()
                    ),
                    e,
                )
            })?;
            stream.set_parameters(&encoder);

            (encoder, stream_index)
        };

        // Write the container header
        output
            .write_header()
            .map_err(|e| StageError::external("failed to write video header", e))?;

        let stream_time_base = output
            .stream(stream_index)
            .ok_or_else(|| {
                StageError::invalid(
                    "internal error: video output missing after initialization (please report a bug)",
                )
            })?
            .time_base();

        // Create a scaler to convert from RGB24 (from the image crate) to YUV420P (for the encoder)
        let scaler = ScalingContext::get(
            Pixel::RGB24,
            width,
            height,
            Pixel::YUV420P,
            output_width,
            output_height,
            ScalingFlags::from(self.config.scaling.clone()),
        )
        .map_err(|e| StageError::external("failed to create scaling context", e))?;

        self.encoder = Some(encoder);
        self.scaler = Some(scaler);
        self.output = Some(output);
        self.stream_index = Some(stream_index);
        self.stream_time_base = Some(stream_time_base);

        Ok(())
    }

    /// Adds a single frame to the video from raw image data.
    ///
    /// On the first call, this function will initialize the encoder with the dimensions of the
    /// provided image. Subsequent calls must provide images with the same dimensions.
    ///
    /// The data is expected to be in a format that the `image` crate can decode (e.g., JPEG).
    /// The image is decoded, converted to RGB, scaled to YUV420P, and then sent to the encoder.
    ///
    /// # Errors
    ///
    /// Returns an error if the image cannot be decoded or FFmpeg fails to encode the frame.
    pub fn add_data(&mut self, data: &[u8]) -> Result<(), StageError> {
        // Load image from memory and convert to RGB8
        let img = image::load_from_memory(data)
            .map_err(|e| StageError::invalid_with("failed to decode image data", e))?;
        self.add_frame(&img)
    }

    /// Adds a single frame to the video from a `DynamicImage`.
    ///
    /// This method avoids re-encoding/decoding if you already have a `DynamicImage`.
    ///
    /// # Errors
    ///
    /// Returns an error if FFmpeg fails to encode the frame.
    pub fn add_frame(&mut self, img: &DynamicImage) -> Result<(), StageError> {
        let rgb_img = img.to_rgb8();
        let output_dimensions = self
            .config
            .output_dimensions(rgb_img.width(), rgb_img.height())?;

        // Lazily initialize the encoder with the dimensions of the first frame
        if self.encoder.is_none() {
            self.init_encoder(rgb_img.width(), rgb_img.height())?;
        }

        // INVARIANT: encoder and scaler are guaranteed to exist after init_encoder succeeds
        #[allow(clippy::expect_used)]
        let encoder = self
            .encoder
            .as_mut()
            .expect("encoder should be initialized");
        // INVARIANT: scaler is guaranteed to exist after init_encoder succeeds
        #[allow(clippy::expect_used)]
        let scaler = self.scaler.as_mut().expect("scaler should be initialized");

        // Create an RGB frame from the raw image data.
        // FFmpeg may pad the frame buffer for alignment, so we need to copy row by row
        // using the actual stride (linesize) rather than a simple copy_from_slice.
        let rgb_frame = rgb_frame_from_image(&rgb_img);

        // Create a YUV frame for the encoder
        let mut yuv_frame = Video::new(Pixel::YUV420P, output_dimensions.0, output_dimensions.1);
        yuv_frame.set_color_range(ffmpeg::util::color::Range::MPEG);
        yuv_frame.set_pts(Some(self.next_pts));
        self.next_pts += 1;

        // Scale the RGB frame to the YUV frame
        scaler
            .run(&rgb_frame, &mut yuv_frame)
            .map_err(|e| StageError::external("failed to scale frame from RGB to YUV", e))?;

        // Send the YUV frame to the encoder
        encoder
            .send_frame(&yuv_frame)
            .map_err(|e| StageError::external("failed to send frame to encoder", e))?;

        // Drain any available encoded packets
        self.drain_packets()
    }

    /// Finalizes the video encoding process.
    ///
    /// This flushes the encoder, writes any remaining packets, and writes the video trailer
    /// to the output file. This must be called to produce a valid video file.
    /// If no frames were added, this function does nothing.
    ///
    /// # Post-condition
    ///
    /// After calling this method, the internal encoder and output are reset to `None`.
    /// The following behavior applies:
    ///
    /// - **Drop**: Will be a no-op (safe)
    /// - **add_data() / add_frame()**: Will trigger re-initialization, creating a new
    ///   encoder that overwrites the same output file. The `next_pts` counter is **not**
    ///   reset, so the new file will have discontinuous timestamps starting from where
    ///   the previous encoding left off. This is almost certainly unintended behavior.
    ///
    /// **Recommendation**: Do not reuse the encoder after calling `finish()`.
    /// Create a new `SoftwareVideoEncoder` instance if you need to encode another file.
    ///
    /// # Note
    ///
    /// If encoding completes normally, prefer calling `finish()` explicitly
    /// for proper error handling. Drop will attempt cleanup but errors are
    /// only logged, not propagated.
    ///
    /// # Errors
    ///
    /// Returns an error if FFmpeg fails to flush remaining packets or write the trailer.
    pub fn finish(&mut self) -> Result<(), StageError> {
        if self.encoder.is_none() {
            return Ok(());
        }

        // INVARIANT: encoder exists - finalize is only called after successful frame processing
        #[allow(clippy::expect_used)]
        self.encoder
            .as_mut()
            .expect("encoder should exist")
            .send_eof()
            .map_err(|e| StageError::external("failed to signal EOF to encoder", e))?;

        // Drain any final packets
        self.drain_packets()?;

        // INVARIANT: output exists - finalize is only called after successful initialization
        #[allow(clippy::expect_used)]
        self.output
            .as_mut()
            .expect("output should exist")
            .write_trailer()
            .map_err(|e| StageError::external("failed to write video trailer", e))?;

        // Mark as finished so Drop knows not to do anything
        // Why: Prevent Drop from double-flushing or writing trailer again
        self.encoder = None;
        self.output = None;
        self.scaler = None;

        Ok(())
    }

    /// Receives encoded packets from the encoder and writes them to the output file.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - FFmpeg fails to receive a packet from the encoder
    /// - `write_packet` fails to write a packet to the output file
    fn drain_packets(&mut self) -> Result<(), StageError> {
        if self.encoder.is_none() {
            return Ok(());
        }

        loop {
            let mut packet = ffmpeg::Packet::empty();
            // INVARIANT: encoder existence is checked at function entry
            #[allow(clippy::unwrap_used)]
            match self.encoder.as_mut().unwrap().receive_packet(&mut packet) {
                Ok(()) => self.write_packet(packet)?, // Propagate write errors
                // The encoder has been fully drained
                Err(ffmpeg::Error::Eof) => break,
                // More data is needed to encode a packet
                Err(ffmpeg::Error::Other { errno }) if errno == EAGAIN => break,
                Err(err) => {
                    return Err(StageError::external(
                        "failed to receive encoded packet from FFmpeg encoder",
                        err,
                    ));
                }
            }
        }
        Ok(())
    }

    /// Rescales timestamps and writes a packet to the output container.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Required fields (encoder, stream_index, stream_time_base, output) are None
    /// - FFmpeg fails to write the packet to the output file
    fn write_packet(&mut self, mut packet: ffmpeg::Packet) -> Result<(), StageError> {
        let encoder = self
            .encoder
            .as_ref()
            .ok_or_else(|| StageError::invalid("write_packet called but encoder is None"))?;
        let stream_index = self
            .stream_index
            .ok_or_else(|| StageError::invalid("write_packet called but stream_index is None"))?;
        let stream_time_base = self.stream_time_base.ok_or_else(|| {
            StageError::invalid("write_packet called but stream_time_base is None")
        })?;
        let output = self
            .output
            .as_mut()
            .ok_or_else(|| StageError::invalid("write_packet called but output is None"))?;

        let encoder_time_base = encoder.time_base();

        // Rescale the packet's timestamps from the encoder's time base to the stream's time base
        packet.rescale_ts(encoder_time_base, stream_time_base);
        packet.set_stream(stream_index);

        // Set the packet duration
        let frame_duration =
            unsafe { av_rescale_q(1, encoder_time_base.into(), stream_time_base.into()) };
        packet.set_duration(frame_duration.max(1));

        // Write the packet to the output file
        packet
            .write_interleaved(output)
            .map_err(|e| StageError::external("failed to write packet to output", e))
    }
}

impl Drop for SoftwareVideoEncoder {
    fn drop(&mut self) {
        // If finish() was already called (encoder == None), do nothing
        if self.encoder.is_none() {
            return;
        }

        // Errors in Drop are logged only to avoid panic.
        // Why: Panic in Drop can cause double panic, which is fatal
        //      when called during stack unwinding.

        // 1. Send EOF to encoder to flush remaining frames
        let eof_ok = if let Some(encoder) = self.encoder.as_mut() {
            match encoder.send_eof() {
                Ok(()) => true,
                Err(e) => {
                    tracing::warn!(
                        path = %self.output_path,
                        error = %e,
                        "Failed to send EOF to encoder on drop"
                    );
                    false
                }
            }
        } else {
            false
        };

        // 2. Drain packets only if EOF succeeded
        // Why: After EOF failure, encoder state is undefined, so drain_packets is risky
        if eof_ok && let Err(e) = self.drain_packets() {
            tracing::warn!(
                path = %self.output_path,
                error = %e,
                "Failed to drain packets on drop"
            );
        }

        // 3. Write output file trailer (always try)
        // Why: Without moov atom, MP4 file cannot be played.
        //      OutputContext is independent from encoder, so writing trailer
        //      is worth trying even after EOF/drain failure.
        if let Some(output) = self.output.as_mut()
            && let Err(e) = output.write_trailer()
        {
            tracing::warn!(
                path = %self.output_path,
                error = %e,
                "Failed to write video trailer on drop"
            );
        }

        tracing::debug!(path = %self.output_path, "SoftwareVideoEncoder dropped");
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    use camino::Utf8PathBuf;
    use serial_test::serial;
    use tempfile::tempdir;

    use crate::common::ImageShape;
    use crate::core::Stage;
    use crate::ingest::rosbag2_ingestor::{Rosbag2Ingestor, Rosbag2IngestorConfig};
    use crate::schema::metadata::AiroaMetadata;
    use crate::schema::metadata::v2_0::{
        Device, EnvType, Environment, Episode, File, GitSource, MetadataV2_0, Program, Robot,
        Runner, RunnerType, Segment, Source,
    };
    use crate::testutil::{McapGenerator, McapGeneratorConfig};

    /// Create test metadata with a unique UUID.
    /// The label parameter is used only for documentation/debugging purposes.
    fn create_test_metadata(_label: &str) -> MetadataV2_0 {
        // Generate a random UUID for testing
        let uuid = uuid::Uuid::new_v4().to_string();

        MetadataV2_0 {
            schema: "https://example.com/schema".to_string(),
            schema_version: "2.0".to_string(),
            uuid,
            robot: Robot {
                uri: None,
                robot_type: "HSR".to_string(),
                id: "test-robot".to_string(),
                checksum: None,
            },
            files: vec![File {
                file_type: "rosbag".to_string(),
                name: "data.bag".to_string(),
                checksum: None,
            }],
            environment: Environment {
                env_type: EnvType::RealWorld,
                site: "test_lab".to_string(),
                location: None,
            },
            runner: Runner {
                runner_type: RunnerType::Operator,
                organization: "test".to_string(),
                name: "TestOperator".to_string(),
            },
            devices: vec![Device {
                role: "controller".to_string(),
                device_type: "joystick".to_string(),
                id: "joystick001".to_string(),
            }],
            programs: vec![Program {
                role: "interface".to_string(),
                name: "test".to_string(),
                source: Source {
                    git: Some(GitSource {
                        uri: "https://example.com".to_string(),
                        hash: "v1.0".to_string(),
                        branch: "main".to_string(),
                        tag: None,
                    }),
                },
            }],
            episode: Episode {
                start_time: 0.0,
                end_time: 1.0,
                success: true,
                label: "test".to_string(),
            },
            labels: vec!["test instruction".to_string()],
            segments: vec![Segment {
                start_time: 0.0,
                end_time: 1.0,
                label_idx: 0,
                success: true,
            }],
        }
    }

    fn nvenc_smoke_tests_enabled() -> bool {
        std::env::var_os("REBAKE_RUN_NVENC_TESTS").is_some()
    }

    fn nvenc_smoke_codec_configs() -> Vec<(&'static str, CodecConfig)> {
        vec![
            (
                "h264",
                CodecConfig::H264Nvenc {
                    qp: 28,
                    gpu: None,
                    preset: NvencPreset::P4,
                    tune: None,
                    profile: None,
                    b_frames: DEFAULT_NVENC_B_FRAMES,
                    rc_lookahead: None,
                },
            ),
            (
                "h265",
                CodecConfig::H265Nvenc {
                    qp: 30,
                    gpu: None,
                    preset: NvencPreset::P4,
                    tune: None,
                    profile: None,
                    b_frames: DEFAULT_NVENC_B_FRAMES,
                    rc_lookahead: None,
                },
            ),
            (
                "av1",
                CodecConfig::Av1Nvenc {
                    qp: 80,
                    gpu: None,
                    preset: NvencPreset::P4,
                    tune: None,
                    profile: None,
                    b_frames: DEFAULT_NVENC_B_FRAMES,
                    rc_lookahead: None,
                },
            ),
        ]
    }

    fn nvenc_smoke_config(codec_config: CodecConfig) -> VideoEncoderConfig {
        VideoEncoderConfig::new(30)
            .set_gop(2)
            .set_codec_config(codec_config)
    }

    fn nvenc_smoke_image(seed: u8) -> DynamicImage {
        let img = image::RgbImage::from_fn(256, 256, |x, y| {
            image::Rgb([
                seed.wrapping_add(x as u8),
                seed.wrapping_add(y as u8),
                seed.wrapping_add((x ^ y) as u8),
            ])
        });
        DynamicImage::ImageRgb8(img)
    }

    fn encode_png_bytes(img: &DynamicImage) -> Vec<u8> {
        let mut buffer = std::io::Cursor::new(Vec::new());
        img.write_to(&mut buffer, image::ImageFormat::Png)
            .expect("PNG fixture encoding should succeed");
        buffer.into_inner()
    }

    fn assert_nonempty_file(path: &Utf8Path) {
        let metadata = std::fs::metadata(path.as_std_path()).expect("output file should exist");
        assert!(
            metadata.len() > 0,
            "output file should not be empty: {path}"
        );
    }

    #[test]
    #[serial(ffmpeg)]
    fn test_software_video_encoder() {
        // Generate synthetic MCAP with Image data
        let dir = tempdir().unwrap();
        let output_dir = Utf8PathBuf::try_from(dir.path().to_path_buf()).unwrap();

        let config = McapGeneratorConfig {
            num_frames: 10,
            fps: 30,
            image_size: (64, 64),
            generate_images: true,
            generate_tf: false,
            ..Default::default()
        };

        let generator = McapGenerator::new(config);
        let mcap_path = generator.generate(&output_dir).unwrap();

        // Ingest the MCAP to get image_data (JPEG encoded)
        let mut context = crate::core::Context::default();
        context.set_rosbag_path(mcap_path);
        let mut ingestor = Rosbag2Ingestor::new(Rosbag2IngestorConfig::without_metadata());
        let context = ingestor.run(context).unwrap();

        let image_data = context.image_data.expect("image_data should exist");
        let frames = image_data
            .get("/camera/image_raw")
            .expect("expected image frames for camera");

        let temp_dir = tempdir().unwrap();
        let output_path = Utf8PathBuf::from_path_buf(temp_dir.path().join("test.mp4")).unwrap();

        let encoder_config = VideoEncoderConfig::default();
        let mut encoder = SoftwareVideoEncoder::new(&output_path, encoder_config);

        let frame_count = frames.len();
        for frame in frames {
            encoder.add_data(&frame.bytes).unwrap();
        }
        encoder.finish().unwrap();

        // Verify that the video file exists and has the correct number of frames
        assert!(output_path.exists());

        let ictx = ffmpeg::format::input(&output_path).unwrap();
        let input = ictx.streams().best(ffmpeg::media::Type::Video).unwrap();
        assert_eq!(input.frames(), frame_count as i64);
    }

    #[test]
    #[serial(ffmpeg)]
    fn test_software_video_encoder_resizes_output() {
        // Generate synthetic MCAP with Image data
        let dir = tempdir().unwrap();
        let output_dir = Utf8PathBuf::try_from(dir.path().to_path_buf()).unwrap();

        let config = McapGeneratorConfig {
            num_frames: 10,
            fps: 30,
            image_size: (64, 48),
            generate_images: true,
            generate_tf: false,
            ..Default::default()
        };

        let generator = McapGenerator::new(config);
        let mcap_path = generator.generate(&output_dir).unwrap();

        let mut context = crate::core::Context::default();
        context.set_rosbag_path(mcap_path);
        let mut ingestor = Rosbag2Ingestor::new(Rosbag2IngestorConfig::without_metadata());
        let context = ingestor.run(context).unwrap();

        let image_data = context.image_data.expect("image_data should exist");
        let frames = image_data
            .get("/camera/image_raw")
            .expect("expected image frames for camera");

        let temp_dir = tempdir().unwrap();
        let output_path =
            Utf8PathBuf::from_path_buf(temp_dir.path().join("test_resized.mp4")).unwrap();

        let encoder_config = VideoEncoderConfig::new(30).set_resize(Some(ResizeConfig {
            width: 32,
            height: 24,
        }));
        let mut encoder = SoftwareVideoEncoder::new(&output_path, encoder_config);

        for frame in frames {
            encoder.add_data(&frame.bytes).unwrap();
        }
        encoder.finish().unwrap();

        let ictx = ffmpeg::format::input(&output_path).unwrap();
        let input = ictx.streams().best(ffmpeg::media::Type::Video).unwrap();
        let decoder_context =
            ffmpeg::codec::context::Context::from_parameters(input.parameters()).unwrap();
        let decoder = decoder_context.decoder().video().unwrap();

        assert_eq!(decoder.width(), 32);
        assert_eq!(decoder.height(), 24);
    }

    #[test]
    #[serial(ffmpeg)]
    fn test_software_video_encoder_with_lp_and_pin() {
        // Generate synthetic MCAP with Image data
        let dir = tempdir().unwrap();
        let output_dir = Utf8PathBuf::try_from(dir.path().to_path_buf()).unwrap();

        let config = McapGeneratorConfig {
            num_frames: 10,
            fps: 30,
            image_size: (64, 64),
            generate_images: true,
            generate_tf: false,
            ..Default::default()
        };

        let generator = McapGenerator::new(config);
        let mcap_path = generator.generate(&output_dir).unwrap();

        // Ingest the MCAP to get image_data (JPEG encoded)
        let mut context = crate::core::Context::default();
        context.set_rosbag_path(mcap_path);
        let mut ingestor = Rosbag2Ingestor::new(Rosbag2IngestorConfig::without_metadata());
        let context = ingestor.run(context).unwrap();

        let image_data = context.image_data.expect("image_data should exist");
        let frames = image_data
            .get("/camera/image_raw")
            .expect("expected image frames for camera");

        let temp_dir = tempdir().unwrap();
        let output_path = Utf8PathBuf::from_path_buf(temp_dir.path().join("test.mp4")).unwrap();

        // Test with lp=2 and pin=0 (no pinning)
        // This verifies that svtav1-params lp=2:pin=0:preset=6 is passed to the encoder
        let encoder_config = VideoEncoderConfig::new(30).set_codec_config(CodecConfig::AV1 {
            lp: Some(2),
            pin: Some(0),
            preset: 6,
            film_grain: None,
            film_grain_denoise: None,
            lookahead: None,
            fast_decode: None,
        });
        let mut encoder = SoftwareVideoEncoder::new(&output_path, encoder_config);

        let frame_count = frames.len();
        for frame in frames {
            encoder.add_data(&frame.bytes).unwrap();
        }
        encoder.finish().unwrap();

        // Verify that the video file exists and has the correct number of frames
        assert!(output_path.exists());

        let ictx = ffmpeg::format::input(&output_path).unwrap();
        let input = ictx.streams().best(ffmpeg::media::Type::Video).unwrap();
        assert_eq!(input.frames(), frame_count as i64);
    }

    #[test]
    #[serial(ffmpeg)]
    fn test_video_encoder_stage_creates_uuid_subdirectory() {
        // Generate synthetic MCAP with Image data
        let dir = tempdir().unwrap();
        let output_dir = Utf8PathBuf::try_from(dir.path().to_path_buf()).unwrap();

        let config = McapGeneratorConfig {
            num_frames: 5,
            fps: 30,
            image_size: (64, 64),
            generate_images: true,
            generate_tf: false,
            ..Default::default()
        };

        let generator = McapGenerator::new(config);
        let mcap_path = generator.generate(&output_dir).unwrap();

        // Ingest the MCAP to get image_data (JPEG encoded)
        let mut context = crate::core::Context::default();
        context.set_rosbag_path(mcap_path);
        let mut ingestor = Rosbag2Ingestor::new(Rosbag2IngestorConfig::without_metadata());
        let mut context = ingestor.run(context).unwrap();

        // Set up metadata with a specific UUID
        let metadata = create_test_metadata("test-uuid-abc123");
        let test_uuid = metadata.uuid.to_string();
        context.set_airoa_metadata(AiroaMetadata::V2_0(metadata));

        // Set video_cache_dir in context
        let video_cache_dir = tempdir().unwrap();
        let video_cache_path =
            Utf8PathBuf::from_path_buf(video_cache_dir.path().to_path_buf()).unwrap();
        context.set_video_cache_dir(video_cache_path.clone());

        let encoder_config = VideoEncoderConfig::default();
        let mut encoder = VideoEncoder::new(encoder_config);

        // Run the encoder stage
        let result_context = encoder.run(context).unwrap();

        // Verify that video was created in UUID subdirectory
        let expected_video_path = video_cache_path
            .join(&test_uuid)
            .join("camera/image_raw.mp4");
        assert!(
            expected_video_path.exists(),
            "Video should be created at {expected_video_path}"
        );

        let actual_path = result_context
            .resolve_video_path("/camera/image_raw")
            .expect("video path for camera topic should exist");
        assert_eq!(actual_path, expected_video_path);

        let artifact = result_context
            .video_registry
            .as_ref()
            .expect("video_registry should exist")
            .get("/camera/image_raw")
            .expect("video artifact for camera topic should exist");
        assert_eq!(artifact.video_path, "camera/image_raw.mp4");

        // Verify video_cache_dir in context is updated to include UUID
        let result_video_cache_dir = result_context
            .video_cache_dir()
            .expect("video_cache_dir should be set");
        assert_eq!(result_video_cache_dir, &video_cache_path.join(&test_uuid));
    }

    #[test]
    #[serial(ffmpeg)]
    fn test_video_encoder_stage_records_resized_dimensions_in_artifact() {
        let dir = tempdir().unwrap();
        let output_dir = Utf8PathBuf::try_from(dir.path().to_path_buf()).unwrap();

        let config = McapGeneratorConfig {
            num_frames: 5,
            fps: 30,
            image_size: (64, 48),
            generate_images: true,
            generate_tf: false,
            ..Default::default()
        };

        let generator = McapGenerator::new(config);
        let mcap_path = generator.generate(&output_dir).unwrap();

        let mut context = crate::core::Context::default();
        context.set_rosbag_path(mcap_path);
        let mut ingestor = Rosbag2Ingestor::new(Rosbag2IngestorConfig::without_metadata());
        let mut context = ingestor.run(context).unwrap();

        context.set_airoa_metadata(AiroaMetadata::V2_0(create_test_metadata(
            "resized-artifact",
        )));

        let video_cache_dir = tempdir().unwrap();
        let video_cache_path =
            Utf8PathBuf::from_path_buf(video_cache_dir.path().to_path_buf()).unwrap();
        context.set_video_cache_dir(video_cache_path);

        let encoder_config = VideoEncoderConfig::default().set_resize(Some(ResizeConfig {
            width: 32,
            height: 24,
        }));
        let mut encoder = VideoEncoder::new(encoder_config);

        let result_context = encoder.run(context).unwrap();
        let artifact = result_context
            .video_registry()
            .expect("video_registry should exist")
            .get("/camera/image_raw")
            .expect("video artifact for camera topic should exist");

        assert_eq!(artifact.metadata.width, 32);
        assert_eq!(artifact.metadata.height, 24);
    }

    #[test]
    #[serial(ffmpeg)]
    fn test_video_encoder_stage_falls_back_to_default_video_cache() {
        // Generate synthetic MCAP with Image data
        let dir = tempdir().unwrap();
        let mcap_output_dir = Utf8PathBuf::try_from(dir.path().to_path_buf()).unwrap();

        let config = McapGeneratorConfig {
            num_frames: 5,
            fps: 30,
            image_size: (64, 64),
            generate_images: true,
            generate_tf: false,
            ..Default::default()
        };

        let generator = McapGenerator::new(config);
        let mcap_path = generator.generate(&mcap_output_dir).unwrap();

        // Ingest the MCAP to get image_data (JPEG encoded)
        let mut context = crate::core::Context::default();
        context.set_rosbag_path(mcap_path);
        let mut ingestor = Rosbag2Ingestor::new(Rosbag2IngestorConfig::without_metadata());
        let mut context = ingestor.run(context).unwrap();

        // Set up metadata with a specific UUID
        let metadata = create_test_metadata("fallback-uuid-xyz789");
        let test_uuid = metadata.uuid.to_string();
        context.set_airoa_metadata(AiroaMetadata::V2_0(metadata));

        // Change to a temp directory so ./video_cache is created there
        let working_dir = tempdir().unwrap();
        let original_cwd = std::env::current_dir().unwrap();
        std::env::set_current_dir(working_dir.path()).unwrap();

        // Do NOT set video_cache_dir - encoder should use ./video_cache as fallback
        // Create encoder
        let encoder_config = VideoEncoderConfig::default();
        let mut encoder = VideoEncoder::new(encoder_config);

        // Run the encoder stage
        let result_context = encoder.run(context).unwrap();

        // Restore original working directory
        std::env::set_current_dir(&original_cwd).unwrap();

        // Verify that video was created in ./video_cache/{uuid}/
        let expected_video_path = Utf8PathBuf::from_path_buf(working_dir.path().to_path_buf())
            .unwrap()
            .join("video_cache")
            .join(&test_uuid)
            .join("camera/image_raw.mp4");
        assert!(
            expected_video_path.exists(),
            "Video should be created at {expected_video_path}"
        );

        let actual_path = result_context
            .resolve_video_path("/camera/image_raw")
            .expect("video path for camera topic should exist");
        assert_eq!(actual_path, expected_video_path);
    }

    #[test]
    fn test_video_encoder_stage_fails_without_metadata() {
        // Generate synthetic MCAP with Image data
        let dir = tempdir().unwrap();
        let output_dir = Utf8PathBuf::try_from(dir.path().to_path_buf()).unwrap();

        let config = McapGeneratorConfig {
            num_frames: 5,
            fps: 30,
            image_size: (64, 64),
            generate_images: true,
            generate_tf: false,
            ..Default::default()
        };

        let generator = McapGenerator::new(config);
        let mcap_path = generator.generate(&output_dir).unwrap();

        // Ingest the MCAP to get image_data (without metadata)
        let mut context = crate::core::Context::default();
        context.set_rosbag_path(mcap_path);
        let mut ingestor = Rosbag2Ingestor::new(Rosbag2IngestorConfig::without_metadata());
        let mut context = ingestor.run(context).unwrap();

        // Set video_cache_dir but NOT airoa_metadata
        let video_cache_dir = tempdir().unwrap();
        let video_cache_path =
            Utf8PathBuf::from_path_buf(video_cache_dir.path().to_path_buf()).unwrap();
        context.set_video_cache_dir(video_cache_path);

        let encoder_config = VideoEncoderConfig::default();
        let mut encoder = VideoEncoder::new(encoder_config);

        // Run the encoder stage - should fail because metadata is missing
        let result = encoder.run(context);
        match result {
            Ok(_) => panic!("Should fail without airoa_metadata"),
            Err(err) => {
                assert!(
                    err.reason().contains("airoa_metadata"),
                    "Error should mention airoa_metadata: {}",
                    err.reason()
                );
            }
        }
    }

    #[test]
    #[serial(ffmpeg)]
    fn test_software_video_encoder_h264() {
        // Skip if libx264 is not available
        if ffmpeg::encoder::find_by_name("libx264").is_none() {
            eprintln!("libx264 not available, skipping test");
            return;
        }

        // Generate synthetic MCAP with Image data
        let dir = tempdir().unwrap();
        let output_dir = Utf8PathBuf::try_from(dir.path().to_path_buf()).unwrap();

        let config = McapGeneratorConfig {
            num_frames: 10,
            fps: 30,
            image_size: (64, 64),
            generate_images: true,
            generate_tf: false,
            ..Default::default()
        };

        let generator = McapGenerator::new(config);
        let mcap_path = generator.generate(&output_dir).unwrap();

        // Ingest the MCAP to get image_data (JPEG encoded)
        let mut context = crate::core::Context::default();
        context.set_rosbag_path(mcap_path);
        let mut ingestor = Rosbag2Ingestor::new(Rosbag2IngestorConfig::without_metadata());
        let context = ingestor.run(context).unwrap();

        let image_data = context.image_data.expect("image_data should exist");
        let frames = image_data
            .get("/camera/image_raw")
            .expect("expected image frames for camera");

        let temp_dir = tempdir().unwrap();
        let output_path =
            Utf8PathBuf::from_path_buf(temp_dir.path().join("test_h264.mp4")).unwrap();

        // Test H.264 encoding with Fast preset
        let encoder_config = VideoEncoderConfig::new(30)
            .set_codec_config(CodecConfig::H264 {
                threads: Some(4),
                preset: X264Preset::Fast,
                tune: vec![],
            })
            .set_crf("23".to_string());
        let mut encoder = SoftwareVideoEncoder::new(&output_path, encoder_config);

        let frame_count = frames.len();
        for frame in frames {
            encoder.add_data(&frame.bytes).unwrap();
        }
        encoder.finish().unwrap();

        // Verify that the video file exists and has the correct number of frames
        assert!(output_path.exists());

        let ictx = ffmpeg::format::input(&output_path).unwrap();
        let input = ictx.streams().best(ffmpeg::media::Type::Video).unwrap();
        assert_eq!(input.frames(), frame_count as i64);
    }

    #[test]
    #[serial(ffmpeg)]
    fn test_software_video_encoder_h265() {
        // Skip if libx265 is not available
        if ffmpeg::encoder::find_by_name("libx265").is_none() {
            eprintln!("libx265 not available, skipping test");
            return;
        }

        // Generate synthetic MCAP with Image data
        let dir = tempdir().unwrap();
        let output_dir = Utf8PathBuf::try_from(dir.path().to_path_buf()).unwrap();

        let config = McapGeneratorConfig {
            num_frames: 10,
            fps: 30,
            image_size: (64, 64),
            generate_images: true,
            generate_tf: false,
            ..Default::default()
        };

        let generator = McapGenerator::new(config);
        let mcap_path = generator.generate(&output_dir).unwrap();

        // Ingest the MCAP to get image_data (JPEG encoded)
        let mut context = crate::core::Context::default();
        context.set_rosbag_path(mcap_path);
        let mut ingestor = Rosbag2Ingestor::new(Rosbag2IngestorConfig::without_metadata());
        let context = ingestor.run(context).unwrap();

        let image_data = context.image_data.expect("image_data should exist");
        let frames = image_data
            .get("/camera/image_raw")
            .expect("expected image frames for camera");

        let temp_dir = tempdir().unwrap();
        let output_path =
            Utf8PathBuf::from_path_buf(temp_dir.path().join("test_h265.mp4")).unwrap();

        // Test H.265 encoding with Veryfast preset
        let encoder_config = VideoEncoderConfig::new(30)
            .set_codec_config(CodecConfig::H265 {
                threads: Some(4),
                preset: X264Preset::Veryfast,
                tune: vec![],
                frame_threads: None,
            })
            .set_crf("28".to_string());
        let mut encoder = SoftwareVideoEncoder::new(&output_path, encoder_config);

        let frame_count = frames.len();
        for frame in frames {
            encoder.add_data(&frame.bytes).unwrap();
        }
        encoder.finish().unwrap();

        // Verify that the video file exists and has the correct number of frames
        assert!(output_path.exists());

        let ictx = ffmpeg::format::input(&output_path).unwrap();
        let input = ictx.streams().best(ffmpeg::media::Type::Video).unwrap();
        assert_eq!(input.frames(), frame_count as i64);
    }

    #[test]
    fn test_codec_config_serde() {
        // Test AV1 serialization/deserialization
        let av1_yaml = r#"
codec: AV1
lp: 2
pin: 0
preset: 8
"#;
        let av1: CodecConfig = serde_yaml::from_str(av1_yaml).unwrap();
        match av1 {
            CodecConfig::AV1 {
                lp, pin, preset, ..
            } => {
                assert_eq!(lp, Some(2));
                assert_eq!(pin, Some(0));
                assert_eq!(preset, 8);
            }
            _ => panic!("Expected AV1 codec config"),
        }

        // Test AV1 with new parameters (film-grain, lookahead, fast-decode)
        let av1_full_yaml = r#"
codec: AV1
lp: 4
preset: 6
film-grain: 10
film-grain-denoise: true
lookahead: 60
fast-decode: 1
"#;
        let av1_full: CodecConfig = serde_yaml::from_str(av1_full_yaml).unwrap();
        match av1_full {
            CodecConfig::AV1 {
                lp,
                preset,
                film_grain,
                film_grain_denoise,
                lookahead,
                fast_decode,
                ..
            } => {
                assert_eq!(lp, Some(4));
                assert_eq!(preset, 6);
                assert_eq!(film_grain, Some(10));
                assert_eq!(film_grain_denoise, Some(true));
                assert_eq!(lookahead, Some(60));
                assert_eq!(fast_decode, Some(1));
            }
            _ => panic!("Expected AV1 codec config"),
        }

        // Test H264 with case-insensitive alias
        let h264_yaml = r#"
codec: h264
threads: 4
preset: Fast
"#;
        let h264: CodecConfig = serde_yaml::from_str(h264_yaml).unwrap();
        match h264 {
            CodecConfig::H264 {
                threads,
                preset,
                tune,
            } => {
                assert_eq!(threads, Some(4));
                assert_eq!(preset, X264Preset::Fast);
                assert!(tune.is_empty());
            }
            _ => panic!("Expected H264 codec config"),
        }

        // Test H264 with tune
        let h264_tune_yaml = r#"
codec: H264
preset: Medium
tune: [Film, FastDecode]
"#;
        let h264_tune: CodecConfig = serde_yaml::from_str(h264_tune_yaml).unwrap();
        match h264_tune {
            CodecConfig::H264 { tune, .. } => {
                assert_eq!(tune.len(), 2);
                assert_eq!(tune[0], X264Tune::Film);
                assert_eq!(tune[1], X264Tune::FastDecode);
            }
            _ => panic!("Expected H264 codec config"),
        }

        // Test H265 with H.265 format
        let h265_yaml = r#"
codec: "H.265"
threads: 8
preset: Slow
"#;
        let h265: CodecConfig = serde_yaml::from_str(h265_yaml).unwrap();
        match h265 {
            CodecConfig::H265 {
                threads,
                preset,
                tune,
                ..
            } => {
                assert_eq!(threads, Some(8));
                assert_eq!(preset, X264Preset::Slow);
                assert!(tune.is_empty());
            }
            _ => panic!("Expected H265 codec config"),
        }

        // Test H265 with tune
        let h265_tune_yaml = r#"
codec: H265
preset: Fast
tune: [Grain, ZeroLatency]
"#;
        let h265_tune: CodecConfig = serde_yaml::from_str(h265_tune_yaml).unwrap();
        match h265_tune {
            CodecConfig::H265 { tune, .. } => {
                assert_eq!(tune.len(), 2);
                assert_eq!(tune[0], X265Tune::Grain);
                assert_eq!(tune[1], X265Tune::ZeroLatency);
            }
            _ => panic!("Expected H265 codec config"),
        }
    }

    #[test]
    fn test_nvenc_codec_config_serde_and_validation() {
        let h264_yaml = r#"
codec: h264_nvenc
qp: 24
preset: P5
tune: Hq
profile: high
b_frames: 2
rc_lookahead: 16
"#;
        let h264: CodecConfig = serde_yaml::from_str(h264_yaml).unwrap();
        assert_eq!(
            h264,
            CodecConfig::H264Nvenc {
                qp: 24,
                gpu: None,
                preset: NvencPreset::P5,
                tune: Some(NvencTune::Hq),
                profile: Some("high".to_string()),
                b_frames: 2,
                rc_lookahead: Some(16),
            }
        );
        assert_eq!(h264.ffmpeg_encoder_name(), "h264_nvenc");
        assert_eq!(h264.codec_family_name(), "h264");
        h264.validate().unwrap();

        let av1_yaml = r#"
codec: AV1_NVENC
qp: 80
gpu: 0
preset: p4
tune: ll
b-frames: 3
rc-lookahead: 24
"#;
        let av1: CodecConfig = serde_yaml::from_str(av1_yaml).unwrap();
        assert_eq!(
            av1,
            CodecConfig::Av1Nvenc {
                qp: 80,
                gpu: Some(0),
                preset: NvencPreset::P4,
                tune: Some(NvencTune::Ll),
                profile: None,
                b_frames: 3,
                rc_lookahead: Some(24),
            }
        );
        assert!(av1.is_nvenc());
        assert!(av1.uses_ffmpeg_cli_encoder());
        assert_eq!(av1.ffmpeg_encoder_name(), "av1_nvenc");
        assert_eq!(av1.codec_family_name(), "av1");
        av1.validate().unwrap();
    }

    #[test]
    fn test_h264_nvenc_serde_defaults_use_measured_general_profile() {
        let h264: CodecConfig = serde_yaml::from_str("codec: H264_NVENC").unwrap();
        assert_eq!(
            h264,
            CodecConfig::H264Nvenc {
                qp: DEFAULT_NVENC_H264_QP,
                gpu: None,
                preset: DEFAULT_NVENC_H264_PRESET,
                tune: Some(DEFAULT_NVENC_H264_TUNE),
                profile: Some(DEFAULT_NVENC_H264_PROFILE.to_string()),
                b_frames: DEFAULT_NVENC_H264_B_FRAMES,
                rc_lookahead: Some(DEFAULT_NVENC_H264_RC_LOOKAHEAD),
            }
        );
        h264.validate().unwrap();
    }

    #[test]
    fn test_av1_nvenc_yaml_uses_measured_defaults() {
        let av1: CodecConfig = serde_yaml::from_str("codec: AV1_NVENC\n").unwrap();
        assert_eq!(
            av1,
            CodecConfig::Av1Nvenc {
                qp: DEFAULT_NVENC_AV1_QP,
                gpu: None,
                preset: DEFAULT_NVENC_AV1_PRESET,
                tune: None,
                profile: None,
                b_frames: DEFAULT_NVENC_B_FRAMES,
                rc_lookahead: None,
            }
        );
        av1.validate().unwrap();
    }

    #[test]
    fn nvenc_b_frames_require_long_enough_gop() {
        let config =
            VideoEncoderConfig::new(30)
                .set_gop(2)
                .set_codec_config(CodecConfig::H264Nvenc {
                    qp: 24,
                    gpu: None,
                    preset: NvencPreset::P4,
                    tune: None,
                    profile: None,
                    b_frames: 2,
                    rc_lookahead: None,
                });

        let err = config.validate().unwrap_err();
        assert!(err.to_string().contains("gop must be greater"));
    }

    #[test]
    fn nvenc_rejects_b_frames_before_gop_arithmetic() {
        let config =
            VideoEncoderConfig::new(30)
                .set_gop(20)
                .set_codec_config(CodecConfig::H264Nvenc {
                    qp: 24,
                    gpu: None,
                    preset: NvencPreset::P4,
                    tune: None,
                    profile: None,
                    b_frames: u32::MAX,
                    rc_lookahead: None,
                });

        let err = config.validate().unwrap_err();
        assert!(err.to_string().contains("b_frames must be between"));
    }

    #[test]
    #[ignore = "requires NVIDIA GPU, nvidia-container-toolkit/runtime access, and FFmpeg NVENC encoders"]
    #[serial(ffmpeg)]
    fn nvenc_rgb_rawvideo_smoke_all_codecs() {
        if !nvenc_smoke_tests_enabled() {
            eprintln!("set REBAKE_RUN_NVENC_TESTS=1 to run NVENC smoke tests");
            return;
        }

        let temp_dir = tempdir().unwrap();
        for (name, codec_config) in nvenc_smoke_codec_configs() {
            let output_path =
                Utf8PathBuf::from_path_buf(temp_dir.path().join(format!("raw_{name}.mp4")))
                    .unwrap();
            let mut encoder =
                NvencVideoEncoder::new(&output_path, nvenc_smoke_config(codec_config));

            for frame_index in 0..3u8 {
                encoder
                    .add_frame(&nvenc_smoke_image(frame_index * 17))
                    .expect("NVENC rawvideo frame encode should succeed");
            }
            encoder
                .finish()
                .expect("NVENC rawvideo finish should succeed");
            assert_nonempty_file(&output_path);
        }
    }

    #[test]
    #[ignore = "requires NVIDIA GPU, nvidia-container-toolkit/runtime access, and FFmpeg NVENC encoders"]
    #[serial(ffmpeg)]
    fn nvenc_rgb_image2pipe_smoke_all_codecs() {
        if !nvenc_smoke_tests_enabled() {
            eprintln!("set REBAKE_RUN_NVENC_TESTS=1 to run NVENC smoke tests");
            return;
        }

        let temp_dir = tempdir().unwrap();
        for (name, codec_config) in nvenc_smoke_codec_configs() {
            let output_path =
                Utf8PathBuf::from_path_buf(temp_dir.path().join(format!("pipe_{name}.mp4")))
                    .unwrap();
            let mut encoder =
                NvencVideoEncoder::new(&output_path, nvenc_smoke_config(codec_config));

            for frame_index in 0..3u8 {
                let bytes = encode_png_bytes(&nvenc_smoke_image(frame_index * 23));
                encoder
                    .add_data(&bytes)
                    .expect("NVENC image2pipe frame encode should succeed");
            }
            encoder
                .finish()
                .expect("NVENC image2pipe finish should succeed");
            assert_nonempty_file(&output_path);
        }
    }

    #[test]
    #[ignore = "requires NVIDIA GPU, nvidia-container-toolkit/runtime access, and FFmpeg NVENC encoders"]
    #[serial(ffmpeg)]
    fn nvenc_rgb_b_frames_and_lookahead_smoke() {
        if !nvenc_smoke_tests_enabled() {
            eprintln!("set REBAKE_RUN_NVENC_TESTS=1 to run NVENC smoke tests");
            return;
        }

        let temp_dir = tempdir().unwrap();
        let cases = vec![
            (
                "h264",
                CodecConfig::H264Nvenc {
                    qp: 28,
                    gpu: None,
                    preset: NvencPreset::P4,
                    tune: Some(NvencTune::Hq),
                    profile: None,
                    b_frames: 2,
                    rc_lookahead: Some(8),
                },
            ),
            (
                "h265",
                CodecConfig::H265Nvenc {
                    qp: 30,
                    gpu: None,
                    preset: NvencPreset::P4,
                    tune: Some(NvencTune::Hq),
                    profile: None,
                    b_frames: 2,
                    rc_lookahead: Some(8),
                },
            ),
            (
                "av1",
                CodecConfig::Av1Nvenc {
                    qp: 130,
                    gpu: None,
                    preset: NvencPreset::P4,
                    tune: Some(NvencTune::Hq),
                    profile: None,
                    b_frames: 2,
                    rc_lookahead: Some(8),
                },
            ),
        ];

        for (name, codec_config) in cases {
            let output_path =
                Utf8PathBuf::from_path_buf(temp_dir.path().join(format!("b_frames_{name}.mp4")))
                    .unwrap();
            let config = VideoEncoderConfig::new(30)
                .set_gop(12)
                .set_codec_config(codec_config);
            let mut encoder = NvencVideoEncoder::new(&output_path, config);

            for frame_index in 0..12u8 {
                encoder
                    .add_frame(&nvenc_smoke_image(frame_index * 11))
                    .expect("NVENC B-frame frame encode should succeed");
            }
            encoder
                .finish()
                .expect("NVENC B-frame finish should succeed");
            assert_nonempty_file(&output_path);
        }
    }

    #[test]
    fn test_unknown_fields_rejected() {
        // VideoEncoderConfig should reject unknown fields (e.g., lp at top level)
        let invalid_config_yaml = r#"
fps: 30
gop: 2
crf: "30"
scaling: Bicubic
lp: 6
"#;
        let result: Result<VideoEncoderConfig, _> = serde_yaml::from_str(invalid_config_yaml);
        assert!(
            result.is_err(),
            "Should reject unknown field 'lp' at VideoEncoderConfig level"
        );
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("unknown field"),
            "Error should mention 'unknown field': {}",
            err_msg
        );

        // CodecConfig should reject unknown fields within codec_config
        let invalid_codec_yaml = r#"
codec: AV1
lp: 2
unknown_option: true
"#;
        let result: Result<CodecConfig, _> = serde_yaml::from_str(invalid_codec_yaml);
        assert!(
            result.is_err(),
            "Should reject unknown field 'unknown_option' in CodecConfig"
        );
    }

    #[test]
    fn test_codec_config_validation() {
        // AV1: Valid configuration
        let av1_valid = CodecConfig::AV1 {
            lp: Some(2),
            pin: None,
            preset: 6,
            film_grain: Some(10),
            film_grain_denoise: Some(true),
            lookahead: Some(60),
            fast_decode: Some(1),
        };
        assert!(av1_valid.validate().is_ok());

        // AV1: Invalid film-grain (out of range)
        let av1_invalid_fg = CodecConfig::AV1 {
            lp: None,
            pin: None,
            preset: 6,
            film_grain: Some(51), // Invalid: max is 50
            film_grain_denoise: None,
            lookahead: None,
            fast_decode: None,
        };
        let result = av1_invalid_fg.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().reason().contains("film-grain"));

        // AV1: Invalid lookahead (out of range)
        let av1_invalid_la = CodecConfig::AV1 {
            lp: None,
            pin: None,
            preset: 6,
            film_grain: None,
            film_grain_denoise: None,
            lookahead: Some(121), // Invalid: max is 120
            fast_decode: None,
        };
        let result = av1_invalid_la.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().reason().contains("lookahead"));

        // AV1: Invalid fast-decode (out of range)
        let av1_invalid_fd = CodecConfig::AV1 {
            lp: None,
            pin: None,
            preset: 6,
            film_grain: None,
            film_grain_denoise: None,
            lookahead: None,
            fast_decode: Some(3), // Invalid: max is 2
        };
        let result = av1_invalid_fd.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().reason().contains("fast-decode"));

        // H264: Valid combination (one PSY + one non-PSY)
        let h264_valid = CodecConfig::H264 {
            threads: None,
            preset: X264Preset::Medium,
            tune: vec![X264Tune::Film, X264Tune::FastDecode],
        };
        assert!(h264_valid.validate().is_ok());

        // H264: Invalid combination (two PSY tunings)
        let h264_invalid = CodecConfig::H264 {
            threads: None,
            preset: X264Preset::Medium,
            tune: vec![X264Tune::Film, X264Tune::Grain], // Invalid: two PSY
        };
        let result = h264_invalid.validate();
        assert!(result.is_err());
        let err_msg = result.unwrap_err().reason();
        assert!(err_msg.contains("PSY tuning"));
        assert!(err_msg.contains("film"));
        assert!(err_msg.contains("grain"));

        // H264: Valid (non-PSY only)
        let h264_non_psy = CodecConfig::H264 {
            threads: None,
            preset: X264Preset::Fast,
            tune: vec![X264Tune::FastDecode, X264Tune::ZeroLatency],
        };
        assert!(h264_non_psy.validate().is_ok());

        // H265: Valid combination
        let h265_valid = CodecConfig::H265 {
            threads: None,
            preset: X264Preset::Medium,
            tune: vec![X265Tune::Grain, X265Tune::ZeroLatency],
            frame_threads: None,
        };
        assert!(h265_valid.validate().is_ok());

        // H265: Invalid combination (two PSY tunings)
        let h265_invalid = CodecConfig::H265 {
            threads: None,
            preset: X264Preset::Medium,
            tune: vec![X265Tune::Psnr, X265Tune::Ssim], // Invalid: two PSY
            frame_threads: None,
        };
        let result = h265_invalid.validate();
        assert!(result.is_err());
        let err_msg = result.unwrap_err().reason();
        assert!(err_msg.contains("PSY tuning"));
    }

    #[test]
    fn test_video_metadata() {
        let config = VideoEncoderConfig {
            fps: 15,
            gop: 2,
            crf: "30".to_string(),
            scaling: ScalingFlag::Bicubic,
            resize: Some(ResizeConfig {
                width: 320,
                height: 240,
            }),
            codec_config: CodecConfig::H264Vaapi {
                qp: 23,
                device: None,
                profile: None,
                b_depth: None,
                async_depth: None,
            },
        };

        let metadata = config.video_metadata(640, 480).unwrap();

        assert_eq!(metadata.media_type, "rgb");
        assert_eq!(metadata.codec_family, "h264");
        assert_eq!(metadata.encoder_name, "h264_vaapi");
        assert_eq!(metadata.pix_fmt, "yuv420p");
        assert_eq!(metadata.width, 640);
        assert_eq!(metadata.height, 480);
        assert_eq!(metadata.fps, 15);

        let normalized: VideoEncoderConfig =
            serde_json::from_str(&metadata.encoding_config_json).unwrap();
        assert_eq!(normalized, config);
    }

    #[test]
    fn test_video_metadata_rejects_zero_dimensions() {
        let config = VideoEncoderConfig::default();

        let error = config.video_metadata(0, 480).unwrap_err();
        assert!(error.reason().contains("positive width and height"));
    }

    #[test]
    fn test_video_encoder_config_output_dimensions_use_resize_when_configured() {
        let config = VideoEncoderConfig::new(30).set_resize(Some(ResizeConfig {
            width: 320,
            height: 240,
        }));

        let output_dimensions = config.output_dimensions(1280, 720).unwrap();
        assert_eq!(output_dimensions, (320, 240));

        let output_shape = config.output_shape(ImageShape::new(720, 1280, 3)).unwrap();
        assert_eq!(output_shape, ImageShape::new(240, 320, 3));
    }

    #[test]
    fn test_video_encoder_config_rejects_odd_resize_dimensions() {
        let config = VideoEncoderConfig::new(30).set_resize(Some(ResizeConfig {
            width: 321,
            height: 240,
        }));

        let error = config.validate().unwrap_err();
        assert!(error.reason().contains("must be even"));
    }

    #[test]
    fn test_video_artifact() {
        let config = VideoEncoderConfig::default();

        let artifact = config
            .video_artifact("videos/camera.mp4", 320, 240)
            .unwrap();

        assert_eq!(artifact.video_path, "videos/camera.mp4");
        assert_eq!(artifact.metadata.media_type, "rgb");
        assert_eq!(artifact.metadata.width, 320);
        assert_eq!(artifact.metadata.height, 240);
    }

    #[test]
    fn test_default_video_encoder_config_uses_canonical_av1_defaults() {
        let config = VideoEncoderConfig::default();

        assert_eq!(config.fps, 100);
        assert_eq!(config.gop, 20);
        assert_eq!(config.crf, "34");
        assert_eq!(config.scaling, ScalingFlag::Bicubic);
        assert_eq!(config.resize, None);
        match config.codec_config {
            CodecConfig::AV1 {
                lp,
                pin,
                preset,
                film_grain,
                film_grain_denoise,
                lookahead,
                fast_decode,
            } => {
                assert_eq!(lp, None);
                assert_eq!(pin, None);
                assert_eq!(preset, 10);
                assert_eq!(film_grain, None);
                assert_eq!(film_grain_denoise, None);
                assert_eq!(lookahead, None);
                assert_eq!(fast_decode, None);
            }
            _ => panic!("Expected AV1 codec config"),
        }
    }

    #[test]
    fn test_tune_enum_as_str() {
        // X264Tune
        assert_eq!(X264Tune::Film.as_str(), "film");
        assert_eq!(X264Tune::Animation.as_str(), "animation");
        assert_eq!(X264Tune::Grain.as_str(), "grain");
        assert_eq!(X264Tune::StillImage.as_str(), "stillimage");
        assert_eq!(X264Tune::Psnr.as_str(), "psnr");
        assert_eq!(X264Tune::Ssim.as_str(), "ssim");
        assert_eq!(X264Tune::FastDecode.as_str(), "fastdecode");
        assert_eq!(X264Tune::ZeroLatency.as_str(), "zerolatency");

        // X265Tune
        assert_eq!(X265Tune::Psnr.as_str(), "psnr");
        assert_eq!(X265Tune::Ssim.as_str(), "ssim");
        assert_eq!(X265Tune::Grain.as_str(), "grain");
        assert_eq!(X265Tune::FastDecode.as_str(), "fastdecode");
        assert_eq!(X265Tune::ZeroLatency.as_str(), "zerolatency");
        assert_eq!(X265Tune::Animation.as_str(), "animation");
    }

    #[test]
    fn test_tune_is_psy() {
        // X264Tune PSY tunings
        assert!(X264Tune::Film.is_psy());
        assert!(X264Tune::Animation.is_psy());
        assert!(X264Tune::Grain.is_psy());
        assert!(X264Tune::StillImage.is_psy());
        assert!(X264Tune::Psnr.is_psy());
        assert!(X264Tune::Ssim.is_psy());
        // X264Tune non-PSY tunings
        assert!(!X264Tune::FastDecode.is_psy());
        assert!(!X264Tune::ZeroLatency.is_psy());

        // X265Tune PSY tunings
        assert!(X265Tune::Psnr.is_psy());
        assert!(X265Tune::Ssim.is_psy());
        assert!(X265Tune::Grain.is_psy());
        assert!(X265Tune::Animation.is_psy());
        // X265Tune non-PSY tunings
        assert!(!X265Tune::FastDecode.is_psy());
        assert!(!X265Tune::ZeroLatency.is_psy());
    }

    // =========================================================================
    // Drop Implementation Tests
    // =========================================================================

    #[test]
    #[serial(ffmpeg)]
    fn test_software_video_encoder_drop_without_finish() {
        // Drop without calling finish() should not panic and should produce a valid file
        let dir = tempdir().unwrap();
        let output_dir = Utf8PathBuf::try_from(dir.path().to_path_buf()).unwrap();

        let config = McapGeneratorConfig {
            num_frames: 5,
            fps: 30,
            image_size: (64, 64),
            generate_images: true,
            generate_tf: false,
            ..Default::default()
        };

        let generator = McapGenerator::new(config);
        let mcap_path = generator.generate(&output_dir).unwrap();

        let mut context = crate::core::Context::default();
        context.set_rosbag_path(mcap_path);
        let mut ingestor = Rosbag2Ingestor::new(Rosbag2IngestorConfig::without_metadata());
        let context = ingestor.run(context).unwrap();

        let image_data = context.image_data.expect("image_data should exist");
        let frames = image_data
            .get("/camera/image_raw")
            .expect("expected image frames");

        let temp_dir = tempdir().unwrap();
        let output_path =
            Utf8PathBuf::from_path_buf(temp_dir.path().join("test_drop.mp4")).unwrap();

        {
            let encoder_config = VideoEncoderConfig::default();
            let mut encoder = SoftwareVideoEncoder::new(&output_path, encoder_config);

            for frame in frames {
                encoder.add_data(&frame.bytes).unwrap();
            }
            // Exit scope without calling finish() -> Drop is called
        }

        // Verify that file exists and is playable
        assert!(output_path.exists());
        let ictx = ffmpeg::format::input(&output_path).unwrap();
        let input = ictx.streams().best(ffmpeg::media::Type::Video).unwrap();
        assert!(input.frames() > 0);
    }

    #[test]
    #[serial(ffmpeg)]
    fn test_software_video_encoder_drop_after_finish() {
        // finish() called, then drop - should not do anything in drop
        let dir = tempdir().unwrap();
        let output_dir = Utf8PathBuf::try_from(dir.path().to_path_buf()).unwrap();

        let config = McapGeneratorConfig {
            num_frames: 5,
            fps: 30,
            image_size: (64, 64),
            generate_images: true,
            generate_tf: false,
            ..Default::default()
        };

        let generator = McapGenerator::new(config);
        let mcap_path = generator.generate(&output_dir).unwrap();

        let mut context = crate::core::Context::default();
        context.set_rosbag_path(mcap_path);
        let mut ingestor = Rosbag2Ingestor::new(Rosbag2IngestorConfig::without_metadata());
        let context = ingestor.run(context).unwrap();

        let image_data = context.image_data.expect("image_data should exist");
        let frames = image_data
            .get("/camera/image_raw")
            .expect("expected image frames");
        let frame_count = frames.len();

        let temp_dir = tempdir().unwrap();
        let output_path =
            Utf8PathBuf::from_path_buf(temp_dir.path().join("test_finish_then_drop.mp4")).unwrap();

        {
            let encoder_config = VideoEncoderConfig::default();
            let mut encoder = SoftwareVideoEncoder::new(&output_path, encoder_config);

            for frame in frames {
                encoder.add_data(&frame.bytes).unwrap();
            }
            encoder.finish().unwrap(); // Explicitly call finish
            // Drop is called after this, but it does nothing
        }

        assert!(output_path.exists());
        let ictx = ffmpeg::format::input(&output_path).unwrap();
        let input = ictx.streams().best(ffmpeg::media::Type::Video).unwrap();
        assert_eq!(input.frames(), frame_count as i64);
    }

    #[test]
    #[serial(ffmpeg)]
    fn test_software_video_encoder_drop_during_panic() {
        // Verify that Drop is called even during panic unwinding
        // This checks that resources are released even during panic
        use std::panic::catch_unwind;

        let temp_dir = tempdir().unwrap();
        let output_path =
            Utf8PathBuf::from_path_buf(temp_dir.path().join("panic_test.mp4")).unwrap();

        // Create a simple test image
        let img = image::RgbImage::from_fn(64, 64, |x, y| image::Rgb([x as u8, y as u8, 128]));
        let dynamic_img = image::DynamicImage::ImageRgb8(img);

        let result = catch_unwind(std::panic::AssertUnwindSafe(|| {
            let config = VideoEncoderConfig::default();
            let mut encoder = SoftwareVideoEncoder::new(&output_path, config);

            // Add a frame to initialize the encoder
            encoder.add_frame(&dynamic_img).unwrap();

            // Simulate a panic during processing
            panic!("intentional panic for testing Drop behavior");
        }));

        // Verify that panic occurred
        assert!(result.is_err(), "Expected panic to occur");

        // Verify that file was created (Drop was called)
        // Note: The file may be incomplete, but it should exist
        assert!(
            output_path.exists(),
            "Output file should exist after Drop (even if incomplete)"
        );
    }

    /// Edge case: returns Ok even when image_data is None (only video_cache_dir is set)
    #[test]
    fn test_video_encoder_handles_missing_image_data_gracefully() {
        let dir = tempdir().unwrap();
        let output_dir = Utf8PathBuf::try_from(dir.path().to_path_buf()).unwrap();

        let config = VideoEncoderConfig::default();
        let mut encoder = VideoEncoder::new(config);

        let mut context = crate::core::Context::default();
        // image_data is not set (None)
        // airoa_metadata is required, so set it (requires argument)
        context.set_airoa_metadata(AiroaMetadata::V2_0(create_test_metadata(
            "missing-image-data",
        )));
        context.set_video_cache_dir(output_dir);

        let result = encoder.run(context);

        // Returns Ok even when image_data=None
        assert!(result.is_ok());
        let ctx = result.unwrap();
        assert!(ctx.video_cache_dir().is_some());
    }

    #[test]
    #[serial(ffmpeg)]
    fn test_vaapi_encoder_h264() {
        use image::{DynamicImage, RgbImage};

        if !is_vaapi_available() {
            eprintln!("VA-API not available, skipping test");
            return;
        }

        eprintln!("Testing VaapiVideoEncoder with H264...");

        let temp_dir = tempdir().unwrap();
        let output_path =
            Utf8PathBuf::from_path_buf(temp_dir.path().join("test_vaapi_h264.mp4")).unwrap();

        // Create encoder config with H264 VAAPI
        let config =
            VideoEncoderConfig::new(30)
                .set_gop(2)
                .set_codec_config(CodecConfig::H264Vaapi {
                    qp: 23,
                    device: None,
                    profile: None,
                    b_depth: None,
                    async_depth: None,
                });

        let mut encoder = VaapiVideoEncoder::new(&output_path, config);

        // Create and add test frames
        for i in 0..30u8 {
            let mut img = RgbImage::new(1280, 720);
            for (_x, _y, pixel) in img.enumerate_pixels_mut() {
                pixel.0 = [i.wrapping_mul(8), 128, 64];
            }
            let dyn_img = DynamicImage::ImageRgb8(img);
            encoder.add_frame(&dyn_img).unwrap();
        }

        // Finish encoding
        encoder.finish().unwrap();

        // Verify output
        assert!(output_path.exists(), "Output file should exist");
        let metadata = std::fs::metadata(&output_path).unwrap();
        assert!(metadata.len() > 0, "Output file should not be empty");
        eprintln!("VA-API H264 encoding succeeded: {} bytes", metadata.len());
    }

    #[test]
    #[serial(ffmpeg)]
    fn test_vaapi_encoder_h264_resizes_output() {
        use image::{DynamicImage, RgbImage};

        if !is_vaapi_available() {
            eprintln!("VA-API not available, skipping test");
            return;
        }

        let temp_dir = tempdir().unwrap();
        let output_path =
            Utf8PathBuf::from_path_buf(temp_dir.path().join("test_vaapi_h264_resized.mp4"))
                .unwrap();

        let config = VideoEncoderConfig::new(30)
            .set_gop(2)
            .set_resize(Some(ResizeConfig {
                width: 640,
                height: 360,
            }))
            .set_codec_config(CodecConfig::H264Vaapi {
                qp: 23,
                device: None,
                profile: None,
                b_depth: None,
                async_depth: None,
            });

        let mut encoder = VaapiVideoEncoder::new(&output_path, config);

        for i in 0..30u8 {
            let mut img = RgbImage::new(1280, 720);
            for (_x, _y, pixel) in img.enumerate_pixels_mut() {
                pixel.0 = [i.wrapping_mul(8), 128, 64];
            }
            let dyn_img = DynamicImage::ImageRgb8(img);
            encoder.add_frame(&dyn_img).unwrap();
        }

        encoder.finish().unwrap();

        let ictx = ffmpeg::format::input(&output_path).unwrap();
        let input = ictx.streams().best(ffmpeg::media::Type::Video).unwrap();
        let decoder_context =
            ffmpeg::codec::context::Context::from_parameters(input.parameters()).unwrap();
        let decoder = decoder_context.decoder().video().unwrap();

        assert_eq!(decoder.width(), 640);
        assert_eq!(decoder.height(), 360);
    }

    #[test]
    #[serial(ffmpeg)]
    fn test_vaapi_encoder_h264_resizes_output_via_image2pipe() {
        use image::{DynamicImage, ImageFormat as ExternalImageFormat, RgbImage};
        use std::io::Cursor;

        if !is_vaapi_available() {
            eprintln!("VA-API not available, skipping test");
            return;
        }

        let temp_dir = tempdir().unwrap();
        let output_path =
            Utf8PathBuf::from_path_buf(temp_dir.path().join("test_vaapi_h264_resized_pipe.mp4"))
                .unwrap();

        let config = VideoEncoderConfig::new(30)
            .set_gop(2)
            .set_resize(Some(ResizeConfig {
                width: 640,
                height: 360,
            }))
            .set_codec_config(CodecConfig::H264Vaapi {
                qp: 23,
                device: None,
                profile: None,
                b_depth: None,
                async_depth: None,
            });

        let mut encoder = VaapiVideoEncoder::new(&output_path, config);

        for i in 0..30u8 {
            let mut img = RgbImage::new(1280, 720);
            for (_x, _y, pixel) in img.enumerate_pixels_mut() {
                pixel.0 = [i.wrapping_mul(8), 128, 64];
            }

            let mut encoded = Cursor::new(Vec::new());
            DynamicImage::ImageRgb8(img)
                .write_to(&mut encoded, ExternalImageFormat::Jpeg)
                .unwrap();
            encoder.add_data(&encoded.into_inner()).unwrap();
        }

        encoder.finish().unwrap();

        let ictx = ffmpeg::format::input(&output_path).unwrap();
        let input = ictx.streams().best(ffmpeg::media::Type::Video).unwrap();
        let decoder_context =
            ffmpeg::codec::context::Context::from_parameters(input.parameters()).unwrap();
        let decoder = decoder_context.decoder().video().unwrap();

        assert_eq!(decoder.width(), 640);
        assert_eq!(decoder.height(), 360);
    }

    #[test]
    #[serial(ffmpeg)]
    fn test_vaapi_encoder_h265_resizes_output() {
        use image::{DynamicImage, RgbImage};

        if !is_vaapi_available() {
            eprintln!("VA-API not available, skipping test");
            return;
        }

        let temp_dir = tempdir().unwrap();
        let output_path =
            Utf8PathBuf::from_path_buf(temp_dir.path().join("test_vaapi_h265_resized.mp4"))
                .unwrap();

        let config = VideoEncoderConfig::new(30)
            .set_gop(2)
            .set_resize(Some(ResizeConfig {
                width: 640,
                height: 360,
            }))
            .set_codec_config(CodecConfig::H265Vaapi {
                qp: 28,
                device: None,
                profile: None,
                async_depth: None,
            });

        let mut encoder = VaapiVideoEncoder::new(&output_path, config);

        for i in 0..30u8 {
            let mut img = RgbImage::new(1280, 720);
            for (_x, _y, pixel) in img.enumerate_pixels_mut() {
                pixel.0 = [i.wrapping_mul(8), 128, 64];
            }
            let dyn_img = DynamicImage::ImageRgb8(img);
            encoder.add_frame(&dyn_img).unwrap();
        }

        encoder.finish().unwrap();

        let ictx = ffmpeg::format::input(&output_path).unwrap();
        let input = ictx.streams().best(ffmpeg::media::Type::Video).unwrap();
        let decoder_context =
            ffmpeg::codec::context::Context::from_parameters(input.parameters()).unwrap();
        let decoder = decoder_context.decoder().video().unwrap();

        assert_eq!(decoder.width(), 640);
        assert_eq!(decoder.height(), 360);
    }

    #[test]
    #[serial(ffmpeg)]
    fn test_vaapi_encoder_av1() {
        use image::{DynamicImage, RgbImage};

        if !is_vaapi_available() {
            eprintln!("VA-API not available, skipping test");
            return;
        }

        eprintln!("Testing VaapiVideoEncoder with AV1...");

        let temp_dir = tempdir().unwrap();
        let output_path =
            Utf8PathBuf::from_path_buf(temp_dir.path().join("test_vaapi_av1.mp4")).unwrap();

        // Create encoder config with AV1 VAAPI
        let config =
            VideoEncoderConfig::new(30)
                .set_gop(2)
                .set_codec_config(CodecConfig::Av1Vaapi {
                    qp: 128,
                    device: None,
                    profile: None,
                    b_depth: None,
                    async_depth: None,
                });

        let mut encoder = VaapiVideoEncoder::new(&output_path, config);

        // Create and add test frames
        for i in 0..30u8 {
            let mut img = RgbImage::new(1280, 720);
            for (_x, _y, pixel) in img.enumerate_pixels_mut() {
                pixel.0 = [i.wrapping_mul(8), 128, 64];
            }
            let dyn_img = DynamicImage::ImageRgb8(img);
            encoder.add_frame(&dyn_img).unwrap();
        }

        // Finish encoding
        encoder.finish().unwrap();

        // Verify output
        assert!(output_path.exists(), "Output file should exist");
        let metadata = std::fs::metadata(&output_path).unwrap();
        assert!(metadata.len() > 0, "Output file should not be empty");
        eprintln!("VA-API AV1 encoding succeeded: {} bytes", metadata.len());
    }

    #[test]
    #[serial(ffmpeg)]
    fn test_vaapi_encoder_av1_resizes_output() {
        use image::{DynamicImage, RgbImage};

        if !is_vaapi_available() {
            eprintln!("VA-API not available, skipping test");
            return;
        }

        let temp_dir = tempdir().unwrap();
        let output_path =
            Utf8PathBuf::from_path_buf(temp_dir.path().join("test_vaapi_av1_resized.mp4")).unwrap();

        let config = VideoEncoderConfig::new(30)
            .set_gop(2)
            .set_resize(Some(ResizeConfig {
                width: 640,
                height: 360,
            }))
            .set_codec_config(CodecConfig::Av1Vaapi {
                qp: 128,
                device: None,
                profile: None,
                b_depth: None,
                async_depth: None,
            });

        let mut encoder = VaapiVideoEncoder::new(&output_path, config);

        for i in 0..30u8 {
            let mut img = RgbImage::new(1280, 720);
            for (_x, _y, pixel) in img.enumerate_pixels_mut() {
                pixel.0 = [i.wrapping_mul(8), 128, 64];
            }
            let dyn_img = DynamicImage::ImageRgb8(img);
            encoder.add_frame(&dyn_img).unwrap();
        }

        encoder.finish().unwrap();

        let ictx = ffmpeg::format::input(&output_path).unwrap();
        let input = ictx.streams().best(ffmpeg::media::Type::Video).unwrap();
        let decoder_context =
            ffmpeg::codec::context::Context::from_parameters(input.parameters()).unwrap();
        let decoder = decoder_context.decoder().video().unwrap();

        assert_eq!(decoder.width(), 640);
        assert_eq!(decoder.height(), 360);
    }

    // =========================================================================
    // ImageFormat Detection Tests
    // =========================================================================

    #[test]
    fn test_image_format_detect_jpeg() {
        // Standard JPEG header with JFIF marker: FF D8 FF E0
        let jpeg_data = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, 0x4A, 0x46, 0x49, 0x46];
        assert_eq!(ImageFormat::detect(&jpeg_data), Some(ImageFormat::Jpeg));

        // Minimal valid JPEG detection (4 bytes with FF D8 FF xx)
        let jpeg_minimal = vec![0xFF, 0xD8, 0xFF, 0xE1];
        assert_eq!(ImageFormat::detect(&jpeg_minimal), Some(ImageFormat::Jpeg));
    }

    #[test]
    fn test_image_format_detect_png() {
        // Standard PNG header: 89 50 4E 47 0D 0A 1A 0A
        let png_data = vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00];
        assert_eq!(ImageFormat::detect(&png_data), Some(ImageFormat::Png));
    }

    #[test]
    fn test_image_format_detect_unknown() {
        // Unknown format
        let unknown_data = vec![0x00, 0x01, 0x02, 0x03, 0x04, 0x05];
        assert_eq!(ImageFormat::detect(&unknown_data), None);

        // Too short data
        let short_data = vec![0xFF, 0xD8];
        assert_eq!(ImageFormat::detect(&short_data), None);

        // Empty data
        let empty_data: Vec<u8> = vec![];
        assert_eq!(ImageFormat::detect(&empty_data), None);
    }

    #[test]
    fn test_image_format_ffmpeg_decoder() {
        assert_eq!(ImageFormat::Jpeg.ffmpeg_decoder(), "mjpeg");
        assert_eq!(ImageFormat::Png.ffmpeg_decoder(), "png");
    }

    #[test]
    fn test_scaling_flag_ffmpeg_cli_scale_flag_omits_param_default() {
        assert_eq!(
            ScalingFlag::ParamDefault.ffmpeg_cli_scale_flag().unwrap(),
            None
        );
    }

    #[test]
    fn test_scaling_flag_ffmpeg_cli_scale_flag_rejects_unsupported_flag() {
        let error = ScalingFlag::DirectBgr.ffmpeg_cli_scale_flag().unwrap_err();
        assert!(
            error
                .reason()
                .contains("is not supported by FFmpeg CLI resize filters")
        );
    }

    #[test]
    fn test_video_encoder_config_rejects_vaapi_resize_with_unsupported_scaling_flag() {
        let config = VideoEncoderConfig::new(30)
            .set_scaling(ScalingFlag::DirectBgr)
            .set_resize(Some(ResizeConfig {
                width: 320,
                height: 240,
            }))
            .set_codec_config(CodecConfig::H264Vaapi {
                qp: 23,
                device: None,
                profile: None,
                b_depth: None,
                async_depth: None,
            });

        let error = config.validate().unwrap_err();
        assert!(
            error
                .reason()
                .contains("is not supported by FFmpeg CLI resize filters")
        );
    }

    #[test]
    fn test_vaapi_video_filter_omits_flags_for_param_default() {
        let config = VideoEncoderConfig::new(30)
            .set_scaling(ScalingFlag::ParamDefault)
            .set_resize(Some(ResizeConfig {
                width: 320,
                height: 240,
            }))
            .set_codec_config(CodecConfig::H264Vaapi {
                qp: 23,
                device: None,
                profile: None,
                b_depth: None,
                async_depth: None,
            });
        let temp_dir = tempdir().unwrap();
        let output_path = Utf8PathBuf::from_path_buf(temp_dir.path().join("test.mp4")).unwrap();
        let encoder = VaapiVideoEncoder::new(&output_path, config);

        assert_eq!(
            encoder.ffmpeg_video_filter().unwrap(),
            "scale=320:240,format=nv12,hwupload"
        );
    }

    #[test]
    fn test_input_mode_initial_state() {
        let config = VideoEncoderConfig::new(30).set_codec_config(CodecConfig::H264Vaapi {
            qp: 23,
            device: None,
            profile: None,
            b_depth: None,
            async_depth: None,
        });
        let temp_dir = tempdir().unwrap();
        let output_path = Utf8PathBuf::from_path_buf(temp_dir.path().join("test.mp4")).unwrap();
        let encoder = VaapiVideoEncoder::new(&output_path, config);

        // Initial state should be Unknown
        assert_eq!(encoder.input_mode, InputMode::Unknown);
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod yaml_file_tests {
    use super::*;

    #[test]
    fn test_parse_h264_yaml_config() {
        let yaml = r#"
fps: 30
gop: 10
crf: "23"
scaling: Bicubic
resize:
  width: 320
  height: 240
codec_config:
  codec: H264
  preset: medium
"#;
        let config: VideoEncoderConfig = serde_yaml::from_str(yaml).expect("Failed to parse YAML");
        assert_eq!(config.fps, 30);
        assert_eq!(config.gop, 10);
        assert_eq!(config.crf, "23");
        assert_eq!(
            config.resize,
            Some(ResizeConfig {
                width: 320,
                height: 240,
            })
        );
        match &config.codec_config {
            CodecConfig::H264 { preset, .. } => {
                assert_eq!(*preset, X264Preset::Medium);
            }
            _ => panic!("Expected H264 config"),
        }
    }

    #[test]
    fn test_parse_h265_yaml_config() {
        let yaml = r#"
fps: 30
gop: 10
crf: "28"
scaling: Bicubic
codec_config:
  codec: H265
  preset: medium
"#;
        let config: VideoEncoderConfig = serde_yaml::from_str(yaml).expect("Failed to parse YAML");
        match &config.codec_config {
            CodecConfig::H265 { preset, .. } => {
                assert_eq!(*preset, X264Preset::Medium);
            }
            _ => panic!("Expected H265 config"),
        }
    }

    #[test]
    fn test_parse_av1_yaml_config() {
        let yaml = r#"
fps: 30
gop: 10
crf: "28"
scaling: Bicubic
codec_config:
  codec: AV1
  preset: 6
"#;
        let config: VideoEncoderConfig = serde_yaml::from_str(yaml).expect("Failed to parse YAML");
        match &config.codec_config {
            CodecConfig::AV1 { preset, .. } => {
                assert_eq!(*preset, 6);
            }
            _ => panic!("Expected AV1 config"),
        }
    }

    #[test]
    fn test_parse_h264_vaapi_yaml_config() {
        let yaml = r#"
fps: 30
gop: 10
crf: "23"
scaling: Bicubic
codec_config:
  codec: H264_VAAPI
  qp: 23
"#;
        let config: VideoEncoderConfig = serde_yaml::from_str(yaml).expect("Failed to parse YAML");
        match &config.codec_config {
            CodecConfig::H264Vaapi { qp, .. } => {
                assert_eq!(*qp, 23);
            }
            _ => panic!("Expected H264_VAAPI config"),
        }
    }
}
