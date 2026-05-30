use polars::prelude::*;
use serde::{Deserialize, Serialize};

use crate::core::error::OptionExt;
use crate::core::stage::{Context, Stage, StageConfig, StageError};

/// Column name for the rosbag UUID
pub const ROSBAG_UUID_COL: &str = "rosbag_uuid";

/// Configuration for the `UuidEnricher` stage.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct UuidEnricherConfig {}

impl UuidEnricherConfig {
    pub fn new() -> Self {
        Self {}
    }
}

#[typetag::serde(name = "UuidEnricherConfig")]
impl StageConfig for UuidEnricherConfig {
    fn build(&self) -> Box<dyn Stage> {
        Box::new(UuidEnricher::new(self.clone()))
    }
}

/// A stage that adds a `rosbag_uuid` column to all DataFrames in the context.
///
/// This enricher reads the UUID from the airoa metadata (meta.json) that was loaded
/// by the Ingestor, and adds it as a column to every topic's DataFrame.
///
/// This enables tracking which rosbag each record came from when multiple rosbags
/// are processed and stored together (e.g., in Iceberg tables).
///
/// # Preconditions
///
/// - `dataset`: **Required** (all topics as LazyFrame)
/// - `airoa_metadata`: Conditional (if missing, stage is skipped)
///
/// # Postconditions
///
/// - `dataset`: **Guaranteed** (all topics enriched with `rosbag_uuid` column)
///
/// # Errors
///
/// - [`StageError::Skip`]: `airoa_metadata` not found in context (stage skipped gracefully)
/// - [`StageError::MissingData`]: `dataset` not set in context
pub struct UuidEnricher;

impl UuidEnricher {
    pub fn new(_config: UuidEnricherConfig) -> Self {
        Self
    }
}

impl Stage for UuidEnricher {
    fn name(&self) -> &'static str {
        "uuid_enricher"
    }

    fn run(&mut self, mut context: Context) -> Result<Context, StageError> {
        let uuid = context
            .airoa_metadata()
            .map(|m| m.uuid_string())
            .ok_or_else(|| {
                StageError::skip("airoa_metadata not found in context, skipping UUID enrichment")
            })?;

        let dataset = context.dataset.take().or_missing("dataset in context")?;

        let enriched_dataset = dataset
            .into_iter()
            .map(|(topic, lf)| {
                let enriched_lf = lf.with_column(lit(uuid.clone()).alias(ROSBAG_UUID_COL));
                (topic, enriched_lf)
            })
            .collect();

        context.set_dataset(enriched_dataset);
        Ok(context)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    use crate::schema::metadata::AiroaMetadata;
    use crate::schema::metadata::v2_0::{
        Device, EnvType, Environment, Episode, File, GitSource, MetadataV2_0, Program, Robot,
        Runner, RunnerType, Segment, Source,
    };

    fn create_test_metadata() -> MetadataV2_0 {
        MetadataV2_0 {
            schema: "https://example.com/schema".to_string(),
            schema_version: "2.0".to_string(),
            uuid: "f0d3f012-a96c-477a-a549-407a28788e79".to_string(),
            robot: Robot {
                uri: None,
                robot_type: "HSR".to_string(),
                id: "hsr2".to_string(),
                checksum: None,
            },
            files: vec![File {
                file_type: "rosbag".to_string(),
                name: "data.bag".to_string(),
                checksum: None,
            }],
            environment: Environment {
                env_type: EnvType::RealWorld,
                site: "test_lab".to_string(),
                location: None,
            },
            runner: Runner {
                runner_type: RunnerType::Operator,
                organization: "test".to_string(),
                name: "TestOperator".to_string(),
            },
            devices: vec![Device {
                role: "controller".to_string(),
                device_type: "joystick".to_string(),
                id: "joystick001".to_string(),
            }],
            programs: vec![Program {
                role: "interface".to_string(),
                name: "test".to_string(),
                source: Source {
                    git: Some(GitSource {
                        uri: "https://example.com".to_string(),
                        hash: "v1.0".to_string(),
                        branch: "main".to_string(),
                        tag: None,
                    }),
                },
            }],
            episode: Episode {
                start_time: 0.0,
                end_time: 10.0,
                success: true,
                label: "Test".to_string(),
            },
            labels: vec!["test".to_string()],
            segments: vec![Segment {
                start_time: 0.0,
                end_time: 10.0,
                label_idx: 0,
                success: true,
            }],
        }
    }

    #[test]
    fn test_uuid_enricher() {
        // Create test DataFrame
        let df = df! {
            "timestamp_ns" => [1u64, 2, 3],
            "value" => [10i32, 20, 30]
        }
        .unwrap()
        .lazy();

        let mut dataset = HashMap::new();
        dataset.insert("/test_topic".to_string(), df);

        let mut context = Context::new(dataset);
        context.set_airoa_metadata(AiroaMetadata::V2_0(create_test_metadata()));

        let mut enricher = UuidEnricher::new(UuidEnricherConfig::new());
        let result = enricher.run(context).unwrap();

        let enriched_df = result
            .dataset
            .unwrap()
            .get("/test_topic")
            .unwrap()
            .clone()
            .collect()
            .unwrap();

        // Check that rosbag_uuid column was added
        assert!(enriched_df.column(ROSBAG_UUID_COL).is_ok());

        // Check that all values are the expected UUID
        let uuid_col = enriched_df.column(ROSBAG_UUID_COL).unwrap();
        let uuid_values: Vec<&str> = uuid_col.str().unwrap().into_no_null_iter().collect();
        let expected_uuid = "f0d3f012-a96c-477a-a549-407a28788e79";
        assert_eq!(
            uuid_values,
            vec![expected_uuid, expected_uuid, expected_uuid]
        );
    }

    /// Edge case: returns Skip error (normal skip) when airoa_metadata does not exist
    #[test]
    fn test_enrich_skips_when_no_metadata() {
        let df = df! {
            "timestamp_ns" => [1u64, 2, 3],
            "value" => [1.0, 2.0, 3.0]
        }
        .unwrap()
        .lazy();

        let mut dataset = HashMap::new();
        dataset.insert("/test_topic".to_string(), df);

        let context = Context::new(dataset);
        // airoa_metadata is NOT set

        let mut enricher = UuidEnricher::new(UuidEnricherConfig::new());
        let result = enricher.run(context);

        assert!(result.is_err());
        let err = result.err().unwrap();
        assert!(matches!(err, StageError::Skip { .. }));
    }
}
