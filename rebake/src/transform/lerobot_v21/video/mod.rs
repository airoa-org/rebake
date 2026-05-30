//! Video encoding pipeline for LeRobot datasets.
//!
//! Provides the video encoding pipeline that converts image frames
//! to video files in the LeRobot directory structure.
//!
//! # Responsibilities
//!
//! - Owns: Video file generation for LeRobot output
//! - Does not own: General video encoding (see [`crate::encode`] module)

pub mod frame_provider;
pub mod pipeline;

pub use pipeline::{VideoEncoderPipeline, VideoStats};
