use std::mem;

use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;

use rebake::core::Stage;
use rebake::core::stage::{Context, StageError};
use rebake::enrich::head_command_enricher::{HeadCommandEnricher, HeadCommandEnricherConfig};

use crate::core::PyContext;

#[pyclass]
#[derive(Clone, Default)]
pub struct PyHeadCommandEnricherConfig {
    inner: HeadCommandEnricherConfig,
}

impl PyHeadCommandEnricherConfig {
    pub(crate) fn clone_inner(&self) -> HeadCommandEnricherConfig {
        self.inner.clone()
    }
}

#[pymethods]
impl PyHeadCommandEnricherConfig {
    #[new]
    pub fn new() -> Self {
        Self {
            inner: HeadCommandEnricherConfig::new(),
        }
    }

    pub fn build(&self) -> PyHeadCommandEnricher {
        PyHeadCommandEnricher {
            config: self.inner.clone(),
        }
    }
}

#[pyclass]
pub struct PyHeadCommandEnricher {
    config: HeadCommandEnricherConfig,
}

#[pymethods]
impl PyHeadCommandEnricher {
    #[new]
    pub fn new(config: &PyHeadCommandEnricherConfig) -> Self {
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

impl PyHeadCommandEnricher {
    fn execute(&self, context: Context) -> Result<Context, StageError> {
        let mut enricher = HeadCommandEnricher::new(self.config.clone());
        enricher.run(context)
    }
}
