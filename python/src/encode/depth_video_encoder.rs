use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;

use super::video_encoder::{PyNvencPreset, PyNvencTune};
use rebake::encode::depth_video_encoder::{
    DEFAULT_DEPTH_NVENC_AV1_QP, DEFAULT_DEPTH_NVENC_H265_QP, DepthCodecConfig, DepthVideoConfig,
};
use rebake::encode::nvenc::DEFAULT_NVENC_B_FRAMES;

/// Codec-specific configuration for depth video encoding.
///
/// Each variant contains parameters specific to that codec.
/// Use static factory methods to create instances.
///
/// Example:
///     >>> config = DepthCodecConfig.av1(crf=4, preset=4)
///     >>> config = DepthCodecConfig.h265_vaapi(qp=18)
///     >>> config = DepthCodecConfig.ffv1()
#[pyclass(module = "rebake.encode", name = "DepthCodecConfig")]
#[derive(Clone)]
pub struct PyDepthCodecConfig {
    pub inner: DepthCodecConfig,
}

#[pymethods]
impl PyDepthCodecConfig {
    /// Serialize to YAML string.
    ///
    /// Returns:
    ///     YAML representation of the codec configuration.
    pub fn to_yaml(&self) -> PyResult<String> {
        serde_yaml::to_string(&self.inner)
            .map_err(|e| PyRuntimeError::new_err(format!("YAML serialization failed: {e}")))
    }

    /// Create AV1 (SVT-AV1) software encoder configuration (default).
    ///
    /// Args:
    ///     crf: Constant Rate Factor (0-63, lower = better quality). Default: 4
    ///     preset: SVT-AV1 preset (0-13, lower = better quality/slower). Default: 4
    ///
    /// Example:
    ///     >>> config = DepthCodecConfig.av1(crf=4, preset=4)
    #[staticmethod]
    #[pyo3(signature = (crf=4, preset=4))]
    pub fn av1(crf: u32, preset: u32) -> Self {
        Self {
            inner: DepthCodecConfig::AV1 { crf, preset },
        }
    }

    /// Create HEVC VA-API hardware encoder configuration.
    ///
    /// Args:
    ///     qp: Quantization parameter (0-51, lower = better quality). Default: 18
    ///     device: VA-API device path. Default: /dev/dri/renderD128
    ///
    /// Example:
    ///     >>> config = DepthCodecConfig.h265_vaapi(qp=18)
    #[staticmethod]
    #[pyo3(signature = (qp=18, device=None))]
    pub fn h265_vaapi(qp: u32, device: Option<String>) -> Self {
        Self {
            inner: DepthCodecConfig::H265Vaapi { qp, device },
        }
    }

    /// Create AV1 VA-API hardware encoder configuration.
    ///
    /// Args:
    ///     global_quality: Global quality parameter (0-255, lower = better quality). Default: 35
    ///     device: VA-API device path. Default: /dev/dri/renderD128
    ///
    /// Example:
    ///     >>> config = DepthCodecConfig.av1_vaapi(global_quality=35)
    #[staticmethod]
    #[pyo3(signature = (global_quality=35, device=None))]
    pub fn av1_vaapi(global_quality: u32, device: Option<String>) -> Self {
        Self {
            inner: DepthCodecConfig::Av1Vaapi {
                global_quality,
                device,
            },
        }
    }

    /// Create HEVC NVENC hardware encoder configuration.
    #[staticmethod]
    #[pyo3(signature = (qp=DEFAULT_DEPTH_NVENC_H265_QP, gpu=None, preset=PyNvencPreset::P4, tune=None, b_frames=DEFAULT_NVENC_B_FRAMES, rc_lookahead=None))]
    pub fn h265_nvenc(
        qp: u32,
        gpu: Option<u32>,
        preset: PyNvencPreset,
        tune: Option<PyNvencTune>,
        b_frames: u32,
        rc_lookahead: Option<u32>,
    ) -> Self {
        Self {
            inner: DepthCodecConfig::H265Nvenc {
                qp,
                gpu,
                preset: preset.into(),
                tune: tune.map(Into::into),
                b_frames,
                rc_lookahead,
            },
        }
    }

    /// Create AV1 NVENC hardware encoder configuration.
    ///
    #[staticmethod]
    #[pyo3(signature = (qp=DEFAULT_DEPTH_NVENC_AV1_QP, gpu=None, preset=PyNvencPreset::P4, tune=None, b_frames=DEFAULT_NVENC_B_FRAMES, rc_lookahead=None))]
    pub fn av1_nvenc(
        qp: u32,
        gpu: Option<u32>,
        preset: PyNvencPreset,
        tune: Option<PyNvencTune>,
        b_frames: u32,
        rc_lookahead: Option<u32>,
    ) -> Self {
        Self {
            inner: DepthCodecConfig::Av1Nvenc {
                qp,
                gpu,
                preset: preset.into(),
                tune: tune.map(Into::into),
                b_frames,
                rc_lookahead,
            },
        }
    }

    /// Create FFV1 lossless encoder configuration.
    ///
    /// Example:
    ///     >>> config = DepthCodecConfig.ffv1()
    #[staticmethod]
    pub fn ffv1() -> Self {
        Self {
            inner: DepthCodecConfig::Ffv1,
        }
    }

    /// Check if this codec is lossless (FFV1).
    ///
    /// Returns:
    ///     True if lossless, False if lossy.
    ///
    /// Example:
    ///     >>> DepthCodecConfig.ffv1().is_lossless()
    ///     True
    ///     >>> DepthCodecConfig.av1().is_lossless()
    ///     False
    pub fn is_lossless(&self) -> bool {
        self.inner.is_lossless()
    }

    /// Get the video file extension for this codec.
    ///
    /// Returns:
    ///     File extension string: "mkv" for FFV1, "mp4" for all other codecs.
    ///
    /// Example:
    ///     >>> DepthCodecConfig.av1().video_extension()
    ///     'mp4'
    ///     >>> DepthCodecConfig.ffv1().video_extension()
    ///     'mkv'
    pub fn video_extension(&self) -> &str {
        self.inner.video_extension()
    }
}

/// Depth video encoding configuration.
///
/// Controls how depth frames (16-bit grayscale) are compressed into video files.
/// For lossy codecs, depth values are quantized via Q10Clip4 (16-bit → 10-bit)
/// before encoding. FFV1 lossless encodes raw gray16le without quantization.
///
/// Example:
///     >>> from rebake.encode import DepthVideoConfig, DepthCodecConfig
///     >>> config = DepthVideoConfig(
///     ...     depth_max_mm=4092,
///     ...     fps=30,
///     ...     codec_config=DepthCodecConfig.av1(crf=4, preset=4),
///     ... )
///     >>> print(config.to_yaml())
#[pyclass(module = "rebake.encode", name = "DepthVideoConfig")]
#[derive(Clone)]
pub struct PyDepthVideoConfig {
    pub inner: DepthVideoConfig,
}

#[pymethods]
impl PyDepthVideoConfig {
    /// Create a new depth video encoder configuration.
    ///
    /// Args:
    ///     depth_max_mm: Maximum depth in millimeters for Q10Clip4 quantization.
    ///         Pixels with depth > depth_max_mm are clipped to 0 (invalid).
    ///         Ignored when codec is FFV1 (lossless). Default: 4092
    ///     fps: Frames per second. Default: 30
    ///     codec_config: Codec-specific configuration. Default: AV1 (CRF=4, preset=4)
    #[new]
    #[pyo3(signature = (depth_max_mm=4092, fps=30, codec_config=None))]
    pub fn new(depth_max_mm: u16, fps: u32, codec_config: Option<PyDepthCodecConfig>) -> Self {
        let codec = codec_config.map(|c| c.inner).unwrap_or_default();
        Self {
            inner: DepthVideoConfig {
                depth_max_mm,
                codec_config: codec,
                fps,
            },
        }
    }

    /// Serialize to YAML string.
    ///
    /// Returns:
    ///     YAML representation of the config, suitable for use with rebake-cli.
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
    ///     >>> config = DepthVideoConfig(depth_max_mm=4092, fps=30,
    ///     ...     codec_config=DepthCodecConfig.av1(crf=4, preset=4))
    ///     >>> config.to_dict()
    ///     {'depth_max_mm': 4092, 'codec_config': {'codec': 'AV1', 'crf': 4, 'preset': 4}, 'fps': 30}
    pub fn to_dict(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let yaml_str = self.to_yaml()?;
        let yaml_module = py.import("yaml")?;
        let dict = yaml_module.call_method1("safe_load", (yaml_str,))?;
        Ok(dict.into())
    }
}
