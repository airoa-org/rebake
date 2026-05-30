//! Core abstractions for the rebake pipeline.
//!
//! This module provides the foundational types used throughout the pipeline:
//! - [`Stage`] and [`StageConfig`] traits for pipeline stages
//! - [`StageError`] for error handling
//! - [`Context`] for sharing data between stages
//!
//! # Responsibilities
//!
//! - Owns: Pipeline execution model, error types, inter-stage data sharing
//! - Does not own: Specific stage implementations (see [`crate::ingest`], [`crate::synchronize`], etc.)

pub mod conversion;
pub mod error;
pub mod stage;

pub use conversion::{
    arrow_batch_to_polars, lazy_to_record_batch_rechunk, lazy_to_record_batches_iter,
    polars_batch_to_arrow, record_batch_to_lazy,
};
pub use error::{BoxError, OptionExt, PolarsExt, ResultExt, StageError, StageResult};
pub use stage::{Context, DynError, Stage, StageConfig};
