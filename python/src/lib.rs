pub mod analysis;
pub mod common;
pub mod core;
pub mod decode;
pub mod encode;
pub mod enrich;
pub mod export;
pub mod ingest;
pub mod merge;
pub mod pipeline;
pub mod synchronize;
pub mod transform;

#[pyo3::pymodule]
mod _internal {
    #[pymodule_export]
    use super::analysis::analysis;
    #[pymodule_export]
    use super::common::common;
    #[pymodule_export]
    use super::core::core;
    #[pymodule_export]
    use super::decode::decode;
    #[pymodule_export]
    use super::encode::encode;
    #[pymodule_export]
    use super::enrich::enrich;
    #[pymodule_export]
    use super::export::export;
    #[pymodule_export]
    use super::ingest::ingest;
    #[pymodule_export]
    use super::merge::merge;
    #[pymodule_export]
    use super::pipeline::pipeline;
    #[pymodule_export]
    use super::synchronize::synchronize;
    #[pymodule_export]
    use super::transform::transform;
}
