use std::mem;

use pyo3::Bound;
use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;

use rebake::core::stage::Stage;
use rebake::ingest::rosbag1_ingestor::{Rosbag1Ingestor, Rosbag1IngestorConfig};

use crate::core::PyContext;

#[pyclass(module = "rebake.ingest")]
#[derive(Default)]
pub struct PyRosbag1IngestorConfig {
    inner: Rosbag1IngestorConfig,
}

impl PyRosbag1IngestorConfig {
    pub(crate) fn clone_inner(&self) -> Rosbag1IngestorConfig {
        self.inner.clone()
    }
}

#[pymethods]
impl PyRosbag1IngestorConfig {
    #[new]
    #[pyo3(signature = (require_metadata=true))]
    pub fn new(require_metadata: bool) -> Self {
        Self {
            inner: Rosbag1IngestorConfig { require_metadata },
        }
    }

    /// Create a config that does not require metadata (for testing).
    #[staticmethod]
    pub fn without_metadata() -> Self {
        Self {
            inner: Rosbag1IngestorConfig::without_metadata(),
        }
    }

    /// Whether metadata (meta.json) is required.
    #[getter]
    pub fn require_metadata(&self) -> bool {
        self.inner.require_metadata
    }
}

#[pyclass(module = "rebake.ingest")]
pub struct PyRosbag1Ingestor {
    config: Rosbag1IngestorConfig,
}

#[pymethods]
impl PyRosbag1Ingestor {
    #[new]
    pub fn new(config: &PyRosbag1IngestorConfig) -> Self {
        Self {
            config: config.clone_inner(),
        }
    }

    pub fn run(&self, context: Bound<'_, PyContext>) -> PyResult<()> {
        let mut guard = context.borrow_mut();
        let current = mem::take(&mut guard.inner);

        let mut ingestor = Rosbag1Ingestor::new(self.config.clone());
        let updated = ingestor
            .run(current)
            .map_err(|err| PyRuntimeError::new_err(err.to_string()))?;

        guard.inner = updated;
        Ok(())
    }
}
