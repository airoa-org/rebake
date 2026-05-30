use std::mem;

use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;

use rebake::core::Stage;
use rebake::core::stage::{Context, StageError};
use rebake::enrich::hand_command_enricher::{HandCommandEnricher, HandCommandEnricherConfig};

use crate::core::PyContext;

#[pyclass]
#[derive(Clone, Default)]
pub struct PyHandCommandEnricherConfig {
    inner: HandCommandEnricherConfig,
}

impl PyHandCommandEnricherConfig {
    pub(crate) fn clone_inner(&self) -> HandCommandEnricherConfig {
        self.inner.clone()
    }
}

#[pymethods]
impl PyHandCommandEnricherConfig {
    #[new]
    pub fn new() -> Self {
        Self {
            inner: HandCommandEnricherConfig::new(),
        }
    }

    pub fn build(&self) -> PyHandCommandEnricher {
        PyHandCommandEnricher {
            config: self.inner.clone(),
        }
    }
}

#[pyclass]
pub struct PyHandCommandEnricher {
    config: HandCommandEnricherConfig,
}

#[pymethods]
impl PyHandCommandEnricher {
    #[new]
    pub fn new(config: &PyHandCommandEnricherConfig) -> Self {
        Self {
            config: config.clone_inner(),
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

impl PyHandCommandEnricher {
    fn execute(&self, context: Context) -> Result<Context, StageError> {
        let mut enricher = HandCommandEnricher::new(self.config.clone());
        enricher.run(context)
    }
}
