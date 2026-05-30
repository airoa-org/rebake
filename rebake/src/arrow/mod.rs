//! Arrow schema utilities.
//!
//! Provides utilities for building Arrow schemas from ROS message types
//! and working with Arrow data structures.
//!
//! # Responsibilities
//!
//! - Owns: Arrow schema construction from ROS types
//! - Does not own: ROS message parsing (see [`crate::ros`] module)

pub mod arrow_schema_builder;
pub mod utils;

pub use arrow_schema_builder::ArrowSchemaBuilder;
pub use utils::{
    create_builtin_message_definition_table, create_duration_message_definition,
    create_time_message_definition,
};
