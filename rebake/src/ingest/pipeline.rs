use std::collections::HashMap;
use std::sync::Arc;

use arrow::array::ArrayBuilder;
use arrow::datatypes::Schema;
use arrow_array::RecordBatch;

use crate::arrow::arrow_schema_builder::ArrowSchemaBuilder;
use crate::arrow::utils::create_builtin_message_definition_table;
use crate::common::{DepthFrame, ImageFrame, PointCloudFrame};
use crate::core::error::{ResultExt, StageError, StageResult};
use crate::ingest::common::{
    get_raw_image_extension, is_compressed_depth_topic, is_raw_image_type, normalize_ros_format,
};
use crate::ingest::ros_msg_arrow_parser::{
    RosMsgArrowParser, create_array_builder, decode_compressed_image, decode_image,
    decode_point_cloud2,
};
use crate::ros::msg_deserializer::RosGeneration;
use crate::ros::schema_type_parser::parse_schema_text_to_message_definition_table;
use crate::ros::types::MessageDefinition;

/// A processing pipeline that converts ROS messages into Parquet and other file formats.
///
/// The pipeline takes ROS message definitions and data, processes them, and saves the
/// output to a specified directory. It handles standard topics by converting them to
/// Parquet files. For specialized topics like `CompressedImage` and `CompressedDepth`,
/// it captures the binary payload, records a sequential index inside the dataset, and retains
/// the decoded bytes in memory so that downstream stages can persist them.
///
/// ## Topic-Level Schema Isolation
///
/// Each topic maintains its own `MessageDefinition` table, ensuring that schema definitions
/// are isolated per topic. This prevents collisions when the same ROS type name (e.g.,
/// `CompressedImage`) appears in different contexts with different layouts — for example,
/// as a canonical nested type within `MarkerArray` (`uint8[] data`) and as an externalized
/// top-level topic (`uint32 index`).
pub struct Pipeline {
    /// The generation of the ROS protocol.
    ros_generation: RosGeneration,
    /// Maps topic names to their corresponding ROS message type names.
    topic_name_type_name_table: HashMap<String, String>,
    /// Maps topic names to their own `MessageDefinition` table.
    /// Each topic gets an independent table to avoid collisions when the same short type name
    /// (e.g., `CompressedImage`) has different field layouts across topics.
    topic_message_definition_table: HashMap<String, HashMap<String, MessageDefinition>>,
    /// Maps topic names to their corresponding Arrow `Schema`.
    topic_name_schema_table: HashMap<String, Arc<Schema>>,
    /// Maps topic names to a list of `ArrayBuilder`s for constructing Arrow arrays.
    topic_name_array_builders_table: HashMap<String, Vec<Box<dyn ArrayBuilder>>>,
    /// Maps topic names to their corresponding image data.
    image_topic_data_table: Option<HashMap<String, Vec<ImageFrame>>>,
    depth_topic_data_table: Option<HashMap<String, Vec<DepthFrame>>>,
    pointcloud_topic_data_table: Option<HashMap<String, Vec<PointCloudFrame>>>,
    /// A counter for generating unique file names for image/depth topics.
    image_topic_id_table: HashMap<String, usize>,
    pointcloud_topic_id_table: HashMap<String, usize>,
}

impl Pipeline {
    pub fn new(ros_generation: RosGeneration) -> Self {
        Self {
            ros_generation,
            topic_name_type_name_table: HashMap::new(),
            topic_message_definition_table: HashMap::new(),
            topic_name_schema_table: HashMap::new(),
            topic_name_array_builders_table: HashMap::new(),
            image_topic_data_table: None,
            depth_topic_data_table: None,
            pointcloud_topic_data_table: None,
            image_topic_id_table: HashMap::new(),
            pointcloud_topic_id_table: HashMap::new(),
        }
    }

    /// Takes the buffered image payload table out of the pipeline.
    pub fn take_image_topic_data_table(&mut self) -> HashMap<String, Vec<ImageFrame>> {
        self.image_topic_data_table.take().unwrap_or_default()
    }

    /// Takes the buffered depth payload table out of the pipeline.
    pub fn take_depth_topic_data_table(&mut self) -> HashMap<String, Vec<DepthFrame>> {
        self.depth_topic_data_table.take().unwrap_or_default()
    }

    /// Takes the buffered point cloud payload table out of the pipeline.
    pub fn take_pointcloud_topic_data_table(&mut self) -> HashMap<String, Vec<PointCloudFrame>> {
        self.pointcloud_topic_data_table.take().unwrap_or_default()
    }

    /// Adds a new message definition to the pipeline and prepares for data processing.
    ///
    /// This method performs a multi-step conversion:
    /// 1. **Parse Text:** The raw message definition text is parsed into a topic-specific
    ///    `MessageDefinition` table (seeded with builtin definitions for `time` and `duration`).
    /// 2. **Build Schema:** The topic-specific table is used to build an Arrow `Schema`.
    /// 3. **Create Builders:** `ArrayBuilder`s are created based on the Arrow `Schema` to prepare for
    ///    populating data.
    ///
    /// Each topic receives its own `MessageDefinition` table so that type name collisions
    /// across topics (e.g., canonical vs externalized `CompressedImage`) cannot occur.
    ///
    /// This setup is performed once per topic.
    ///
    /// # Errors
    ///
    /// Returns `StageError::InvalidData` if a field's data type is unsupported for Arrow conversion.
    pub fn add_message_definition(
        &mut self,
        topic_name: &str,
        type_name: &str,
        message_definition_text: &str,
    ) -> Result<(), StageError> {
        if self.topic_name_type_name_table.contains_key(topic_name) {
            return Ok(());
        }

        self.topic_name_type_name_table
            .insert(topic_name.to_string(), type_name.to_string());

        // 1. Message definition text -> topic-specific `MessageDefinition` table
        let mut topic_defs = create_builtin_message_definition_table();
        parse_schema_text_to_message_definition_table(
            type_name,
            message_definition_text,
            &mut topic_defs,
        )?;

        // 2. `MessageDefinition`s -> `arrow::datatypes::Schema`
        let arrow_schema_builder = ArrowSchemaBuilder::new(&topic_defs);
        let schema = arrow_schema_builder.build(type_name)?;
        self.topic_name_schema_table
            .insert(topic_name.to_string(), schema);

        // 3. `arrow::datatypes::Schema` -> `arrow::array::ArrayBuilder`s
        #[allow(clippy::unwrap_used)]
        let schema = self.topic_name_schema_table.get(topic_name).unwrap();
        let array_builders: Result<Vec<_>, _> = schema
            .fields()
            .iter()
            .map(|field| create_array_builder(field, ""))
            .collect();
        self.topic_name_array_builders_table
            .insert(topic_name.to_string(), array_builders?);

        // 4. Store topic-specific definition table for use during message parsing
        self.topic_message_definition_table
            .insert(topic_name.to_string(), topic_defs);

        Ok(())
    }

    /// Processes a single ROS message's data and appends it to the appropriate builders.
    ///
    /// # Contract
    ///
    /// The `topic_name` must be registered via `add_message_definition()` before calling.
    ///
    /// # Errors
    ///
    /// Returns `StageError::MissingData` if the message type is not defined.
    /// Returns `StageError::InvalidData` if string fields contain invalid UTF-8.
    pub fn add_message_data(
        &mut self,
        topic_name: &str,
        data: &[u8],
        timestamp_ns: u64,
        publish_timestamp_ns: Option<u64>,
    ) -> StageResult<()> {
        // CONTRACT: topic_name was registered via add_message_definition() by the caller
        #[allow(clippy::expect_used)]
        let type_name = self
            .topic_name_type_name_table
            .get(topic_name)
            .expect("topic_name must be registered via add_message_definition()");
        // CONTRACT: topic_name was registered via add_message_definition() by the caller
        #[allow(clippy::expect_used)]
        let array_builders = self
            .topic_name_array_builders_table
            .get_mut(topic_name)
            .expect("topic_name must be registered via add_message_definition()");
        // CONTRACT: topic_name was registered via add_message_definition() by the caller
        #[allow(clippy::expect_used)]
        let topic_defs = self
            .topic_message_definition_table
            .get(topic_name)
            .expect("topic_name must be registered via add_message_definition()");

        // Create a parser using the topic-specific definition table to ensure
        // the parser sees the correct field layout for this topic.
        let mut ros_msg_arrow_parser = RosMsgArrowParser::new(
            self.ros_generation,
            topic_defs,
            type_name,
            data,
            timestamp_ns,
            publish_timestamp_ns,
        );
        ros_msg_arrow_parser.parse(array_builders)
    }

    /// Initializes the image counter for a new image-based topic.
    pub fn add_image_info(&mut self, topic_name: &str) {
        self.image_topic_id_table.insert(topic_name.to_string(), 0);
    }

    /// Initializes the point cloud counter for a new PointCloud2 topic.
    pub fn add_pointcloud_info(&mut self, topic_name: &str) {
        self.pointcloud_topic_id_table
            .insert(topic_name.to_string(), 0);
    }

    /// Processes a compressed image or depth message and retains the binary payload in memory.
    ///
    /// The decoded bytes are stored in the pipeline so that downstream stages can decide how and
    /// where to persist them. The remaining message fields are appended to the Arrow builders
    /// together with a reference index.
    ///
    /// # Errors
    ///
    /// Returns `StageError::InvalidData` if the image encoding is unsupported, or
    /// `StageError::External` if JPEG encoding fails.
    pub fn add_image_data(
        &mut self,
        topic_name: &str,
        data: &[u8],
        timestamp_ns: u64,
        publish_timestamp_ns: Option<u64>,
    ) -> Result<(), StageError> {
        // CONTRACT: topic_name registered via add_message_definition() before this call
        #[allow(clippy::unwrap_used)]
        let type_name = self.topic_name_type_name_table.get(topic_name).unwrap();
        let is_raw = is_raw_image_type(type_name);

        // CONTRACT: topic_name registered via add_message_definition() before this call
        #[allow(clippy::unwrap_used)]
        let id = *self.image_topic_id_table.get(topic_name).unwrap();

        // Decode the message. This populates the builders with metadata (e.g., header)
        // and returns the raw binary data (e.g., the JPEG payload).
        // CONTRACT: topic_name registered via add_message_definition() before this call
        #[allow(clippy::unwrap_used)]
        let array_builders = self
            .topic_name_array_builders_table
            .get_mut(topic_name)
            .unwrap();

        let is_compressed_depth = !is_raw && is_compressed_depth_topic(topic_name);

        if is_raw {
            let data = decode_image(
                array_builders,
                data,
                timestamp_ns,
                publish_timestamp_ns,
                id as u32,
                self.ros_generation,
                topic_name,
            )?;
            // Raw images are JPEG-encoded by decode_image.
            let entry = self
                .image_topic_data_table
                .get_or_insert_with(HashMap::new)
                .entry(topic_name.to_string())
                .or_default();
            entry.push(ImageFrame::new(id as u32, get_raw_image_extension(), data));
        } else {
            let (data, ros_format) = decode_compressed_image(
                array_builders,
                data,
                timestamp_ns,
                publish_timestamp_ns,
                id as u32,
                self.ros_generation,
            )?;

            if is_compressed_depth {
                let entry = self
                    .depth_topic_data_table
                    .get_or_insert_with(HashMap::new)
                    .entry(topic_name.to_string())
                    .or_default();
                let mut frame = DepthFrame::new(id as u32, "bin", data);
                frame.set_ros_format(ros_format);
                entry.push(frame);
            } else {
                let extension = normalize_ros_format(&ros_format);
                if is_visual_image_extension(&extension) {
                    let entry = self
                        .image_topic_data_table
                        .get_or_insert_with(HashMap::new)
                        .entry(topic_name.to_string())
                        .or_default();
                    entry.push(ImageFrame::new(id as u32, extension, data));
                } else {
                    let entry = self
                        .depth_topic_data_table
                        .get_or_insert_with(HashMap::new)
                        .entry(topic_name.to_string())
                        .or_default();
                    let mut frame = DepthFrame::new(id as u32, extension, data);
                    frame.set_ros_format(ros_format);
                    entry.push(frame);
                }
            }
        }

        self.image_topic_id_table
            .insert(topic_name.to_string(), id + 1);
        Ok(())
    }

    /// Processes a PointCloud2 message, storing its binary payload out-of-band.
    ///
    /// # Errors
    ///
    /// Returns `StageError::InvalidData` if the message contains invalid UTF-8 strings.
    pub fn add_pointcloud_data(
        &mut self,
        topic_name: &str,
        data: &[u8],
        timestamp_ns: u64,
        publish_timestamp_ns: Option<u64>,
    ) -> StageResult<()> {
        // CONTRACT: topic_name registered via add_message_definition() before this call
        #[allow(clippy::unwrap_used)]
        let id = *self.pointcloud_topic_id_table.get(topic_name).unwrap();
        // CONTRACT: topic_name registered via add_message_definition() before this call
        #[allow(clippy::unwrap_used)]
        let array_builders = self
            .topic_name_array_builders_table
            .get_mut(topic_name)
            .unwrap();
        let data = decode_point_cloud2(
            array_builders,
            data,
            timestamp_ns,
            publish_timestamp_ns,
            id as u32,
            self.ros_generation,
        )?;

        let entry = self
            .pointcloud_topic_data_table
            .get_or_insert_with(HashMap::new)
            .entry(topic_name.to_string())
            .or_default();
        entry.push(PointCloudFrame::new(id as u32, "bin", data));

        self.pointcloud_topic_id_table
            .insert(topic_name.to_string(), id + 1);
        Ok(())
    }

    /// Finalizes the data processing and builds a `RecordBatch` for each topic.
    ///
    /// This method consumes the `ArrayBuilder`s, converting them into final `Array`s
    /// and packaging them into `RecordBatch`es. Each `RecordBatch` corresponds to a
    /// single output Parquet file.
    ///
    /// # Errors
    ///
    /// Returns `StageError::External` if constructing a `RecordBatch` fails due to
    /// mismatched schema and array lengths.
    pub fn finish(&mut self) -> StageResult<HashMap<String, RecordBatch>> {
        let mut record_batches = HashMap::new();
        let topic_names = self
            .topic_name_array_builders_table
            .keys()
            .cloned()
            .collect::<Vec<_>>();

        for topic_name in topic_names {
            // INVARIANT: topic_name was just retrieved from topic_name_array_builders_table
            // so it must exist in topic_name_schema_table (both populated by add_message_definition)
            #[allow(clippy::expect_used)]
            let schema = self
                .topic_name_schema_table
                .get(&topic_name)
                .expect("topic_name must exist in schema table - internal invariant");
            // INVARIANT: topic_name was retrieved from topic_name_array_builders_table.keys()
            #[allow(clippy::expect_used)]
            let mut array_builders = self
                .topic_name_array_builders_table
                .remove(&topic_name)
                .expect("topic_name must exist in array builders table - internal invariant");

            let built_arrays = array_builders
                .iter_mut()
                .map(|builder| builder.finish())
                .collect::<Vec<_>>();
            let record_batch = RecordBatch::try_new(schema.clone(), built_arrays).with_context(
                format!("failed to create RecordBatch for topic '{topic_name}'"),
            )?;

            record_batches.insert(topic_name, record_batch);
        }

        Ok(record_batches)
    }
}

/// Returns true for file extensions that should be treated as regular images.
fn is_visual_image_extension(extension: &str) -> bool {
    matches!(extension, "jpg" | "png" | "webp")
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use arrow::datatypes::DataType;

    // ROS2 CompressedImage schema definition (canonical, with `uint8[] data`).
    const COMPRESSED_IMAGE_CANONICAL: &str = "\
Header header
string format
uint8[] data

================================================================================
MSG: std_msgs/Header
time stamp
string frame_id
";

    // A minimal type that nests CompressedImage as a field (simulating how
    // `visualization_msgs/Marker` contains `sensor_msgs/CompressedImage` via its
    // `texture` field in ROS2).
    const MARKER_WITH_NESTED_COMPRESSED_IMAGE: &str = "\
Header header
string ns
int32 id
int32 type
int32 action
sensor_msgs/CompressedImage texture

================================================================================
MSG: std_msgs/Header
time stamp
string frame_id
================================================================================
MSG: sensor_msgs/CompressedImage
Header header
string format
uint8[] data
";

    // Externalized CompressedImage schema (with `uint32 index` replacing `uint8[] data`).
    // This is what the ingestor produces after calling
    // `message_definition.replace("uint8[] data", "uint32 index")`.
    const COMPRESSED_IMAGE_EXTERNALIZED: &str = "\
Header header
string format
uint32 index

================================================================================
MSG: std_msgs/Header
time stamp
string frame_id
";

    // Regression test: when a canonical `CompressedImage` (with `uint8[] data`) is
    // registered first as a nested type inside another topic, and an externalized
    // `CompressedImage` (with `uint32 index`) is registered second for a camera topic,
    // the externalized topic must use the correct schema with `UInt32` for the index field.
    //
    // Before the topic-level schema isolation fix, the shared `message_definition_table`
    // used first-writer-wins semantics with short type name keys, causing the externalized
    // definition to be silently dropped. This led to a `schema mismatch` panic when
    // `decode_compressed_image_ros2` tried to downcast `array_builders[3]` to `UInt32Builder`.
    // Tests that topic-local schemas survive canonical and externalized collisions.
    #[test]
    fn externalized_topic_works_when_canonical_registered_first() {
        let mut pipeline = Pipeline::new(RosGeneration::ROS2);

        // Step 1: Register a topic that nests canonical CompressedImage (uint8[] data)
        pipeline
            .add_message_definition(
                "/markers",
                "visualization_msgs/MarkerLike",
                MARKER_WITH_NESTED_COMPRESSED_IMAGE,
            )
            .expect("registering marker topic should succeed");

        // Step 2: Register a camera topic with externalized CompressedImage (uint32 index)
        pipeline
            .add_message_definition(
                "/camera/compressed",
                "sensor_msgs/CompressedImage",
                COMPRESSED_IMAGE_EXTERNALIZED,
            )
            .expect("registering externalized compressed image topic should succeed");

        // Step 3: Verify the externalized topic has the correct schema
        let schema = pipeline
            .topic_name_schema_table
            .get("/camera/compressed")
            .expect("schema must exist for camera topic");

        // Schema fields: [timestamp_ns, publish_timestamp_ns, header, format, index]
        assert_eq!(schema.fields().len(), 5, "expected 5 fields in schema");
        assert_eq!(schema.field(0).name(), "timestamp_ns");
        assert_eq!(schema.field(1).name(), "publish_timestamp_ns");
        assert_eq!(schema.field(2).name(), "header");
        assert_eq!(schema.field(3).name(), "format");
        assert_eq!(schema.field(4).name(), "index");

        // The critical assertion: index must be UInt32, not List<UInt8>
        assert_eq!(
            *schema.field(4).data_type(),
            DataType::UInt32,
            "index field must be UInt32 (externalized), not List<UInt8> (canonical)"
        );

        // Step 4: Verify the marker topic retains the canonical schema with uint8[] data
        let marker_schema = pipeline
            .topic_name_schema_table
            .get("/markers")
            .expect("schema must exist for marker topic");

        // Find the texture field (nested CompressedImage)
        let texture_field = marker_schema.field_with_name("texture").unwrap();
        match texture_field.data_type() {
            DataType::Struct(fields) => {
                // CompressedImage fields: [header, format, data]
                let data_field = fields
                    .iter()
                    .find(|f| f.name() == "data")
                    .expect("data field must exist in canonical CompressedImage");
                // data must be List<UInt8> (canonical), not UInt32 (externalized)
                assert!(
                    matches!(data_field.data_type(), DataType::List(_)),
                    "data field in canonical CompressedImage must be List<UInt8>, got {:?}",
                    data_field.data_type()
                );
            }
            other => panic!("expected Struct for texture field, got {other:?}"),
        }
    }

    // Verify that two topics sharing the same type name but with different schemas
    // (one canonical, one externalized) produce independent RecordBatches via `finish()`.
    // Tests that `finish()` keeps canonical and externalized schemas separate.
    #[test]
    fn finish_produces_correct_schemas_for_both_canonical_and_externalized() {
        let mut pipeline = Pipeline::new(RosGeneration::ROS2);

        // Register canonical CompressedImage topic (e.g., a topic that just logs images
        // without externalization — this tests that canonical also works)
        pipeline
            .add_message_definition(
                "/topic_canonical",
                "sensor_msgs/CompressedImage",
                COMPRESSED_IMAGE_CANONICAL,
            )
            .expect("registering canonical topic should succeed");

        // Register externalized CompressedImage topic
        pipeline
            .add_message_definition(
                "/topic_externalized",
                "sensor_msgs/CompressedImage",
                COMPRESSED_IMAGE_EXTERNALIZED,
            )
            .expect("registering externalized topic should succeed");

        let record_batches = pipeline.finish().unwrap();

        // Both topics should produce RecordBatches
        assert!(record_batches.contains_key("/topic_canonical"));
        assert!(record_batches.contains_key("/topic_externalized"));

        // Canonical: field[4] = "data" (List<UInt8>)
        let canonical_schema = record_batches["/topic_canonical"].schema();
        assert_eq!(canonical_schema.field(4).name(), "data");
        assert!(
            matches!(canonical_schema.field(4).data_type(), DataType::List(_)),
            "canonical data field must be List type"
        );

        // Externalized: field[4] = "index" (UInt32)
        let externalized_schema = record_batches["/topic_externalized"].schema();
        assert_eq!(externalized_schema.field(4).name(), "index");
        assert_eq!(
            *externalized_schema.field(4).data_type(),
            DataType::UInt32,
            "externalized index field must be UInt32"
        );
    }

    // Tests that `compressedDepth` topics still route to depth data when the transport is PNG.
    #[test]
    fn compressed_depth_topics_route_to_depth_even_when_transport_is_png() {
        assert!(is_compressed_depth_topic("/camera/depth/compressedDepth"));
        assert!(!is_compressed_depth_topic("/camera/color/compressed"));
        assert!(is_visual_image_extension("png"));
        assert!(!is_visual_image_extension("bin"));
    }
}
