pub mod context;
pub mod metadata;

pub use context::*;
pub use metadata::*;

use arrow::record_batch::RecordBatch;
use arrow_pyarrow::PyArrowType;
use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;
use pyo3::types::PyModuleMethods;

use rebake::schema::metadata::arrow::airoa_metadata_to_record_batch;
use rebake::schema::metadata::{parse_metadata, parse_metadata_as_v2_0};

/// Convert metadata JSON to an Arrow RecordBatch.
///
/// This function converts an Airoa metadata dictionary (as JSON string)
/// to an Arrow RecordBatch, preserving the full nested structure.
///
/// Supports both V1.3 and V2.0 metadata formats. The original format is
/// preserved (no automatic conversion between versions).
///
/// Args:
///     metadata_json: Metadata as a JSON string (V1.3 or V2.0 format).
///
/// Returns:
///     Arrow RecordBatch containing the metadata in its original schema
///     (V1.3 schema for V1.3 input, V2.0 schema for V2.0 input).
#[pyfunction]
pub fn metadata_to_arrow(metadata_json: &str) -> PyResult<PyArrowType<RecordBatch>> {
    let metadata = parse_metadata(metadata_json)
        .map_err(|e| PyRuntimeError::new_err(format!("Failed to parse metadata JSON: {e}")))?;
    let batch = airoa_metadata_to_record_batch(&metadata)
        .map_err(|e| PyRuntimeError::new_err(format!("Failed to convert metadata: {e}")))?;
    Ok(PyArrowType(batch))
}

/// Normalize metadata JSON to canonical V2.0 JSON.
///
/// This function accepts either V1.3 or V2.0 metadata JSON:
/// - V1.3 input is converted to V2.0
/// - V2.0 input is parsed and re-serialized canonically
///
/// Returns:
///     Canonical V2.0 metadata JSON.
#[pyfunction]
pub fn normalize_metadata_json_to_v2_0(metadata_json: &str) -> PyResult<String> {
    let metadata = parse_metadata_as_v2_0(metadata_json)
        .map_err(|e| PyRuntimeError::new_err(format!("Failed to normalize metadata JSON: {e}")))?;
    serde_json::to_string(&metadata).map_err(|e| {
        PyRuntimeError::new_err(format!("Failed to serialize normalized metadata: {e}"))
    })
}

#[pyo3::pymodule(name = "core")]
pub fn core(
    _py: pyo3::Python<'_>,
    m: &pyo3::Bound<'_, pyo3::types::PyModule>,
) -> pyo3::PyResult<()> {
    m.add_class::<context::PyContext>()?;
    m.add_class::<metadata::PyEnvType>()?;
    m.add_class::<metadata::PyRunnerType>()?;
    m.add_class::<metadata::PyRobot>()?;
    m.add_class::<metadata::PyFile>()?;
    m.add_class::<metadata::PyEnvironment>()?;
    m.add_class::<metadata::PyRunner>()?;
    m.add_class::<metadata::PyDevice>()?;
    m.add_class::<metadata::PyGitSource>()?;
    m.add_class::<metadata::PySource>()?;
    m.add_class::<metadata::PyProgram>()?;
    m.add_class::<metadata::PyEpisode>()?;
    m.add_class::<metadata::PySegment>()?;
    m.add_class::<metadata::PyMetadataV2_0>()?;
    m.add_function(wrap_pyfunction!(metadata_to_arrow, m)?)?;
    m.add_function(wrap_pyfunction!(metadata::parse_metadata_as_v2_0, m)?)?;
    m.add_function(wrap_pyfunction!(normalize_metadata_json_to_v2_0, m)?)?;
    Ok(())
}
