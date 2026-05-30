pub mod delta_joint_position_enricher;
pub mod delta_transform_enricher;
pub mod hand_command_enricher;
pub mod head_command_enricher;
pub mod shift_enricher;
pub mod tf_buffer_enricher;
pub mod tf_chain_enricher;
pub mod uuid_enricher;

pub use delta_joint_position_enricher::*;
pub use delta_transform_enricher::*;
pub use hand_command_enricher::*;
pub use head_command_enricher::*;
pub use shift_enricher::*;
pub use tf_buffer_enricher::*;
pub use tf_chain_enricher::*;
pub use uuid_enricher::*;

use pyo3::types::PyModuleMethods;

#[pyo3::pymodule(name = "enrich")]
pub fn enrich(
    _py: pyo3::Python<'_>,
    m: &pyo3::Bound<'_, pyo3::types::PyModule>,
) -> pyo3::PyResult<()> {
    m.add_class::<tf_buffer_enricher::PyTfBufferEnricherConfig>()?;
    m.add_class::<tf_buffer_enricher::PyTfBufferEnricher>()?;
    m.add_class::<tf_chain_enricher::PyTfChainEnricherConfig>()?;
    m.add_class::<tf_chain_enricher::PyTfChainEnricher>()?;
    m.add_class::<tf_chain_enricher::PyFramePair>()?;
    m.add_class::<delta_joint_position_enricher::PyDeltaJointPositionEnricherConfig>()?;
    m.add_class::<delta_joint_position_enricher::PyDeltaJointPositionEnricher>()?;
    m.add_class::<delta_transform_enricher::PyDeltaTransformEnricherConfig>()?;
    m.add_class::<delta_transform_enricher::PyDeltaTransformEnricher>()?;
    m.add_class::<hand_command_enricher::PyHandCommandEnricherConfig>()?;
    m.add_class::<hand_command_enricher::PyHandCommandEnricher>()?;
    m.add_class::<head_command_enricher::PyHeadCommandEnricherConfig>()?;
    m.add_class::<head_command_enricher::PyHeadCommandEnricher>()?;
    m.add_class::<shift_enricher::PyShiftEnricherConfig>()?;
    m.add_class::<shift_enricher::PyShiftEnricher>()?;
    m.add_class::<uuid_enricher::PyUuidEnricherConfig>()?;
    m.add_class::<uuid_enricher::PyUuidEnricher>()?;
    Ok(())
}
