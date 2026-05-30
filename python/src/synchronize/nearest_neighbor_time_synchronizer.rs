use std::{error::Error, io, mem};

use pyo3::Bound;
use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;

use rebake::core::stage::{Context, StageConfig};
use rebake::synchronize::NearestNeighborTimeSynchronizerConfig;

use crate::core::PyContext;

#[pyclass(module = "rebake.synchronize")]
pub struct PyNearestNeighborTimeSynchronizerConfig {
    inner: NearestNeighborTimeSynchronizerConfig,
}

impl PyNearestNeighborTimeSynchronizerConfig {
    pub(crate) fn clone_inner(&self) -> NearestNeighborTimeSynchronizerConfig {
        self.inner.clone()
    }
}

#[pymethods]
impl PyNearestNeighborTimeSynchronizerConfig {
    #[new]
    pub fn new(fps: u32) -> Self {
        Self {
            inner: NearestNeighborTimeSynchronizerConfig::new(fps),
        }
    }
}

#[pyclass(module = "rebake.synchronize")]
pub struct PyNearestNeighborTimeSynchronizer {
    config: NearestNeighborTimeSynchronizerConfig,
}

#[pymethods]
impl PyNearestNeighborTimeSynchronizer {
    #[new]
    pub fn new(config: &PyNearestNeighborTimeSynchronizerConfig) -> Self {
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

impl PyNearestNeighborTimeSynchronizer {
    fn execute(&self, context: Context) -> Result<Context, Box<dyn Error>> {
        let mut stage = self.config.build();
        stage
            .run(context)
            .map_err(|err| Box::new(io::Error::other(err.reason().to_string())) as Box<dyn Error>)
    }
}
