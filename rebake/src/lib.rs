//! rebake - ROS bag dataset pipeline
//!
//! This crate provides a pipeline-based system for converting ROS bag files
//! (.bag, .mcap) into structured datasets for robot learning.
//! The current `transform` module includes a LeRobot v2.1 transformer.
//!
//! # Architecture
//!
//! The pipeline consists of composable stages. A typical pipeline looks like:
//!
//! ```text
//! Ingest → Enrich → Synchronize → Transform
//! ```
//!
//! Stages are optional and configurable. `LeRobotV21Transformer` can encode
//! videos directly from `image_data`. Use `VideoEncoder` when you need to
//! pre-encode videos and populate `video_registry` instead.
//!
//! Each stage implements the [`Stage`](crate::core::Stage) trait and communicates
//! via [`Context`](crate::core::Context).
//!
//! # Main Components
//!
//! - [`Orchestrator`](crate::orchestrator::Orchestrator) - Pipeline execution engine
//! - [`Stage`](crate::core::Stage) - Stage trait for pipeline steps
//! - [`Context`](crate::core::Context) - Data container passed between stages
//! - [`StageError`](crate::core::StageError) - Unified error type
//!
//! # Module Overview
//!
//! | Module | Description |
//! |--------|-------------|
//! | [`ingest`] | ROS bag file readers (ROS 1 .bag, ROS 2 .mcap) |
//! | [`enrich`] | Data enrichment (TF chains, velocity calculations) |
//! | [`synchronize`] | Time synchronization (ZOH, nearest neighbor) |
//! | [`transform`] | Output format conversion (LeRobot v2.1) |
//! | [`encode`] | Image/video encoding (AV1, H.264, H.265) |
//! | [`decode`] | Video decoding |
//! | [`merge`] | LeRobot v2.1 dataset merging |
//! | [`schema`] | Metadata schemas |
//! | [`ros`] | ROS message parsing utilities |
//!
//! # Configuration
//!
//! Pipeline behavior is controlled through YAML configuration files.
//! See [`OrchestratorConfig`](crate::orchestrator::OrchestratorConfig) for details,
//! and `docs/cli.md` for CLI usage.

pub mod analysis;
pub mod arrow;
pub mod common;
pub mod core;
pub mod decode;
pub mod encode;
pub mod enrich;
pub mod export;
pub mod ingest;
pub mod merge;
pub mod orchestrator;
pub mod pipeline;
pub mod ros;
pub mod schema;
pub mod synchronize;
pub mod transform;

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
pub mod test_utils;

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
pub mod testutil;
