//! Pipeline orchestration.
//!
//! Provides the orchestrator that executes a sequence of pipeline stages,
//! managing context flow and process isolation for parallel processing.
//!
//! # Responsibilities
//!
//! - Owns: Stage sequencing, context management, process isolation
//! - Does not own: Individual stage implementations (see [`crate::ingest`], [`crate::synchronize`], etc.)

#![allow(clippy::module_inception)]

pub mod orchestrator;

pub use orchestrator::{Orchestrator, OrchestratorConfig, PipelineError};
