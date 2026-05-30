pub mod depth_video_encoder;
pub mod image_encoder;
pub mod video_encoder;

use pyo3::types::PyModuleMethods;

#[pyo3::pymodule(name = "encode")]
pub fn encode(
    _py: pyo3::Python<'_>,
    m: &pyo3::Bound<'_, pyo3::types::PyModule>,
) -> pyo3::PyResult<()> {
    // ImageEncoder
    m.add_class::<image_encoder::PyImageEncoderConfig>()?;
    m.add_class::<image_encoder::PyImageEncoder>()?;
    // VideoEncoder
    m.add_class::<video_encoder::PyVideoEncoderConfig>()?;
    m.add_class::<video_encoder::PyVideoEncoder>()?;
    m.add_class::<video_encoder::PyScalingFlag>()?;
    m.add_class::<video_encoder::PyCodecConfig>()?;
    m.add_class::<video_encoder::PyX264Preset>()?;
    m.add_class::<video_encoder::PyX264Tune>()?;
    m.add_class::<video_encoder::PyX265Tune>()?;
    m.add_class::<video_encoder::PyNvencPreset>()?;
    m.add_class::<video_encoder::PyNvencTune>()?;
    // DepthVideoEncoder
    m.add_class::<depth_video_encoder::PyDepthCodecConfig>()?;
    m.add_class::<depth_video_encoder::PyDepthVideoConfig>()?;
    // VA-API availability check
    m.add_function(pyo3::wrap_pyfunction!(
        video_encoder::py_is_vaapi_available,
        m
    )?)?;
    m.add_function(pyo3::wrap_pyfunction!(
        video_encoder::py_validate_video_config_json,
        m
    )?)?;
    m.add_function(pyo3::wrap_pyfunction!(
        video_encoder::py_build_video_metadata_json,
        m
    )?)?;
    m.add_function(pyo3::wrap_pyfunction!(
        video_encoder::py_build_video_artifact_json,
        m
    )?)?;
    Ok(())
}
