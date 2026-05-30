use std::mem;

use camino::Utf8PathBuf;
use pyo3::Bound;
use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;

use rebake::core::stage::Stage;
use rebake::ingest::rosbag2_ingestor::{Rosbag2Ingestor, Rosbag2IngestorConfig, read_metadata};

use crate::core::PyContext;

#[pyclass(module = "rebake.ingest")]
pub struct PyRosbag2IngestorConfig {
    inner: Rosbag2IngestorConfig,
}

impl PyRosbag2IngestorConfig {
    pub(crate) fn clone_inner(&self) -> Rosbag2IngestorConfig {
        self.inner.clone()
    }
}

#[pymethods]
impl PyRosbag2IngestorConfig {
    #[new]
    #[pyo3(signature = (require_metadata=true))]
    pub fn new(require_metadata: bool) -> Self {
        Self {
            inner: Rosbag2IngestorConfig { require_metadata },
        }
    }

    /// Create a config that does not require metadata (for testing).
    #[staticmethod]
    pub fn without_metadata() -> Self {
        Self {
            inner: Rosbag2IngestorConfig::without_metadata(),
        }
    }

    /// Whether metadata (meta.json) is required.
    #[getter]
    pub fn require_metadata(&self) -> bool {
        self.inner.require_metadata
    }

    pub fn build(&self) -> PyRosbag2Ingestor {
        PyRosbag2Ingestor {
            config: self.inner.clone(),
        }
    }
}

#[pyclass(module = "rebake.ingest")]
pub struct PyRosbag2Ingestor {
    config: Rosbag2IngestorConfig,
}

#[pymethods]
impl PyRosbag2Ingestor {
    #[new]
    pub fn new(config: &PyRosbag2IngestorConfig) -> Self {
        Self {
            config: config.clone_inner(),
        }
    }

    pub fn run(&self, context: Bound<'_, PyContext>) -> PyResult<()> {
        let mut guard = context.borrow_mut();
        let current = mem::take(&mut guard.inner);

        let mut ingestor = Rosbag2Ingestor::new(self.config.clone());
        let updated = ingestor
            .run(current)
            .map_err(|err| PyRuntimeError::new_err(err.to_string()))?;

        guard.inner = updated;
        Ok(())
    }
}

/// Read only the metadata from a rosbag without full ingestion.
///
/// This function reads the `meta.json` file from the parent directory of the
/// rosbag path. It is useful for extracting the UUID before deciding whether
/// to perform expensive full ingestion.
///
/// Args:
///     rosbag_path: Path to the .mcap file. The meta.json is expected
///         to be in the parent directory.
///
/// Returns:
///     The metadata as a JSON string.
///
/// Raises:
///     RuntimeError: If the metadata cannot be read or parsed.
#[pyfunction]
pub fn py_read_metadata(rosbag_path: &str) -> PyResult<String> {
    let path = Utf8PathBuf::from(rosbag_path);
    let metadata = read_metadata(&path).map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
    serde_json::to_string(&metadata)
        .map_err(|e| PyRuntimeError::new_err(format!("failed to serialize metadata: {}", e)))
}
