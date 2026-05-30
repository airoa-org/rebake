use std::mem;

use pyo3::Bound;
use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;

use rebake::pipeline::PipelineConfig;

use crate::core::PyContext;

/// PyO3 module that exports PyPipelineConfig and PyPipeline to Python.
#[pyo3::pymodule]
pub mod pipeline {
    #[pymodule_export]
    use super::PyPipeline;
    #[pymodule_export]
    use super::PyPipelineConfig;
}

/// Python binding for rebake's PipelineConfig.
///
/// Follows rebake's standard Config -> build() -> entity pattern.
/// Call `build()` to create a `PyPipeline` that can execute stages.
#[pyclass(module = "rebake.pipeline")]
pub struct PyPipelineConfig {
    inner: PipelineConfig,
}

#[pymethods]
impl PyPipelineConfig {
    /// Create a PipelineConfig from a JSON string.
    ///
    /// The JSON format matches rebake's pipeline YAML:
    /// `{"stage_configs": [{"ConfigClassName": {params}}, ...]}`
    ///
    /// Raises `ValueError` if JSON is invalid or contains unknown stage names.
    #[new]
    fn new(json: &str) -> PyResult<Self> {
        let inner = PipelineConfig::from_json(json)
            .map_err(|e| PyValueError::new_err(format!("Invalid pipeline config JSON: {e}")))?;
        Ok(Self { inner })
    }

    /// Build a Pipeline from this configuration.
    fn build(&self) -> PyPipeline {
        PyPipeline {
            inner: self.inner.build(),
        }
    }
}

/// Python binding for rebake's Pipeline.
///
/// Created by `PyPipelineConfig.build()`. Executes all stages sequentially.
#[pyclass(module = "rebake.pipeline")]
pub struct PyPipeline {
    inner: rebake::pipeline::Pipeline,
}

#[pymethods]
impl PyPipeline {
    /// Execute all stages on the given context.
    ///
    /// Uses the `mem::take` pattern (same as all other stage bindings):
    /// 1. Take Context out of PyContext (replace with Default)
    /// 2. Run all stages in Rust
    /// 3. Put result back into PyContext
    fn run(&mut self, context: Bound<'_, PyContext>) -> PyResult<()> {
        let mut guard = context.borrow_mut();
        let current = mem::take(&mut guard.inner);
        let updated = self
            .inner
            .run(current)
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        guard.inner = updated;
        Ok(())
    }
}
