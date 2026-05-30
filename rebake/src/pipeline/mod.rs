//! Minimal pipeline configuration for embedding into external systems.
//!
//! Unlike [`OrchestratorConfig`](crate::orchestrator::OrchestratorConfig),
//! `PipelineConfig` contains only the stage definitions — no `work_dir`,
//! `save_contexts`, or parallelism. It is designed for embedding into
//! larger systems (e.g., Dagster) where orchestration is handled externally.
//!
//! The JSON/YAML format is identical to the `stage_configs` section of
//! `OrchestratorConfig`, using `typetag::serde` for automatic deserialization
//! of trait objects.
//!
//! # Usage
//!
//! Follows rebake's standard `Config` -> `build()` -> entity pattern:
//!
//! ```ignore
//! let config = PipelineConfig::from_json(json)?;
//! let pipeline = config.build();
//! let context = pipeline.run(context)?;
//! ```
//!
//! # JSON Format
//!
//! ```json
//! {
//!   "stage_configs": [
//!     {"ZeroOrderHoldTimeSynchronizerConfig": {"fps": 10}},
//!     {"TfChainEnricherConfig": {"frame_pairs": [
//!       {"source": "base_footprint", "target": "hand_left_left_finger_tip_frame"}
//!     ]}}
//!   ]
//! }
//! ```
//!
//! The single-key dict pattern (`{"ClassName": {params}}`) is handled
//! automatically by `typetag::serde` on the [`StageConfig`] trait.

use serde::{Deserialize, Serialize};

use crate::core::StageError;
use crate::core::stage::{Context, StageConfig};

/// Configuration for a minimal pipeline: a list of stage configs.
///
/// This is the serializable configuration. Call [`build()`](PipelineConfig::build)
/// to create a [`Pipeline`] that can execute the stages.
#[derive(Serialize, Deserialize)]
pub struct PipelineConfig {
    pub stage_configs: Vec<Box<dyn StageConfig>>,
}

impl PipelineConfig {
    /// Deserialize a `PipelineConfig` from a JSON string.
    ///
    /// # Errors
    ///
    /// Returns `serde_json::Error` if the JSON is invalid, contains
    /// unknown stage config names, or has invalid parameters.
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }

    /// Build a [`Pipeline`] from this configuration.
    pub fn build(&self) -> Pipeline {
        Pipeline {
            stages: self.stage_configs.iter().map(|c| c.build()).collect(),
        }
    }
}

/// An executable pipeline that runs stages sequentially.
///
/// Created by [`PipelineConfig::build()`]. Each call to [`run()`](Pipeline::run)
/// executes all stages in order, passing the [`Context`] through each stage.
pub struct Pipeline {
    stages: Vec<Box<dyn crate::core::stage::Stage>>,
}

impl Pipeline {
    /// Execute all stages sequentially, passing [`Context`] through each.
    ///
    /// # Errors
    ///
    /// Returns [`StageError`] if any stage fails during execution.
    pub fn run(&mut self, mut context: Context) -> Result<Context, StageError> {
        for stage in &mut self.stages {
            context = stage.run(context)?;
        }
        Ok(context)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_hsr2_json() {
        let json = r#"{
            "stage_configs": [
                {"ZeroOrderHoldTimeSynchronizerConfig": {"fps": 10}},
                {"TfChainEnricherConfig": {"frame_pairs": [
                    {"source": "base_footprint", "target": "hand_left_left_finger_tip_frame"}
                ]}}
            ]
        }"#;
        let config = PipelineConfig::from_json(json).unwrap();
        assert_eq!(config.stage_configs.len(), 2);
    }

    #[test]
    fn test_build_creates_pipeline() {
        let json = r#"{
            "stage_configs": [
                {"ZeroOrderHoldTimeSynchronizerConfig": {"fps": 10}}
            ]
        }"#;
        let config = PipelineConfig::from_json(json).unwrap();
        let _pipeline = config.build();
    }

    #[test]
    fn test_parse_empty_stages() {
        let json = r#"{"stage_configs": []}"#;
        let config = PipelineConfig::from_json(json).unwrap();
        assert_eq!(config.stage_configs.len(), 0);
    }

    #[test]
    fn test_parse_unknown_stage_fails() {
        let json = r#"{"stage_configs": [{"NonExistentConfig": {}}]}"#;
        assert!(PipelineConfig::from_json(json).is_err());
    }

    #[test]
    fn test_parse_invalid_params_fails() {
        let json = r#"{"stage_configs": [{"ZeroOrderHoldTimeSynchronizerConfig": {"fps": "not_a_number"}}]}"#;
        assert!(PipelineConfig::from_json(json).is_err());
    }
}
