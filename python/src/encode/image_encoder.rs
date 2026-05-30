//! PyO3 bindings for ImageEncoder.
//!
//! This module provides thin FFI wrappers that call the pure Rust implementation.
//! All logic resides in `rebake::encode::image_encoder`.

use std::mem;

use pyo3::Bound;
use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;

use crate::core::PyContext;
use rebake::core::stage::Stage;
use rebake::encode::image_encoder::{ImageEncoder, ImageEncoderConfig};

/// Configuration for the image encoder.
///
/// This encoder has no configuration parameters - it simply saves all image
/// data to individual files in the output directory.
#[pyclass(module = "rebake.encode", name = "ImageEncoderConfig")]
#[derive(Clone, Default)]
pub struct PyImageEncoderConfig {
    pub inner: ImageEncoderConfig,
}

#[pymethods]
impl PyImageEncoderConfig {
    #[new]
    pub fn new() -> Self {
        Self {
            inner: ImageEncoderConfig::default(),
        }
    }
}

/// Saves image frames to individual files.
///
/// This encoder takes image data from the Context and saves each frame
/// as an individual file. The output directory structure mirrors the
/// topic name hierarchy.
#[pyclass(module = "rebake.encode", name = "ImageEncoder")]
pub struct PyImageEncoder {
    config: ImageEncoderConfig,
}

#[pymethods]
impl PyImageEncoder {
    #[new]
    pub fn new(config: &PyImageEncoderConfig) -> Self {
        Self {
            config: config.inner.clone(),
        }
    }

    /// Run the encoder on the given context.
    ///
    /// Saves all image data to individual files in the output directory.
    ///
    /// Args:
    ///     context: The context containing image data. Must have:
    ///         - `output_dir` set
    ///         - `image_data` (optional - returns early if missing)
    ///
    /// Raises:
    ///     RuntimeError: If output_dir is not set or I/O fails.
    pub fn run(&self, context: Bound<'_, PyContext>) -> PyResult<()> {
        let mut guard = context.borrow_mut();
        let current = mem::take(&mut guard.inner);
        let mut stage = ImageEncoder::new(self.config.clone());
        let updated = stage
            .run(current)
            .map_err(|err| PyRuntimeError::new_err(err.to_string()))?;
        guard.inner = updated;
        Ok(())
    }
}
