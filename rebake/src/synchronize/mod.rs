//! Time synchronization for multi-topic data.
//!
//! Provides time synchronizers that align data from multiple ROS topics
//! to a common timeline, enabling consistent frame-by-frame processing.
//!
//! # Available Synchronizers
//!
//! - [`ZeroOrderHoldTimeSynchronizerConfig`]: Resamples to uniform FPS using zero-order hold (adds `is_fresh` column)
//! - [`NearestNeighborTimeSynchronizerConfig`]: Resamples to uniform FPS using nearest neighbor (adds `is_fresh` column)
//! - [`TimestampMergeTimeSynchronizerConfig`]: Creates a non-uniform timeline by merging and sorting timestamps from all topics, then applies backward as-of join (ZOH) to fill values
//!
//! # Responsibilities
//!
//! - Owns: Time alignment, resampling strategies, `synched_timestamp_ns` column creation
//! - Does not own: Data enrichment (see [`crate::enrich`] module)

#![allow(clippy::module_inception)]

pub mod nearest_neighbor_time_synchronizer;
pub mod synchronize;
pub mod time_synchronizer;
pub mod timestamp_merge_time_synchronizer;
pub mod utils;
pub mod zero_order_hold_time_synchronizer;

pub use nearest_neighbor_time_synchronizer::{
    NearestNeighborTimeSynchronizer, NearestNeighborTimeSynchronizerConfig,
};
pub use synchronize::{TimeSynchronizerConfig, synchronize};
pub use time_synchronizer::{
    IS_FRESH_COL, ORIGINAL_TIMESTAMP_COL, SYNCHED_TIMESTAMP_COL, SyncTimeline, TimeSynchronizer,
    TimestampIndex, TopicFrames,
};
pub use timestamp_merge_time_synchronizer::{
    TimestampMergeTimeSynchronizer, TimestampMergeTimeSynchronizerConfig,
};
pub use utils::{load_parquet_frames, scan_parquet_frames};
pub use zero_order_hold_time_synchronizer::{
    ZeroOrderHoldTimeSynchronizer, ZeroOrderHoldTimeSynchronizerConfig,
};
