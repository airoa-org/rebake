//! Data enrichment stages.
//!
//! Provides enrichers that add derived columns to the dataset, such as
//! TF transformations, delta values, and UUID columns.
//!
//! # Available Enrichers
//!
//! - [`TfBufferEnricherConfig`]: Builds a time-indexed TF buffer from `/tf` and `/tf_static`
//! - [`TfChainEnricherConfig`]: Computes transformation chains between frames
//! - [`UuidEnricherConfig`]: Adds `rosbag_uuid` column to all tables
//! - [`DeltaJointPositionEnricherConfig`]: Computes delta joint positions
//! - [`DeltaTransformEnricherConfig`]: Computes delta transforms
//! - [`HandCommandEnricherConfig`]: Derives hand commands
//! - [`HeadCommandEnricherConfig`]: Derives head commands (pan/tilt)
//! - [`ShiftEnricherConfig`]: Shifts column values by N steps for temporal offset
//!
//! # Responsibilities
//!
//! - Owns: Derived column computation, TF buffer management
//! - Does not own: Time synchronization (see [`crate::synchronize`] module)

pub mod delta_joint_position_enricher;
pub mod delta_transform_enricher;
mod expr;
pub mod hand_command_enricher;
pub mod head_command_enricher;
pub mod shift_enricher;
pub mod tf_buffer_enricher;
mod tf_chain;
pub mod tf_chain_enricher;
pub mod trajectory_utils;
pub mod uuid_enricher;

pub use delta_joint_position_enricher::{
    DeltaJointPositionEnricher, DeltaJointPositionEnricherConfig,
};
pub use delta_transform_enricher::{
    DeltaReferenceFrame, DeltaTransformEnricher, DeltaTransformEnricherConfig,
};
pub use hand_command_enricher::{HandCommandEnricher, HandCommandEnricherConfig};
pub use head_command_enricher::{HeadCommandEnricher, HeadCommandEnricherConfig};
pub use shift_enricher::{FillStrategy, ShiftEnricher, ShiftEnricherConfig};
pub use tf_buffer_enricher::{TfBufferEnricher, TfBufferEnricherConfig};
pub use tf_chain_enricher::{FramePair, TfChainEnricher, TfChainEnricherConfig};
pub use uuid_enricher::{ROSBAG_UUID_COL, UuidEnricher, UuidEnricherConfig};
