//! LeRobot v2.1 dataset format transformer.
//!
//! Transforms synchronized ROS bag data into the LeRobot v2.1 dataset format,
//! which is used for training Vision-Language-Action (VLA) models.
//!
//! # Output Structure
//!
//! - `data/chunk-*/episode_*.parquet`: Episode data files
//! - `meta/info.json`: Dataset metadata
//! - `meta/episodes.jsonl`: Episode index
//! - `meta/episodes_stats.jsonl`: Episode statistics
//! - `meta/tasks.jsonl`: Task definitions
//! - `videos/`: Encoded video files
//!
//! # Responsibilities
//!
//! - Owns: LeRobot v2.1 format generation, episode segmentation
//! - Does not own: Video encoding (delegated to [`crate::encode`] module)

mod annotations;

pub mod episodes;
pub mod feature;
pub mod info;
pub mod io;
pub mod lerobot_dataset_metadata;
pub mod lerobot_v21_transformer;
pub mod metadata;
pub mod segment;
pub mod timeline;
pub mod video;

pub use episodes::Episodes;
pub use feature::{DType, Feature, VideoInfo};
pub use info::Info;
pub use lerobot_dataset_metadata::{
    LeRobotInfo, LeRobotMetadata, LeRobotTask, LeRobotTasks, LeRobotTasksVec,
};
pub use lerobot_v21_transformer::{LeRobotV21Transformer, LeRobotV21TransformerConfig};
