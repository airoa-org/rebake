pub mod video_decoder;

use pyo3::types::PyModuleMethods;

#[pyo3::pymodule(name = "decode")]
pub fn decode(
    _py: pyo3::Python<'_>,
    m: &pyo3::Bound<'_, pyo3::types::PyModule>,
) -> pyo3::PyResult<()> {
    m.add_class::<video_decoder::PyVideoDecoderConfig>()?;
    m.add_class::<video_decoder::PyVideoDecoder>()?;
    Ok(())
}
