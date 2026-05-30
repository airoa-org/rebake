//! Schema definitions for rebake data structures.
//!
//! Provides feature mapping types and Airoa metadata structures
//! used throughout the pipeline.
//!
//! # Responsibilities
//!
//! - Owns: Feature map definitions, metadata schema
//! - Does not own: Arrow schema building (see [`crate::arrow`] module)

pub mod features;
pub mod metadata;

pub use features::{RobotModelSource, TopicFeatureMap, TopicFeatureMapEntry};
