//! MCAP file generator for testing.
//!
//! This module generates minimal MCAP files containing ROS2 messages
//! for testing the rebake pipeline without requiring actual robot data.
//!
//! # Supported Message Types
//!
//! - `sensor_msgs/msg/JointState` - Joint positions, velocities, efforts
//! - `sensor_msgs/msg/Image` - Raw RGB images
//! - `tf2_msgs/msg/TFMessage` - Transform frames
//!
//! # Example
//!
//! ```ignore
//! use rebake::testutil::{McapGenerator, McapGeneratorConfig};
//!
//! let config = McapGeneratorConfig::default();
//! let generator = McapGenerator::new(config);
//! let mcap_path = generator.generate(&output_dir)?;
//! ```

use std::collections::BTreeMap;
use std::io::{BufWriter, Write};

use camino::{Utf8Path, Utf8PathBuf};
use mcap::records::MessageHeader;
use mcap::write::WriteOptions;

/// Configuration for MCAP test file generation.
#[derive(Clone, Debug)]
pub struct McapGeneratorConfig {
    /// Number of frames/messages to generate per topic.
    pub num_frames: usize,
    /// Simulated frame rate (determines timestamp intervals).
    pub fps: usize,
    /// Joint names for JointState messages.
    pub joint_names: Vec<String>,
    /// Image dimensions (width, height).
    pub image_size: (u32, u32),
    /// Base frame for TF transforms.
    pub base_frame: String,
    /// Child frames for TF transforms.
    pub child_frames: Vec<String>,
    /// Whether to generate image topics.
    pub generate_images: bool,
    /// Whether to generate TF topics.
    pub generate_tf: bool,
    /// Offset applied to MCAP publish_time relative to log_time.
    pub publish_time_offset_ns: u64,
}

impl Default for McapGeneratorConfig {
    fn default() -> Self {
        Self {
            num_frames: 10,
            fps: 30,
            joint_names: vec![
                "joint1".to_string(),
                "joint2".to_string(),
                "joint3".to_string(),
            ],
            image_size: (64, 64),
            base_frame: "base_link".to_string(),
            child_frames: vec!["hand_link".to_string(), "camera_link".to_string()],
            generate_images: true,
            generate_tf: true,
            publish_time_offset_ns: 0,
        }
    }
}

/// MCAP file generator for creating test data.
pub struct McapGenerator {
    config: McapGeneratorConfig,
}

impl McapGenerator {
    /// Creates a new MCAP generator with the given configuration.
    pub fn new(config: McapGeneratorConfig) -> Self {
        Self { config }
    }

    /// Creates a new MCAP generator with default configuration.
    pub fn with_defaults() -> Self {
        Self::new(McapGeneratorConfig::default())
    }

    /// Generates an MCAP file at the specified path.
    ///
    /// Creates a rosbag2-style directory structure with:
    /// - metadata.yaml
    /// - <name>.mcap
    ///
    /// Returns the path to the generated MCAP file.
    pub fn generate(&self, output_dir: &Utf8Path) -> std::io::Result<Utf8PathBuf> {
        std::fs::create_dir_all(output_dir)?;

        let mcap_path = output_dir.join("test_0.mcap");
        let file = std::fs::File::create(&mcap_path)?;
        let writer = BufWriter::new(file);

        let mut mcap_writer = WriteOptions::new()
            .compression(None)
            .create(writer)
            .map_err(std::io::Error::other)?;

        // Add JointState schema and channel
        let joint_state_schema_id = mcap_writer
            .add_schema(
                "sensor_msgs/msg/JointState",
                "ros2msg",
                JOINT_STATE_SCHEMA.as_bytes(),
            )
            .map_err(std::io::Error::other)?;

        let joint_state_channel = mcap_writer
            .add_channel(
                joint_state_schema_id,
                "/joint_states",
                "cdr",
                &BTreeMap::new(),
            )
            .map_err(std::io::Error::other)?;

        // Add Image schema and channel (if enabled)
        let image_channel = if self.config.generate_images {
            let image_schema_id = mcap_writer
                .add_schema("sensor_msgs/msg/Image", "ros2msg", IMAGE_SCHEMA.as_bytes())
                .map_err(std::io::Error::other)?;

            Some(
                mcap_writer
                    .add_channel(
                        image_schema_id,
                        "/camera/image_raw",
                        "cdr",
                        &BTreeMap::new(),
                    )
                    .map_err(std::io::Error::other)?,
            )
        } else {
            None
        };

        // Add TF schema and channels (if enabled)
        // We create both /tf and /tf_static channels using the same schema
        let (tf_channel, tf_static_channel) = if self.config.generate_tf {
            let tf_schema_id = mcap_writer
                .add_schema(
                    "tf2_msgs/msg/TFMessage",
                    "ros2msg",
                    TF_MESSAGE_SCHEMA.as_bytes(),
                )
                .map_err(std::io::Error::other)?;

            let tf_ch = mcap_writer
                .add_channel(tf_schema_id, "/tf", "cdr", &BTreeMap::new())
                .map_err(std::io::Error::other)?;

            let tf_static_ch = mcap_writer
                .add_channel(tf_schema_id, "/tf_static", "cdr", &BTreeMap::new())
                .map_err(std::io::Error::other)?;

            (Some(tf_ch), Some(tf_static_ch))
        } else {
            (None, None)
        };

        // Generate messages
        let interval_ns = 1_000_000_000 / self.config.fps as u64;
        let start_time_ns = 1_000_000_000u64; // Start at 1 second

        // Write tf_static once at the beginning (static transforms don't change)
        if let Some(channel_id) = tf_static_channel {
            let tf_static_data = serialize_tf_message(
                &self.config.base_frame,
                &self.config.child_frames,
                0,
                start_time_ns,
            );
            let publish_time_ns = start_time_ns + self.config.publish_time_offset_ns;

            mcap_writer
                .write_to_known_channel(
                    &MessageHeader {
                        channel_id,
                        sequence: 0,
                        log_time: start_time_ns,
                        publish_time: publish_time_ns,
                    },
                    &tf_static_data,
                )
                .map_err(std::io::Error::other)?;
        }

        for i in 0..self.config.num_frames {
            let timestamp_ns = start_time_ns + (i as u64) * interval_ns;
            let publish_time_ns = timestamp_ns + self.config.publish_time_offset_ns;

            // Generate JointState message
            let joint_state_data = serialize_joint_state(&self.config.joint_names, i, timestamp_ns);

            mcap_writer
                .write_to_known_channel(
                    &MessageHeader {
                        channel_id: joint_state_channel,
                        sequence: i as u32,
                        log_time: timestamp_ns,
                        publish_time: publish_time_ns,
                    },
                    &joint_state_data,
                )
                .map_err(std::io::Error::other)?;

            // Generate Image message (if enabled)
            if let Some(channel_id) = image_channel {
                let image_data = serialize_image(
                    self.config.image_size.0,
                    self.config.image_size.1,
                    i,
                    timestamp_ns,
                );

                mcap_writer
                    .write_to_known_channel(
                        &MessageHeader {
                            channel_id,
                            sequence: i as u32,
                            log_time: timestamp_ns,
                            publish_time: publish_time_ns,
                        },
                        &image_data,
                    )
                    .map_err(std::io::Error::other)?;
            }

            // Generate TF message (if enabled)
            if let Some(channel_id) = tf_channel {
                let tf_data = serialize_tf_message(
                    &self.config.base_frame,
                    &self.config.child_frames,
                    i,
                    timestamp_ns,
                );

                mcap_writer
                    .write_to_known_channel(
                        &MessageHeader {
                            channel_id,
                            sequence: i as u32,
                            log_time: timestamp_ns,
                            publish_time: publish_time_ns,
                        },
                        &tf_data,
                    )
                    .map_err(std::io::Error::other)?;
            }
        }

        mcap_writer.finish().map_err(std::io::Error::other)?;

        // Write metadata.yaml for rosbag2 compatibility
        self.write_metadata_yaml(output_dir, &mcap_path)?;

        Ok(mcap_path)
    }

    fn write_metadata_yaml(
        &self,
        output_dir: &Utf8Path,
        mcap_path: &Utf8Path,
    ) -> std::io::Result<()> {
        let metadata_path = output_dir.join("metadata.yaml");
        let mcap_filename = mcap_path.file_name().unwrap_or("test_0.mcap");

        let mut topics = vec![format!(
            r#"    - topic_metadata:
        name: /joint_states
        type: sensor_msgs/msg/JointState
        serialization_format: cdr
      message_count: {}"#,
            self.config.num_frames
        )];

        if self.config.generate_images {
            topics.push(format!(
                r#"    - topic_metadata:
        name: /camera/image_raw
        type: sensor_msgs/msg/Image
        serialization_format: cdr
      message_count: {}"#,
                self.config.num_frames
            ));
        }

        if self.config.generate_tf {
            topics.push(format!(
                r#"    - topic_metadata:
        name: /tf
        type: tf2_msgs/msg/TFMessage
        serialization_format: cdr
      message_count: {}"#,
                self.config.num_frames
            ));
            topics.push(
                r#"    - topic_metadata:
        name: /tf_static
        type: tf2_msgs/msg/TFMessage
        serialization_format: cdr
      message_count: 1"#
                    .to_string(),
            );
        }

        let total_messages = self.config.num_frames
            * (1 + if self.config.generate_images { 1 } else { 0 }
                + if self.config.generate_tf { 1 } else { 0 })
            + if self.config.generate_tf { 1 } else { 0 }; // +1 for tf_static

        let metadata = format!(
            r#"rosbag2_bagfile_information:
  version: 9
  storage_identifier: mcap
  relative_file_paths:
    - {mcap_filename}
  duration:
    nanoseconds: {duration_ns}
  starting_time:
    nanoseconds_since_epoch: 1000000000
  message_count: {total_messages}
  topics_with_message_count:
{topics}
  compression_format: ""
  compression_mode: ""
"#,
            mcap_filename = mcap_filename,
            duration_ns =
                (self.config.num_frames as u64 - 1) * (1_000_000_000 / self.config.fps as u64),
            total_messages = total_messages,
            topics = topics.join("\n"),
        );

        let mut file = std::fs::File::create(metadata_path)?;
        file.write_all(metadata.as_bytes())?;

        Ok(())
    }
}

// =============================================================================
// Message Schemas
// =============================================================================

/// ROS message schema for sensor_msgs/msg/JointState (ROS2 .msg format, no seq)
const JOINT_STATE_SCHEMA: &str = r#"Header header
string[] name
float64[] position
float64[] velocity
float64[] effort

================================================================================
MSG: std_msgs/Header
time stamp
string frame_id
"#;

/// ROS message schema for sensor_msgs/msg/Image (ROS2 .msg format, no seq)
const IMAGE_SCHEMA: &str = r#"Header header
uint32 height
uint32 width
string encoding
uint8 is_bigendian
uint32 step
uint8[] data

================================================================================
MSG: std_msgs/Header
time stamp
string frame_id
"#;

/// ROS message schema for tf2_msgs/msg/TFMessage (ROS2 .msg format, no seq)
const TF_MESSAGE_SCHEMA: &str = r#"TransformStamped[] transforms

================================================================================
MSG: geometry_msgs/TransformStamped
Header header
string child_frame_id
Transform transform

================================================================================
MSG: std_msgs/Header
time stamp
string frame_id

================================================================================
MSG: geometry_msgs/Transform
Vector3 translation
Quaternion rotation

================================================================================
MSG: geometry_msgs/Vector3
float64 x
float64 y
float64 z

================================================================================
MSG: geometry_msgs/Quaternion
float64 x
float64 y
float64 z
float64 w
"#;

// =============================================================================
// Message Serialization
// =============================================================================

/// Serializes a JointState message to CDR format (ROS2 format without seq).
fn serialize_joint_state(joint_names: &[String], frame_index: usize, timestamp_ns: u64) -> Vec<u8> {
    let mut data = Vec::with_capacity(256);

    // CDR header: encapsulation kind (little endian)
    data.extend_from_slice(&[0x00, 0x01, 0x00, 0x00]);

    // Header.stamp.sec (int32) - ROS2 Header has no seq field
    let sec = (timestamp_ns / 1_000_000_000) as i32;
    data.extend_from_slice(&sec.to_le_bytes());

    // Header.stamp.nanosec (uint32)
    let nanosec = (timestamp_ns % 1_000_000_000) as u32;
    data.extend_from_slice(&nanosec.to_le_bytes());

    // Header.frame_id (string)
    write_cdr_string(&mut data, "base_link");

    // name (string[])
    write_cdr_string_array(&mut data, joint_names);

    // position (float64[])
    let positions: Vec<f64> = joint_names
        .iter()
        .enumerate()
        .map(|(j, _)| 0.1 * (frame_index as f64) * ((j + 1) as f64))
        .collect();
    write_cdr_f64_array(&mut data, &positions);

    // velocity (float64[])
    let velocities: Vec<f64> = vec![0.0; joint_names.len()];
    write_cdr_f64_array(&mut data, &velocities);

    // effort (float64[])
    let efforts: Vec<f64> = vec![0.0; joint_names.len()];
    write_cdr_f64_array(&mut data, &efforts);

    data
}

/// Serializes an Image message to CDR format (ROS2 format without seq).
fn serialize_image(width: u32, height: u32, frame_index: usize, timestamp_ns: u64) -> Vec<u8> {
    let mut data = Vec::with_capacity(256 + (width * height * 3) as usize);

    // CDR header: encapsulation kind (little endian)
    data.extend_from_slice(&[0x00, 0x01, 0x00, 0x00]);

    // Header.stamp.sec (int32) - ROS2 Header has no seq field
    let sec = (timestamp_ns / 1_000_000_000) as i32;
    data.extend_from_slice(&sec.to_le_bytes());

    // Header.stamp.nanosec (uint32)
    let nanosec = (timestamp_ns % 1_000_000_000) as u32;
    data.extend_from_slice(&nanosec.to_le_bytes());

    // Header.frame_id (string)
    write_cdr_string(&mut data, "camera_link");

    // height (uint32)
    align_to(&mut data, 4);
    data.extend_from_slice(&height.to_le_bytes());

    // width (uint32)
    data.extend_from_slice(&width.to_le_bytes());

    // encoding (string)
    write_cdr_string(&mut data, "rgb8");

    // is_bigendian (uint8)
    align_to(&mut data, 1);
    data.push(0);

    // step (uint32)
    align_to(&mut data, 4);
    let step = width * 3;
    data.extend_from_slice(&step.to_le_bytes());

    // data (uint8[])
    let image_data = generate_test_image(width, height, frame_index);
    write_cdr_u8_array(&mut data, &image_data);

    data
}

/// Generates a simple test image with varying colors.
fn generate_test_image(width: u32, height: u32, frame_index: usize) -> Vec<u8> {
    let mut pixels = Vec::with_capacity((width * height * 3) as usize);

    let r_base = ((frame_index * 25) % 256) as u8;
    let g_base = ((frame_index * 10) % 256) as u8;
    let b_base = 128u8;

    for y in 0..height {
        for x in 0..width {
            // Add some spatial variation
            let r = r_base.wrapping_add(x as u8);
            let g = g_base.wrapping_add(y as u8);
            let b = b_base;
            pixels.push(r);
            pixels.push(g);
            pixels.push(b);
        }
    }

    pixels
}

/// Serializes a TFMessage to CDR format (ROS2 format without seq).
fn serialize_tf_message(
    base_frame: &str,
    child_frames: &[String],
    frame_index: usize,
    timestamp_ns: u64,
) -> Vec<u8> {
    let mut data = Vec::with_capacity(512);

    // CDR header: encapsulation kind (little endian)
    data.extend_from_slice(&[0x00, 0x01, 0x00, 0x00]);

    // transforms (TransformStamped[])
    // Sequence length
    align_to(&mut data, 4);
    data.extend_from_slice(&(child_frames.len() as u32).to_le_bytes());

    for (i, child_frame) in child_frames.iter().enumerate() {
        // TransformStamped.header.stamp.sec (int32) - ROS2 Header has no seq field
        align_to(&mut data, 4);
        let sec = (timestamp_ns / 1_000_000_000) as i32;
        data.extend_from_slice(&sec.to_le_bytes());

        // TransformStamped.header.stamp.nanosec (uint32)
        let nanosec = (timestamp_ns % 1_000_000_000) as u32;
        data.extend_from_slice(&nanosec.to_le_bytes());

        // TransformStamped.header.frame_id (string)
        write_cdr_string(&mut data, base_frame);

        // TransformStamped.child_frame_id (string)
        write_cdr_string(&mut data, child_frame);

        // TransformStamped.transform.translation (Vector3)
        align_to(&mut data, 8);
        let tx = 0.1 * ((i + 1) as f64);
        let ty = 0.0f64;
        let tz = 0.5 + 0.01 * (frame_index as f64);
        data.extend_from_slice(&tx.to_le_bytes());
        data.extend_from_slice(&ty.to_le_bytes());
        data.extend_from_slice(&tz.to_le_bytes());

        // TransformStamped.transform.rotation (Quaternion - identity)
        let qx = 0.0f64;
        let qy = 0.0f64;
        let qz = 0.0f64;
        let qw = 1.0f64;
        data.extend_from_slice(&qx.to_le_bytes());
        data.extend_from_slice(&qy.to_le_bytes());
        data.extend_from_slice(&qz.to_le_bytes());
        data.extend_from_slice(&qw.to_le_bytes());

        // Note: frame_index is not used for seq anymore but kept for potential future use
        let _ = frame_index;
    }

    data
}

// =============================================================================
// CDR Serialization Helpers
// =============================================================================

/// Writes a CDR string (with null terminator and length prefix).
fn write_cdr_string(data: &mut Vec<u8>, s: &str) {
    // Align to 4 bytes for the length field
    align_to(data, 4);

    // Length includes null terminator
    let len = (s.len() + 1) as u32;
    data.extend_from_slice(&len.to_le_bytes());

    // String content
    data.extend_from_slice(s.as_bytes());

    // Null terminator
    data.push(0);
}

/// Writes a CDR string array.
fn write_cdr_string_array(data: &mut Vec<u8>, strings: &[String]) {
    // Align to 4 bytes for the sequence length
    align_to(data, 4);

    // Sequence length
    let len = strings.len() as u32;
    data.extend_from_slice(&len.to_le_bytes());

    // Each string
    for s in strings {
        write_cdr_string(data, s);
    }
}

/// Writes a CDR f64 array.
fn write_cdr_f64_array(data: &mut Vec<u8>, values: &[f64]) {
    // Align to 4 bytes for the sequence length
    align_to(data, 4);

    // Sequence length
    let len = values.len() as u32;
    data.extend_from_slice(&len.to_le_bytes());

    // Align to 8 bytes for f64 values
    align_to(data, 8);

    // Values
    for &v in values {
        data.extend_from_slice(&v.to_le_bytes());
    }
}

/// Writes a CDR u8 array.
fn write_cdr_u8_array(data: &mut Vec<u8>, values: &[u8]) {
    // Align to 4 bytes for the sequence length
    align_to(data, 4);

    // Sequence length
    let len = values.len() as u32;
    data.extend_from_slice(&len.to_le_bytes());

    // Values (no alignment needed for u8)
    data.extend_from_slice(values);
}

/// Aligns the data buffer to the specified boundary.
fn align_to(data: &mut Vec<u8>, alignment: usize) {
    let current_pos = data.len();
    // CDR alignment is relative to the start of the data (after the 4-byte header)
    let data_offset = current_pos.saturating_sub(4);
    let remainder = data_offset % alignment;
    if remainder != 0 {
        let padding = alignment - remainder;
        data.extend(std::iter::repeat_n(0u8, padding));
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_generate_mcap() {
        let dir = tempdir().unwrap();
        let output_dir = Utf8PathBuf::try_from(dir.path().to_path_buf()).unwrap();

        let generator = McapGenerator::with_defaults();
        let mcap_path = generator.generate(&output_dir).unwrap();

        assert!(mcap_path.exists());
        assert!(output_dir.join("metadata.yaml").exists());
    }

    #[test]
    fn test_mcap_can_be_read() {
        use mcap::MessageStream;
        use memmap2::Mmap;
        use std::fs::File;

        let dir = tempdir().unwrap();
        let output_dir = Utf8PathBuf::try_from(dir.path().to_path_buf()).unwrap();

        let config = McapGeneratorConfig {
            num_frames: 5,
            generate_images: false,
            generate_tf: false,
            ..Default::default()
        };
        let generator = McapGenerator::new(config);
        let mcap_path = generator.generate(&output_dir).unwrap();

        // Read and verify
        let file = File::open(mcap_path.as_std_path()).unwrap();
        let mmap = unsafe { Mmap::map(&file).unwrap() };
        let stream = MessageStream::new(&mmap).unwrap();

        let messages: Vec<_> = stream.collect();
        assert_eq!(messages.len(), 5);

        for (i, msg_result) in messages.iter().enumerate() {
            let msg = msg_result.as_ref().unwrap();
            assert_eq!(msg.channel.topic, "/joint_states");
            assert_eq!(msg.sequence, i as u32);
        }
    }

    #[test]
    fn test_mcap_with_all_topics() {
        use mcap::MessageStream;
        use memmap2::Mmap;
        use std::collections::HashSet;
        use std::fs::File;

        let dir = tempdir().unwrap();
        let output_dir = Utf8PathBuf::try_from(dir.path().to_path_buf()).unwrap();

        let config = McapGeneratorConfig {
            num_frames: 3,
            generate_images: true,
            generate_tf: true,
            ..Default::default()
        };
        let generator = McapGenerator::new(config);
        let mcap_path = generator.generate(&output_dir).unwrap();

        // Read and verify
        let file = File::open(mcap_path.as_std_path()).unwrap();
        let mmap = unsafe { Mmap::map(&file).unwrap() };
        let stream = MessageStream::new(&mmap).unwrap();

        let messages: Vec<_> = stream.collect();
        // 3 frames * 3 topics (joint_states, image_raw, tf) + 1 tf_static = 10 messages
        assert_eq!(messages.len(), 10);

        // Verify all topics are present
        let topics: HashSet<_> = messages
            .iter()
            .filter_map(|r| r.as_ref().ok())
            .map(|m| m.channel.topic.as_str())
            .collect();

        assert!(topics.contains("/joint_states"));
        assert!(topics.contains("/camera/image_raw"));
        assert!(topics.contains("/tf"));
        assert!(topics.contains("/tf_static"));
    }

    #[test]
    fn test_mcap_can_be_ingested() {
        use crate::core::{Context, Stage};
        use crate::ingest::rosbag2_ingestor::{Rosbag2Ingestor, Rosbag2IngestorConfig};

        let dir = tempdir().unwrap();
        let output_dir = Utf8PathBuf::try_from(dir.path().to_path_buf()).unwrap();

        let config = McapGeneratorConfig {
            num_frames: 10,
            fps: 30,
            joint_names: vec![
                "joint1".to_string(),
                "joint2".to_string(),
                "joint3".to_string(),
            ],
            generate_images: false,
            generate_tf: false,
            ..Default::default()
        };
        let generator = McapGenerator::new(config);
        let mcap_path = generator.generate(&output_dir).unwrap();

        // Ingest with Rosbag2Ingestor
        let mut context = Context::default();
        context.set_rosbag_path(mcap_path);

        let mut ingestor = Rosbag2Ingestor::new(Rosbag2IngestorConfig::without_metadata());
        let context = ingestor.run(context).expect("ingestor should succeed");

        // Verify dataset
        let dataset = context.dataset.as_ref().expect("dataset should exist");
        assert!(dataset.contains_key("/joint_states"));

        let df = dataset["/joint_states"].clone().collect().unwrap();
        assert_eq!(df.height(), 10);

        // Verify columns exist
        let column_names: Vec<_> = df.get_column_names().into_iter().collect();
        assert!(
            df.column("timestamp_ns").is_ok(),
            "expected timestamp_ns, got columns: {:?}",
            column_names
        );
        assert!(
            df.column("publish_timestamp_ns").is_ok(),
            "expected publish_timestamp_ns, got columns: {:?}",
            column_names
        );
        assert!(
            df.column("name").is_ok(),
            "expected name, got columns: {:?}",
            column_names
        );
        assert!(
            df.column("position").is_ok(),
            "expected position, got columns: {:?}",
            column_names
        );
    }

    #[test]
    fn test_mcap_with_images_can_be_ingested() {
        use crate::core::{Context, Stage};
        use crate::ingest::rosbag2_ingestor::{Rosbag2Ingestor, Rosbag2IngestorConfig};

        let dir = tempdir().unwrap();
        let output_dir = Utf8PathBuf::try_from(dir.path().to_path_buf()).unwrap();

        let config = McapGeneratorConfig {
            num_frames: 5,
            fps: 30,
            image_size: (32, 32),
            generate_images: true,
            generate_tf: false,
            ..Default::default()
        };
        let generator = McapGenerator::new(config);
        let mcap_path = generator.generate(&output_dir).unwrap();

        // Ingest with Rosbag2Ingestor
        let mut context = Context::default();
        context.set_rosbag_path(mcap_path);

        let mut ingestor = Rosbag2Ingestor::new(Rosbag2IngestorConfig::without_metadata());
        let context = ingestor.run(context).expect("ingestor should succeed");

        // Verify dataset
        let dataset = context.dataset.as_ref().expect("dataset should exist");
        assert!(dataset.contains_key("/joint_states"));
        assert!(
            dataset.contains_key("/camera/image_raw"),
            "expected /camera/image_raw topic, got: {:?}",
            dataset.keys().collect::<Vec<_>>()
        );

        // Verify image data is stored separately
        let image_data = context
            .image_data
            .as_ref()
            .expect("image_data should exist");
        assert!(
            image_data.contains_key("/camera/image_raw"),
            "expected image data for /camera/image_raw"
        );
        assert_eq!(image_data["/camera/image_raw"].len(), 5);
    }

    #[test]
    fn test_mcap_with_tf_can_be_ingested() {
        use crate::core::{Context, Stage};
        use crate::ingest::rosbag2_ingestor::{Rosbag2Ingestor, Rosbag2IngestorConfig};

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

        // Ingest with Rosbag2Ingestor
        let mut context = Context::default();
        context.set_rosbag_path(mcap_path);

        let mut ingestor = Rosbag2Ingestor::new(Rosbag2IngestorConfig::without_metadata());
        let context = ingestor.run(context).expect("ingestor should succeed");

        // Verify dataset
        let dataset = context.dataset.as_ref().expect("dataset should exist");
        assert!(dataset.contains_key("/joint_states"));
        assert!(
            dataset.contains_key("/tf"),
            "expected /tf topic, got: {:?}",
            dataset.keys().collect::<Vec<_>>()
        );

        let df = dataset["/tf"].clone().collect().unwrap();
        assert_eq!(df.height(), 5);
    }
}
