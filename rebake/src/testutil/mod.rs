//! Test utilities for rebake.
//!
//! This module provides utilities for generating test data, including
//! MCAP files with ROS2 messages.

mod mcap_generator;

pub use mcap_generator::{McapGenerator, McapGeneratorConfig};
