use std::collections::HashMap;
use std::fs;

use camino::{Utf8Path, Utf8PathBuf};
use polars::prelude::{DataFrame, IntoLazy, LazyFrame, ParquetReader, PlPath, SerReader};
use serde::{Deserialize, Serialize};

use crate::common::topic_name_to_flat_file_stem;
use crate::core::conversion::lazy_to_record_batch_rechunk;
use crate::core::error::{OptionExt, PolarsExt, StageResult};
use crate::core::stage::{Context, PipelineInputKind, Stage, StageConfig, StageError};
use crate::encode::video_artifact::{VideoArtifact, VideoMetadata};
use crate::schema::metadata::AiroaMetadata;
use crate::schema::metadata::arrow::record_batch_to_metadata;

const PARQUET_DIR_NAME: &str = "parquet";
const METADATA_FILE_NAME: &str = "_metadata.parquet";
const TOPIC_TYPE_MAP_FILE_NAME: &str = "_topic_type_map.parquet";
const VIDEO_REGISTRY_FILE_NAME: &str = "_video_registry.parquet";

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct ParquetVideoIngestorConfig {
    #[serde(default)]
    pub input_dir: Option<String>,
}

impl ParquetVideoIngestorConfig {
    pub fn new(input_dir: impl Into<String>) -> Self {
        Self {
            input_dir: Some(input_dir.into()),
        }
    }
}

#[typetag::serde(name = "ParquetVideoIngestorConfig")]
impl StageConfig for ParquetVideoIngestorConfig {
    fn build(&self) -> Box<dyn Stage> {
        Box::new(ParquetVideoIngestor::new(self.clone()))
    }

    fn pipeline_input_kind(&self) -> Option<PipelineInputKind> {
        Some(PipelineInputKind::ParquetVideoBundle)
    }
}

pub struct ParquetVideoIngestor {
    config: ParquetVideoIngestorConfig,
}

impl ParquetVideoIngestor {
    pub fn new(config: ParquetVideoIngestorConfig) -> Self {
        Self { config }
    }

    fn normalize_input_dir(input_dir: Utf8PathBuf) -> StageResult<Utf8PathBuf> {
        if input_dir.is_absolute() {
            return Ok(input_dir);
        }

        let current_dir = std::env::current_dir()
            .map_err(|e| StageError::invalid_with("failed to get current directory", e))?;
        let current_dir = Utf8PathBuf::try_from(current_dir)
            .map_err(|e| StageError::invalid_with("current directory is not valid UTF-8", e))?;
        Ok(current_dir.join(input_dir))
    }

    fn resolve_input_dir(&self, context: &Context) -> StageResult<Utf8PathBuf> {
        if let Some(input_dir) = &self.config.input_dir {
            return Self::normalize_input_dir(Utf8PathBuf::from(input_dir));
        }

        let bundle_root = context.bundle_root().cloned().or_missing(
            "bundle_root in context (set by Orchestrator or provide ParquetVideoIngestorConfig.input_dir)",
        )?;
        Self::normalize_input_dir(bundle_root)
    }

    fn read_parquet_dataframe(path: &Utf8Path) -> StageResult<DataFrame> {
        let file = fs::File::open(path.as_std_path())
            .map_err(|e| StageError::io(format!("failed to open parquet file: {path}"), e))?;
        ParquetReader::new(file).finish().map_err(|e| {
            StageError::invalid_with(format!("failed to read parquet file: {path}"), e)
        })
    }

    fn load_metadata(parquet_dir: &Utf8Path) -> StageResult<AiroaMetadata> {
        let metadata_path = parquet_dir.join(METADATA_FILE_NAME);
        let metadata_df = Self::read_parquet_dataframe(&metadata_path)?;
        let record_batch = lazy_to_record_batch_rechunk(&metadata_df.lazy());
        let metadata = record_batch_to_metadata(&record_batch)?;
        Ok(AiroaMetadata::V2_0(metadata))
    }

    fn load_topic_type_map(parquet_dir: &Utf8Path) -> StageResult<HashMap<String, String>> {
        let map_path = parquet_dir.join(TOPIC_TYPE_MAP_FILE_NAME);
        let df = Self::read_parquet_dataframe(&map_path)?;

        let topics = df
            .column("topic_name")
            .or_invalid("topic_type_map is missing topic_name column")?
            .str()
            .or_invalid("topic_type_map.topic_name must be string")?;
        let message_types = df
            .column("message_type")
            .or_invalid("topic_type_map is missing message_type column")?
            .str()
            .or_invalid("topic_type_map.message_type must be string")?;

        let mut map = HashMap::with_capacity(df.height());
        for idx in 0..df.height() {
            let topic = topics
                .get(idx)
                .ok_or_else(|| StageError::invalid("topic_type_map.topic_name contains null"))?;
            let message_type = message_types
                .get(idx)
                .ok_or_else(|| StageError::invalid("topic_type_map.message_type contains null"))?;

            if map
                .insert(topic.to_string(), message_type.to_string())
                .is_some()
            {
                return Err(StageError::invalid(format!(
                    "duplicate topic in topic_type_map: {topic}"
                )));
            }
        }

        Ok(map)
    }

    fn load_topic_parquets(
        parquet_dir: &Utf8Path,
        topic_type_map: &HashMap<String, String>,
    ) -> StageResult<HashMap<String, LazyFrame>> {
        let mut dataset = HashMap::with_capacity(topic_type_map.len());

        for topic in topic_type_map.keys() {
            let file_stem = topic_name_to_flat_file_stem(topic);
            let parquet_path = parquet_dir.join(format!("{file_stem}.parquet"));
            let lazy_frame =
                LazyFrame::scan_parquet(PlPath::new(parquet_path.as_str()), Default::default())
                    .map_err(|e| {
                        StageError::external(
                            format!("failed to scan topic parquet: {parquet_path}"),
                            e,
                        )
                    })?;
            dataset.insert(topic.clone(), lazy_frame);
        }

        Ok(dataset)
    }

    fn load_video_registry(parquet_dir: &Utf8Path) -> StageResult<HashMap<String, VideoArtifact>> {
        let registry_path = parquet_dir.join(VIDEO_REGISTRY_FILE_NAME);
        if !registry_path.exists() {
            return Ok(HashMap::new());
        }

        let df = Self::read_parquet_dataframe(&registry_path)?;

        let topics = df
            .column("topic_name")
            .or_invalid("video_registry is missing topic_name column")?
            .str()
            .or_invalid("video_registry.topic_name must be string")?;
        let video_paths = df
            .column("video_path")
            .or_invalid("video_registry is missing video_path column")?
            .str()
            .or_invalid("video_registry.video_path must be string")?;
        let media_types = df
            .column("media_type")
            .or_invalid("video_registry is missing media_type column")?
            .str()
            .or_invalid("video_registry.media_type must be string")?;
        let codec_families = df
            .column("codec_family")
            .or_invalid("video_registry is missing codec_family column")?
            .str()
            .or_invalid("video_registry.codec_family must be string")?;
        let encoder_names = df
            .column("encoder_name")
            .or_invalid("video_registry is missing encoder_name column")?
            .str()
            .or_invalid("video_registry.encoder_name must be string")?;
        let pix_fmts = df
            .column("pix_fmt")
            .or_invalid("video_registry is missing pix_fmt column")?
            .str()
            .or_invalid("video_registry.pix_fmt must be string")?;
        let widths = df
            .column("width")
            .or_invalid("video_registry is missing width column")?
            .u32()
            .or_invalid("video_registry.width must be u32")?;
        let heights = df
            .column("height")
            .or_invalid("video_registry is missing height column")?
            .u32()
            .or_invalid("video_registry.height must be u32")?;
        let fps_values = df
            .column("fps")
            .or_invalid("video_registry is missing fps column")?
            .u32()
            .or_invalid("video_registry.fps must be u32")?;
        let encoding_configs = df
            .column("encoding_config_json")
            .or_invalid("video_registry is missing encoding_config_json column")?
            .str()
            .or_invalid("video_registry.encoding_config_json must be string")?;

        let mut video_registry = HashMap::with_capacity(df.height());
        for idx in 0..df.height() {
            let topic = topics
                .get(idx)
                .ok_or_else(|| StageError::invalid("video_registry.topic_name contains null"))?;
            let artifact = VideoArtifact {
                video_path: video_paths
                    .get(idx)
                    .ok_or_else(|| StageError::invalid("video_registry.video_path contains null"))?
                    .to_string(),
                metadata: VideoMetadata {
                    media_type: media_types
                        .get(idx)
                        .ok_or_else(|| {
                            StageError::invalid("video_registry.media_type contains null")
                        })?
                        .to_string(),
                    codec_family: codec_families
                        .get(idx)
                        .ok_or_else(|| {
                            StageError::invalid("video_registry.codec_family contains null")
                        })?
                        .to_string(),
                    encoder_name: encoder_names
                        .get(idx)
                        .ok_or_else(|| {
                            StageError::invalid("video_registry.encoder_name contains null")
                        })?
                        .to_string(),
                    pix_fmt: pix_fmts
                        .get(idx)
                        .ok_or_else(|| StageError::invalid("video_registry.pix_fmt contains null"))?
                        .to_string(),
                    width: widths
                        .get(idx)
                        .ok_or_else(|| StageError::invalid("video_registry.width contains null"))?,
                    height: heights.get(idx).ok_or_else(|| {
                        StageError::invalid("video_registry.height contains null")
                    })?,
                    fps: fps_values
                        .get(idx)
                        .ok_or_else(|| StageError::invalid("video_registry.fps contains null"))?,
                    encoding_config_json: encoding_configs
                        .get(idx)
                        .ok_or_else(|| {
                            StageError::invalid("video_registry.encoding_config_json contains null")
                        })?
                        .to_string(),
                },
            };

            if video_registry.insert(topic.to_string(), artifact).is_some() {
                return Err(StageError::invalid(format!(
                    "duplicate topic in video_registry: {topic}"
                )));
            }
        }

        Ok(video_registry)
    }
}

impl Stage for ParquetVideoIngestor {
    fn name(&self) -> &'static str {
        "parquet_video_ingestor"
    }

    fn run(&mut self, mut context: Context) -> Result<Context, StageError> {
        let bundle_root = self.resolve_input_dir(&context)?;
        let parquet_dir = bundle_root.join(PARQUET_DIR_NAME);

        let topic_type_map = Self::load_topic_type_map(&parquet_dir)?;
        let dataset = Self::load_topic_parquets(&parquet_dir, &topic_type_map)?;
        let metadata = Self::load_metadata(&parquet_dir)?;
        let video_registry = Self::load_video_registry(&parquet_dir)?;

        context.set_bundle_root(bundle_root.clone());
        context.set_output_dir(bundle_root);
        context.set_dataset(dataset);
        context.set_topic_message_type_map(topic_type_map);
        context.set_airoa_metadata(metadata);

        if !video_registry.is_empty() {
            context.set_video_registry(video_registry);
            context.populate_image_topic_shapes_from_video_registry()?;
        }

        Ok(context)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use std::fs;

    use crate::common::ImageShape;
    use crate::core::conversion::arrow_batch_to_polars;
    use crate::schema::metadata::arrow::metadata_to_record_batch;
    use crate::schema::metadata::parse_metadata;
    use polars::df;
    use polars::prelude::ParquetWriter;
    use tempfile::tempdir;

    fn write_df(path: &Utf8Path, mut df: DataFrame) {
        let mut file = fs::File::create(path.as_std_path()).unwrap();
        ParquetWriter::new(&mut file).finish(&mut df).unwrap();
    }

    #[test]
    fn parquet_video_ingestor_restores_export_bundle() {
        let temp_dir = tempdir().unwrap();
        let bundle_root = Utf8PathBuf::from_path_buf(temp_dir.path().to_path_buf()).unwrap();
        let parquet_dir = bundle_root.join(PARQUET_DIR_NAME);
        let videos_dir = bundle_root.join("videos");
        fs::create_dir_all(parquet_dir.as_std_path()).unwrap();
        fs::create_dir_all(videos_dir.as_std_path()).unwrap();

        let topic_df = df! {
            "timestamp_ns" => vec![1_i64, 2_i64],
            "value" => vec![10_i64, 20_i64],
        }
        .unwrap();
        write_df(
            &parquet_dir.join(format!(
                "{}.parquet",
                topic_name_to_flat_file_stem("/camera/image_raw")
            )),
            topic_df.clone(),
        );

        let topic_type_map_df = df! {
            "rosbag_uuid" => vec!["uuid-123"],
            "topic_name" => vec!["/camera/image_raw"],
            "message_type" => vec!["sensor_msgs/msg/Image"],
        }
        .unwrap();
        write_df(
            &parquet_dir.join(TOPIC_TYPE_MAP_FILE_NAME),
            topic_type_map_df,
        );

        let metadata_json = fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/testdata/metadata/v2.0/meta.json"
        ))
        .unwrap();
        let metadata = parse_metadata(&metadata_json).unwrap().into_v2_0().unwrap();
        let metadata_batch = metadata_to_record_batch(&metadata).unwrap();
        let metadata_df = arrow_batch_to_polars(&metadata_batch);
        write_df(&parquet_dir.join(METADATA_FILE_NAME), metadata_df);

        let video_relative_path = "videos/camera__image_raw.mp4";
        fs::write(
            bundle_root.join(video_relative_path).as_std_path(),
            b"dummy",
        )
        .unwrap();
        let video_registry_df = df! {
            "rosbag_uuid" => vec!["uuid-123"],
            "topic_name" => vec!["/camera/image_raw"],
            "video_path" => vec![video_relative_path],
            "media_type" => vec!["rgb"],
            "codec_family" => vec!["av1"],
            "encoder_name" => vec!["libsvtav1"],
            "pix_fmt" => vec!["yuv420p"],
            "width" => vec![640_u32],
            "height" => vec![480_u32],
            "fps" => vec![30_u32],
            "encoding_config_json" => vec![r#"{"fps":30,"gop":2,"crf":"30","scaling":"Bicubic","codec_config":{"codec":"AV1","preset":10}}"#],
        }
        .unwrap();
        write_df(
            &parquet_dir.join(VIDEO_REGISTRY_FILE_NAME),
            video_registry_df,
        );

        let mut input_context = Context::default();
        input_context.set_bundle_root(bundle_root.clone());
        let mut ingestor = ParquetVideoIngestor::new(ParquetVideoIngestorConfig::default());
        let context = ingestor.run(input_context).unwrap();

        assert_eq!(context.bundle_root(), Some(&bundle_root));
        assert_eq!(context.output_dir(), Some(&bundle_root));
        assert!(context.dataset().unwrap().contains_key("/camera/image_raw"));
        assert_eq!(
            context
                .topic_message_type_map()
                .unwrap()
                .get("/camera/image_raw")
                .unwrap(),
            "sensor_msgs/msg/Image"
        );
        assert_eq!(
            context.airoa_metadata().unwrap().uuid_string(),
            metadata.uuid
        );
        assert_eq!(
            context.resolve_video_path("/camera/image_raw").unwrap(),
            bundle_root.join(video_relative_path)
        );

        let shape = context
            .image_topic_shapes()
            .unwrap()
            .get("/camera/image_raw")
            .unwrap();
        assert_eq!(*shape, ImageShape::new(480, 640, 3));

        let collected = context
            .dataset()
            .unwrap()
            .get("/camera/image_raw")
            .unwrap()
            .clone()
            .collect()
            .unwrap();
        assert_eq!(collected.shape(), topic_df.shape());
    }
}
