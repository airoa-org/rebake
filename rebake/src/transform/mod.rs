//! Output format transformers.
//!
//! Provides transformers that convert synchronized datasets into
//! specific output formats for machine learning training.
//!
//! # Responsibilities
//!
//! - Owns: Dataset format conversion for ML training
//! - Does not own: Data synchronization (see [`crate::synchronize`] module)

pub mod lerobot_v21;

pub use lerobot_v21::{
    DType, Episodes, Feature, Info, LeRobotInfo, LeRobotMetadata, LeRobotTask, LeRobotTasks,
    LeRobotTasksVec, LeRobotV21Transformer, LeRobotV21TransformerConfig, VideoInfo,
};
