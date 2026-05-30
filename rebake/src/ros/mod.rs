//! ROS message parsing utilities.
//!
//! Provides types and utilities for parsing ROS message definitions
//! and deserializing ROS message data from bag files.
//!
//! # Responsibilities
//!
//! - Owns: ROS message schema parsing, message deserialization
//! - Does not own: Rosbag file reading (see [`crate::ingest`] module)

pub mod msg_deserializer;
pub mod schema;
pub mod schema_type_parser;
pub mod types;

pub use msg_deserializer::{Endianness, RosGeneration, RosMsgDeserializer};
pub use schema::{SchemaSection, split_schema_text_to_sections};
pub use schema_type_parser::parse_schema_text_to_message_definition_table;
pub use types::{BaseType, FieldDefinition, FieldType, MessageDefinition, Primitive};
