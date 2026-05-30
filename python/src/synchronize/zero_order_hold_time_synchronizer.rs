use std::{error::Error, io, mem};

use pyo3::Bound;
use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;

use rebake::core::stage::{Context, StageConfig};
use rebake::synchronize::ZeroOrderHoldTimeSynchronizerConfig;

use crate::core::PyContext;

#[pyclass(module = "rebake.synchronize")]
pub struct PyZeroOrderHoldTimeSynchronizerConfig {
    inner: ZeroOrderHoldTimeSynchronizerConfig,
}

impl PyZeroOrderHoldTimeSynchronizerConfig {
    pub(crate) fn clone_inner(&self) -> ZeroOrderHoldTimeSynchronizerConfig {
        self.inner.clone()
    }
}

#[pymethods]
impl PyZeroOrderHoldTimeSynchronizerConfig {
    #[new]
    pub fn new(fps: u32) -> Self {
        Self {
            inner: ZeroOrderHoldTimeSynchronizerConfig::new(fps),
        }
    }

    pub fn build(&self) -> PyZeroOrderHoldTimeSynchronizer {
        PyZeroOrderHoldTimeSynchronizer {
            config: self.inner.clone(),
        }
    }
}

#[pyclass(module = "rebake.synchronize")]
pub struct PyZeroOrderHoldTimeSynchronizer {
    config: ZeroOrderHoldTimeSynchronizerConfig,
}

#[pymethods]
impl PyZeroOrderHoldTimeSynchronizer {
    #[new]
    pub fn new(config: &PyZeroOrderHoldTimeSynchronizerConfig) -> Self {
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

impl PyZeroOrderHoldTimeSynchronizer {
    fn execute(&self, context: Context) -> Result<Context, Box<dyn Error>> {
        let mut stage = self.config.build();
        stage
            .run(context)
            .map_err(|err| Box::new(io::Error::other(err.reason().to_string())) as Box<dyn Error>)
    }
}
