use polars::lazy::dsl::as_struct;
use polars::prelude::*;
use serde::{Deserialize, Serialize};

use crate::core::error::OptionExt;
use crate::core::stage::{Context, Stage, StageConfig, StageError};

/// Configuration for the `TfBufferEnricher` stage.
///
/// The `TfBufferEnricher` does not have any configuration parameters.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct TfBufferEnricherConfig {}

impl TfBufferEnricherConfig {
    pub fn new() -> Self {
        Self::default()
    }
}

#[typetag::serde(name = "TfBufferEnricherConfig")]
impl StageConfig for TfBufferEnricherConfig {
    fn build(&self) -> Box<dyn Stage> {
        Box::new(TfBufferEnricher::new(self.clone()))
    }
}

/// An enricher that processes `/tf` and optional `/tf_static` topics to create a time-indexed TF buffer.
///
/// This stage reads all TF messages, including static transforms when available, and constructs a complete
/// TF buffer. For each timestamp in the original `/tf` topic, it ensures that a transform
/// is available for every unique parent-child frame pair. It achieves this by back-filling
/// transforms using an as-of join, effectively carrying forward the last known transform for
/// each pair.
///
/// The output is a new DataFrame named `/tf_buffer` added to the context, where each row
/// corresponds to a timestamp and contains a list of all available transforms at that time.
/// This buffer is essential for downstream stages like `TfChainEnricher` that need to
/// look up transform chains at arbitrary times.
///
/// # Preconditions
///
/// - `dataset`: **Required** (must contain `/tf` topic)
/// - `dataset`: **Optional** (may contain `/tf_static` topic)
///
/// # Postconditions
///
/// - `dataset`: **Guaranteed** (with `/tf_buffer` topic added)
///
/// # Errors
///
/// - [`StageError::MissingData`]: `dataset` not set or `/tf` topic missing
pub struct TfBufferEnricher {
    _config: TfBufferEnricherConfig,
}

impl TfBufferEnricher {
    pub fn new(_config: TfBufferEnricherConfig) -> Self {
        Self { _config }
    }
}

impl Stage for TfBufferEnricher {
    fn name(&self) -> &'static str {
        "tf_buffer_enricher"
    }

    /// Builds a TF buffer representation by filling missing timestamps with the
    /// most recent transform for every `child_frame_id`, and tags each entry with
    /// an `is_fresh` flag indicating whether it originates from the current timestamp.
    fn run(&mut self, mut context: Context) -> Result<Context, StageError> {
        let dataset = context.dataset().or_missing("dataset in context")?;

        let tf_data_name = dataset
            .keys()
            .collect::<Vec<_>>()
            .iter()
            .rfind(|s| s.ends_with("tf"))
            .or_missing("tf topic in dataset")?
            .to_string();
        let tf_frame = dataset
            .get(&tf_data_name)
            .or_missing("tf dataframe in dataset")?
            .clone();
        let tf_static_frame = dataset
            .keys()
            .collect::<Vec<_>>()
            .iter()
            .rfind(|s| s.ends_with("tf_static"))
            .and_then(|name| dataset.get(*name))
            .cloned();
        let tf_df = tf_frame.clone().collect()?;

        // Preserve provenance-like columns from /tf, but drop per-message transport timestamps.
        // A /tf_buffer row is a derived snapshot that can contain backfilled transforms from
        // different source messages, so it does not have a single well-defined publish timestamp.
        let extra_columns: Vec<String> = tf_df
            .get_column_names()
            .iter()
            .filter(|name| {
                **name != "timestamp_ns"
                    && **name != "publish_timestamp_ns"
                    && **name != "transforms"
            })
            .map(|name| name.to_string())
            .collect();

        let mut exploded_transforms = explode_transforms(tf_df)?;

        if let Some(tf_static_frame) = tf_static_frame {
            let static_frame = tf_static_frame.clone().collect()?;
            if static_frame.height() > 0 {
                let static_transforms = explode_transforms(static_frame)?;
                exploded_transforms.vstack_mut(&static_transforms)?;
            }
        }

        let expected_child_count = count_unique_child_frames(&exploded_transforms)?;

        let time_frame_grid = build_time_frame_grid(&exploded_transforms)?;
        let backfilled_transforms =
            backfill_latest_transforms(time_frame_grid, &exploded_transforms)?;
        let valid_transforms = filter_rows_with_transforms(backfilled_transforms)?;

        let mut tf_buffer = group_transforms_by_timestamp(valid_transforms, expected_child_count);

        // Add extra columns from /tf to the output (broadcast first row's values)
        if !extra_columns.is_empty() {
            let tf_df = tf_frame.collect()?;
            for col_name in &extra_columns {
                if let Ok(col) = tf_df.column(col_name.as_str())
                    && !col.is_empty()
                {
                    // CONTRACT: !col.is_empty() check above guarantees index 0 is valid
                    #[allow(clippy::expect_used)]
                    let first_value = col.get(0).expect("column is not empty - checked above");
                    tf_buffer = tf_buffer.with_column(
                        Expr::Literal(first_value.into_static().into()).alias(col_name),
                    );
                }
            }
        }

        context.insert_data(format!("{}_buffer", tf_data_name), tf_buffer)?;
        Ok(context)
    }
}

/// Expands the nested `transforms` list column into flat rows with the same schema
/// as the original TF messages.
fn explode_transforms(df: DataFrame) -> PolarsResult<DataFrame> {
    df.explode(["transforms"])?
        .unnest(["transforms"])?
        .lazy()
        .with_columns([col("timestamp_ns").alias("source_timestamp_ns")])
        .collect()
}

/// Builds the Cartesian product of distinct timestamps and child frames so that
/// every combination can be filled via an as-of join.
fn build_time_frame_grid(transforms: &DataFrame) -> PolarsResult<DataFrame> {
    let child_frames = transforms.select(["child_frame_id"])?.unique_stable(
        None,
        UniqueKeepStrategy::First,
        None,
    )?;

    let timestamps = transforms
        .select(["timestamp_ns"])?
        .unique_stable(None, UniqueKeepStrategy::First, None)?
        .sort(["timestamp_ns"], SortMultipleOptions::default())?;

    timestamps.cross_join(&child_frames, None, None)
}

/// Adds the latest known transform to each time/frame grid row using a backward as-of join.
fn backfill_latest_transforms(
    time_frame_grid: DataFrame,
    transforms: &DataFrame,
) -> PolarsResult<DataFrame> {
    let sorted_transforms = transforms.sort(
        ["child_frame_id", "timestamp_ns"],
        SortMultipleOptions::default(),
    )?;

    time_frame_grid.join_asof_by(
        &sorted_transforms,
        "timestamp_ns",
        "timestamp_ns",
        ["child_frame_id"],
        ["child_frame_id"],
        AsofStrategy::Backward,
        None,
        true,
        false,
    )
}

/// Filters out grid rows that still lack a valid transform after the join and
/// computes an `is_fresh` flag by comparing the joined source timestamp with the
/// target timeline.
fn filter_rows_with_transforms(df: DataFrame) -> PolarsResult<DataFrame> {
    let mask = df.column("header")?.is_not_null();
    let filtered = df.filter(&mask)?;

    filtered
        .lazy()
        .with_columns([col("source_timestamp_ns")
            .eq(col("timestamp_ns"))
            .alias("is_fresh")])
        .select([
            col("timestamp_ns"),
            col("child_frame_id"),
            col("header"),
            col("transform"),
            col("is_fresh"),
        ])
        .collect()
}

/// Reassembles the TF buffer as a lazy dataset grouped by timestamp, embedding
/// `child_frame_id`, `header`, `transform`, and `is_fresh` within each list entry.
fn group_transforms_by_timestamp(df: DataFrame, expected_child_count: usize) -> LazyFrame {
    df.lazy()
        .with_columns([as_struct(vec![
            col("child_frame_id"),
            col("header"),
            col("transform"),
            col("is_fresh"),
        ])
        .alias("transforms")])
        .group_by([col("timestamp_ns")])
        .agg([
            col("transforms").implode(),
            col("child_frame_id").n_unique().alias("child_frame_count"),
        ])
        .filter(col("child_frame_count").eq(lit(expected_child_count as u32)))
        .select([col("timestamp_ns"), col("transforms")])
}

fn count_unique_child_frames(transforms: &DataFrame) -> PolarsResult<usize> {
    transforms.column("child_frame_id")?.n_unique()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use std::fs;

    use super::*;
    use camino::Utf8PathBuf;
    use tempfile::tempdir;

    use crate::ingest::rosbag2_ingestor::{Rosbag2Ingestor, Rosbag2IngestorConfig};
    use crate::testutil::{McapGenerator, McapGeneratorConfig};

    #[test]
    fn test_tf_buffer_enricher_materialize() {
        // Generate synthetic MCAP with TF data
        let dir = tempdir().unwrap();
        let output_dir = Utf8PathBuf::try_from(dir.path().to_path_buf()).unwrap();

        let config = McapGeneratorConfig {
            num_frames: 20,
            fps: 10,
            base_frame: "base_link".to_string(),
            child_frames: vec!["arm_link".to_string(), "hand_link".to_string()],
            generate_images: false,
            generate_tf: true,
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

        // Run enricher
        let mut enricher = TfBufferEnricherConfig::new().build();
        let temp_dir = tempdir().unwrap();
        let output_path = temp_dir.path().join("tf_buffer.parquet");
        let output_path = Utf8PathBuf::from_path_buf(output_path).unwrap();

        let context = Context::new(dataset);
        let context = enricher.run(context).unwrap();
        let dataset = context.dataset.unwrap();
        let mut df = dataset
            .get("/tf_buffer")
            .unwrap()
            .clone()
            .collect()
            .unwrap();

        // Ensure child frames are present after enrichment
        let flattened = df
            .clone()
            .explode(["transforms"])
            .unwrap()
            .unnest(["transforms"])
            .unwrap();
        let arm_link_count = flattened
            .lazy()
            .filter(col("child_frame_id").eq(lit("arm_link")))
            .select([col("child_frame_id")])
            .collect()
            .unwrap()
            .height();
        assert!(
            arm_link_count > 0,
            "expected at least one arm_link transform in tf_buffer"
        );

        let mut file = fs::File::create(output_path.as_std_path()).unwrap();
        ParquetWriter::new(&mut file).finish(&mut df).unwrap();
        assert!(output_path.as_std_path().exists());
    }

    #[test]
    fn test_tf_buffer_preserves_extra_columns() {
        // Generate synthetic MCAP with TF data
        let dir = tempdir().unwrap();
        let output_dir = Utf8PathBuf::try_from(dir.path().to_path_buf()).unwrap();

        let config = McapGeneratorConfig {
            num_frames: 10,
            fps: 10,
            base_frame: "base_link".to_string(),
            child_frames: vec!["arm_link".to_string()],
            generate_images: false,
            generate_tf: true,
            ..Default::default()
        };

        let generator = McapGenerator::new(config);
        let mcap_path = generator.generate(&output_dir).unwrap();

        // Ingest the MCAP
        let mut context = Context::default();
        context.set_rosbag_path(mcap_path);
        let mut ingestor = Rosbag2Ingestor::new(Rosbag2IngestorConfig::without_metadata());
        let context = ingestor.run(context).unwrap();
        let mut dataset = context.dataset.unwrap();

        // Add rosbag_uuid column to /tf
        let tf_lf = dataset.get("/tf").unwrap().clone();
        let tf_with_uuid = tf_lf.with_column(lit("test-uuid-123").alias("rosbag_uuid"));
        dataset.insert("/tf".to_string(), tf_with_uuid);

        // Also add to /tf_static if it exists
        if let Some(tf_static_lf) = dataset.get("/tf_static") {
            let tf_static_with_uuid = tf_static_lf
                .clone()
                .with_column(lit("test-uuid-123").alias("rosbag_uuid"));
            dataset.insert("/tf_static".to_string(), tf_static_with_uuid);
        }

        // Run TfBufferEnricher
        let context = Context::new(dataset);
        let mut enricher = TfBufferEnricher::new(TfBufferEnricherConfig::new());
        let result = enricher.run(context).unwrap();

        let tf_buffer = result
            .dataset
            .unwrap()
            .get("/tf_buffer")
            .unwrap()
            .clone()
            .collect()
            .unwrap();

        // Verify rosbag_uuid column exists in output
        assert!(
            tf_buffer.column("rosbag_uuid").is_ok(),
            "tf_buffer should contain rosbag_uuid column"
        );

        // Verify the value is correct
        let uuid_col = tf_buffer.column("rosbag_uuid").unwrap();
        let uuid_values: Vec<&str> = uuid_col.str().unwrap().into_no_null_iter().collect();
        assert!(
            uuid_values.iter().all(|v| *v == "test-uuid-123"),
            "all rosbag_uuid values should be 'test-uuid-123'"
        );
    }

    #[test]
    fn test_tf_buffer_omits_publish_timestamp_ns() {
        let dir = tempdir().unwrap();
        let output_dir = Utf8PathBuf::try_from(dir.path().to_path_buf()).unwrap();

        let config = McapGeneratorConfig {
            num_frames: 10,
            fps: 10,
            base_frame: "base_link".to_string(),
            child_frames: vec!["arm_link".to_string()],
            generate_images: false,
            generate_tf: true,
            ..Default::default()
        };

        let generator = McapGenerator::new(config);
        let mcap_path = generator.generate(&output_dir).unwrap();

        let mut context = Context::default();
        context.set_rosbag_path(mcap_path);
        let mut ingestor = Rosbag2Ingestor::new(Rosbag2IngestorConfig::without_metadata());
        let context = ingestor.run(context).unwrap();
        let dataset = context.dataset.unwrap();

        let tf_df = dataset.get("/tf").unwrap().clone().collect().unwrap();
        assert!(
            tf_df.column("publish_timestamp_ns").is_ok(),
            "/tf should retain publish_timestamp_ns from ROS2 ingest"
        );

        let context = Context::new(dataset);
        let mut enricher = TfBufferEnricher::new(TfBufferEnricherConfig::new());
        let result = enricher.run(context).unwrap();

        let tf_buffer = result
            .dataset
            .unwrap()
            .get("/tf_buffer")
            .unwrap()
            .clone()
            .collect()
            .unwrap();

        assert!(
            tf_buffer.column("publish_timestamp_ns").is_err(),
            "/tf_buffer should not expose a top-level publish_timestamp_ns"
        );
    }

    /// Normal case: /tf_buffer topic is created when /tf and /tf_static topics exist
    #[test]
    fn test_enrich_creates_tf_buffer_topic() {
        let dir = tempdir().unwrap();
        let output_dir = Utf8PathBuf::try_from(dir.path().to_path_buf()).unwrap();

        let config = McapGeneratorConfig {
            num_frames: 5,
            fps: 30,
            generate_images: false,
            generate_tf: true,
            ..Default::default()
        };
        let generator = McapGenerator::new(config);
        let mcap_path = generator.generate(&output_dir).unwrap();

        let mut context = Context::default();
        context.set_rosbag_path(mcap_path);

        let mut ingestor = Rosbag2Ingestor::new(Rosbag2IngestorConfig::without_metadata());
        let context = ingestor.run(context).expect("ingestor should succeed");

        let tf_config = TfBufferEnricherConfig::new();
        let mut tf_enricher = tf_config.build();

        let result = tf_enricher.run(context);

        assert!(result.is_ok());
        let ctx = result.unwrap();
        let dataset = ctx.dataset.unwrap();
        assert!(
            dataset.contains_key("/tf_buffer"),
            "/tf_buffer topic should be created"
        );
    }

    /// /tf_static is optional: /tf_buffer can still be created from /tf alone.
    #[test]
    fn test_enrich_creates_tf_buffer_topic_without_tf_static() {
        let dir = tempdir().unwrap();
        let output_dir = Utf8PathBuf::try_from(dir.path().to_path_buf()).unwrap();

        let config = McapGeneratorConfig {
            num_frames: 5,
            fps: 30,
            generate_images: false,
            generate_tf: true,
            ..Default::default()
        };
        let generator = McapGenerator::new(config);
        let mcap_path = generator.generate(&output_dir).unwrap();

        let mut context = Context::default();
        context.set_rosbag_path(mcap_path);

        let mut ingestor = Rosbag2Ingestor::new(Rosbag2IngestorConfig::without_metadata());
        let mut context = ingestor.run(context).expect("ingestor should succeed");
        context.dataset.as_mut().unwrap().remove("/tf_static");

        let tf_config = TfBufferEnricherConfig::new();
        let mut tf_enricher = tf_config.build();

        let result = tf_enricher.run(context);

        assert!(result.is_ok());
        let ctx = result.unwrap();
        let dataset = ctx.dataset.unwrap();
        assert!(
            dataset.contains_key("/tf_buffer"),
            "/tf_buffer topic should be created without /tf_static"
        );
    }

    /// Error case: returns MissingData error when /tf topic does not exist
    #[test]
    fn test_enrich_missing_tf_topic_returns_missing_data_error() {
        let dir = tempdir().unwrap();
        let output_dir = Utf8PathBuf::try_from(dir.path().to_path_buf()).unwrap();

        let config = McapGeneratorConfig {
            num_frames: 5,
            fps: 30,
            generate_images: false,
            generate_tf: false,
            ..Default::default()
        };
        let generator = McapGenerator::new(config);
        let mcap_path = generator.generate(&output_dir).unwrap();

        let mut context = Context::default();
        context.set_rosbag_path(mcap_path);

        let mut ingestor = Rosbag2Ingestor::new(Rosbag2IngestorConfig::without_metadata());
        let context = ingestor.run(context).expect("ingestor should succeed");

        let tf_config = TfBufferEnricherConfig::new();
        let mut tf_enricher = tf_config.build();

        let result = tf_enricher.run(context);

        assert!(result.is_err());
        let err = result.err().unwrap();
        assert!(matches!(err, StageError::MissingData(_)));
    }
}
