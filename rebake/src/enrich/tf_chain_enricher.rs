use std::collections::BTreeMap;

extern crate nalgebra as na;
use polars::prelude::*;
use serde::{Deserialize, Serialize};

use super::tf_chain::{ChainComputation, TfChainBuilder};
use crate::core::error::{OptionExt, PolarsExt};
use crate::core::stage::{Context, Stage, StageConfig, StageError};

/// Defines a pair of coordinate frames for which to compute a transform chain.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FramePair {
    /// The name of the source coordinate frame.
    pub source: String,
    /// The name of the target coordinate frame.
    pub target: String,
}

/// Configuration for the `TfChainEnricher` stage.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TfChainEnricherConfig {
    /// A list of `FramePair`s specifying the transform chains to be computed.
    pub frame_pairs: Vec<FramePair>,
}

impl TfChainEnricherConfig {
    pub fn new(frame_pairs: Vec<FramePair>) -> Self {
        Self { frame_pairs }
    }
}

#[typetag::serde(name = "TfChainEnricherConfig")]
impl StageConfig for TfChainEnricherConfig {
    fn build(&self) -> Box<dyn Stage> {
        Box::new(TfChainEnricher::new(self.clone()))
    }
}

/// An enricher that computes and records the transformation chain between specified frame pairs.
///
/// This stage depends on the `/tf_buffer` created by the `TfBufferEnricher`. For each timestamp
/// in the buffer, it calculates the full kinematic chain required to transform a point from a
/// `source` frame to a `target` frame, as defined in the `TfChainEnricherConfig`.
///
/// The result is a new DataFrame named `/tf_chain` added to the context. This DataFrame contains
/// a nested structure where, for each timestamp, the computed transform (translation and rotation)
/// for each requested frame pair is stored alongside an `is_fresh` flag that indicates whether any
/// transform on the path was updated at that timestamp. This is useful for analyzing the relative
/// motion between different parts of a robot or system over time.
///
/// ### Output Schema Example
///
/// The resulting `/tf_chain` DataFrame has a `timestamp_ns` column and additional columns
/// for each unique `source` frame specified in the configuration. The structure is nested
/// as follows.
///
/// If the config contains `frame_pairs` like:
/// - `source: "base_link"`, `target: "hand_palm_link"`
/// - `source: "base_link"`, `target: "odom"`
///
/// The output DataFrame schema will look like this:
///
/// ```text
/// root
///  |-- timestamp_ns: u64
///  |-- base_link: struct
///  |    |-- hand_palm_link: struct
///  |    |    |-- transform: struct
///  |    |    |    |-- translation: struct
///  |    |    |    |    |-- x: f64
///  |    |    |    |    |-- y: f64
///  |    |    |    |    |-- z: f64
///  |    |    |    |-- rotation: struct
///  |    |    |    |    |-- x: f64
///  |    |    |    |    |-- y: f64
///  |    |    |    |    |-- z: f64
///  |    |    |    |    |-- w: f64
///  |    |    |-- is_fresh: bool
///  |    |-- odom: struct
///  |    |    |-- transform: struct
///  |    |    |-- is_fresh: bool
/// ```
///
/// # Preconditions
///
/// - `dataset`: **Required** (must contain `/tf_buffer` topic from `TfBufferEnricher`)
///
/// # Postconditions
///
/// - `dataset`: **Guaranteed** (with `/tf_chain` topic added)
///
/// # Errors
///
/// - [`StageError::MissingData`]: `dataset` not set or `/tf_buffer` topic missing
/// - [`StageError::External`]: Failed to collect tf_buffer dataframe
pub struct TfChainEnricher {
    frame_pairs: Vec<FramePair>,
    builder: Option<TfChainBuilder>,
}

impl TfChainEnricher {
    pub fn new(config: TfChainEnricherConfig) -> Self {
        Self {
            frame_pairs: config.frame_pairs,
            builder: None,
        }
    }
}

impl Stage for TfChainEnricher {
    fn name(&self) -> &'static str {
        "tf_chain_enricher"
    }

    fn run(&mut self, mut context: Context) -> Result<Context, StageError> {
        let dataset = context.dataset().or_missing("dataset in context")?;

        let tf_buffer_data_name = dataset
            .keys()
            .collect::<Vec<_>>()
            .iter()
            .rfind(|s| s.contains("tf_buffer"))
            .or_missing("tf_buffer topic in dataset")?
            .to_string();

        let tf_buffer = dataset
            .get(&tf_buffer_data_name)
            .or_missing("tf_buffer dataframe in dataset")?
            .clone()
            .collect()
            .map_err(|e| StageError::external("failed to collect tf_buffer dataframe", e))?;

        let pair_refs: Vec<(&str, &str)> = self
            .frame_pairs
            .iter()
            .map(|pair| (pair.source.as_str(), pair.target.as_str()))
            .collect();
        let mut pair_transforms: Vec<Vec<ChainComputation>> = self
            .frame_pairs
            .iter()
            .map(|_| Vec::with_capacity(tf_buffer.height()))
            .collect();

        for row_idx in 0..tf_buffer.height() {
            let last_row_transforms = tf_buffer
                .column("transforms")?
                .list()?
                .get_as_series(row_idx)
                .or_missing(&format!("transforms at row {}", row_idx))?;

            let row_transforms = if let Some(builder) = self.builder.as_mut() {
                builder.update(&last_row_transforms)?
            } else {
                let (builder, transforms) =
                    TfChainBuilder::initialize(&pair_refs, &last_row_transforms)?;
                self.builder = Some(builder);
                transforms
            };

            for (pair_idx, result) in row_transforms.into_iter().enumerate() {
                pair_transforms[pair_idx].push(result);
            }
        }

        let mut grouped: BTreeMap<String, Vec<Series>> = BTreeMap::new();
        for (pair, transforms) in self.frame_pairs.iter().zip(pair_transforms.into_iter()) {
            let target_series =
                build_target_series(pair.target.as_str(), &transforms, tf_buffer.height())?;
            grouped
                .entry(pair.source.clone())
                .or_default()
                .push(target_series);
        }

        let mut top_fields = Vec::with_capacity(grouped.len());
        for (source_frame_id, target_serieses) in grouped.iter() {
            let source_frame = StructChunked::from_series(
                source_frame_id.clone().into(),
                tf_buffer.height(),
                target_serieses.iter(),
            )?
            .into_series();
            top_fields.push(source_frame);
        }

        let mut columns: Vec<Series> = Vec::with_capacity(top_fields.len() + 2);

        // Include synched_timestamp_ns if present (i.e., if synchronizer was run before this stage)
        if let Ok(synched_ts) = tf_buffer.column("synched_timestamp_ns") {
            let synched_ts_series = synched_ts
                .u64()
                .or_invalid("synched_timestamp_ns must be UInt64")?
                .clone()
                .into_series();
            columns.push(synched_ts_series);
        }

        let timestamp_series = tf_buffer
            .column("timestamp_ns")?
            .u64()
            .or_invalid("timestamp_ns must be UInt64")?
            .clone()
            .into_series();
        columns.push(timestamp_series);
        columns.extend(top_fields);

        let df = DataFrame::new(columns.into_iter().map(|s| s.into()).collect())?;

        context.insert_data("/tf_chain".to_string(), df.lazy())?;
        Ok(context)
    }
}

fn build_target_series(
    target_name: &str,
    results: &[ChainComputation],
    expected_len: usize,
) -> PolarsResult<Series> {
    let transform_series = build_transform_series(results, expected_len)?;
    let freshness_series = build_freshness_series(results, expected_len);
    StructChunked::from_series(
        target_name.into(),
        expected_len,
        [transform_series, freshness_series].iter(),
    )
    .map(|chunked| chunked.into_series())
}

fn build_transform_series(
    results: &[ChainComputation],
    expected_len: usize,
) -> PolarsResult<Series> {
    let translation = build_translation_series(results, expected_len)?;
    let rotation = build_rotation_series(results, expected_len)?;
    StructChunked::from_series(
        "transform".into(),
        expected_len,
        [translation, rotation].iter(),
    )
    .map(|chunked| chunked.into_series())
}

fn build_freshness_series(results: &[ChainComputation], expected_len: usize) -> Series {
    let is_fresh: Vec<bool> = results.iter().map(|result| result.is_fresh).collect();
    debug_assert_eq!(is_fresh.len(), expected_len);
    Series::new("is_fresh".into(), is_fresh)
}

fn build_translation_series(
    results: &[ChainComputation],
    expected_len: usize,
) -> PolarsResult<Series> {
    if results.len() != expected_len {
        return Err(polars_err!(
            ComputeError: "expected {expected_len} transforms but found {}",
            results.len()
        ));
    }

    let transform_x: Vec<f64> = results
        .iter()
        .map(|result| result.transform.translation.vector.x)
        .collect();
    let transform_y: Vec<f64> = results
        .iter()
        .map(|result| result.transform.translation.vector.y)
        .collect();
    let transform_z: Vec<f64> = results
        .iter()
        .map(|result| result.transform.translation.vector.z)
        .collect();
    let translation_fields = [
        Series::new("x".into(), transform_x),
        Series::new("y".into(), transform_y),
        Series::new("z".into(), transform_z),
    ];
    StructChunked::from_series(
        "translation".into(),
        expected_len,
        translation_fields.iter(),
    )
    .map(|chunked| chunked.into_series())
}

fn build_rotation_series(
    results: &[ChainComputation],
    expected_len: usize,
) -> PolarsResult<Series> {
    if results.len() != expected_len {
        return Err(polars_err!(
            ComputeError: "expected {expected_len} transforms but found {}",
            results.len()
        ));
    }

    let rotation_x: Vec<f64> = results
        .iter()
        .map(|result| result.transform.rotation.coords[0])
        .collect();
    let rotation_y: Vec<f64> = results
        .iter()
        .map(|result| result.transform.rotation.coords[1])
        .collect();
    let rotation_z: Vec<f64> = results
        .iter()
        .map(|result| result.transform.rotation.coords[2])
        .collect();
    let rotation_w: Vec<f64> = results
        .iter()
        .map(|result| result.transform.rotation.coords[3])
        .collect();
    let rotation_fields = [
        Series::new("x".into(), rotation_x),
        Series::new("y".into(), rotation_y),
        Series::new("z".into(), rotation_z),
        Series::new("w".into(), rotation_w),
    ];
    StructChunked::from_series("rotation".into(), expected_len, rotation_fields.iter())
        .map(|chunked| chunked.into_series())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use std::fs;

    use camino::Utf8PathBuf;
    use tempfile::tempdir;

    use super::*;
    use crate::enrich::tf_buffer_enricher::TfBufferEnricherConfig;
    use crate::ingest::rosbag2_ingestor::{Rosbag2Ingestor, Rosbag2IngestorConfig};
    use crate::testutil::{McapGenerator, McapGeneratorConfig};

    #[test]
    fn test_tf_chain_enricher() {
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
        let mut context = ingestor.run(context).unwrap();

        // Run TF buffer enricher first
        let mut tf_buffer = TfBufferEnricherConfig::new().build();
        context = tf_buffer.run(context).unwrap();

        // Define frame pairs that match the generated TF tree
        let frame_pairs = vec![
            FramePair {
                source: "base_link".to_string(),
                target: "arm_link".to_string(),
            },
            FramePair {
                source: "base_link".to_string(),
                target: "hand_link".to_string(),
            },
        ];

        let mut enricher = TfChainEnricherConfig::new(frame_pairs).build();
        let context = enricher.run(context).unwrap();
        let dataset = context.dataset.unwrap();
        let mut df = dataset.get("/tf_chain").unwrap().clone().collect().unwrap();

        assert!(
            df.column("timestamp_ns").is_ok(),
            "tf_chain must include timestamp_ns column"
        );
        assert_eq!(df.column("timestamp_ns").unwrap().len(), df.height());
        let pair_is_fresh = df
            .clone()
            .lazy()
            .select([col("base_link")
                .struct_()
                .field_by_name("hand_link")
                .struct_()
                .field_by_name("is_fresh")
                .alias("pair_is_fresh")])
            .collect()
            .unwrap();
        assert_eq!(
            pair_is_fresh.column("pair_is_fresh").unwrap().len(),
            df.height()
        );

        let temp_dir = tempdir().unwrap();
        let output_path = temp_dir.path().join("tf_chain.parquet");
        let output_path = Utf8PathBuf::from_path_buf(output_path).unwrap();
        let mut file = fs::File::create(output_path.as_std_path()).unwrap();
        ParquetWriter::new(&mut file).finish(&mut df).unwrap();
        assert!(output_path.as_std_path().exists());
    }

    #[test]
    fn test_tf_chain_enricher_supports_sibling_frame_pairs() {
        let dir = tempdir().unwrap();
        let output_dir = Utf8PathBuf::try_from(dir.path().to_path_buf()).unwrap();

        let config = McapGeneratorConfig {
            num_frames: 20,
            fps: 10,
            base_frame: "base_link".to_string(),
            child_frames: vec!["left_tip".to_string(), "right_tip".to_string()],
            generate_images: false,
            generate_tf: true,
            ..Default::default()
        };

        let generator = McapGenerator::new(config);
        let mcap_path = generator.generate(&output_dir).unwrap();

        let mut context = Context::default();
        context.set_rosbag_path(mcap_path);
        let mut ingestor = Rosbag2Ingestor::new(Rosbag2IngestorConfig::without_metadata());
        let mut context = ingestor.run(context).unwrap();

        let mut tf_buffer = TfBufferEnricherConfig::new().build();
        context = tf_buffer.run(context).unwrap();

        let frame_pairs = vec![FramePair {
            source: "left_tip".to_string(),
            target: "right_tip".to_string(),
        }];

        let mut enricher = TfChainEnricherConfig::new(frame_pairs).build();
        let context = enricher.run(context).unwrap();
        let dataset = context.dataset.unwrap();
        let df = dataset.get("/tf_chain").unwrap().clone().collect().unwrap();

        let left_tip = df.column("left_tip").unwrap().struct_().unwrap().clone();
        let right_tip_series = left_tip.field_by_name("right_tip").unwrap().clone();
        let right_tip = right_tip_series.struct_().unwrap().clone();

        assert!(
            right_tip.field_by_name("transform").is_ok(),
            "sibling frame pair should expose a transform field"
        );
        assert!(
            right_tip.field_by_name("is_fresh").is_ok(),
            "sibling frame pair should expose an is_fresh field"
        );
    }
}
