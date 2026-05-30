//! ParquetVideoExporter stage for structured data export.
//!
//! This module provides a terminal stage that exports the Context's dataset
//! to a structured directory format with Parquet files and encoded video files.
//!
//! # Output Structure
//!
//! ```text
//! {output_dir}/{uuid}/
//!   parquet/
//!     {topic}.parquet           # Topic data with rosbag_uuid column
//!     _metadata.parquet         # Airoa metadata
//!     _topic_type_map.parquet   # Topic name to message type mapping
//!     _video_registry.parquet   # Topic name to video metadata mapping
//!   videos/
//!     {topic}.mp4               # Encoded video files
//! ```

use std::collections::HashMap;
use std::fs;

use camino::{Utf8Path, Utf8PathBuf};
use polars::prelude::*;
use serde::{Deserialize, Serialize};

use crate::common::{
    ImageFrame, ImageShape, resolve_or_infer_image_shape, topic_name_to_flat_file_stem,
};
use crate::core::conversion::arrow_batch_to_polars;
use crate::core::error::{OptionExt, PolarsExt, StageResult};
use crate::core::stage::{Context, Stage, StageConfig, StageError};
use crate::encode::depth_video_encoder::{DepthVideoConfig, encode_depth_videos};
use crate::encode::video_artifact::VideoArtifact;
use crate::encode::video_encoder::{VideoEncoderConfig, VideoEncoderVariant};
use crate::schema::metadata::AiroaMetadata;
use crate::schema::metadata::arrow::metadata_to_record_batch;

fn build_rgb_video_artifact(
    topic: &str,
    frames: &[ImageFrame],
    image_topic_shapes: Option<&HashMap<String, ImageShape>>,
    video_path: &str,
    video_config: &VideoEncoderConfig,
) -> StageResult<VideoArtifact> {
    let shape =
        resolve_or_infer_image_shape(topic, frames, image_topic_shapes, 3).ok_or_else(|| {
            StageError::invalid(format!(
                "missing image shape for video artifact metadata: {topic}"
            ))
        })?;

    let (output_width, output_height) =
        video_config.output_dimensions(shape.width as u32, shape.height as u32)?;
    video_config.video_artifact(video_path.to_string(), output_width, output_height)
}

/// Configuration for the ParquetVideoExporter stage.
///
/// ParquetVideoExporter is the terminal stage of a pipeline that exports
/// the Context's dataset to a structured directory format with Parquet files
/// and encoded video files.
///
/// # Output Structure
///
/// ```text
/// {output_dir}/{uuid}/
///   parquet/
///     {topic}.parquet           # Topic data with rosbag_uuid column
///     _metadata.parquet         # Airoa metadata
///     _topic_type_map.parquet   # Topic name to message type mapping
///     _video_registry.parquet   # Topic name to video metadata mapping
///   videos/
///     {topic}.mp4               # Encoded video files
/// ```
///
/// # Example YAML Configuration
///
/// ```yaml
/// stage_configs:
///   - Rosbag2IngestorConfig: {}
///   - UuidEnricherConfig: {}
///   - ParquetVideoExporterConfig:
///       output_dir: "/data/export_output"
///       # video_config is optional; defaults to VideoEncoderConfig::default()
///       video_config:
///         fps: 30
///         gop: 2
///         crf: "30"
///         codec_config:
///           codec: AV1
/// ```
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ParquetVideoExporterConfig {
    /// Root output directory. UUID subdirectory is created automatically.
    pub output_dir: String,

    /// Video encoder configuration.
    ///
    /// If not specified, `VideoEncoderConfig::default()` will be used.
    #[serde(default)]
    pub video_config: Option<VideoEncoderConfig>,

    /// Depth video encoder configuration.
    ///
    /// If specified, depth topics in `context.depth_data` are encoded as video.
    /// If not specified, depth topics are silently skipped.
    #[serde(default)]
    pub depth_config: Option<DepthVideoConfig>,
}

impl ParquetVideoExporterConfig {
    /// Creates a new ParquetVideoExporterConfig with the specified output directory.
    pub fn new(output_dir: impl Into<String>) -> Self {
        Self {
            output_dir: output_dir.into(),
            video_config: None,
            depth_config: None,
        }
    }

    /// Sets the video encoder configuration.
    pub fn with_video_config(mut self, config: VideoEncoderConfig) -> Self {
        self.video_config = Some(config);
        self
    }

    /// Sets the depth video encoder configuration.
    pub fn with_depth_config(mut self, config: DepthVideoConfig) -> Self {
        self.depth_config = Some(config);
        self
    }

    /// Returns the video config or the default.
    fn video_config_or_default(&self) -> VideoEncoderConfig {
        self.video_config.clone().unwrap_or_default()
    }
}

#[typetag::serde(name = "ParquetVideoExporterConfig")]
impl StageConfig for ParquetVideoExporterConfig {
    fn build(&self) -> Box<dyn Stage> {
        Box::new(ParquetVideoExporter::new(self.clone()))
    }
}

/// A pipeline stage that exports Context data to structured Parquet + Video format.
///
/// # Preconditions
///
/// - `dataset`: **Required** - HashMap of topic names to LazyFrames
/// - `airoa_metadata`: **Required** - Metadata containing UUID
/// - `topic_message_type_map`: **Required** - Topic to message type mapping
/// - `image_data`: Conditional - If present, videos will be encoded
///
/// # Postconditions
///
/// - `output_dir`: **Guaranteed** - Set to `{config.output_dir}/{uuid}`
/// - `bundle_root`: **Guaranteed** - Set to `{config.output_dir}/{uuid}`
/// - `video_registry`: Conditional - Set only if videos were written
///
/// # Output Files
///
/// - `parquet/{topic}.parquet` - Each topic's data
/// - `parquet/_metadata.parquet` - Airoa metadata as Parquet
/// - `parquet/_topic_type_map.parquet` - Topic name to message type mapping
/// - `parquet/_video_registry.parquet` - Topic name to video metadata mapping (if videos exist)
/// - `videos/{topic}.mp4` - Encoded videos (if image_data exists)
pub struct ParquetVideoExporter {
    config: ParquetVideoExporterConfig,
}

impl ParquetVideoExporter {
    /// Creates a new ParquetVideoExporter with the given configuration.
    pub fn new(config: ParquetVideoExporterConfig) -> Self {
        Self { config }
    }

    /// Encodes RGB videos and returns topic -> video artifact with relative paths.
    fn encode_videos(
        &self,
        image_data: &HashMap<String, Vec<ImageFrame>>,
        image_topic_shapes: Option<&HashMap<String, ImageShape>>,
        videos_dir: &Utf8Path,
        video_config: &VideoEncoderConfig,
    ) -> StageResult<HashMap<String, VideoArtifact>> {
        let mut video_artifacts = HashMap::new();

        for (topic_name, frames) in image_data {
            if frames.is_empty() {
                continue;
            }

            let sanitized_topic_name = topic_name_to_flat_file_stem(topic_name);
            let video_filename = format!("{}.mp4", sanitized_topic_name);
            let output_path = videos_dir.join(&video_filename);

            tracing::debug!(
                "Encoding {} frames for topic {} to {}",
                frames.len(),
                topic_name,
                output_path
            );

            let mut encoder = VideoEncoderVariant::from_config(&output_path, video_config.clone());
            for frame in frames {
                encoder.add_data(&frame.bytes)?;
            }
            encoder.finish()?;

            let relative_path = format!("videos/{}", video_filename);
            let artifact = build_rgb_video_artifact(
                topic_name,
                frames,
                image_topic_shapes,
                &relative_path,
                video_config,
            )?;
            video_artifacts.insert(topic_name.clone(), artifact);
        }

        Ok(video_artifacts)
    }

    /// Writes each topic's LazyFrame as a Parquet file.
    fn write_topic_parquets(
        &self,
        dataset: &HashMap<String, LazyFrame>,
        parquet_dir: &Utf8Path,
    ) -> StageResult<()> {
        for (topic, lf) in dataset {
            let sanitized_topic_name = topic_name_to_flat_file_stem(topic);
            let output_path = parquet_dir.join(format!("{}.parquet", sanitized_topic_name));

            // Create parent directories if needed
            if let Some(parent) = output_path.parent() {
                fs::create_dir_all(parent.as_std_path()).map_err(|e| {
                    StageError::io(format!("failed to create directory: {}", parent), e)
                })?;
            }

            let mut df = lf
                .clone()
                .collect()
                .or_invalid("failed to collect LazyFrame")?;

            let mut file = fs::File::create(output_path.as_std_path())
                .map_err(|e| StageError::io(format!("failed to create {}", output_path), e))?;

            ParquetWriter::new(&mut file)
                .finish(&mut df)
                .or_invalid("failed to write parquet")?;

            tracing::debug!("Wrote topic parquet: {}", output_path);
        }

        Ok(())
    }

    /// Writes canonical V2.0 Airoa metadata as a Parquet file.
    fn write_metadata_parquet(
        &self,
        metadata: &AiroaMetadata,
        parquet_dir: &Utf8Path,
    ) -> StageResult<()> {
        let output_path = parquet_dir.join("_metadata.parquet");

        let metadata_v2 = metadata.clone().into_v2_0()?;
        let record_batch = metadata_to_record_batch(&metadata_v2)?;

        // Convert Arrow RecordBatch to Polars DataFrame
        let mut df = arrow_batch_to_polars(&record_batch);

        let mut file = fs::File::create(output_path.as_std_path())
            .map_err(|e| StageError::io(format!("failed to create {}", output_path), e))?;

        ParquetWriter::new(&mut file)
            .finish(&mut df)
            .or_invalid("failed to write metadata parquet")?;

        tracing::debug!("Wrote metadata parquet: {}", output_path);

        Ok(())
    }

    /// Writes the topic type map as a Parquet file.
    ///
    /// Schema: [rosbag_uuid: Utf8, topic_name: Utf8, message_type: Utf8]
    fn write_topic_type_map_parquet(
        &self,
        uuid: &str,
        topic_type_map: &HashMap<String, String>,
        parquet_dir: &Utf8Path,
    ) -> StageResult<()> {
        let output_path = parquet_dir.join("_topic_type_map.parquet");

        let mut uuids = Vec::with_capacity(topic_type_map.len());
        let mut topics = Vec::with_capacity(topic_type_map.len());
        let mut types = Vec::with_capacity(topic_type_map.len());

        for (topic, msg_type) in topic_type_map {
            uuids.push(uuid.to_string());
            topics.push(topic.clone());
            types.push(msg_type.clone());
        }

        let mut df = df! {
            "rosbag_uuid" => uuids,
            "topic_name" => topics,
            "message_type" => types,
        }
        .or_invalid("failed to create topic_type_map dataframe")?;

        let mut file = fs::File::create(output_path.as_std_path())
            .map_err(|e| StageError::io(format!("failed to create {}", output_path), e))?;

        ParquetWriter::new(&mut file)
            .finish(&mut df)
            .or_invalid("failed to write topic_type_map parquet")?;

        tracing::debug!("Wrote topic_type_map parquet: {}", output_path);

        Ok(())
    }

    /// Writes the video registry as a Parquet file.
    ///
    /// Schema:
    /// [rosbag_uuid, topic_name, video_path, media_type, codec_family,
    ///  encoder_name, pix_fmt, width, height, fps, encoding_config_json]
    ///
    /// The video_path is stored as a relative path (e.g., "videos/camera__rgb__image_raw.mp4")
    /// for portability when uploading to S3 or other storage.
    /// The media_type column indicates "rgb" or "depth" for downstream consumers.
    fn write_video_registry_parquet(
        &self,
        uuid: &str,
        video_artifacts: &HashMap<String, VideoArtifact>,
        parquet_dir: &Utf8Path,
    ) -> StageResult<()> {
        let output_path = parquet_dir.join("_video_registry.parquet");

        let mut uuids = Vec::with_capacity(video_artifacts.len());
        let mut topics = Vec::with_capacity(video_artifacts.len());
        let mut paths = Vec::with_capacity(video_artifacts.len());
        let mut media_types = Vec::with_capacity(video_artifacts.len());
        let mut codec_families = Vec::with_capacity(video_artifacts.len());
        let mut encoder_names = Vec::with_capacity(video_artifacts.len());
        let mut pix_fmts = Vec::with_capacity(video_artifacts.len());
        let mut widths = Vec::with_capacity(video_artifacts.len());
        let mut heights = Vec::with_capacity(video_artifacts.len());
        let mut fps_values = Vec::with_capacity(video_artifacts.len());
        let mut encoding_configs = Vec::with_capacity(video_artifacts.len());

        for (topic, artifact) in video_artifacts {
            uuids.push(uuid.to_string());
            topics.push(topic.clone());
            paths.push(artifact.video_path.clone());
            media_types.push(artifact.metadata.media_type.clone());
            codec_families.push(artifact.metadata.codec_family.clone());
            encoder_names.push(artifact.metadata.encoder_name.clone());
            pix_fmts.push(artifact.metadata.pix_fmt.clone());
            widths.push(artifact.metadata.width);
            heights.push(artifact.metadata.height);
            fps_values.push(artifact.metadata.fps);
            encoding_configs.push(artifact.metadata.encoding_config_json.clone());
        }

        let mut df = df! {
            "rosbag_uuid" => uuids,
            "topic_name" => topics,
            "video_path" => paths,
            "media_type" => media_types,
            "codec_family" => codec_families,
            "encoder_name" => encoder_names,
            "pix_fmt" => pix_fmts,
            "width" => widths,
            "height" => heights,
            "fps" => fps_values,
            "encoding_config_json" => encoding_configs,
        }
        .or_invalid("failed to create video_registry dataframe")?;

        let mut file = fs::File::create(output_path.as_std_path())
            .map_err(|e| StageError::io(format!("failed to create {}", output_path), e))?;

        ParquetWriter::new(&mut file)
            .finish(&mut df)
            .or_invalid("failed to write video_registry parquet")?;

        tracing::debug!("Wrote video_registry parquet: {}", output_path);

        Ok(())
    }
}

impl Stage for ParquetVideoExporter {
    fn name(&self) -> &'static str {
        "parquet_video_exporter"
    }

    fn run(&mut self, mut context: Context) -> Result<Context, StageError> {
        // 1. Validation - check required preconditions and extract needed data
        // Clone data early to avoid borrow issues when mutating context later
        let dataset = context
            .dataset
            .as_ref()
            .or_missing("dataset is required for ParquetVideoExporter")?
            .clone();

        let metadata = context
            .airoa_metadata
            .as_ref()
            .or_missing("airoa_metadata is required for ParquetVideoExporter")?
            .clone();

        let topic_type_map = context
            .topic_message_type_map
            .as_ref()
            .or_missing("topic_message_type_map is required for ParquetVideoExporter")?
            .clone();

        let uuid = metadata.uuid_string();

        // 2. Setup directories
        let base_dir = Utf8PathBuf::from(&self.config.output_dir).join(&uuid);
        let parquet_dir = base_dir.join("parquet");
        let videos_dir = base_dir.join("videos");

        fs::create_dir_all(parquet_dir.as_std_path()).map_err(|e| {
            StageError::io(format!("failed to create parquet dir: {}", parquet_dir), e)
        })?;
        fs::create_dir_all(videos_dir.as_std_path()).map_err(|e| {
            StageError::io(format!("failed to create videos dir: {}", videos_dir), e)
        })?;

        tracing::info!("Exporting to {} (uuid: {})", base_dir, uuid);

        // 3. Video Encoding (if image_data exists)
        let video_config = self.config.video_config_or_default();
        let mut video_artifacts = if let Some(image_data) = context.image_data.as_ref() {
            self.encode_videos(
                image_data,
                context.image_topic_shapes.as_ref(),
                &videos_dir,
                &video_config,
            )?
        } else {
            HashMap::new()
        };

        // 3b. Depth Video Encoding (if depth_data exists and depth_config is set)
        let depth_video_artifacts = if let (Some(depth_data), Some(depth_config)) =
            (context.depth_data.as_ref(), &self.config.depth_config)
        {
            encode_depth_videos(depth_data, &videos_dir, depth_config)?
        } else {
            HashMap::new()
        };

        let dataset_len = dataset.len();
        let video_count = video_artifacts.len() + depth_video_artifacts.len();

        // 4. Write Parquet files
        self.write_topic_parquets(&dataset, &parquet_dir)?;
        self.write_metadata_parquet(&metadata, &parquet_dir)?;
        self.write_topic_type_map_parquet(&uuid, &topic_type_map, &parquet_dir)?;

        // Merge artifacts for registry and context
        video_artifacts.extend(depth_video_artifacts);

        if !video_artifacts.is_empty() {
            self.write_video_registry_parquet(&uuid, &video_artifacts, &parquet_dir)?;
            context.set_video_registry(video_artifacts);
        }

        // 5. Update context
        context.set_bundle_root(base_dir.clone());
        context.set_output_dir(base_dir.clone());

        tracing::info!(
            "Export complete: {} topic parquets, {} videos",
            dataset_len,
            video_count
        );

        Ok(context)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::schema::metadata::parse_metadata;

    #[test]
    fn test_topic_name_to_flat_file_stem() {
        assert_eq!(
            topic_name_to_flat_file_stem("/camera/rgb/image_raw"),
            "camera__rgb__image_raw"
        );
        assert_eq!(
            topic_name_to_flat_file_stem("camera/rgb/image_raw"),
            "camera__rgb__image_raw"
        );
        assert_eq!(
            topic_name_to_flat_file_stem("/joint_states"),
            "joint_states"
        );
        assert_eq!(topic_name_to_flat_file_stem("joint_states"), "joint_states");
    }

    #[test]
    fn test_config_new() {
        let config = ParquetVideoExporterConfig::new("/data/output");
        assert_eq!(config.output_dir, "/data/output");
        assert!(config.video_config.is_none());
    }

    #[test]
    fn test_config_with_video_config() {
        let video_config = VideoEncoderConfig::new(60);
        let config =
            ParquetVideoExporterConfig::new("/data/output").with_video_config(video_config.clone());

        assert_eq!(config.video_config.as_ref().unwrap().fps, 60);
    }

    #[test]
    fn test_config_video_config_or_default() {
        // Without video config - should return VideoEncoderConfig::default()
        let config = ParquetVideoExporterConfig::new("/data/output");
        let video_config = config.video_config_or_default();
        let expected = VideoEncoderConfig::default();
        assert_eq!(video_config.fps, expected.fps);
        assert_eq!(video_config.gop, expected.gop);

        // With video config - should return the specified one
        let custom_video = VideoEncoderConfig::new(60).set_gop(5);
        let config =
            ParquetVideoExporterConfig::new("/data/output").with_video_config(custom_video);
        let video_config = config.video_config_or_default();
        assert_eq!(video_config.fps, 60);
        assert_eq!(video_config.gop, 5);
    }

    #[test]
    fn test_config_yaml_serialization() {
        let config = ParquetVideoExporterConfig::new("/data/output");
        let yaml = serde_yaml::to_string(&config).unwrap();
        assert!(yaml.contains("output_dir"));
        assert!(yaml.contains("/data/output"));
    }

    #[test]
    fn test_write_metadata_parquet_normalizes_v1_3_to_v2_0() {
        use std::fs;
        use tempfile::tempdir;

        let temp_dir = tempdir().unwrap();
        let parquet_dir = Utf8PathBuf::try_from(temp_dir.path().to_path_buf()).unwrap();
        let exporter = ParquetVideoExporter::new(ParquetVideoExporterConfig::new("/unused"));

        let v1_3_json = fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/testdata/metadata/v1.3/meta.json"
        ))
        .unwrap();
        let metadata = parse_metadata(&v1_3_json).unwrap();

        exporter
            .write_metadata_parquet(&metadata, &parquet_dir)
            .unwrap();

        let file = fs::File::open(parquet_dir.join("_metadata.parquet").as_std_path()).unwrap();
        let df = ParquetReader::new(file).finish().unwrap();

        assert!(
            df.get_column_names()
                .iter()
                .any(|name| *name == "schema_version")
        );
        assert!(!df.get_column_names().iter().any(|name| *name == "version"));
    }
}
