use std::collections::HashMap;

use camino::{Utf8Path, Utf8PathBuf};
use polars::prelude::*;
use serde::{Deserialize, Serialize};

use super::annotations::{build_pa_segment_annotations, build_segment_annotations};
use super::episodes::Episodes;
use super::io::DatasetWriter;
use super::lerobot_dataset_metadata::LeRobotMetadata;
use super::metadata::{MetadataComposer, episode_task_index, find_label, register_labels};
use super::segment::{
    SegmentAssembler, concatenate_segment_frames, filter_segments_with_indices_within_range,
    validate_required_topics,
};
use super::timeline::{TimelineFormatter, synched_timestamp_range};
use super::video::VideoEncoderPipeline;
use super::video::frame_provider::{FrameProvider, InMemoryFrameProvider, VideoFileFrameProvider};
use crate::core::error::OptionExt;
use crate::core::stage::{Context, Stage, StageConfig, StageError};
use crate::encode::video_encoder::VideoEncoderConfig;
use crate::schema::metadata::v2_0::{MetadataV2_0, Segment};
use crate::schema::{RobotModelSource, TopicFeatureMap, TopicFeatureMapEntry};

const LEROBOT_VERSION: &str = "v2.1";
const METADATA_VERSION: &str = "1.0";
const DEFAULT_TASK_TYPE_SHT: &str = "SHT";
const DEFAULT_TASK_TYPE_PA: &str = "PA";
const DEFAULT_EPISODE_LABEL: &str = "";
const DEFAULT_CHUNK_ID: &str = "000";
const DEFAULT_EPISODE_ID: &str = "000000";
const DATA_PATH_TEMPLATE: &str =
    "data/chunk-{episode_chunk:03d}/episode_{episode_index:06d}.parquet";
const VIDEO_PATH_TEMPLATE: &str =
    "videos/chunk-{episode_chunk:03d}/{video_key}/episode_{episode_index:06d}.mp4";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LeRobotV21TransformerConfig {
    pub outdir: String,
    pub robot_model: RobotModelSource,
    #[serde(default)]
    pub video_config: VideoEncoderConfig,
    /// If true, generate separate episodes for each Primitive Action (PA mode).
    /// If false (default), combine all segments into a single episode (SHT mode).
    #[serde(default)]
    pub separate_per_primitive: bool,
}

impl LeRobotV21TransformerConfig {
    pub fn new(outdir: &str, robot_model: RobotModelSource) -> Self {
        Self {
            outdir: outdir.to_string(),
            robot_model,
            video_config: VideoEncoderConfig::default(),
            separate_per_primitive: false,
        }
    }
}

#[typetag::serde(name = "LeRobotV21TransformerConfig")]
impl StageConfig for LeRobotV21TransformerConfig {
    fn build(&self) -> Box<dyn Stage> {
        Box::new(LeRobotV21Transformer::new(self.clone()))
    }
}

/// A pipeline stage that transforms synchronized rosbag data into LeRobot v2.1 dataset format.
///
/// This transformer is typically the final stage in a `rebake` pipeline. It takes time-synchronized
/// ROS topics and airoa metadata, assembles them into episodes based on segment definitions, and
/// outputs the complete LeRobot v2.1 dataset structure including parquet files, encoded videos,
/// and JSON metadata.
///
/// # Execution Modes
///
/// The transformer supports two modes controlled by `separate_per_primitive`:
///
/// - **SHT Mode (default)**: Short Horizon Task mode combines all segments into a single episode.
///   The `next.done` flag is true only at the end of each segment, allowing the model to learn
///   sub-task boundaries.
///
/// - **PA Mode**: Primitive Action mode generates separate episodes for each segment. Each segment
///   becomes an independent episode with its own parquet file and video files.
///
/// # Output Structure
///
/// For a rosbag with UUID `abc123`, the output directory structure is:
/// ```text
/// {outdir}/abc123/
/// ├── data/
/// │   └── chunk-000/
/// │       ├── episode_000000.parquet  (SHT: single file, PA: per-segment)
/// │       └── episode_000001.parquet  (PA mode only)
/// ├── videos/
/// │   └── chunk-000/
/// │       └── observation.image.hand/
/// │           └── episode_000000.mp4
/// └── meta/
///     ├── info.json
///     ├── episodes.jsonl
///     └── episodes_stats.jsonl
/// ```
///
/// # Preconditions
///
/// - `dataset`: **Required** (must contain `synched_timestamp_ns` column from time synchronization)
/// - `dataset`: **Required** (must contain every topic referenced by `robot_model`)
/// - `airoa_metadata`: **Required** (V1.3 or V2.0 format with segments and labels)
/// - `fps`: Conditional (if not set, attempts to infer from dataset frame spacing)
/// - `image_data` OR `video_registry`: Conditional (at least one required for video encoding)
///
/// # Postconditions
///
/// - `dataset`: **Guaranteed** (replaced with LeRobot v2.1 format under `/lerobot_v21` topic)
/// - Output files: **Guaranteed** (parquet, videos, and metadata files written to `{outdir}/{uuid}/`)
///
/// # Errors
///
/// - [`StageError::MissingData`]: `dataset` not set, `airoa_metadata` not set, `fps` not set
///   and cannot be inferred, `video_registry` missing when `image_data` is absent
/// - [`StageError::Io`]: Robot model file read failure, output file write failure
/// - [`StageError::InvalidData`]: No segments overlap with the synchronized timeline,
///   task index not registered, label index not found, robot_model references missing
///   topics, unexpected column dtype
/// - [`StageError::External`]: Segment assembly failure, video encoding failure
pub struct LeRobotV21Transformer {
    outdir: Utf8PathBuf,
    robot_model: RobotModelSource,
    metadata: LeRobotMetadata,
    video_config: VideoEncoderConfig,
    separate_per_primitive: bool,
}

impl LeRobotV21Transformer {
    pub fn new(config: LeRobotV21TransformerConfig) -> Self {
        let outdir = Utf8PathBuf::from(config.outdir.clone());
        Self {
            outdir,
            robot_model: config.robot_model,
            metadata: LeRobotMetadata::default(),
            video_config: config.video_config,
            separate_per_primitive: config.separate_per_primitive,
        }
    }

    pub fn outdir(&self) -> &Utf8PathBuf {
        &self.outdir
    }
}

impl Stage for LeRobotV21Transformer {
    fn name(&self) -> &'static str {
        "lerobot_v21_transformer"
    }

    fn run(&mut self, context: Context) -> Result<Context, StageError> {
        execute_pipeline(
            context,
            &mut self.metadata,
            &self.video_config,
            &self.outdir,
            &self.robot_model,
            self.separate_per_primitive,
        )
    }
}

fn execute_pipeline(
    mut context: Context,
    metadata: &mut LeRobotMetadata,
    video_config: &VideoEncoderConfig,
    base_outdir: &Utf8Path,
    robot_model: &RobotModelSource,
    separate_per_primitive: bool,
) -> Result<Context, StageError> {
    context.populate_image_topic_shapes_from_video_registry()?;
    let dataset = context.dataset.take().or_missing("dataset in context")?;

    // Take ownership of metadata and convert to V2.0 format.
    // This is where V1.3 -> V2.0 conversion happens (if metadata was V1.3).
    let airoa_metadata = context
        .take_airoa_metadata()
        .or_missing("airoa_metadata in context (did Rosbag2Ingestor load meta.json?)")?
        .into_v2_0()?;

    // Create UUID subdirectory for this rosbag
    let uuid_string = airoa_metadata.uuid.to_string();
    let outdir = base_outdir.join(&uuid_string);

    // Register labels as tasks in LeRobot metadata
    register_labels(metadata, &airoa_metadata.labels);

    // Also register episode label as a task (for composite/SHT task index)
    if !airoa_metadata.episode.label.is_empty() {
        register_labels(
            metadata,
            std::slice::from_ref(&airoa_metadata.episode.label),
        );
    }

    let topic_feature_map = robot_model
        .resolve()
        .map_err(|e| StageError::io("failed to resolve robot model".to_string(), e))?;
    validate_required_topics(&topic_feature_map, &dataset)?;

    // In V2_0, segments are already non-composite (composite is in episode field)
    let (timeline_start, timeline_end) =
        synched_timestamp_range(&dataset)?.or_missing("synched_timestamp_ns values in dataset")?;

    let indexed_segments = filter_segments_with_indices_within_range(
        &airoa_metadata.segments,
        timeline_start,
        timeline_end,
    );
    if indexed_segments.is_empty() {
        let message = format!(
            "no segments overlap with the synchronized timeline produced by rebake \
uuid: {}, timeline_start: {}, timeline_end: {}",
            uuid_string, timeline_start, timeline_end
        );
        return Err(StageError::invalid(message));
    }

    let frame_spacing = TimelineFormatter::derive_frame_spacing(&dataset)?;
    let fps = context
        .fps()
        .or_else(|| {
            if frame_spacing > 0.0 {
                Some((1.0_f64 / frame_spacing).round().max(1.0) as usize)
            } else {
                None
            }
        })
        .or_missing("fps (not set and cannot be inferred from dataset)")?;

    // Choose execution mode based on separate_per_primitive flag
    if separate_per_primitive {
        execute_pa_pipeline(
            context,
            metadata,
            video_config,
            &outdir,
            &topic_feature_map,
            &dataset,
            &indexed_segments,
            &airoa_metadata,
            frame_spacing,
            fps,
        )
    } else {
        let segments = indexed_segments
            .iter()
            .map(|(_, segment)| segment.clone())
            .collect::<Vec<_>>();
        execute_sht_pipeline(
            context,
            metadata,
            video_config,
            &outdir,
            &topic_feature_map,
            &dataset,
            &segments,
            &airoa_metadata,
            frame_spacing,
            fps,
        )
    }
}

/// Execute SHT (Short Horizon Task) mode: combine all segments into a single episode.
#[allow(clippy::too_many_arguments)]
fn execute_sht_pipeline(
    mut context: Context,
    metadata: &mut LeRobotMetadata,
    video_config: &VideoEncoderConfig,
    outdir: &Utf8Path,
    topic_feature_map: &TopicFeatureMap,
    dataset: &HashMap<String, LazyFrame>,
    segments: &[Segment],
    airoa_metadata: &MetadataV2_0,
    frame_spacing: f64,
    fps: usize,
) -> Result<Context, StageError> {
    let video_entries = collect_video_entries(topic_feature_map);
    let assembler = SegmentAssembler::new(topic_feature_map);
    let mut video_pipeline =
        VideoEncoderPipeline::new(outdir, video_config.clone(), &topic_feature_map.map);

    // Initialize FrameProvider
    // If image_data is present in context, use InMemoryFrameProvider.
    // Otherwise, use VideoFileFrameProvider to read from video files.
    let mut video_file_provider;
    let mut in_memory_provider;
    let frame_provider: &mut dyn FrameProvider = if let Some(image_data) = &context.image_data {
        in_memory_provider = InMemoryFrameProvider::new(image_data);
        &mut in_memory_provider
    } else {
        let video_registry = context.video_registry().ok_or_else(|| {
            StageError::missing("video_registry in context (did a video encoder run?)")
        })?;
        video_file_provider =
            VideoFileFrameProvider::new(context.bundle_root().cloned(), video_registry.clone());
        &mut video_file_provider
    };

    // Get composite task index from episode label
    let composite_task_idx = episode_task_index(metadata, &airoa_metadata.episode);

    let mut segment_frames = Vec::with_capacity(segments.len());
    for segment in segments {
        let task_name = find_label(&airoa_metadata.labels, segment.label_idx)?;
        let task_index = metadata
            .get_task_index(task_name)
            .copied()
            .ok_or_else(|| StageError::invalid("task index is not registered"))?
            as i64;

        let mut segment_frame = assembler
            .assemble(dataset, segment)
            .map_err(|e| StageError::external_boxed("failed to assemble segment", e))?;

        encode_video_frames(
            &mut video_pipeline,
            &video_entries,
            &segment_frame,
            frame_provider,
        )?;

        let annotations = build_segment_annotations(
            segment,
            segment_frame.height(),
            task_index,
            composite_task_idx,
        )?;
        segment_frame = segment_frame.hstack(annotations.get_columns())?;

        segment_frames.push(segment_frame);
    }

    let mut joined = concatenate_segment_frames(segment_frames)?;
    TimelineFormatter::append_axes(&mut joined, frame_spacing)?;
    TimelineFormatter::remove_image_index_columns(&mut joined)?;

    let video_stats = video_pipeline.finalize()?;

    let mut parquet_df = joined.clone();
    DatasetWriter::write_parquet(
        outdir,
        DEFAULT_CHUNK_ID,
        DEFAULT_EPISODE_ID,
        &mut parquet_df,
    )?;
    let last_segment_success = segments
        .last()
        .map(|segment| segment.success)
        .unwrap_or(true);

    let mut composer = MetadataComposer::new(
        metadata,
        topic_feature_map,
        context.image_data.as_ref(),
        context.image_topic_shapes.as_ref(),
        video_config,
        fps,
    );
    let lerobot_tasks = composer.update_info(
        &joined,
        &video_stats,
        outdir,
        airoa_metadata,
        DATA_PATH_TEMPLATE,
        VIDEO_PATH_TEMPLATE,
        LEROBOT_VERSION,
        video_stats.len(),
    )?;
    let episode = composer.build_episode(
        &joined,
        airoa_metadata,
        &lerobot_tasks,
        last_segment_success,
        METADATA_VERSION,
        DEFAULT_EPISODE_LABEL,
        DEFAULT_TASK_TYPE_SHT,
    )?;
    DatasetWriter::write_episode_stats(&joined, outdir, &video_stats, &episode.episode_id)?;
    episode.save(outdir)?;

    let lerobot_dataset = HashMap::from([("/lerobot_v21".to_string(), joined.lazy())]);
    context.set_dataset(lerobot_dataset);

    Ok(context)
}

/// Execute PA (Primitive Action) mode: generate separate episodes for each segment.
#[allow(clippy::too_many_arguments)]
fn execute_pa_pipeline(
    mut context: Context,
    metadata: &mut LeRobotMetadata,
    video_config: &VideoEncoderConfig,
    outdir: &Utf8Path,
    topic_feature_map: &TopicFeatureMap,
    dataset: &HashMap<String, LazyFrame>,
    indexed_segments: &[(usize, Segment)],
    airoa_metadata: &MetadataV2_0,
    frame_spacing: f64,
    fps: usize,
) -> Result<Context, StageError> {
    let video_entries = collect_video_entries(topic_feature_map);
    let assembler = SegmentAssembler::new(topic_feature_map);

    // Initialize FrameProvider
    let mut video_file_provider;
    let mut in_memory_provider;
    let frame_provider: &mut dyn FrameProvider = if let Some(image_data) = &context.image_data {
        in_memory_provider = InMemoryFrameProvider::new(image_data);
        &mut in_memory_provider
    } else {
        let video_registry = context.video_registry().ok_or_else(|| {
            StageError::missing("video_registry in context (did a video encoder run?)")
        })?;
        video_file_provider =
            VideoFileFrameProvider::new(context.bundle_root().cloned(), video_registry.clone());
        &mut video_file_provider
    };

    // Get composite task index from episode label
    let composite_task_idx = episode_task_index(metadata, &airoa_metadata.episode);

    // Accumulators for cumulative statistics
    let mut global_frame_offset: usize = 0;
    let mut total_frames: usize = 0;
    let mut total_videos: usize = 0;
    // Store video stats per episode for episodes_stats.jsonl (not merged)
    let mut episode_video_stats_list: Vec<
        HashMap<String, crate::transform::lerobot_v21::video::VideoStats>,
    > = Vec::new();
    // Store merged video stats for info.json (needs aggregated stats for features)
    let mut all_video_stats: HashMap<String, crate::transform::lerobot_v21::video::VideoStats> =
        HashMap::new();
    let mut all_episode_frames: Vec<DataFrame> = Vec::new();
    let mut all_episodes: Vec<Episodes> = Vec::new();

    // Process each segment as a separate episode
    for (episode_index, (source_segment_index, segment)) in indexed_segments.iter().enumerate() {
        let task_name = find_label(&airoa_metadata.labels, segment.label_idx)?;
        let task_index = metadata
            .get_task_index(task_name)
            .copied()
            .ok_or_else(|| StageError::invalid("task index is not registered"))?
            as i64;

        // Assemble segment data
        let mut segment_frame = assembler
            .assemble(dataset, segment)
            .map_err(|e| StageError::external_boxed("failed to assemble segment", e))?;

        // Create new video pipeline for each episode (design choice B from PA_MODE_IMPLEMENTATION.md)
        let mut video_pipeline = VideoEncoderPipeline::new_with_episode_id(
            outdir,
            video_config.clone(),
            &topic_feature_map.map,
            episode_index,
        );

        encode_video_frames(
            &mut video_pipeline,
            &video_entries,
            &segment_frame,
            frame_provider,
        )?;

        // Build PA-mode annotations (no next.done between segments, each episode ends with done=true)
        let annotations = build_pa_segment_annotations(
            segment,
            segment_frame.height(),
            task_index,
            composite_task_idx,
        )?;
        segment_frame = segment_frame.hstack(annotations.get_columns())?;

        // Add timeline axes for this episode (episode-local frame_index, global index)
        TimelineFormatter::append_axes_for_episode(
            &mut segment_frame,
            frame_spacing,
            episode_index,
            global_frame_offset,
        )?;
        TimelineFormatter::remove_image_index_columns(&mut segment_frame)?;

        let episode_video_stats = video_pipeline.finalize()?;

        // Write episode parquet file
        let episode_id = format!("{:06}", episode_index);
        let mut parquet_df = segment_frame.clone();
        DatasetWriter::write_parquet(outdir, DEFAULT_CHUNK_ID, &episode_id, &mut parquet_df)?;

        // Accumulate statistics
        let episode_frame_count = segment_frame.height();
        global_frame_offset += episode_frame_count;
        total_frames += episode_frame_count;
        total_videos += episode_video_stats.len();

        // Store episode video stats for episodes_stats.jsonl
        episode_video_stats_list.push(episode_video_stats.clone());

        // Merge video stats for info.json (aggregated across all episodes)
        for (key, stats) in &episode_video_stats {
            all_video_stats.insert(key.clone(), stats.clone());
        }

        // Build episode metadata (will be written at the end)
        // Clone tasks before creating composer to avoid borrow conflict
        let lerobot_tasks =
            crate::transform::lerobot_v21::lerobot_dataset_metadata::LeRobotTasksVec::from(
                metadata.tasks.clone(),
            );
        let composer = MetadataComposer::new(
            metadata,
            topic_feature_map,
            context.image_data.as_ref(),
            context.image_topic_shapes.as_ref(),
            video_config,
            fps,
        );
        let episode = composer.build_episode_for_pa(
            &segment_frame,
            airoa_metadata,
            &lerobot_tasks,
            segment.success,
            METADATA_VERSION,
            DEFAULT_EPISODE_LABEL,
            DEFAULT_TASK_TYPE_PA,
            episode_index,
            *source_segment_index,
            task_name,
        )?;
        all_episodes.push(episode);

        all_episode_frames.push(segment_frame);
    }

    // Write all episodes to episodes.jsonl (overwrites existing file)
    Episodes::save_all(&all_episodes, outdir)?;

    // Write per-episode stats to episodes_stats.jsonl
    let episode_stats_refs: Vec<_> = all_episode_frames
        .iter()
        .zip(episode_video_stats_list.iter())
        .zip(all_episodes.iter())
        .enumerate()
        .map(|(idx, ((df, vs), episode))| (idx, episode.episode_id.as_str(), df, vs))
        .collect();
    DatasetWriter::write_episode_stats_all(&episode_stats_refs, outdir)?;

    // Update info.json with cumulative totals
    let mut composer = MetadataComposer::new(
        metadata,
        topic_feature_map,
        context.image_data.as_ref(),
        context.image_topic_shapes.as_ref(),
        video_config,
        fps,
    );

    // Use first episode frame as reference for features (all episodes have same schema)
    let reference_frame = all_episode_frames
        .first()
        .ok_or_else(|| StageError::invalid("no episode frames generated"))?;

    composer.update_info_for_pa(
        reference_frame,
        &all_video_stats,
        outdir,
        airoa_metadata,
        DATA_PATH_TEMPLATE,
        VIDEO_PATH_TEMPLATE,
        LEROBOT_VERSION,
        indexed_segments.len(),
        total_frames,
        total_videos,
    )?;

    // Combine all episode frames for context output
    let combined = concatenate_segment_frames(all_episode_frames)?;
    let lerobot_dataset = HashMap::from([("/lerobot_v21".to_string(), combined.lazy())]);
    context.set_dataset(lerobot_dataset);

    Ok(context)
}

fn collect_video_entries(topic_feature_map: &TopicFeatureMap) -> Vec<(String, String)> {
    topic_feature_map
        .map
        .iter()
        .filter_map(|entry| {
            if let TopicFeatureMapEntry::Video { topic, feature, .. } = entry {
                Some((topic.clone(), feature.clone()))
            } else {
                None
            }
        })
        .collect()
}

fn encode_video_frames(
    pipeline: &mut VideoEncoderPipeline,
    video_entries: &[(String, String)],
    frame: &DataFrame,
    frame_provider: &mut dyn FrameProvider,
) -> Result<(), StageError> {
    for (topic, feature) in video_entries {
        if !pipeline.contains_topic(topic) {
            continue;
        }

        // We expect the provider to have this topic since it's in the pipeline config.
        // If missing, get_frame() will return an error.

        let column_name = format!("image_index_{feature}");
        let column = match frame.column(&column_name) {
            Ok(col) => col,
            Err(_) => continue,
        };

        // SAFETY: The dtype is checked in the match arm, so the downcast is guaranteed to succeed.
        #[allow(clippy::expect_used)]
        let indices: Vec<usize> = match column.dtype() {
            DataType::UInt32 => column
                .u32()
                .expect("dtype verified as UInt32")
                .into_iter()
                .flatten()
                .map(|v| v as usize)
                .collect(),
            DataType::UInt64 => column
                .u64()
                .expect("dtype verified as UInt64")
                .into_iter()
                .flatten()
                .map(|v| v as usize)
                .collect(),
            DataType::Int64 => column
                .i64()
                .expect("dtype verified as Int64")
                .into_iter()
                .flatten()
                .map(|v| v as usize)
                .collect(),
            other => {
                return Err(StageError::invalid(format!(
                    "unexpected index dtype for video column: {other:?}"
                )));
            }
        };

        if indices.is_empty() {
            continue;
        }

        pipeline
            .encode(topic, &indices, frame_provider)
            .map_err(|e| StageError::external_boxed("failed to encode video frame", e))?;
    }

    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    use crate::schema::metadata::parse_metadata;
    use crate::schema::{TopicFeatureMap, TopicFeatureMapEntry};
    use crate::transform::lerobot_v21::io::DatasetWriter;
    use polars::{
        df,
        prelude::{DataType, IntoLazy, PlSmallStr, Series},
    };

    #[test]
    fn dataset_writer_generates_stats_file() {
        let numeric = Series::new("numeric".into(), &[1.0_f64, 3.0, 5.0]);
        let flag = Series::new("flag".into(), &[true, false, true]);

        let mut builder = polars::chunked_array::builder::get_list_builder(
            &DataType::Float64,
            3,
            2,
            PlSmallStr::from("vector"),
        );
        builder
            .append_series(&Series::new(PlSmallStr::EMPTY, &[1.0, 2.0]))
            .unwrap();
        builder
            .append_series(&Series::new(PlSmallStr::EMPTY, &[3.0, 4.0]))
            .unwrap();
        builder
            .append_series(&Series::new(PlSmallStr::EMPTY, &[5.0, 6.0]))
            .unwrap();
        let vector = builder.finish().into_series();

        let episode_index = Series::new("episode_index".into(), &[0_i64, 0, 0]);

        let df = DataFrame::new(vec![
            numeric.into(),
            flag.into(),
            vector.into(),
            episode_index.into(),
        ])
        .unwrap();

        let temp_dir = tempfile::tempdir().unwrap();
        let outdir = camino::Utf8PathBuf::from_path_buf(temp_dir.path().to_path_buf()).unwrap();
        let video_stats = std::collections::HashMap::new();
        DatasetWriter::write_episode_stats(&df, &outdir, &video_stats, "episode-id-0").unwrap();

        let stats_path = outdir.join("meta/episodes_stats.jsonl");
        let content = std::fs::read_to_string(stats_path.as_std_path()).unwrap();
        let line = content.lines().next().unwrap();
        let json: serde_json::Value = serde_json::from_str(line).unwrap();
        assert_eq!(json["episode_index"].as_i64().unwrap(), 0);
        assert_eq!(json["episode_id"].as_str().unwrap(), "episode-id-0");
        let stats = json["stats"].as_object().unwrap();

        let numeric_stats = stats.get("numeric").unwrap();
        let numeric_min = numeric_stats["min"].as_array().unwrap();
        assert_eq!(numeric_min.len(), 1);
        assert!((numeric_min[0].as_f64().unwrap() - 1.0).abs() < 1e-12);
    }

    #[test]
    fn dataset_writer_writes_episode_stats_row_even_when_stats_are_empty() {
        let df = df! { "note" => ["metadata-only"] }.unwrap();
        let temp_dir = tempfile::tempdir().unwrap();
        let outdir = camino::Utf8PathBuf::from_path_buf(temp_dir.path().to_path_buf()).unwrap();
        let video_stats = std::collections::HashMap::new();
        let episodes = vec![(42_usize, "episode-id-42", &df, &video_stats)];

        DatasetWriter::write_episode_stats_all(&episodes, &outdir).unwrap();

        let stats_path = outdir.join("meta/episodes_stats.jsonl");
        let content = std::fs::read_to_string(stats_path.as_std_path()).unwrap();
        let lines = content.lines().collect::<Vec<_>>();
        assert_eq!(lines.len(), 1);

        let json: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(json["episode_index"].as_u64().unwrap(), 42);
        assert_eq!(json["episode_id"].as_str().unwrap(), "episode-id-42");
        assert!(json["stats"].as_object().unwrap().is_empty());
    }

    fn get_hsr_lerobot_v21_topic_feature_map() -> TopicFeatureMap {
        TopicFeatureMap {
            map: vec![
                TopicFeatureMapEntry::Parquet {
                    topic: "/hsrb/arm_trajectory_controller/command".to_string(),
                    field: "/points/0/positions".to_string(),
                    feature: "action.arm".to_string(),
                    names: Some(vec![
                        "arm_lift_joint".to_string(),
                        "arm_flex_joint".to_string(),
                        "arm_roll_joint".to_string(),
                        "wrist_flex_joint".to_string(),
                        "wrist_roll_joint".to_string(),
                    ]),
                    description: Some("absolute action for arm joints".to_string()),
                },
                TopicFeatureMapEntry::Parquet {
                    topic: "/hsrb/arm_trajectory_controller/command".to_string(),
                    field: "/is_fresh".to_string(),
                    feature: "action.arm.is_fresh".to_string(),
                    names: None,
                    description: None,
                },
                TopicFeatureMapEntry::Parquet {
                    topic: "/hsrb/gripper_controller/command".to_string(),
                    field: "/points/0/positions".to_string(),
                    feature: "action.gripper".to_string(),
                    names: Some(vec!["hand_motor_joint".to_string()]),
                    description: Some("absolute action for gripper".to_string()),
                },
                TopicFeatureMapEntry::Parquet {
                    topic: "/hsrb/gripper_controller/command".to_string(),
                    field: "/is_fresh".to_string(),
                    feature: "action.gripper.is_fresh".to_string(),
                    names: None,
                    description: None,
                },
                TopicFeatureMapEntry::Parquet {
                    topic: "/hsrb/head_trajectory_controller/command".to_string(),
                    field: "/points/0/positions".to_string(),
                    feature: "action.head".to_string(),
                    names: Some(vec![
                        "head_pan_joint".to_string(),
                        "head_tilt_joint".to_string(),
                    ]),
                    description: Some("absolute action for head joints".to_string()),
                },
                TopicFeatureMapEntry::Parquet {
                    topic: "/hsrb/head_trajectory_controller/command".to_string(),
                    field: "/is_fresh".to_string(),
                    feature: "action.head.is_fresh".to_string(),
                    names: None,
                    description: None,
                },
                TopicFeatureMapEntry::Parquet {
                    topic: "/hsrb/command_velocity".to_string(),
                    field: "/linear".to_string(),
                    feature: "action.base".to_string(),
                    names: Some(vec![
                        "base_x".to_string(),
                        "base_y".to_string(),
                        "base_t".to_string(),
                    ]),
                    description: Some("delta action for base".to_string()),
                },
                TopicFeatureMapEntry::Parquet {
                    topic: "/hsrb/command_velocity".to_string(),
                    field: "/is_fresh".to_string(),
                    feature: "action.base.is_fresh".to_string(),
                    names: None,
                    description: None,
                },
                TopicFeatureMapEntry::Parquet {
                    topic: "/tf_chain".to_string(),
                    field: "/base_link/hand_palm_link/transform".to_string(),
                    feature: "action.absolute".to_string(),
                    names: Some(vec![
                        "x".to_string(),
                        "y".to_string(),
                        "z".to_string(),
                        "qx".to_string(),
                        "qy".to_string(),
                        "qz".to_string(),
                        "qw".to_string(),
                    ]),
                    description: Some(
                        "absolute pose of the end effector relative to base_link".to_string(),
                    ),
                },
                TopicFeatureMapEntry::Parquet {
                    topic: "/tf_chain".to_string(),
                    field: "/is_fresh".to_string(),
                    feature: "action.absolute.is_fresh".to_string(),
                    names: None,
                    description: None,
                },
                TopicFeatureMapEntry::Parquet {
                    topic: "/tf_chain".to_string(),
                    field: "/base_link/hand_palm_link/delta_transform".to_string(),
                    feature: "action.relative".to_string(),
                    names: Some(vec![
                        "x".to_string(),
                        "y".to_string(),
                        "z".to_string(),
                        "qx".to_string(),
                        "qy".to_string(),
                        "qz".to_string(),
                        "qw".to_string(),
                    ]),
                    description: Some(
                        "delta pose of the end effector relative to base_link".to_string(),
                    ),
                },
                TopicFeatureMapEntry::Parquet {
                    topic: "/tf_chain".to_string(),
                    field: "/is_fresh".to_string(),
                    feature: "action.relative.is_fresh".to_string(),
                    names: None,
                    description: None,
                },
                TopicFeatureMapEntry::Parquet {
                    topic: "/hsrb/joint_states".to_string(),
                    field: "/position".to_string(),
                    feature: "observation.state".to_string(),
                    names: Some(vec![
                        "arm_flex_joint".to_string(),
                        "arm_lift_joint".to_string(),
                        "arm_roll_joint".to_string(),
                        "base_l_drive_wheel_joint".to_string(),
                        "base_r_drive_wheel_joint".to_string(),
                        "base_roll_joint".to_string(),
                        "hand_l_spring_proximal_joint".to_string(),
                        "hand_motor_joint".to_string(),
                        "hand_r_spring_proximal_joint".to_string(),
                        "head_pan_joint".to_string(),
                        "head_tilt_joint".to_string(),
                        "wrist_flex_joint".to_string(),
                        "wrist_roll_joint".to_string(),
                    ]),
                    description: None,
                },
                TopicFeatureMapEntry::Parquet {
                    topic: "/hsrb/joint_states".to_string(),
                    field: "/is_fresh".to_string(),
                    feature: "observation.state.is_fresh".to_string(),
                    names: None,
                    description: None,
                },
                TopicFeatureMapEntry::Parquet {
                    topic: "/hsrb/wrist_wrench/raw".to_string(),
                    field: "/wrench".to_string(),
                    feature: "observation.wrist.wrench".to_string(),
                    names: Some(vec![
                        "force_x".to_string(),
                        "force_y".to_string(),
                        "force_z".to_string(),
                        "torque_x".to_string(),
                        "torque_y".to_string(),
                        "torque_z".to_string(),
                    ]),
                    description: Some("Wrist wrench data (force and torque) flattened".to_string()),
                },
                TopicFeatureMapEntry::Video {
                    topic: "/hsrb/hand_camera/image_raw/compressed".to_string(),
                    feature: "observation.image.hand".to_string(),
                    names: Some(vec![
                        "height".to_string(),
                        "width".to_string(),
                        "channel".to_string(),
                    ]),
                    description: None,
                },
                TopicFeatureMapEntry::Parquet {
                    topic: "/hsrb/hand_camera/image_raw/compressed".to_string(),
                    field: "/is_fresh".to_string(),
                    feature: "observation.image.hand.is_fresh".to_string(),
                    names: None,
                    description: None,
                },
                TopicFeatureMapEntry::Video {
                    topic: "/hsrb/head_rgbd_sensor/rgb/image_rect_color/compressed".to_string(),
                    feature: "observation.image.head".to_string(),
                    names: Some(vec![
                        "height".to_string(),
                        "width".to_string(),
                        "channel".to_string(),
                    ]),
                    description: None,
                },
                TopicFeatureMapEntry::Parquet {
                    topic: "/hsrb/head_rgbd_sensor/rgb/image_rect_color/compressed".to_string(),
                    field: "/is_fresh".to_string(),
                    feature: "observation.image.head.is_fresh".to_string(),
                    names: None,
                    description: None,
                },
                TopicFeatureMapEntry::Parquet {
                    topic: "/tf_chain".to_string(),
                    field: "/base_link/hand_palm_link/transform".to_string(),
                    feature: "observation.end_effector_pose.absolute".to_string(),
                    names: Some(vec![
                        "x".to_string(),
                        "y".to_string(),
                        "z".to_string(),
                        "qx".to_string(),
                        "qy".to_string(),
                        "qz".to_string(),
                        "qw".to_string(),
                    ]),
                    description: None,
                },
                TopicFeatureMapEntry::Parquet {
                    topic: "/tf_chain".to_string(),
                    field: "/base_link/hand_palm_link/delta_transform".to_string(),
                    feature: "observation.end_effector_pose.relative".to_string(),
                    names: Some(vec![
                        "x".to_string(),
                        "y".to_string(),
                        "z".to_string(),
                        "qx".to_string(),
                        "qy".to_string(),
                        "qz".to_string(),
                        "qw".to_string(),
                    ]),
                    description: None,
                },
            ],
        }
    }

    #[test]
    fn test_get_hsr_lerobot_v21_topic_feature_map() {
        let topic_feature_map = get_hsr_lerobot_v21_topic_feature_map();
        let expected_topic_feature_map_string = r#"
[
    {
      "type": "Parquet",
      "topic": "/hsrb/arm_trajectory_controller/command",
      "field": "/points/0/positions",
      "feature": "action.arm",
      "names": [
        "arm_lift_joint",
        "arm_flex_joint",
        "arm_roll_joint",
        "wrist_flex_joint",
        "wrist_roll_joint"
      ],
      "description": "absolute action for arm joints"
    },
    {
      "type": "Parquet",
      "topic": "/hsrb/arm_trajectory_controller/command",
      "field": "/is_fresh",
      "feature": "action.arm.is_fresh"
    },
    {
      "type": "Parquet",
      "topic": "/hsrb/gripper_controller/command",
      "field": "/points/0/positions",
      "feature": "action.gripper",
      "names": [
        "hand_motor_joint"
      ],
      "description": "absolute action for gripper"
    },
    {
      "type": "Parquet",
      "topic": "/hsrb/gripper_controller/command",
      "field": "/is_fresh",
      "feature": "action.gripper.is_fresh"
    },
    {
      "type": "Parquet",
      "topic": "/hsrb/head_trajectory_controller/command",
      "field": "/points/0/positions",
      "feature": "action.head",
      "names": [
        "head_pan_joint",
        "head_tilt_joint"
      ],
      "description": "absolute action for head joints"
    },
    {
      "type": "Parquet",
      "topic": "/hsrb/head_trajectory_controller/command",
      "field": "/is_fresh",
      "feature": "action.head.is_fresh"
    },
    {
      "type": "Parquet",
      "topic": "/hsrb/command_velocity",
      "field": "/linear",
      "feature": "action.base",
      "names": [
        "base_x",
        "base_y",
        "base_t"
      ],
      "description": "delta action for base"
    },
    {
      "type": "Parquet",
      "topic": "/hsrb/command_velocity",
      "field": "/is_fresh",
      "feature": "action.base.is_fresh"
    },
    {
      "type": "Parquet",
      "topic": "/tf_chain",
      "field": "/base_link/hand_palm_link/transform",
      "feature": "action.absolute",
      "names": [
        "x",
        "y",
        "z",
        "qx",
        "qy",
        "qz",
        "qw"
      ],
      "description": "absolute pose of the end effector relative to base_link"
    },
    {
      "type": "Parquet",
      "topic": "/tf_chain",
      "field": "/is_fresh",
      "feature": "action.absolute.is_fresh"
    },
    {
      "type": "Parquet",
      "topic": "/tf_chain",
      "field": "/base_link/hand_palm_link/delta_transform",
      "feature": "action.relative",
      "names": [
        "x",
        "y",
        "z",
        "qx",
        "qy",
        "qz",
        "qw"
      ],
      "description": "delta pose of the end effector relative to base_link"
    },
    {
      "type": "Parquet",
      "topic": "/tf_chain",
      "field": "/is_fresh",
      "feature": "action.relative.is_fresh"
    },
    {
      "type": "Parquet",
      "topic": "/hsrb/joint_states",
      "field": "/position",
      "feature": "observation.state",
      "names": [
        "arm_flex_joint",
        "arm_lift_joint",
        "arm_roll_joint",
        "base_l_drive_wheel_joint",
        "base_r_drive_wheel_joint",
        "base_roll_joint",
        "hand_l_spring_proximal_joint",
        "hand_motor_joint",
        "hand_r_spring_proximal_joint",
        "head_pan_joint",
        "head_tilt_joint",
        "wrist_flex_joint",
        "wrist_roll_joint"
      ]
    },
    {
      "type": "Parquet",
      "topic": "/hsrb/joint_states",
      "field": "/is_fresh",
      "feature": "observation.state.is_fresh"
    },
    {
      "type": "Parquet",
      "topic": "/hsrb/wrist_wrench/raw",
      "field": "/wrench",
      "feature": "observation.wrist.wrench",
      "names": [
        "force_x",
        "force_y",
        "force_z",
        "torque_x",
        "torque_y",
        "torque_z"
      ],
      "description": "Wrist wrench data (force and torque) flattened"
    },
    {
      "type": "Video",
      "topic": "/hsrb/hand_camera/image_raw/compressed",
      "feature": "observation.image.hand",
      "names": [
        "height",
        "width",
        "channel"
      ]
    },
    {
      "type": "Parquet",
      "topic": "/hsrb/hand_camera/image_raw/compressed",
      "field": "/is_fresh",
      "feature": "observation.image.hand.is_fresh"
    },
    {
      "type": "Video",
      "topic": "/hsrb/head_rgbd_sensor/rgb/image_rect_color/compressed",
      "feature": "observation.image.head",
      "names": [
        "height",
        "width",
        "channel"
      ]
    },
    {
      "type": "Parquet",
      "topic": "/hsrb/head_rgbd_sensor/rgb/image_rect_color/compressed",
      "field": "/is_fresh",
      "feature": "observation.image.head.is_fresh"
    },
    {
      "type": "Parquet",
      "topic": "/tf_chain",
      "field": "/base_link/hand_palm_link/transform",
      "feature": "observation.end_effector_pose.absolute",
      "names": [
        "x",
        "y",
        "z",
        "qx",
        "qy",
        "qz",
        "qw"
      ]
    },
    {
      "type": "Parquet",
      "topic": "/tf_chain",
      "field": "/base_link/hand_palm_link/delta_transform",
      "feature": "observation.end_effector_pose.relative",
      "names": [
        "x",
        "y",
        "z",
        "qx",
        "qy",
        "qz",
        "qw"
      ]
    }
]
"#;
        let expected_topic_feature_map: TopicFeatureMap =
            serde_json::from_str(expected_topic_feature_map_string).unwrap();
        assert_eq!(topic_feature_map, expected_topic_feature_map);
    }

    #[test]
    fn test_get_hsr_lerobot_v21_topic_feature_map_schema() {
        let schema = schemars::schema_for!(TopicFeatureMap);
        let expected_schema_string = r##"
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "title": "Array_of_TopicFeatureMapEntry",
  "type": "array",
  "items": {
    "$ref": "#/$defs/TopicFeatureMapEntry"
  },
  "$defs": {
    "TopicFeatureMapEntry": {
      "oneOf": [
        {
          "type": "object",
          "properties": {
            "description": {
              "description": "Description of the feature",
              "type": [
                "string",
                "null"
              ]
            },
            "feature": {
              "description": "LeRobot feature name",
              "type": "string"
            },
            "field": {
              "description": "JSON Pointer (RFC 6901) identifying the target field within the ROS topic message structure.\nExample: `/points/0/positions`.",
              "type": "string"
            },
            "names": {
              "description": "List of names for the feature dimensions",
              "type": [
                "array",
                "null"
              ],
              "items": {
                "type": "string"
              }
            },
            "topic": {
              "description": "ROS topic name",
              "type": "string"
            },
            "type": {
              "type": "string",
              "const": "Parquet"
            }
          },
          "required": [
            "type",
            "topic",
            "field",
            "feature"
          ]
        },
        {
          "type": "object",
          "properties": {
            "description": {
              "description": "Description of the feature",
              "type": [
                "string",
                "null"
              ]
            },
            "feature": {
              "description": "LeRobot feature name",
              "type": "string"
            },
            "names": {
              "description": "List of names for the feature dimensions",
              "type": [
                "array",
                "null"
              ],
              "items": {
                "type": "string"
              }
            },
            "topic": {
              "description": "ROS topic name",
              "type": "string"
            },
            "type": {
              "type": "string",
              "const": "Video"
            }
          },
          "required": [
            "type",
            "topic",
            "feature"
          ]
        },
        {
          "type": "object",
          "properties": {
            "description": {
              "description": "Description of the feature",
              "type": [
                "string",
                "null"
              ]
            },
            "feature": {
              "description": "LeRobot feature name",
              "type": "string"
            },
            "names": {
              "description": "List of names for the feature dimensions",
              "type": [
                "array",
                "null"
              ],
              "items": {
                "type": "string"
              }
            },
            "topic": {
              "description": "ROS topic name",
              "type": "string"
            },
            "type": {
              "type": "string",
              "const": "Image"
            }
          },
          "required": [
            "type",
            "topic",
            "feature"
          ]
        }
      ]
    }
  }
}
"##;
        let expected_schema: schemars::Schema =
            serde_json::from_str(expected_schema_string).unwrap();

        assert_eq!(schema, expected_schema);
    }

    #[test]
    fn test_get_hsr_lerobot_v21_topic_feature_map_yaml() {
        let topic_feature_map = get_hsr_lerobot_v21_topic_feature_map();
        let expected_topic_feature_map_string = r#"
- type: Parquet
  topic: /hsrb/arm_trajectory_controller/command
  field: /points/0/positions
  feature: action.arm
  names: [arm_lift_joint, arm_flex_joint, arm_roll_joint, wrist_flex_joint, wrist_roll_joint]
  description: absolute action for arm joints
- type: Parquet
  topic: /hsrb/arm_trajectory_controller/command
  field: /is_fresh
  feature: action.arm.is_fresh
- type: Parquet
  topic: /hsrb/gripper_controller/command
  field: /points/0/positions
  feature: action.gripper
  names: [hand_motor_joint]
  description: absolute action for gripper
- type: Parquet
  topic: /hsrb/gripper_controller/command
  field: /is_fresh
  feature: action.gripper.is_fresh
- type: Parquet
  topic: /hsrb/head_trajectory_controller/command
  field: /points/0/positions
  feature: action.head
  names: [head_pan_joint, head_tilt_joint]
  description: absolute action for head joints
- type: Parquet
  topic: /hsrb/head_trajectory_controller/command
  field: /is_fresh
  feature: action.head.is_fresh
- type: Parquet
  topic: /hsrb/command_velocity
  field: /linear
  feature: action.base
  names: [base_x, base_y, base_t]
  description: delta action for base
- type: Parquet
  topic: /hsrb/command_velocity
  field: /is_fresh
  feature: action.base.is_fresh
- type: Parquet
  topic: /tf_chain
  field: /base_link/hand_palm_link/transform
  feature: action.absolute
  names: [x, y, z, qx, qy, qz, qw]
  description: absolute pose of the end effector relative to base_link
- type: Parquet
  topic: /tf_chain
  field: /is_fresh
  feature: action.absolute.is_fresh
- type: Parquet
  topic: /tf_chain
  field: /base_link/hand_palm_link/delta_transform
  feature: action.relative
  names: [x, y, z, qx, qy, qz, qw]
  description: delta pose of the end effector relative to base_link
- type: Parquet
  topic: /tf_chain
  field: /is_fresh
  feature: action.relative.is_fresh
- type: Parquet
  topic: /hsrb/joint_states
  field: /position
  feature: observation.state
  names: [arm_flex_joint, arm_lift_joint, arm_roll_joint, base_l_drive_wheel_joint, base_r_drive_wheel_joint, base_roll_joint, hand_l_spring_proximal_joint, hand_motor_joint, hand_r_spring_proximal_joint, head_pan_joint, head_tilt_joint, wrist_flex_joint, wrist_roll_joint]
- type: Parquet
  topic: /hsrb/joint_states
  field: /is_fresh
  feature: observation.state.is_fresh
- type: Parquet
  topic: /hsrb/wrist_wrench/raw
  field: /wrench
  feature: observation.wrist.wrench
  names: [force_x, force_y, force_z, torque_x, torque_y, torque_z]
  description: Wrist wrench data (force and torque) flattened
- type: Video
  topic: /hsrb/hand_camera/image_raw/compressed
  feature: observation.image.hand
  names: [height, width, channel]
- type: Parquet
  topic: /hsrb/hand_camera/image_raw/compressed
  field: /is_fresh
  feature: observation.image.hand.is_fresh
- type: Video
  topic: /hsrb/head_rgbd_sensor/rgb/image_rect_color/compressed
  feature: observation.image.head
  names: [height, width, channel]
- type: Parquet
  topic: /hsrb/head_rgbd_sensor/rgb/image_rect_color/compressed
  field: /is_fresh
  feature: observation.image.head.is_fresh
- type: Parquet
  topic: /tf_chain
  field: /base_link/hand_palm_link/transform
  feature: observation.end_effector_pose.absolute
  names: [x, y, z, qx, qy, qz, qw]
- type: Parquet
  topic: /tf_chain
  field: /base_link/hand_palm_link/delta_transform
  feature: observation.end_effector_pose.relative
  names: [x, y, z, qx, qy, qz, qw]
"#;
        let expected_topic_feature_map: TopicFeatureMap =
            serde_yaml::from_str(expected_topic_feature_map_string).unwrap();

        assert_eq!(topic_feature_map, expected_topic_feature_map);
    }

    #[test]
    fn test_get_hsr_lerobot_v21_topic_feature_map_yaml_schema() {
        let expected_schema_string = r##"
$schema: https://json-schema.org/draft/2020-12/schema
title: Array_of_TopicFeatureMapEntry
type: array
items:
  $ref: '#/$defs/TopicFeatureMapEntry'
$defs:
  TopicFeatureMapEntry:
    oneOf:
    - type: object
      properties:
        description:
          description: Description of the feature
          type:
          - string
          - 'null'
        feature:
          description: LeRobot feature name
          type: string
        field:
          description: |-
            JSON Pointer (RFC 6901) identifying the target field within the ROS topic message structure.
            Example: `/points/0/positions`.
          type: string
        names:
          description: List of names for the feature dimensions
          type:
          - array
          - 'null'
          items:
            type: string
        topic:
          description: ROS topic name
          type: string
        type:
          type: string
          const: Parquet
      required:
      - type
      - topic
      - field
      - feature
    - type: object
      properties:
        description:
          description: Description of the feature
          type:
          - string
          - 'null'
        feature:
          description: LeRobot feature name
          type: string
        names:
          description: List of names for the feature dimensions
          type:
          - array
          - 'null'
          items:
            type: string
        topic:
          description: ROS topic name
          type: string
        type:
          type: string
          const: Video
      required:
      - type
      - topic
      - feature
    - type: object
      properties:
        description:
          description: Description of the feature
          type:
          - string
          - 'null'
        feature:
          description: LeRobot feature name
          type: string
        names:
          description: List of names for the feature dimensions
          type:
          - array
          - 'null'
          items:
            type: string
        topic:
          description: ROS topic name
          type: string
        type:
          type: string
          const: Image
      required:
      - type
      - topic
      - feature
"##;
        let expected_schema_value: serde_yaml::Value =
            serde_yaml::from_str(expected_schema_string).unwrap();

        let schema = schemars::schema_for!(TopicFeatureMap);
        let yaml_schema = serde_yaml::to_string(&schema).unwrap();
        let actual_schema_value: serde_yaml::Value = serde_yaml::from_str(&yaml_schema).unwrap();

        assert_eq!(actual_schema_value, expected_schema_value);
    }

    /// Error case: returns MissingData error when dataset is not set
    #[test]
    fn test_transformer_fails_without_dataset() {
        use tempfile::tempdir;

        let dir = tempdir().unwrap();
        let outdir = dir.path().to_str().unwrap();

        // CARGO_MANIFEST_DIR points to rebake/, so ../config/... goes one level up
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let robot_model_path = format!("{}/../config/robot_model/yubi.yaml", manifest_dir);

        let config =
            LeRobotV21TransformerConfig::new(outdir, RobotModelSource::Path(robot_model_path));
        let mut stage = config.build();

        let context = Context::default(); // dataset is None

        let result = stage.run(context);

        assert!(result.is_err());
        let err = result.err().unwrap();
        assert!(matches!(err, StageError::MissingData(_)));
    }

    /// Error case: returns MissingData error when airoa_metadata is not set
    #[test]
    fn test_transformer_fails_without_metadata() {
        use std::collections::HashMap;
        use tempfile::tempdir;

        let dir = tempdir().unwrap();
        let outdir = dir.path().to_str().unwrap();

        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let robot_model_path = format!("{}/../config/robot_model/yubi.yaml", manifest_dir);

        let config =
            LeRobotV21TransformerConfig::new(outdir, RobotModelSource::Path(robot_model_path));
        let mut stage = config.build();

        let context = Context {
            dataset: Some(HashMap::new()), // dataset exists but airoa_metadata is missing
            ..Default::default()
        };

        let result = stage.run(context);

        assert!(result.is_err());
        let err = result.err().unwrap();
        assert!(matches!(err, StageError::MissingData(_)));
    }

    #[test]
    fn test_transformer_fails_when_robot_model_topics_are_missing() {
        use std::collections::HashMap;
        use tempfile::tempdir;

        let dir = tempdir().unwrap();
        let outdir = dir.path().to_str().unwrap();

        let config = LeRobotV21TransformerConfig::new(
            outdir,
            RobotModelSource::Inline(TopicFeatureMap {
                map: vec![TopicFeatureMapEntry::Parquet {
                    topic: "/missing_topic".to_string(),
                    field: "/position".to_string(),
                    feature: "action.ee_joint_command".to_string(),
                    names: Some(vec!["right_hand_joint1".to_string()]),
                    description: None,
                }],
            }),
        );
        let mut stage = config.build();

        let metadata_json = r#"
        {
            "$schema": "https://example.com/schema/v2_0.json",
            "schema_version": "2.0",
            "uuid": "123e4567-e89b-12d3-a456-426614174000",
            "robot": {
                "uri": null,
                "type": "test_robot",
                "id": "test-robot-1",
                "checksum": null
            },
            "files": [
                {
                    "type": "mcap",
                    "name": "recording.mcap",
                    "checksum": null
                }
            ],
            "environment": {
                "type": "real_world",
                "site": "test_lab",
                "location": null
            },
            "runner": {
                "type": "operator",
                "organization": "test_org",
                "name": "tester"
            },
            "devices": [],
            "programs": [
                {
                    "role": "interface",
                    "name": "teleop",
                    "source": {
                        "git": {
                            "uri": "https://example.com/interface.git",
                            "hash": "abc123",
                            "branch": "main",
                            "tag": null
                        }
                    }
                },
                {
                    "role": "data_collection",
                    "name": "collector",
                    "source": {
                        "git": {
                            "uri": "https://example.com/collector.git",
                            "hash": "def456",
                            "branch": "main",
                            "tag": null
                        }
                    }
                }
            ],
            "episode": {
                "start_time": 0.0,
                "end_time": 1.0,
                "success": true,
                "label": "pick"
            },
            "labels": ["pick"],
            "segments": [
                {
                    "start_time": 0.0,
                    "end_time": 1.0,
                    "label_idx": 0,
                    "success": true
                }
            ]
        }
        "#;

        let mut context = Context::default();
        context.set_dataset(HashMap::from([(
            "/present_topic".to_string(),
            df! {
                "synched_timestamp_ns" => &[0_u64, 100_000_000],
                "timestamp_ns" => &[0_u64, 100_000_000],
                "index" => &[0_u32, 1_u32],
            }
            .unwrap()
            .lazy(),
        )]));
        context.set_fps(10);
        context.set_airoa_metadata(parse_metadata(metadata_json).unwrap());

        let result = stage.run(context);

        assert!(result.is_err());
        let err = result.err().unwrap();
        assert!(matches!(&err, StageError::InvalidData { .. }));
        assert!(err.to_string().contains("/missing_topic"));
        assert!(err.to_string().contains("action.ee_joint_command"));
    }

    #[test]
    fn test_robot_model_source_inline_resolve() {
        let map = get_hsr_lerobot_v21_topic_feature_map();
        let source = RobotModelSource::Inline(map.clone());
        let resolved = source.resolve().unwrap();
        assert_eq!(resolved, map);
    }

    #[test]
    fn test_robot_model_source_path_resolve() {
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let path = format!("{}/../config/robot_model/yubi.yaml", manifest_dir);
        let source = RobotModelSource::Path(path);
        let resolved = source.resolve().unwrap();
        assert!(!resolved.map.is_empty());
    }

    #[test]
    fn test_robot_model_source_serde_roundtrip_path() {
        let source = RobotModelSource::Path("./config/robot_model/yubi.yaml".to_string());
        let json = serde_json::to_string(&source).unwrap();
        let deserialized: RobotModelSource = serde_json::from_str(&json).unwrap();
        assert_eq!(source, deserialized);
    }

    #[test]
    fn test_robot_model_source_serde_roundtrip_inline() {
        let map = get_hsr_lerobot_v21_topic_feature_map();
        let source = RobotModelSource::Inline(map);
        let json = serde_json::to_string(&source).unwrap();
        let deserialized: RobotModelSource = serde_json::from_str(&json).unwrap();
        assert_eq!(source, deserialized);
    }
}
