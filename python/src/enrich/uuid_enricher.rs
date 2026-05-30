use std::mem;

use pyo3::Bound;
use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;

use rebake::core::stage::Stage;
use rebake::enrich::uuid_enricher::{UuidEnricher, UuidEnricherConfig};

use crate::core::PyContext;

#[pyclass(module = "rebake.enrich")]
#[derive(Default)]
pub struct PyUuidEnricherConfig {
    inner: UuidEnricherConfig,
}

impl PyUuidEnricherConfig {
    pub(crate) fn clone_inner(&self) -> UuidEnricherConfig {
        self.inner.clone()
    }
}

#[pymethods]
impl PyUuidEnricherConfig {
    #[new]
    pub fn new() -> Self {
        Self {
            inner: UuidEnricherConfig::new(),
        }
    }

    pub fn build(&self) -> PyUuidEnricher {
        PyUuidEnricher {
            config: self.inner.clone(),
        }
    }
}

#[pyclass(module = "rebake.enrich")]
pub struct PyUuidEnricher {
    config: UuidEnricherConfig,
}

#[pymethods]
impl PyUuidEnricher {
    #[new]
    pub fn new(config: &PyUuidEnricherConfig) -> Self {
        Self {
            config: config.clone_inner(),
        }
    }

    pub fn run(&self, context: Bound<'_, PyContext>) -> PyResult<()> {
        let mut guard = context.borrow_mut();
        let current = mem::take(&mut guard.inner);
        let mut enricher = UuidEnricher::new(self.config.clone());
        let updated = enricher
            .run(current)
            .map_err(|err| PyRuntimeError::new_err(err.to_string()))?;
        guard.inner = updated;
        Ok(())
    }
}
