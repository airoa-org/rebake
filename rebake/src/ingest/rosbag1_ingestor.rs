use std::collections::{HashMap, HashSet};
use std::fs;

use crate::core::error::OptionExt;
use crate::core::{
    Context, Stage, StageConfig, StageError, conversion::record_batch_to_lazy,
    stage::PipelineInputKind,
};
use crate::ingest::common::{
    compute_image_shapes, infer_image_topic_shapes_from_payload, is_compressed_depth_topic,
    is_compressed_image_topic, is_point_cloud2_type,
};
use crate::ingest::pipeline::Pipeline;
use crate::ros::msg_deserializer::RosGeneration;
use crate::schema::metadata::parse_metadata;
use camino::Utf8PathBuf;
use rosbag::{ChunkRecord, MessageRecord, RosBag};
use serde::{Deserialize, Serialize};
use tempfile::tempdir;

/// Configuration for the `Rosbag1Ingestor` stage.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Rosbag1IngestorConfig {
    /// Whether to require meta.json (airoa metadata) to be present.
    /// Defaults to true. Set to false for testing or non-airoa rosbags.
    #[serde(default = "default_require_metadata")]
    pub require_metadata: bool,
}

fn default_require_metadata() -> bool {
    true
}

impl Default for Rosbag1IngestorConfig {
    fn default() -> Self {
        Self {
            require_metadata: true,
        }
    }
}

impl Rosbag1IngestorConfig {
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

#[typetag::serde(name = "Rosbag1IngestorConfig")]
impl StageConfig for Rosbag1IngestorConfig {
    fn build(&self) -> Box<dyn Stage> {
        Box::new(Rosbag1Ingestor::new(self.clone()))
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
///   `timestamp_ns` column (u64, bag record time) and `publish_timestamp_ns` column
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
/// - [`StageError::Io`]: rosbag file read failure, meta.json read failure
/// - [`StageError::InvalidData`]: chunk record or message record parse failure
pub struct Rosbag1Ingestor {
    require_metadata: bool,
}

impl Rosbag1Ingestor {
    pub fn new(config: Rosbag1IngestorConfig) -> Self {
        Self {
            require_metadata: config.require_metadata,
        }
    }
}

impl Stage for Rosbag1Ingestor {
    fn name(&self) -> &'static str {
        "rosbag1_ingestor"
    }

    fn run(&mut self, mut context: Context) -> Result<Context, StageError> {
        let rosbag_path = context
            .rosbag_path()
            .cloned()
            .or_missing("rosbag_path in context")?;

        let bag = RosBag::new(rosbag_path.as_str())
            .map_err(|e| StageError::io(format!("failed to open rosbag: {}", rosbag_path), e))?;

        let mut pipeline = Pipeline::new(RosGeneration::ROS1);

        let mut connections = HashMap::new();
        let mut compressed_image_and_depth_topic_ids = HashSet::new();
        let mut pointcloud_topic_ids = HashSet::new();
        let mut topic_message_type_map = HashMap::new();

        for record in bag.chunk_records() {
            let record =
                record.map_err(|e| StageError::invalid_with("failed to read chunk record", e))?;

            if let ChunkRecord::Chunk(chunk) = record {
                for message_record in chunk.messages() {
                    let message_record = message_record.map_err(|e| {
                        StageError::invalid_with("failed to read message record", e)
                    })?;

                    match message_record {
                        MessageRecord::Connection(connection) => {
                            connections.insert(connection.id, connection.topic.to_string());
                            let mut message_definition = connection.message_definition.to_string();

                            // For compressed image/depth topics, modify the message definition
                            // to replace the `data` field with an index field. This allows the
                            // Parquet file to reference artifacts persisted by downstream stages.
                            if is_compressed_image_topic(connection.topic)
                                || is_compressed_depth_topic(connection.topic)
                            {
                                compressed_image_and_depth_topic_ids.insert(connection.id);
                                message_definition =
                                    message_definition.replace("uint8[] data", "uint32 index");
                                pipeline.add_image_info(connection.topic);
                            } else if is_point_cloud2_type(connection.tp) {
                                pointcloud_topic_ids.insert(connection.id);
                                message_definition =
                                    message_definition.replace("uint8[] data", "uint32 index");
                                pipeline.add_pointcloud_info(connection.topic);
                            }

                            pipeline.add_message_definition(
                                connection.topic,
                                connection.tp,
                                &message_definition,
                            )?;
                            topic_message_type_map
                                .insert(connection.topic.to_string(), connection.tp.to_string());
                        }
                        MessageRecord::MessageData(message_data) => {
                            let topic_name =
                                connections.get(&message_data.conn_id).ok_or_else(|| {
                                    StageError::invalid(format!(
                                        "unknown connection id: {}",
                                        message_data.conn_id
                                    ))
                                })?;
                            if pointcloud_topic_ids.contains(&message_data.conn_id) {
                                pipeline.add_pointcloud_data(
                                    topic_name,
                                    message_data.data,
                                    message_data.time,
                                    None,
                                )?;
                            } else if compressed_image_and_depth_topic_ids
                                .contains(&message_data.conn_id)
                            {
                                pipeline.add_image_data(
                                    topic_name,
                                    message_data.data,
                                    message_data.time,
                                    None,
                                )?;
                            } else {
                                pipeline.add_message_data(
                                    topic_name,
                                    message_data.data,
                                    message_data.time,
                                    None,
                                )?;
                            }
                        }
                    }
                }
            }
        }

        let record_batches = pipeline.finish()?;
        let temp_dir = tempdir().map_err(|e| {
            StageError::io(
                "failed to create temporary directory for ROS1 bag extraction",
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

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use camino::Utf8PathBuf;
    use tempfile::tempdir;

    use super::*;

    #[test]
    #[ignore = "requires external fixture file"]
    fn test_convert_rosbag_to_airoa_format() {
        let rosbag_path = crate::test_utils::rosbag_path().clone();

        let mut ingestor = Rosbag1IngestorConfig::without_metadata().build();
        let temp_dir = tempdir().unwrap();
        let output_dir = Utf8PathBuf::from_path_buf(temp_dir.path().to_path_buf()).unwrap();

        let mut context = Context::default();
        context.set_rosbag_path(rosbag_path);
        context.set_output_dir(output_dir);
        let mut context = ingestor.run(context).unwrap();

        let image_data = context.image_data.as_ref().unwrap();
        let frames = image_data
            .get("/hsrb/hand_camera/image_raw/compressed")
            .unwrap();
        let shape = frames.first().unwrap().shape.unwrap();
        assert!(shape.height > 0 && shape.width > 0);

        let dataset = context.dataset.take().unwrap();

        assert!(!dataset.is_empty());
        for (_topic, frame) in dataset {
            let _df = frame.collect().unwrap();
        }
    }
}
