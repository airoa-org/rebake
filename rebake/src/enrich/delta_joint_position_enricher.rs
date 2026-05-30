use polars::prelude::*;
use serde::{Deserialize, Serialize};

use crate::core::error::OptionExt;
use crate::core::stage::{Context, Stage, StageConfig, StageError};

fn delta_position_expr() -> Expr {
    let current = col("position");
    let previous = current.clone().shift(lit(1));
    let delta = current.clone() - previous.clone();
    let zero = current.clone() - current;

    when(previous.is_null())
        .then(zero)
        .otherwise(delta)
        .alias("delta_position")
}

fn has_float64_list_field(schema: &Schema, field_name: &str) -> bool {
    schema.get(field_name).is_some_and(|dtype| match dtype {
        DataType::List(inner) => matches!(inner.as_ref(), DataType::Float64),
        _ => false,
    })
}

/// Configuration for the `DeltaJointPositionEnricher` stage.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeltaJointPositionEnricherConfig {
    /// A list of topic names to be processed. These topics should contain JointState
    /// message with a `position` field.
    pub topic_names: Vec<String>,
}

impl DeltaJointPositionEnricherConfig {
    pub fn new<T: Into<String>>(topic_names: Vec<T>) -> Self {
        Self {
            topic_names: topic_names.into_iter().map(Into::into).collect(),
        }
    }
}

#[typetag::serde(name = "DeltaJointPositionEnricherConfig")]
impl StageConfig for DeltaJointPositionEnricherConfig {
    fn build(&self) -> Box<dyn Stage> {
        Box::new(DeltaJointPositionEnricher::new(self.clone()))
    }
}

/// An enricher that calculates the change (delta) in joint positions between consecutive messages.
///
/// For each topic specified in the `DeltaJointPositionEnricherConfig`, this stage computes the
/// element-wise difference of the `position` field relative to the previous message in the
/// same topic. The result is stored in a new column named `delta_position`.
///
/// The delta for the first message is always a zero vector of the same dimension as `position`.
///
/// # Preconditions
///
/// - `dataset`: **Required** (topics must have `position` field of type `List<Float64>`)
///
/// # Postconditions
///
/// - `dataset`: **Guaranteed** (specified topics enriched with `delta_position` column)
///
/// Note: Topics without a valid `position` field are silently skipped.
///
/// # Errors
///
/// - [`StageError::MissingData`]: `dataset` not set in context
pub struct DeltaJointPositionEnricher {
    topic_names: Vec<String>,
}

impl DeltaJointPositionEnricher {
    pub fn new(config: DeltaJointPositionEnricherConfig) -> Self {
        Self {
            topic_names: config.topic_names,
        }
    }
}

impl Stage for DeltaJointPositionEnricher {
    fn name(&self) -> &'static str {
        "delta_joint_position_enricher"
    }

    fn run(&mut self, mut context: Context) -> Result<Context, StageError> {
        let dataset = context.dataset_mut().or_missing("dataset in context")?;

        for topic in &self.topic_names {
            let Some(frame) = dataset.get_mut(topic) else {
                continue;
            };

            let mut schema_probe = frame.clone();
            let schema = schema_probe.collect_schema()?;
            if !has_float64_list_field(schema.as_ref(), "position") {
                continue;
            }

            let updated = frame.clone().with_columns(vec![delta_position_expr()]);
            *frame = updated;
        }

        Ok(context)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use camino::Utf8PathBuf;
    use tempfile::tempdir;

    use super::*;
    use crate::ingest::rosbag2_ingestor::{Rosbag2Ingestor, Rosbag2IngestorConfig};
    use crate::testutil::{McapGenerator, McapGeneratorConfig};

    #[test]
    fn enriches_joint_state_position_with_delta() {
        // Generate synthetic MCAP with JointState data
        let dir = tempdir().unwrap();
        let output_dir = Utf8PathBuf::try_from(dir.path().to_path_buf()).unwrap();

        let config = McapGeneratorConfig {
            num_frames: 10,
            fps: 10,
            joint_names: vec!["joint1".to_string(), "joint2".to_string()],
            generate_images: false,
            generate_tf: false,
            ..Default::default()
        };

        let generator = McapGenerator::new(config);
        let mcap_path = generator.generate(&output_dir).unwrap();

        // Ingest the MCAP
        let mut context = Context::default();
        context.set_rosbag_path(mcap_path);
        let mut ingestor = Rosbag2Ingestor::new(Rosbag2IngestorConfig::without_metadata());
        let context = ingestor.run(context).unwrap();
        let dataset = context.dataset.unwrap();

        // Run the enricher
        let mut enricher = DeltaJointPositionEnricherConfig::new(vec!["/joint_states"]).build();

        let context = Context::new(dataset);
        let context = enricher.run(context).unwrap();
        let dataset = context.dataset.unwrap();
        let df = dataset
            .get("/joint_states")
            .unwrap()
            .clone()
            .collect()
            .unwrap();

        let delta_column = df
            .column("delta_position")
            .expect("delta_position column must exist")
            .list()
            .expect("delta_position must be a list column")
            .clone();

        // First row delta should be all zeros
        let first_row_series = delta_column.get_as_series(0).unwrap();
        let first_row = first_row_series.f64().unwrap();
        assert!(first_row.into_iter().all(|v| v.unwrap() == 0.0));

        // Second row delta should have non-zero values (position changes between frames)
        let second_row_series = delta_column.get_as_series(1).unwrap();
        let second_row = second_row_series.f64().unwrap();
        assert!(second_row.into_iter().any(|v| v.unwrap() != 0.0));
    }
}
