//! Data ingestion from ROS bag files.
//!
//! Provides the full ingestion pipeline: reading ROS bag files (ROS1 .bag and ROS2 MCAP),
//! converting messages to Arrow format, and producing Polars LazyFrames.
//!
//! # Available Ingestors
//!
//! - [`Rosbag1IngestorConfig`]: ROS1 bag files (.bag)
//! - [`Rosbag2IngestorConfig`]: ROS2 MCAP files (.mcap)
//! - [`ParquetVideoIngestorConfig`]: rebake intermediate-format bundles (Parquet + video)
//!
//! # Responsibilities
//!
//! - Owns: Rosbag reading, message-to-Arrow conversion, LazyFrame creation
//! - Delegates: Low-level ROS message deserialization (to [`crate::ros`])
//! - Does not own: Time synchronization (see [`crate::synchronize`] module)

pub mod common;
pub mod parquet_video_ingestor;
pub mod pipeline;
pub mod ros_msg_arrow_parser;
pub mod rosbag1_ingestor;
pub mod rosbag2_ingestor;

pub use parquet_video_ingestor::{ParquetVideoIngestor, ParquetVideoIngestorConfig};
pub use rosbag1_ingestor::{Rosbag1Ingestor, Rosbag1IngestorConfig};
pub use rosbag2_ingestor::{Rosbag2Ingestor, Rosbag2IngestorConfig, read_metadata};
