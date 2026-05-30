use std::error::Error;
use std::io;
use std::mem;

use pyo3::Bound;
use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;

use crate::core::PyContext;
use rebake::core::stage::{Context, Stage};
use rebake::encode::nvenc::{
    DEFAULT_NVENC_AV1_QP, DEFAULT_NVENC_B_FRAMES, DEFAULT_NVENC_H264_B_FRAMES,
    DEFAULT_NVENC_H264_PROFILE, DEFAULT_NVENC_H264_QP, DEFAULT_NVENC_H264_RC_LOOKAHEAD,
    DEFAULT_NVENC_H264_TUNE, NvencPreset, NvencTune,
};
use rebake::encode::video_artifact::VideoArtifact;
use rebake::encode::video_encoder::{
    CodecConfig, ScalingFlag, VideoEncoder, VideoEncoderConfig, X264Preset, X264Tune, X265Tune,
    is_vaapi_available,
};

#[pyclass(module = "rebake.encode", eq, eq_int)]
#[derive(Clone, PartialEq)]
pub enum PyScalingFlag {
    FastBilinear,
    Bilinear,
    Bicubic,
    Bicublin,
    Gauss,
    Sinc,
    Lanczos,
    Spline,
    SrcVChrDropMask,
    SrcVChrDropShift,
    ParamDefault,
    PrintInfo,
    FullChrHInt,
    FullChrHInp,
    DirectBgr,
    AccurateRnd,
    BitExact,
    ErrorDiffusion,
}

impl From<PyScalingFlag> for ScalingFlag {
    fn from(flag: PyScalingFlag) -> Self {
        match flag {
            PyScalingFlag::FastBilinear => ScalingFlag::FastBilinear,
            PyScalingFlag::Bilinear => ScalingFlag::Bilinear,
            PyScalingFlag::Bicubic => ScalingFlag::Bicubic,
            PyScalingFlag::Bicublin => ScalingFlag::Bicublin,
            PyScalingFlag::Gauss => ScalingFlag::Gauss,
            PyScalingFlag::Sinc => ScalingFlag::Sinc,
            PyScalingFlag::Lanczos => ScalingFlag::Lanczos,
            PyScalingFlag::Spline => ScalingFlag::Spline,
            PyScalingFlag::SrcVChrDropMask => ScalingFlag::SrcVChrDropMask,
            PyScalingFlag::SrcVChrDropShift => ScalingFlag::SrcVChrDropShift,
            PyScalingFlag::ParamDefault => ScalingFlag::ParamDefault,
            PyScalingFlag::PrintInfo => ScalingFlag::PrintInfo,
            PyScalingFlag::FullChrHInt => ScalingFlag::FullChrHInt,
            PyScalingFlag::FullChrHInp => ScalingFlag::FullChrHInp,
            PyScalingFlag::DirectBgr => ScalingFlag::DirectBgr,
            PyScalingFlag::AccurateRnd => ScalingFlag::AccurateRnd,
            PyScalingFlag::BitExact => ScalingFlag::BitExact,
            PyScalingFlag::ErrorDiffusion => ScalingFlag::ErrorDiffusion,
        }
    }
}

impl From<ScalingFlag> for PyScalingFlag {
    fn from(flag: ScalingFlag) -> Self {
        match flag {
            ScalingFlag::FastBilinear => PyScalingFlag::FastBilinear,
            ScalingFlag::Bilinear => PyScalingFlag::Bilinear,
            ScalingFlag::Bicubic => PyScalingFlag::Bicubic,
            ScalingFlag::Bicublin => PyScalingFlag::Bicublin,
            ScalingFlag::Gauss => PyScalingFlag::Gauss,
            ScalingFlag::Sinc => PyScalingFlag::Sinc,
            ScalingFlag::Lanczos => PyScalingFlag::Lanczos,
            ScalingFlag::Spline => PyScalingFlag::Spline,
            ScalingFlag::SrcVChrDropMask => PyScalingFlag::SrcVChrDropMask,
            ScalingFlag::SrcVChrDropShift => PyScalingFlag::SrcVChrDropShift,
            ScalingFlag::ParamDefault => PyScalingFlag::ParamDefault,
            ScalingFlag::PrintInfo => PyScalingFlag::PrintInfo,
            ScalingFlag::FullChrHInt => PyScalingFlag::FullChrHInt,
            ScalingFlag::FullChrHInp => PyScalingFlag::FullChrHInp,
            ScalingFlag::DirectBgr => PyScalingFlag::DirectBgr,
            ScalingFlag::AccurateRnd => PyScalingFlag::AccurateRnd,
            ScalingFlag::BitExact => PyScalingFlag::BitExact,
            ScalingFlag::ErrorDiffusion => PyScalingFlag::ErrorDiffusion,
        }
    }
}

/// x264/x265 encoder preset for speed vs compression tradeoff.
#[pyclass(module = "rebake.encode", eq, eq_int)]
#[derive(Clone, Default, PartialEq)]
pub enum PyX264Preset {
    Ultrafast,
    Superfast,
    Veryfast,
    Faster,
    Fast,
    #[default]
    Medium,
    Slow,
    Slower,
    Veryslow,
}

impl From<PyX264Preset> for X264Preset {
    fn from(preset: PyX264Preset) -> Self {
        match preset {
            PyX264Preset::Ultrafast => X264Preset::Ultrafast,
            PyX264Preset::Superfast => X264Preset::Superfast,
            PyX264Preset::Veryfast => X264Preset::Veryfast,
            PyX264Preset::Faster => X264Preset::Faster,
            PyX264Preset::Fast => X264Preset::Fast,
            PyX264Preset::Medium => X264Preset::Medium,
            PyX264Preset::Slow => X264Preset::Slow,
            PyX264Preset::Slower => X264Preset::Slower,
            PyX264Preset::Veryslow => X264Preset::Veryslow,
        }
    }
}

impl From<X264Preset> for PyX264Preset {
    fn from(preset: X264Preset) -> Self {
        match preset {
            X264Preset::Ultrafast => PyX264Preset::Ultrafast,
            X264Preset::Superfast => PyX264Preset::Superfast,
            X264Preset::Veryfast => PyX264Preset::Veryfast,
            X264Preset::Faster => PyX264Preset::Faster,
            X264Preset::Fast => PyX264Preset::Fast,
            X264Preset::Medium => PyX264Preset::Medium,
            X264Preset::Slow => PyX264Preset::Slow,
            X264Preset::Slower => PyX264Preset::Slower,
            X264Preset::Veryslow => PyX264Preset::Veryslow,
        }
    }
}

/// x264 (H.264) encoder tuning options.
///
/// PSY tunings (mutually exclusive): Film, Animation, Grain, StillImage, Psnr, Ssim
/// Non-PSY tunings (can combine with one PSY): FastDecode, ZeroLatency
#[pyclass(module = "rebake.encode", eq, eq_int)]
#[derive(Clone, PartialEq)]
pub enum PyX264Tune {
    /// High-quality movie content. Lowers deblocking.
    Film,
    /// Cartoon/anime with large flat areas.
    Animation,
    /// Preserve film grain.
    Grain,
    /// Still image encoding.
    StillImage,
    /// Optimize for PSNR metric (benchmarking).
    Psnr,
    /// Optimize for SSIM metric (benchmarking).
    Ssim,
    /// Faster decoding. Disables CABAC, deblocking, and weighted prediction.
    FastDecode,
    /// Zero latency streaming. Disables B-frames, lookahead, and mbtree.
    ZeroLatency,
}

impl From<PyX264Tune> for X264Tune {
    fn from(tune: PyX264Tune) -> Self {
        match tune {
            PyX264Tune::Film => X264Tune::Film,
            PyX264Tune::Animation => X264Tune::Animation,
            PyX264Tune::Grain => X264Tune::Grain,
            PyX264Tune::StillImage => X264Tune::StillImage,
            PyX264Tune::Psnr => X264Tune::Psnr,
            PyX264Tune::Ssim => X264Tune::Ssim,
            PyX264Tune::FastDecode => X264Tune::FastDecode,
            PyX264Tune::ZeroLatency => X264Tune::ZeroLatency,
        }
    }
}

impl From<X264Tune> for PyX264Tune {
    fn from(tune: X264Tune) -> Self {
        match tune {
            X264Tune::Film => PyX264Tune::Film,
            X264Tune::Animation => PyX264Tune::Animation,
            X264Tune::Grain => PyX264Tune::Grain,
            X264Tune::StillImage => PyX264Tune::StillImage,
            X264Tune::Psnr => PyX264Tune::Psnr,
            X264Tune::Ssim => PyX264Tune::Ssim,
            X264Tune::FastDecode => PyX264Tune::FastDecode,
            X264Tune::ZeroLatency => PyX264Tune::ZeroLatency,
        }
    }
}

/// x265 (H.265/HEVC) encoder tuning options.
///
/// PSY tunings (mutually exclusive): Psnr, Ssim, Grain, Animation
/// Non-PSY tunings (can combine with one PSY): FastDecode, ZeroLatency
#[pyclass(module = "rebake.encode", eq, eq_int)]
#[derive(Clone, PartialEq)]
pub enum PyX265Tune {
    /// Optimize for PSNR metric.
    Psnr,
    /// Optimize for SSIM metric.
    Ssim,
    /// Preserve film grain.
    Grain,
    /// Faster decoding.
    FastDecode,
    /// Zero latency streaming.
    ZeroLatency,
    /// Optimized for animated content.
    Animation,
}

impl From<PyX265Tune> for X265Tune {
    fn from(tune: PyX265Tune) -> Self {
        match tune {
            PyX265Tune::Psnr => X265Tune::Psnr,
            PyX265Tune::Ssim => X265Tune::Ssim,
            PyX265Tune::Grain => X265Tune::Grain,
            PyX265Tune::FastDecode => X265Tune::FastDecode,
            PyX265Tune::ZeroLatency => X265Tune::ZeroLatency,
            PyX265Tune::Animation => X265Tune::Animation,
        }
    }
}

impl From<X265Tune> for PyX265Tune {
    fn from(tune: X265Tune) -> Self {
        match tune {
            X265Tune::Psnr => PyX265Tune::Psnr,
            X265Tune::Ssim => PyX265Tune::Ssim,
            X265Tune::Grain => PyX265Tune::Grain,
            X265Tune::FastDecode => PyX265Tune::FastDecode,
            X265Tune::ZeroLatency => PyX265Tune::ZeroLatency,
            X265Tune::Animation => PyX265Tune::Animation,
        }
    }
}

/// NVIDIA NVENC encoder preset.
#[pyclass(module = "rebake.encode", eq, eq_int)]
#[derive(Clone, Default, PartialEq)]
pub enum PyNvencPreset {
    P1,
    P2,
    P3,
    #[default]
    P4,
    P5,
    P6,
    P7,
}

impl From<PyNvencPreset> for NvencPreset {
    fn from(preset: PyNvencPreset) -> Self {
        match preset {
            PyNvencPreset::P1 => NvencPreset::P1,
            PyNvencPreset::P2 => NvencPreset::P2,
            PyNvencPreset::P3 => NvencPreset::P3,
            PyNvencPreset::P4 => NvencPreset::P4,
            PyNvencPreset::P5 => NvencPreset::P5,
            PyNvencPreset::P6 => NvencPreset::P6,
            PyNvencPreset::P7 => NvencPreset::P7,
        }
    }
}

impl From<NvencPreset> for PyNvencPreset {
    fn from(preset: NvencPreset) -> Self {
        match preset {
            NvencPreset::P1 => PyNvencPreset::P1,
            NvencPreset::P2 => PyNvencPreset::P2,
            NvencPreset::P3 => PyNvencPreset::P3,
            NvencPreset::P4 => PyNvencPreset::P4,
            NvencPreset::P5 => PyNvencPreset::P5,
            NvencPreset::P6 => PyNvencPreset::P6,
            NvencPreset::P7 => PyNvencPreset::P7,
        }
    }
}

/// NVIDIA NVENC tuning mode.
#[pyclass(module = "rebake.encode", eq, eq_int)]
#[derive(Clone, PartialEq)]
pub enum PyNvencTune {
    Hq,
    Ll,
    Ull,
}

impl From<PyNvencTune> for NvencTune {
    fn from(tune: PyNvencTune) -> Self {
        match tune {
            PyNvencTune::Hq => NvencTune::Hq,
            PyNvencTune::Ll => NvencTune::Ll,
            PyNvencTune::Ull => NvencTune::Ull,
        }
    }
}

impl From<NvencTune> for PyNvencTune {
    fn from(tune: NvencTune) -> Self {
        match tune {
            NvencTune::Hq => PyNvencTune::Hq,
            NvencTune::Ll => PyNvencTune::Ll,
            NvencTune::Ull => PyNvencTune::Ull,
        }
    }
}

/// Codec-specific configuration.
///
/// Each variant contains parameters specific to that codec.
#[pyclass(module = "rebake.encode")]
#[derive(Clone, Default)]
pub struct PyCodecConfig {
    pub inner: CodecConfig,
}

#[pymethods]
impl PyCodecConfig {
    /// Serialize to YAML string.
    ///
    /// Returns:
    ///     YAML representation of the codec configuration.
    ///
    /// Example:
    ///     >>> config = CodecConfig.av1()
    ///     >>> print(config.to_yaml())
    ///     codec: AV1
    ///     preset: 10
    pub fn to_yaml(&self) -> PyResult<String> {
        serde_yaml::to_string(&self.inner)
            .map_err(|e| PyRuntimeError::new_err(format!("YAML serialization failed: {e}")))
    }

    /// Create AV1 codec configuration (default).
    ///
    /// Args:
    ///     lp: SVT-AV1 level of parallelism (0=auto, 1-6=explicit level)
    ///     pin: CPU pinning (0=disabled, N=pin to first N cores)
    ///     preset: Quality preset (0-13, lower=better quality/slower). Default: 10
    ///     film_grain: Film grain synthesis level (0=off, 1-50). Recommended: 8 for live-action, 4-6 for animation.
    ///     film_grain_denoise: Apply denoising when film grain is enabled.
    ///     lookahead: Number of frames to look ahead (-1=auto, 0-120).
    ///     fast_decode: Fast decode optimization level (0=off, 1-2).
    #[staticmethod]
    #[pyo3(signature = (lp=None, pin=None, preset=10, film_grain=None, film_grain_denoise=None, lookahead=None, fast_decode=None))]
    pub fn av1(
        lp: Option<u32>,
        pin: Option<u32>,
        preset: u32,
        film_grain: Option<u32>,
        film_grain_denoise: Option<bool>,
        lookahead: Option<i32>,
        fast_decode: Option<u32>,
    ) -> Self {
        Self {
            inner: CodecConfig::AV1 {
                lp,
                pin,
                preset,
                film_grain,
                film_grain_denoise,
                lookahead,
                fast_decode,
            },
        }
    }

    /// Create H.264 codec configuration.
    ///
    /// Args:
    ///     threads: Thread count (None=auto)
    ///     preset: Encoding preset (default: Medium)
    ///     tune: List of tuning options. Only one PSY tuning (Film, Animation, etc.) allowed.
    ///           Can combine with non-PSY tunings (FastDecode, ZeroLatency).
    #[staticmethod]
    #[pyo3(signature = (threads=None, preset=PyX264Preset::Medium, tune=None))]
    pub fn h264(threads: Option<u32>, preset: PyX264Preset, tune: Option<Vec<PyX264Tune>>) -> Self {
        let tune_vec = tune
            .unwrap_or_default()
            .into_iter()
            .map(|t| t.into())
            .collect();
        Self {
            inner: CodecConfig::H264 {
                threads,
                preset: preset.into(),
                tune: tune_vec,
            },
        }
    }

    /// Create H.265 codec configuration.
    ///
    /// Args:
    ///     threads: Thread count (None=auto)
    ///     preset: Encoding preset (default: Medium)
    ///     tune: List of tuning options. Only one PSY tuning (Grain, Animation, etc.) allowed.
    ///           Can combine with non-PSY tunings (FastDecode, ZeroLatency).
    ///     frame_threads: Frame-level parallelism threads (None=auto, let x265 decide)
    #[staticmethod]
    #[pyo3(signature = (threads=None, preset=PyX264Preset::Medium, tune=None, frame_threads=None))]
    pub fn h265(
        threads: Option<u32>,
        preset: PyX264Preset,
        tune: Option<Vec<PyX265Tune>>,
        frame_threads: Option<u32>,
    ) -> Self {
        let tune_vec = tune
            .unwrap_or_default()
            .into_iter()
            .map(|t| t.into())
            .collect();
        Self {
            inner: CodecConfig::H265 {
                threads,
                preset: preset.into(),
                tune: tune_vec,
                frame_threads,
            },
        }
    }

    // =========================================================================
    // VA-API Hardware Encoders (AMD VCN / Intel QSV)
    // =========================================================================

    /// Create H.264 VA-API hardware encoder configuration.
    ///
    /// Requires VA-API compatible hardware (AMD VCN or Intel QSV).
    /// Use `is_vaapi_available()` to check hardware availability.
    ///
    /// Args:
    ///     qp: Quantization parameter (0-51, lower=better quality). Default: 21
    ///     device: VA-API device path. Default: /dev/dri/renderD128
    ///     profile: Encoder profile (constrained_baseline, main, high). Default: high
    ///     b_depth: Maximum B-frame depth (0-4). AMD VCN typically supports 0-1.
    ///     async_depth: Async operation depth for pipelining. Default: 16
    ///
    /// Example:
    ///     >>> if is_vaapi_available():
    ///     ...     config = CodecConfig.h264_vaapi()
    #[staticmethod]
    #[pyo3(signature = (qp=21, device=None, profile=Some("high".to_string()), b_depth=None, async_depth=Some(16)))]
    pub fn h264_vaapi(
        qp: u32,
        device: Option<String>,
        profile: Option<String>,
        b_depth: Option<u32>,
        async_depth: Option<u32>,
    ) -> Self {
        Self {
            inner: CodecConfig::H264Vaapi {
                qp,
                device,
                profile,
                b_depth,
                async_depth,
            },
        }
    }

    /// Create H.265/HEVC VA-API hardware encoder configuration.
    ///
    /// Requires VA-API compatible hardware (AMD VCN or Intel QSV).
    /// Note: AMD VCN does NOT support B-frames for HEVC.
    ///
    /// Args:
    ///     qp: Quantization parameter (0-51, lower=better quality). Default: 29
    ///     device: VA-API device path. Default: /dev/dri/renderD128
    ///     profile: Encoder profile (main, main10)
    ///     async_depth: Async operation depth for pipelining.
    ///
    /// Example:
    ///     >>> if is_vaapi_available():
    ///     ...     config = CodecConfig.h265_vaapi(qp=29)
    #[staticmethod]
    #[pyo3(signature = (qp=29, device=None, profile=None, async_depth=None))]
    pub fn h265_vaapi(
        qp: u32,
        device: Option<String>,
        profile: Option<String>,
        async_depth: Option<u32>,
    ) -> Self {
        Self {
            inner: CodecConfig::H265Vaapi {
                qp,
                device,
                profile,
                async_depth,
            },
        }
    }

    /// Create AV1 VA-API hardware encoder configuration.
    ///
    /// Requires AMD VCN 4.0+ (RDNA 3, e.g., Radeon RX 7000 series, Ryzen 7040+)
    /// or Intel Arc graphics.
    ///
    /// Args:
    ///     qp: Quantization parameter (0-255, lower=better quality). Default: 110
    ///     device: VA-API device path. Default: /dev/dri/renderD128
    ///     profile: Encoder profile (main)
    ///     b_depth: B-frame depth (0-7). Default: None (encoder decides)
    ///     async_depth: Async operation depth for pipelining.
    ///
    /// Example:
    ///     >>> if is_vaapi_available():
    ///     ...     config = CodecConfig.av1_vaapi(qp=110)
    #[staticmethod]
    #[pyo3(signature = (qp=110, device=None, profile=None, b_depth=None, async_depth=None))]
    pub fn av1_vaapi(
        qp: u32,
        device: Option<String>,
        profile: Option<String>,
        b_depth: Option<u32>,
        async_depth: Option<u32>,
    ) -> Self {
        Self {
            inner: CodecConfig::Av1Vaapi {
                qp,
                device,
                profile,
                b_depth,
                async_depth,
            },
        }
    }

    /// Create H.264 NVENC hardware encoder configuration.
    #[staticmethod]
    #[pyo3(signature = (qp=DEFAULT_NVENC_H264_QP, gpu=None, preset=PyNvencPreset::P5, tune=None, profile=None, b_frames=DEFAULT_NVENC_H264_B_FRAMES, rc_lookahead=None))]
    pub fn h264_nvenc(
        qp: u32,
        gpu: Option<u32>,
        preset: PyNvencPreset,
        tune: Option<PyNvencTune>,
        profile: Option<String>,
        b_frames: u32,
        rc_lookahead: Option<u32>,
    ) -> Self {
        Self {
            inner: CodecConfig::H264Nvenc {
                qp,
                gpu,
                preset: preset.into(),
                tune: tune.map(Into::into).or(Some(DEFAULT_NVENC_H264_TUNE)),
                profile: profile.or_else(|| Some(DEFAULT_NVENC_H264_PROFILE.to_string())),
                b_frames,
                rc_lookahead: rc_lookahead.or(Some(DEFAULT_NVENC_H264_RC_LOOKAHEAD)),
            },
        }
    }

    /// Create H.265/HEVC NVENC hardware encoder configuration.
    #[staticmethod]
    #[pyo3(signature = (qp=25, gpu=None, preset=PyNvencPreset::P4, tune=None, profile=None, b_frames=DEFAULT_NVENC_B_FRAMES, rc_lookahead=None))]
    pub fn h265_nvenc(
        qp: u32,
        gpu: Option<u32>,
        preset: PyNvencPreset,
        tune: Option<PyNvencTune>,
        profile: Option<String>,
        b_frames: u32,
        rc_lookahead: Option<u32>,
    ) -> Self {
        Self {
            inner: CodecConfig::H265Nvenc {
                qp,
                gpu,
                preset: preset.into(),
                tune: tune.map(Into::into),
                profile,
                b_frames,
                rc_lookahead,
            },
        }
    }

    /// Create AV1 NVENC hardware encoder configuration.
    ///
    /// Defaults are project-local benchmark values tuned for VMAF >= 93.
    #[staticmethod]
    #[pyo3(signature = (qp=DEFAULT_NVENC_AV1_QP, gpu=None, preset=PyNvencPreset::P7, tune=None, profile=None, b_frames=DEFAULT_NVENC_B_FRAMES, rc_lookahead=None))]
    pub fn av1_nvenc(
        qp: u32,
        gpu: Option<u32>,
        preset: PyNvencPreset,
        tune: Option<PyNvencTune>,
        profile: Option<String>,
        b_frames: u32,
        rc_lookahead: Option<u32>,
    ) -> Self {
        Self {
            inner: CodecConfig::Av1Nvenc {
                qp,
                gpu,
                preset: preset.into(),
                tune: tune.map(Into::into),
                profile,
                b_frames,
                rc_lookahead,
            },
        }
    }
}

#[pyclass(module = "rebake.encode", name = "VideoEncoderConfig")]
#[derive(Clone)]
pub struct PyVideoEncoderConfig {
    pub inner: VideoEncoderConfig,
}

#[pymethods]
impl PyVideoEncoderConfig {
    /// Create a new video encoder configuration.
    ///
    /// Args:
    ///     fps: Frame rate (default: 100)
    ///     gop: Group of Pictures size (default: 20)
    ///     crf: Constant Rate Factor for quality (codec-dependent, lower=better)
    ///     scaling: Scaling algorithm (default: Bicubic)
    ///     codec_config: Codec-specific configuration (default: AV1)
    ///
    /// Example:
    ///     # Default canonical AV1 configuration
    ///     config = VideoEncoderConfig()
    ///
    ///     # H.264 for faster training decode
    ///     config = VideoEncoderConfig(
    ///         codec_config=CodecConfig.h264(threads=4, preset=X264Preset.Fast)
    ///     )
    #[new]
    #[pyo3(signature = (fps=100, gop=20, crf="34".to_string(), scaling=PyScalingFlag::Bicubic, codec_config=None))]
    pub fn new(
        fps: u32,
        gop: u32,
        crf: String,
        scaling: PyScalingFlag,
        codec_config: Option<PyCodecConfig>,
    ) -> Self {
        let codec = codec_config.unwrap_or_default().inner;
        let config = VideoEncoderConfig::new(fps)
            .set_gop(gop)
            .set_crf(crf)
            .set_scaling(scaling.into())
            .set_codec_config(codec);
        Self { inner: config }
    }

    /// Serialize to YAML string.
    ///
    /// Returns:
    ///     YAML representation of the config, suitable for use with rebake-cli.
    ///
    /// Example:
    ///     >>> from rebake._internal.encode import VideoEncoderConfig, CodecConfig
    ///     >>> config = VideoEncoderConfig(fps=30, gop=2, crf="23",
    ///     ...     codec_config=CodecConfig.av1(preset=6))
    ///     >>> print(config.to_yaml())
    ///     fps: 30
    ///     gop: 2
    ///     crf: "23"
    ///     scaling: Bicubic
    ///     codec_config:
    ///       codec: AV1
    ///       preset: 6
    pub fn to_yaml(&self) -> PyResult<String> {
        serde_yaml::to_string(&self.inner)
            .map_err(|e| PyRuntimeError::new_err(format!("YAML serialization failed: {e}")))
    }

    /// Convert to dictionary suitable for YAML serialization.
    ///
    /// This method serializes to YAML and then parses back to a Python dictionary.
    /// The resulting dictionary can be used to construct stage_configs for rebake-cli.
    ///
    /// Returns:
    ///     Dictionary with all config values in YAML-compatible format.
    ///
    /// Example:
    ///     >>> from rebake._internal.encode import VideoEncoderConfig, CodecConfig
    ///     >>> config = VideoEncoderConfig(fps=30, gop=2, crf="23",
    ///     ...     codec_config=CodecConfig.av1(preset=6))
    ///     >>> config.to_dict()
    ///     {'fps': 30, 'gop': 2, 'crf': '23', 'scaling': 'Bicubic',
    ///      'codec_config': {'codec': 'AV1', 'preset': 6}}
    pub fn to_dict(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let yaml_str = self.to_yaml()?;
        let yaml_module = py.import("yaml")?;
        let dict = yaml_module.call_method1("safe_load", (yaml_str,))?;
        Ok(dict.into())
    }
}

#[pyclass(module = "rebake.encode", name = "VideoEncoder")]
pub struct PyVideoEncoder {
    config: VideoEncoderConfig,
}

#[pymethods]
impl PyVideoEncoder {
    #[new]
    pub fn new(config: &PyVideoEncoderConfig) -> Self {
        Self {
            config: config.inner.clone(),
        }
    }

    pub fn run(&self, context: Bound<'_, PyContext>) -> PyResult<()> {
        let mut guard = context.borrow_mut();
        let current = mem::take(&mut guard.inner);
        let updated = self
            .execute(current)
            .map_err(|err| PyRuntimeError::new_err(err.to_string()))?;
        guard.inner = updated;
        Ok(())
    }
}

impl PyVideoEncoder {
    fn execute(&self, context: Context) -> Result<Context, Box<dyn Error>> {
        let mut stage = VideoEncoder::new(self.config.clone());
        stage
            .run(context)
            .map_err(|err| Box::new(io::Error::other(err.reason().to_string())) as Box<dyn Error>)
    }
}

/// Check if VA-API hardware acceleration is available.
///
/// Returns True if the VA-API device (/dev/dri/renderD128) exists on the system.
/// This can be used to determine whether to use hardware or software encoding.
///
/// Returns:
///     bool: True if VA-API is available, False otherwise.
///
/// Example:
///     >>> from rebake._internal.encode import is_vaapi_available, CodecConfig
///     >>> if is_vaapi_available():
///     ...     config = CodecConfig.h264_vaapi()
///     ...     print("Using VA-API hardware encoding")
///     ... else:
///     ...     config = CodecConfig.h264()
///     ...     print("Falling back to software encoding")
#[pyfunction]
pub fn py_is_vaapi_available() -> bool {
    is_vaapi_available()
}

/// Validate and normalize a VideoEncoderConfig JSON string.
///
/// Args:
///     config_json: JSON string matching rebake's VideoEncoderConfig schema.
///     preflight: When True, also verifies FFmpeg encoder availability and
///         hardware device visibility for VA-API/NVENC codecs.
///
/// Returns:
///     Compact JSON string after Rust serde normalization.
#[pyfunction]
#[pyo3(signature = (config_json, preflight=false))]
pub fn py_validate_video_config_json(config_json: String, preflight: bool) -> PyResult<String> {
    let config: VideoEncoderConfig =
        serde_json::from_str(&config_json).map_err(|e| PyValueError::new_err(format!("{e}")))?;

    let validation_result = if preflight {
        config.preflight()
    } else {
        config.validate()
    };
    validation_result.map_err(|e| PyValueError::new_err(e.to_string()))?;

    serde_json::to_string(&config)
        .map_err(|e| PyRuntimeError::new_err(format!("JSON serialization failed: {e}")))
}

/// Build canonical video metadata from a validated RGB video config.
///
/// Args:
///     config_json: JSON string matching rebake's VideoEncoderConfig schema.
///     width: Encoded frame width in pixels.
///     height: Encoded frame height in pixels.
///
/// Returns:
///     Compact JSON string for rebake's VideoMetadata.
#[pyfunction]
pub fn py_build_video_metadata_json(
    config_json: String,
    width: u32,
    height: u32,
) -> PyResult<String> {
    let config: VideoEncoderConfig =
        serde_json::from_str(&config_json).map_err(|e| PyValueError::new_err(format!("{e}")))?;

    let metadata = config
        .video_metadata(width, height)
        .map_err(|e| PyValueError::new_err(e.to_string()))?;

    serde_json::to_string(&metadata)
        .map_err(|e| PyRuntimeError::new_err(format!("JSON serialization failed: {e}")))
}

/// Build a video artifact JSON object from a validated RGB video config and output path.
///
/// Args:
///     config_json: JSON string matching rebake's VideoEncoderConfig schema.
///     video_path: Path to the encoded video file.
///     width: Encoded frame width in pixels.
///     height: Encoded frame height in pixels.
///
/// Returns:
///     Compact JSON string for rebake's VideoArtifact.
#[pyfunction]
pub fn py_build_video_artifact_json(
    config_json: String,
    video_path: String,
    width: u32,
    height: u32,
) -> PyResult<String> {
    let config: VideoEncoderConfig =
        serde_json::from_str(&config_json).map_err(|e| PyValueError::new_err(format!("{e}")))?;

    let artifact: VideoArtifact = config
        .video_artifact(video_path, width, height)
        .map_err(|e| PyValueError::new_err(e.to_string()))?;

    serde_json::to_string(&artifact)
        .map_err(|e| PyRuntimeError::new_err(format!("JSON serialization failed: {e}")))
}
