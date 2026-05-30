//! PyO3 bindings for ParquetVideoExporter.
//!
//! This module provides Python bindings for the ParquetVideoExporter stage,
//! which exports Context data to structured Parquet + Video format.

use std::mem;

use pyo3::Bound;
use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;

use crate::core::PyContext;
use crate::encode::video_encoder::PyVideoEncoderConfig;
use rebake::core::stage::Stage;
use rebake::export::parquet_video_exporter::{ParquetVideoExporter, ParquetVideoExporterConfig};

/// Configuration for the ParquetVideoExporter stage.
///
/// This stage exports Context data to a structured directory format:
/// - Parquet files for each topic
/// - Encoded video files (if image_data exists)
/// - Registry files (_metadata.parquet, _topic_type_map.parquet, _video_registry.parquet)
///
/// Example:
///     >>> config = ParquetVideoExporterConfig("/data/output")
///     >>> exporter = ParquetVideoExporter(config)
///     >>> exporter.run(context)
#[pyclass(module = "rebake.export", name = "ParquetVideoExporterConfig")]
#[derive(Clone)]
pub struct PyParquetVideoExporterConfig {
    pub inner: ParquetVideoExporterConfig,
}

#[pymethods]
impl PyParquetVideoExporterConfig {
    /// Create a new ParquetVideoExporterConfig.
    ///
    /// Args:
    ///     output_dir: Root output directory. UUID subdirectories are created automatically.
    ///     video_config: Optional video encoder configuration. If not provided,
    ///                   defaults to VideoEncoderConfig() (AV1, fps=100, gop=20, crf=34).
    ///
    /// Example:
    ///     >>> # Simple usage with defaults
    ///     >>> config = ParquetVideoExporterConfig("/data/output")
    ///
    ///     >>> # With custom video settings
    ///     >>> from rebake.encode import VideoEncoderConfig, CodecConfig
    ///     >>> video_config = VideoEncoderConfig(fps=60, codec_config=CodecConfig.h264())
    ///     >>> config = ParquetVideoExporterConfig("/data/output", video_config=video_config)
    #[new]
    #[pyo3(signature = (output_dir, video_config=None))]
    pub fn new(output_dir: String, video_config: Option<PyVideoEncoderConfig>) -> Self {
        let mut config = ParquetVideoExporterConfig::new(output_dir);
        if let Some(vc) = video_config {
            config = config.with_video_config(vc.inner);
        }
        Self { inner: config }
    }

    /// Get the output directory.
    #[getter]
    pub fn output_dir(&self) -> &str {
        &self.inner.output_dir
    }

    /// Serialize to YAML string.
    ///
    /// Returns:
    ///     YAML representation of the config, suitable for use with rebake-cli.
    pub fn to_yaml(&self) -> PyResult<String> {
        serde_yaml::to_string(&self.inner)
            .map_err(|e| PyRuntimeError::new_err(format!("YAML serialization failed: {e}")))
    }
}

/// Exports Context data to structured Parquet + Video format.
///
/// This is a terminal stage that creates the following directory structure:
///
/// ```text
/// {output_dir}/{uuid}/
///   parquet/
///     {topic}.parquet           # Topic data
///     _metadata.parquet         # Airoa metadata
///     _topic_type_map.parquet   # Topic name to message type mapping
///     _video_registry.parquet   # Topic name to video path mapping
///   videos/
///     {topic}.mp4               # Encoded video files
/// ```
///
/// Preconditions:
///     - dataset: Required - HashMap of topic names to LazyFrames
///     - airoa_metadata: Required - Metadata containing UUID
///     - topic_message_type_map: Required - Topic to message type mapping
///     - image_data: Optional - If present, videos will be encoded
///
/// Postconditions:
///     - output_dir: Set to {config.output_dir}/{uuid}
///     - bundle_root: Set to the exported bundle root
///     - video_registry: Set if videos were written
#[pyclass(module = "rebake.export", name = "ParquetVideoExporter")]
pub struct PyParquetVideoExporter {
    config: ParquetVideoExporterConfig,
}

#[pymethods]
impl PyParquetVideoExporter {
    /// Create a new ParquetVideoExporter.
    ///
    /// Args:
    ///     config: The exporter configuration.
    #[new]
    pub fn new(config: &PyParquetVideoExporterConfig) -> Self {
        Self {
            config: config.inner.clone(),
        }
    }

    /// Run the exporter on the given context.
    ///
    /// Exports all data to the structured directory format.
    /// Creates Parquet files for each topic, metadata files,
    /// and encodes videos if image_data is present.
    ///
    /// Args:
    ///     context: The context containing data to export. Must have:
    ///         - `dataset` (required)
    ///         - `airoa_metadata` (required)
    ///         - `topic_message_type_map` (required)
    ///         - `image_data` (optional - videos encoded if present)
    ///
    /// After running, the context will have:
    ///     - `output_dir` set to {config.output_dir}/{uuid}
    ///     - `bundle_root` set to the exported bundle root
    ///     - `video_registry` set if videos were written
    ///
    /// Raises:
    ///     RuntimeError: If required preconditions are not met or I/O fails.
    pub fn run(&self, context: Bound<'_, PyContext>) -> PyResult<()> {
        let mut guard = context.borrow_mut();
        let current = mem::take(&mut guard.inner);
        let mut stage = ParquetVideoExporter::new(self.config.clone());
        let updated = stage
            .run(current)
            .map_err(|err| PyRuntimeError::new_err(err.to_string()))?;
        guard.inner = updated;
        Ok(())
    }
}
