//! Export stages for structured output.
//!
//! Provides stages that export Context data to structured formats suitable
//! for data lake ingestion, including Parquet files and encoded videos.
//!
//! # Responsibilities
//!
//! - Owns: Parquet + video export to structured directory layout
//! - Does not own: Video encoding internals (uses [`crate::encode`] internally)

pub mod parquet_video_exporter;

pub use parquet_video_exporter::{ParquetVideoExporter, ParquetVideoExporterConfig};
