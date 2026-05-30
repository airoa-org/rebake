//! Python bindings for export stages.

pub mod parquet_video_exporter;

use pyo3::types::PyModuleMethods;

#[pyo3::pymodule(name = "export")]
pub fn export(
    _py: pyo3::Python<'_>,
    m: &pyo3::Bound<'_, pyo3::types::PyModule>,
) -> pyo3::PyResult<()> {
    // ParquetVideoExporter
    m.add_class::<parquet_video_exporter::PyParquetVideoExporterConfig>()?;
    m.add_class::<parquet_video_exporter::PyParquetVideoExporter>()?;

    Ok(())
}
