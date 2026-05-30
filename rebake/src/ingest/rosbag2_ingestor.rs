use std::collections::HashMap;
use std::fs;

use camino::Utf8PathBuf;
use mcap::MessageStream;
use memmap2::Mmap;
use serde::{Deserialize, Serialize};
use tempfile::tempdir;

use crate::core::error::OptionExt;
use crate::core::{
    Context, Stage, StageConfig, StageError, conversion::record_batch_to_lazy,
    stage::PipelineInputKind,
};
use crate::ingest::common::{
    compute_image_shapes, infer_image_topic_shapes_from_payload, is_compressed_depth_topic,
    is_compressed_image_topic, is_point_cloud2_type, is_raw_image_type,
};
use crate::ingest::pipeline::Pipeline;
use crate::ros::msg_deserializer::RosGeneration;
use crate::schema::metadata::{parse_metadata, parse_metadata_as_v2_0};

/// Configuration for the `Rosbag2Ingestor` stage.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Rosbag2IngestorConfig {
    /// Whether to require meta.json (airoa metadata) to be present.
    /// Defaults to true. Set to false for testing or non-airoa rosbags.
    #[serde(default = "default_require_metadata")]
    pub require_metadata: bool,
}

fn default_require_metadata() -> bool {
    true
}

impl Default for Rosbag2IngestorConfig {
    fn default() -> Self {
        Self {
            require_metadata: true,
        }
    }
}

impl Rosbag2IngestorConfig {
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a config that does not require metadata (for testing).
    pub fn without_metadata() -> Self {
        Self {
            require_metadata: false,
        }
    }
}

#[typetag::serde(name = "Rosbag2IngestorConfig")]
impl StageConfig for Rosbag2IngestorConfig {
    fn build(&self) -> Box<dyn Stage> {
        Box::new(Rosbag2Ingestor::new(self.clone()))
    }

    fn pipeline_input_kind(&self) -> Option<PipelineInputKind> {
        Some(PipelineInputKind::Rosbag)
    }
}

/// A stage that ingests a rosbag file, converting its topics into a dataset of Polars DataFrames.
///
/// This is typically the first stage in a `rebake` pipeline. It requires the `rosbag_path` to be
/// set in the `Context` before execution. The stage reads the `.bag` file and processes each
/// topic based on its message type.
///
/// ### Conversion Rules
///
/// - **General Topics:** Messages are parsed and converted into rows in a Parquet file. A
///   `timestamp_ns` column (u64, MCAP log time) and `publish_timestamp_ns` column
///   (u64, nullable) are added to each record.
///
/// - **Image and Depth Topics (e.g., `sensor_msgs/CompressedImage`):**
///   To avoid storing large binary blobs in Parquet files, the raw `data` from these messages
///   is extracted and stored in an in-memory table within the `Context`. The Parquet record,
///   instead of containing the data, holds a `uint32 index` column. This index corresponds
///   to the position of the raw data in the in-memory table.
///
/// ### Context Output
///
/// After execution, this stage populates the `Context` with the following:
/// - **`dataset`:** A `HashMap<String, LazyFrame>` where keys are topic names and values are
///   Polars `LazyFrame`s created from the temporary Parquet files.
/// - **`image_data`:** An in-memory table holding the raw byte data for all messages from
///   CompressedImage topics.
/// - **`depth_data`:** An in-memory table holding the raw byte data for all messages from
///   CompressedDepth topics.
/// - **`output_dir`:** The path to the temporary directory created to store the generated
///   Parquet files for this ingestion.
///
/// # Preconditions
///
/// - `rosbag_path`: **Required** (set by Orchestrator)
///
/// # Postconditions
///
/// - `dataset`: **Guaranteed** (all topics as LazyFrame)
/// - `image_data`: Conditional (if image topics exist)
/// - `depth_data`: Conditional (if depth topics exist)
/// - `pointcloud_data`: Conditional (if point cloud topics exist)
/// - `output_dir`: **Guaranteed** (temp directory path)
/// - `topic_message_type_map`: **Guaranteed**
/// - `airoa_metadata`: Conditional (depends on `require_metadata` setting)
///
/// # Errors
///
/// - [`StageError::MissingData`]: `rosbag_path` not set
/// - [`StageError::Io`]: MCAP file read failure, meta.json read failure
/// - [`StageError::InvalidData`]: message stream creation failure
pub struct Rosbag2Ingestor {
    require_metadata: bool,
}

impl Rosbag2Ingestor {
    pub fn new(config: Rosbag2IngestorConfig) -> Self {
        Self {
            require_metadata: config.require_metadata,
        }
    }
}

impl Stage for Rosbag2Ingestor {
    fn name(&self) -> &'static str {
        "rosbag2_ingestor"
    }

    fn run(&mut self, mut context: Context) -> Result<Context, StageError> {
        let rosbag_path = context
            .rosbag_path()
            .cloned()
            .or_missing("rosbag_path in context")?;

        let mcap_file = read_mcap(rosbag_path.as_ref())
            .map_err(|e| StageError::io(format!("failed to open mcap file: {}", rosbag_path), e))?;

        let mut pipeline = Pipeline::new(RosGeneration::ROS2);
        let message_stream = MessageStream::new(&mcap_file).map_err(|e| {
            StageError::invalid_with("failed to create message stream from mcap", e)
        })?;
        let mut topic_message_type_map = HashMap::new();

        for message_result in message_stream {
            let message = message_result
                .map_err(|e| StageError::invalid_with("failed to read message from mcap", e))?;
            let Some(schema) = &message.channel.schema else {
                continue;
            };

            if schema.data.is_empty() {
                continue;
            }

            let topic_name = &message.channel.topic;

            // Register schema if new topic
            if !topic_message_type_map.contains_key(topic_name) {
                let type_name = extract_message_type(&schema.name).to_string();
                let mut message_definition = std::str::from_utf8(&schema.data)
                    .map_err(|e| {
                        StageError::invalid_with(
                            format!("schema data is not valid UTF-8 for topic: {}", topic_name),
                            e,
                        )
                    })?
                    .to_string();

                if is_compressed_image_topic(topic_name)
                    || is_compressed_depth_topic(topic_name)
                    || is_raw_image_type(&type_name)
                {
                    message_definition = message_definition.replace("uint8[] data", "uint32 index");
                    pipeline.add_image_info(topic_name);
                } else if is_point_cloud2_type(&type_name) {
                    message_definition = message_definition.replace("uint8[] data", "uint32 index");
                    pipeline.add_pointcloud_info(topic_name);
                }

                pipeline.add_message_definition(topic_name, &type_name, &message_definition)?;
                topic_message_type_map.insert(topic_name.to_string(), schema.name.clone());
            }

            // Process data
            let type_name = extract_message_type(&schema.name);
            let publish_timestamp_ns = Some(message.publish_time);

            if is_point_cloud2_type(type_name) {
                pipeline.add_pointcloud_data(
                    topic_name,
                    &message.data,
                    message.log_time,
                    publish_timestamp_ns,
                )?;
            } else if is_compressed_image_topic(topic_name)
                || is_compressed_depth_topic(topic_name)
                || is_raw_image_type(type_name)
            {
                pipeline.add_image_data(
                    topic_name,
                    &message.data,
                    message.log_time,
                    publish_timestamp_ns,
                )?;
            } else {
                pipeline.add_message_data(
                    topic_name,
                    &message.data,
                    message.log_time,
                    publish_timestamp_ns,
                )?;
            }
        }

        let record_batches = pipeline.finish()?;
        let temp_dir = tempdir().map_err(|e| {
            StageError::io(
                "failed to create temporary directory for MCAP processing",
                e,
            )
        })?;

        let dataset = record_batches
            .into_iter()
            .map(|(topic, batch)| (topic, record_batch_to_lazy(&batch)))
            .collect();

        let mut image_topic_data_table = pipeline.take_image_topic_data_table();
        if !image_topic_data_table.is_empty() {
            let mut image_topic_shapes =
                infer_image_topic_shapes_from_payload(&image_topic_data_table);
            let camera_info_shapes = compute_image_shapes(&dataset, &image_topic_data_table);
            for (topic, shape) in camera_info_shapes {
                image_topic_shapes.entry(topic).or_insert(shape);
            }
            for (topic, frames) in image_topic_data_table.iter_mut() {
                if let Some(shape) = image_topic_shapes.get(topic) {
                    for frame in frames.iter_mut() {
                        frame.set_shape(*shape);
                    }
                }
            }
            if !image_topic_shapes.is_empty() {
                context.set_image_topic_shapes(image_topic_shapes);
            }
            context.set_image_data(image_topic_data_table);
        }
        context.set_dataset(dataset);

        let depth_topic_data_table = pipeline.take_depth_topic_data_table();
        if !depth_topic_data_table.is_empty() {
            context.set_depth_data(depth_topic_data_table);
        }

        let pointcloud_topic_data_table = pipeline.take_pointcloud_topic_data_table();
        if !pointcloud_topic_data_table.is_empty() {
            context.set_pointcloud_data(pointcloud_topic_data_table);
        }

        let temp_path = temp_dir.keep();
        let output_dir = Utf8PathBuf::try_from(temp_path).map_err(|e| {
            StageError::invalid(format!(
                "temporary directory path is not valid UTF-8: {:?}",
                e
            ))
        })?;
        context.set_output_dir(output_dir);
        context.set_rosbag_path(rosbag_path.clone());
        context.set_topic_message_type_map(topic_message_type_map);

        // Load airoa metadata (meta.json) if required
        // Supports both V1.3 and V2.0 formats, converting V1.3 to V2.0 automatically.
        if self.require_metadata {
            let parent = rosbag_path
                .parent()
                .ok_or_else(|| StageError::invalid("rosbag_path must have a parent directory"))?;
            let meta_path = parent.join("meta.json");
            let meta_content = fs::read_to_string(meta_path.as_std_path()).map_err(|e| {
                StageError::io(
                    format!(
                        "failed to read meta.json at {} - airoa metadata is required",
                        meta_path
                    ),
                    e,
                )
            })?;
            let metadata = parse_metadata(&meta_content)?;
            context.set_airoa_metadata(metadata);
        }

        Ok(context)
    }
}

fn read_mcap(path: &str) -> std::io::Result<Mmap> {
    let fd = fs::File::open(path)?;
    // SAFETY: The file is opened read-only and we don't modify it
    unsafe { Mmap::map(&fd) }
}

/// Extract the message type from a full ROS2 type name
/// e.g., "geometry_msgs/msg/Vector3" -> "Vector3"
pub(crate) fn extract_message_type(full_type_name: &str) -> &str {
    full_type_name.rsplit('/').next().unwrap_or(full_type_name)
}

use crate::schema::metadata::MetadataV2_0;

/// Read only the metadata from a rosbag without full ingestion.
///
/// This function reads the `meta.json` file from the parent directory of the
/// rosbag path. It is useful for extracting the UUID before deciding whether
/// to perform expensive full ingestion.
///
/// Supports both V1.3 and V2.0 formats, converting V1.3 to V2.0 automatically.
///
/// # Arguments
///
/// * `rosbag_path` - Path to the `.mcap` file. The `meta.json` is expected
///   to be in the parent directory.
///
/// # Returns
///
/// The parsed metadata on success (always returns V2.0 format).
///
/// # Errors
///
/// Returns a `StageError` if:
/// - The rosbag path has no parent directory
/// - The `meta.json` file cannot be read
/// - The JSON cannot be parsed as valid metadata
pub fn read_metadata(rosbag_path: &Utf8PathBuf) -> Result<MetadataV2_0, StageError> {
    let parent = rosbag_path
        .parent()
        .ok_or_else(|| StageError::invalid("rosbag_path must have a parent directory"))?;
    let meta_path = parent.join("meta.json");
    let meta_content = fs::read_to_string(meta_path.as_std_path())
        .map_err(|e| StageError::io(format!("failed to read meta.json at {}", meta_path), e))?;
    parse_metadata_as_v2_0(&meta_content)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::testutil::{McapGenerator, McapGeneratorConfig};
    use camino::Utf8PathBuf;
    use tempfile::tempdir;

    /// Normal case: ingesting a valid MCAP file sets dataset and topic_message_type_map in Context
    #[test]
    fn test_ingest_valid_mcap_sets_dataset_and_topic_map() {
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

        let mut ingestor = Rosbag2Ingestor::new(Rosbag2IngestorConfig::without_metadata());

        let mut context = Context::default();
        context.set_rosbag_path(mcap_path);

        let result = ingestor.run(context);

        assert!(result.is_ok());
        let ctx = result.unwrap();
        assert!(ctx.dataset.is_some(), "dataset should be set");
        assert!(
            ctx.topic_message_type_map.is_some(),
            "topic_message_type_map should be set"
        );

        let dataset = ctx.dataset.unwrap();
        let df = dataset["/joint_states"].clone().collect().unwrap();
        assert!(df.column("publish_timestamp_ns").is_ok());
    }

    /// Edge case: ingesting an MCAP file with minimal frames (1 frame)
    #[test]
    fn test_ingest_minimal_mcap_succeeds() {
        let dir = tempdir().unwrap();
        let output_dir = Utf8PathBuf::try_from(dir.path().to_path_buf()).unwrap();

        let config = McapGeneratorConfig {
            num_frames: 1,
            fps: 30,
            generate_images: false,
            generate_tf: false,
            ..Default::default()
        };
        let generator = McapGenerator::new(config);
        let mcap_path = generator.generate(&output_dir).unwrap();

        let mut ingestor = Rosbag2Ingestor::new(Rosbag2IngestorConfig::without_metadata());

        let mut context = Context::default();
        context.set_rosbag_path(mcap_path);

        let result = ingestor.run(context);

        assert!(result.is_ok());
        let ctx = result.unwrap();
        let dataset = ctx.dataset.unwrap();
        assert!(dataset.contains_key("/joint_states"));
    }

    #[test]
    fn test_ingest_sets_publish_timestamp_from_mcap_publish_time() {
        let dir = tempdir().unwrap();
        let output_dir = Utf8PathBuf::try_from(dir.path().to_path_buf()).unwrap();

        let config = McapGeneratorConfig {
            num_frames: 3,
            fps: 30,
            generate_images: false,
            generate_tf: false,
            publish_time_offset_ns: 42,
            ..Default::default()
        };
        let generator = McapGenerator::new(config);
        let mcap_path = generator.generate(&output_dir).unwrap();

        let mut ingestor = Rosbag2Ingestor::new(Rosbag2IngestorConfig::without_metadata());

        let mut context = Context::default();
        context.set_rosbag_path(mcap_path);

        let ctx = ingestor.run(context).unwrap();
        let dataset = ctx.dataset.unwrap();
        let df = dataset["/joint_states"].clone().collect().unwrap();

        let timestamps = df
            .column("timestamp_ns")
            .unwrap()
            .u64()
            .unwrap()
            .into_no_null_iter()
            .collect::<Vec<_>>();
        let publish_timestamps = df
            .column("publish_timestamp_ns")
            .unwrap()
            .u64()
            .unwrap()
            .into_iter()
            .collect::<Vec<_>>();

        assert_eq!(publish_timestamps.len(), timestamps.len());
        for (timestamp, publish_timestamp) in timestamps.into_iter().zip(publish_timestamps) {
            assert_eq!(publish_timestamp, Some(timestamp + 42));
        }
    }

    /// Error case: returns MissingData error when rosbag_path is not set
    #[test]
    fn test_ingest_missing_rosbag_path_returns_missing_data_error() {
        let mut ingestor = Rosbag2Ingestor::new(Rosbag2IngestorConfig::without_metadata());

        let context = Context::default();

        let result = ingestor.run(context);

        assert!(result.is_err());
        let err = result.err().unwrap();
        assert!(matches!(err, StageError::MissingData(_)));
    }
}
