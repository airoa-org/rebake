//! Python bindings for dataset merge operations.

use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;
use pyo3::types::PyModuleMethods;

#[pyo3::pymodule(name = "merge")]
pub fn merge(
    _py: pyo3::Python<'_>,
    m: &pyo3::Bound<'_, pyo3::types::PyModule>,
) -> pyo3::PyResult<()> {
    m.add_function(wrap_pyfunction!(py_discover_datasets, m)?)?;
    m.add_function(wrap_pyfunction!(py_merge_datasets, m)?)?;
    Ok(())
}

/// Discover LeRobot dataset directories inside a parent directory.
///
/// Scans immediate subdirectories of `source_dir` for those containing
/// `meta/info.json`. Returns the discovered paths sorted alphabetically.
///
/// Returns an empty list if `source_dir` does not exist.
///
/// Args:
///     source_dir: Path to a directory containing multiple LeRobot dataset subdirectories.
#[pyfunction]
#[pyo3(name = "discover_datasets", signature = (source_dir,))]
fn py_discover_datasets(source_dir: String) -> PyResult<Vec<String>> {
    let paths = rebake::merge::discover_datasets(source_dir.as_ref())
        .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
    Ok(paths.into_iter().map(|p| p.to_string()).collect())
}

/// Merge multiple LeRobot v2.1 datasets into a single dataset.
///
/// Args:
///     source_dir: Path to a directory containing multiple LeRobot dataset subdirectories.
///     output: Path to the output merged dataset directory.
///     chunks_size: Optional override for chunks_size (default: from first source).
///
/// Returns:
///     Number of datasets merged.
///
/// Raises:
///     RuntimeError: If fewer than 2 datasets are found in source_dir.
#[pyfunction]
#[pyo3(name = "merge_datasets", signature = (source_dir, output, chunks_size=None))]
fn py_merge_datasets(
    source_dir: String,
    output: String,
    chunks_size: Option<usize>,
) -> PyResult<u32> {
    let config = rebake::merge::MergeConfig {
        source_dir: source_dir.into(),
        output: output.into(),
        chunks_size,
    };
    rebake::merge::merge_datasets(&config).map_err(|e| PyRuntimeError::new_err(e.to_string()))
}
