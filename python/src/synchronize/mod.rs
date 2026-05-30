pub mod nearest_neighbor_time_synchronizer;
pub mod zero_order_hold_time_synchronizer;

#[pyo3::pymodule]
pub mod synchronize {
    #[pymodule_export]
    use super::nearest_neighbor_time_synchronizer::{
        PyNearestNeighborTimeSynchronizer, PyNearestNeighborTimeSynchronizerConfig,
    };

    #[pymodule_export]
    use super::zero_order_hold_time_synchronizer::{
        PyZeroOrderHoldTimeSynchronizer, PyZeroOrderHoldTimeSynchronizerConfig,
    };
}
