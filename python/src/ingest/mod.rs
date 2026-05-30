pub mod rosbag1_ingestor;
pub mod rosbag2_ingestor;

pub use rosbag1_ingestor::*;
pub use rosbag2_ingestor::*;

use pyo3::types::PyModuleMethods;
use pyo3::wrap_pyfunction;

#[pyo3::pymodule(name = "ingest")]
pub fn ingest(
    _py: pyo3::Python<'_>,
    m: &pyo3::Bound<'_, pyo3::types::PyModule>,
) -> pyo3::PyResult<()> {
    m.add_class::<rosbag1_ingestor::PyRosbag1IngestorConfig>()?;
    m.add_class::<rosbag1_ingestor::PyRosbag1Ingestor>()?;
    m.add_class::<rosbag2_ingestor::PyRosbag2IngestorConfig>()?;
    m.add_class::<rosbag2_ingestor::PyRosbag2Ingestor>()?;
    m.add_function(wrap_pyfunction!(rosbag2_ingestor::py_read_metadata, m)?)?;
    Ok(())
}
