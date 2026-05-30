use std::mem;

use pyo3::Bound;
use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;

use rebake::core::stage::Stage;
use rebake::enrich::tf_buffer_enricher::{TfBufferEnricher, TfBufferEnricherConfig};

use crate::core::PyContext;

#[pyclass(module = "rebake.enrich")]
#[derive(Default)]
pub struct PyTfBufferEnricherConfig {
    inner: TfBufferEnricherConfig,
}

impl PyTfBufferEnricherConfig {
    pub(crate) fn clone_inner(&self) -> TfBufferEnricherConfig {
        self.inner.clone()
    }
}

#[pymethods]
impl PyTfBufferEnricherConfig {
    #[new]
    pub fn new() -> Self {
        Self {
            inner: TfBufferEnricherConfig::new(),
        }
    }

    pub fn build(&self) -> PyTfBufferEnricher {
        PyTfBufferEnricher {
            config: self.inner.clone(),
        }
    }
}

#[pyclass(module = "rebake.enrich")]
pub struct PyTfBufferEnricher {
    config: TfBufferEnricherConfig,
}

#[pymethods]
impl PyTfBufferEnricher {
    #[new]
    pub fn new(config: &PyTfBufferEnricherConfig) -> Self {
        Self {
            config: config.clone_inner(),
        }
    }

    pub fn run(&self, context: Bound<'_, PyContext>) -> PyResult<()> {
        let mut guard = context.borrow_mut();
        let current = mem::take(&mut guard.inner);
        let mut enricher = TfBufferEnricher::new(self.config.clone());
        let updated = enricher
            .run(current)
            .map_err(|err| PyRuntimeError::new_err(err.to_string()))?;
        guard.inner = updated;
        Ok(())
    }
}
