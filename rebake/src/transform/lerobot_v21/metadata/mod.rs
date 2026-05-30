//! Metadata composition for LeRobot datasets.
//!
//! # Overview
//!
//! Composes and writes metadata files for LeRobot v2.1 datasets:
//! - `meta/info.json`: Dataset configuration, feature definitions, robot model
//! - `meta/tasks.jsonl`: Task definitions
//!
//! # Responsibilities
//!
//! - Owns: Composing and serializing `info.json` and `tasks.jsonl`
//! - Does not own: Episode data writing (see [`super::io`])

use std::collections::HashMap;

use camino::Utf8Path;
use indexmap::IndexMap;
use polars::prelude::*;

use crate::common::{ImageFrame, ImageShape, resolve_image_shape};
use crate::core::error::{OptionExt, StageResult};
use crate::core::stage::StageError;
use crate::encode::video_encoder::VideoEncoderConfig;
use crate::schema::metadata::v2_0::{Episode, MetadataV2_0};
use crate::schema::{TopicFeatureMap, TopicFeatureMapEntry};

use super::episodes::{Episodes, format_episode_id};
use super::feature::{DType, Feature};
use super::lerobot_dataset_metadata::{LeRobotMetadata, LeRobotTasksVec};
use super::video::VideoStats;

/// Columns to exclude from info.json features.
/// These are internal rebake columns that should not be exposed to LeRobot.
const EXCLUDED_COLUMNS: &[&str] = &["synched_timestamp_ns"];

/// Standard LeRobot fields that are added at the end of info.json features.
/// These are placed after Video/Image features for better readability.
const STANDARD_FIELDS: &[&str] = &[
    "next.done",
    "short_horizon_task_index",
    "primitive_action_index",
    "success_primitive_action",
    "task_index",
    "index",
    "frame_index",
    "episode_index",
    "timestamp",
];

pub struct MetadataComposer<'a> {
    metadata: &'a mut LeRobotMetadata,
    topic_feature_map: &'a TopicFeatureMap,
    image_data: Option<&'a HashMap<String, Vec<ImageFrame>>>,
    image_topic_shapes: Option<&'a HashMap<String, ImageShape>>,
    video_config: &'a VideoEncoderConfig,
    fps: usize,
}

impl<'a> MetadataComposer<'a> {
    pub fn new(
        metadata: &'a mut LeRobotMetadata,
        topic_feature_map: &'a TopicFeatureMap,
        image_data: Option<&'a HashMap<String, Vec<ImageFrame>>>,
        image_topic_shapes: Option<&'a HashMap<String, ImageShape>>,
        video_config: &'a VideoEncoderConfig,
        fps: usize,
    ) -> Self {
        Self {
            metadata,
            topic_feature_map,
            image_data,
            image_topic_shapes,
            video_config,
            fps,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn update_info(
        &mut self,
        df: &DataFrame,
        _video_stats: &HashMap<String, VideoStats>,
        outdir: &Utf8Path,
        airoa_metadata: &MetadataV2_0,
        data_path_template: &str,
        video_path_template: &str,
        version: &str,
        total_videos: usize,
    ) -> Result<LeRobotTasksVec, StageError> {
        self.metadata.info.codebase_version = version.to_string();
        self.metadata.info.robot_type = airoa_metadata.robot.robot_type.clone();
        self.metadata.info.total_episodes = 1;
        self.metadata.info.total_frames = df.height();
        self.metadata.info.total_tasks = self.metadata.tasks.task_to_task_index.len();
        self.metadata.info.total_videos = total_videos;
        self.metadata.info.total_chunks = 1;
        self.metadata.info.chunks_size = 1000;
        self.metadata.info.fps = self.fps;
        self.metadata.info.splits = IndexMap::from([("train".to_string(), "0:1".to_string())]);
        self.metadata.info.data_path = data_path_template.to_string();
        self.metadata.info.video_path = video_path_template.to_string();

        // Build features from DataFrame columns (excludes internal and standard columns)
        self.metadata.info.features = self.build_features_from_dataframe(df)?;

        // Add Video/Image features from TopicFeatureMap (not present in DataFrame)
        self.add_video_image_features()?;

        // Add standard LeRobot fields at the end for better readability
        self.add_standard_features(df)?;

        self.metadata.info.save(outdir)?;
        let mut lerobot_tasks = LeRobotTasksVec::from(self.metadata.tasks.clone());
        lerobot_tasks.save(outdir)?;
        Ok(lerobot_tasks)
    }

    /// Build features by inferring from DataFrame columns.
    /// Excludes internal columns (synched_timestamp_ns), temporary columns (image_index_*),
    /// and standard fields (which are added last via add_standard_features).
    /// Uses IndexMap to preserve DataFrame column order.
    ///
    /// # Errors
    ///
    /// Returns `StageError::InvalidData` if type inference fails for any column.
    fn build_features_from_dataframe(
        &self,
        df: &DataFrame,
    ) -> StageResult<IndexMap<String, Feature>> {
        df.get_columns()
            .iter()
            .filter(|col| !EXCLUDED_COLUMNS.contains(&col.name().as_str()))
            .filter(|col| !col.name().starts_with("image_index_"))
            .filter(|col| !STANDARD_FIELDS.contains(&col.name().as_str()))
            .map(|col| {
                let feature_name = col.name().to_string();
                let feature = self.create_feature_from_column(col, &feature_name)?;
                Ok((feature_name, feature))
            })
            .collect()
    }

    /// Create a Feature from a DataFrame column, enriching with TopicFeatureMap metadata if available.
    ///
    /// # Errors
    ///
    /// Returns `StageError::InvalidData` if type or shape inference fails.
    fn create_feature_from_column(&self, col: &Column, feature_name: &str) -> StageResult<Feature> {
        let series = col.as_materialized_series();

        // Look up additional metadata from TopicFeatureMap
        let entry = self
            .topic_feature_map
            .map
            .iter()
            .find(|e| e.feature() == feature_name);

        let dtype = infer_dtype_from_series(series)?;
        let shape = infer_shape_from_series(series)?;
        let names = entry.and_then(|e| e.names().cloned());
        let description = entry.and_then(|e| e.description().cloned());

        Ok(Feature {
            dtype,
            shape,
            names,
            video_info: None,
            description,
        })
    }

    /// Add Video/Image features from TopicFeatureMap.
    /// These are not present in the DataFrame as they are output as separate files.
    fn add_video_image_features(&mut self) -> StageResult<()> {
        for entry in &self.topic_feature_map.map {
            match entry {
                TopicFeatureMapEntry::Video { topic, feature, .. }
                | TopicFeatureMapEntry::Image { topic, feature, .. } => {
                    let is_video = matches!(entry, TopicFeatureMapEntry::Video { .. });
                    let dtype = if is_video { DType::Video } else { DType::Image };

                    let resolved_shape = resolve_image_shape(
                        topic,
                        self.image_data
                            .and_then(|map| map.get(topic).map(Vec::as_slice)),
                        self.image_topic_shapes,
                    );

                    let shape = if is_video {
                        match resolved_shape {
                            Some(shape) => self.video_config.output_shape(shape)?.to_vec(),
                            None => self
                                .video_config
                                .configured_output_shape(3)
                                .map(|shape| shape.to_vec())
                                .unwrap_or_default(),
                        }
                    } else {
                        resolved_shape
                            .map(|shape| shape.to_vec())
                            .unwrap_or_default()
                    };

                    let video_info = match entry {
                        TopicFeatureMapEntry::Video { .. } => Some(super::VideoInfo {
                            fps: self.fps,
                            codec: self
                                .video_config
                                .codec_config
                                .codec_family_name()
                                .to_string(),
                            pix_fmt: "yuv420p".to_string(),
                            is_depth_map: false,
                            has_audio: false,
                        }),
                        _ => None,
                    };

                    self.metadata.info.features.insert(
                        feature.clone(),
                        Feature {
                            dtype,
                            shape,
                            names: entry.names().cloned(),
                            video_info,
                            description: entry.description().cloned(),
                        },
                    );
                }
                TopicFeatureMapEntry::Parquet { .. } => {
                    // Parquet features are already handled by build_features_from_dataframe
                }
            }
        }

        Ok(())
    }

    /// Add standard LeRobot fields at the end of features.
    /// These fields are added last for better readability in info.json.
    ///
    /// # Errors
    ///
    /// Returns `StageError::InvalidData` if type or shape inference fails for any field.
    fn add_standard_features(&mut self, df: &DataFrame) -> StageResult<()> {
        for field_name in STANDARD_FIELDS {
            if let Ok(col) = df.column(field_name) {
                let feature = self.create_feature_from_column(col, field_name)?;
                self.metadata
                    .info
                    .features
                    .insert(field_name.to_string(), feature);
            }
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn build_episode(
        &self,
        df: &DataFrame,
        airoa_metadata: &MetadataV2_0,
        lerobot_tasks: &LeRobotTasksVec,
        last_segment_success: bool,
        version: &str,
        episode_label: &str,
        default_task_type: &str,
    ) -> Result<Episodes, StageError> {
        // Find interface (teleoperation) program
        let interface_program = airoa_metadata
            .programs
            .iter()
            .find(|p| p.role == "interface" || p.role == "teleoperation")
            .or_missing(
                "program with role 'interface' or 'teleoperation' in airoa_metadata.programs \
                 (required for git source info in episode metadata)",
            )?;
        let interface_git_info = interface_program
            .source
            .git
            .clone()
            .or_missing("git info for interface program in airoa_metadata")?;

        // Find data_collection program
        let data_collection_program = airoa_metadata
            .programs
            .iter()
            .find(|p| p.role == "data_collection" || p.role == "data_capture")
            .or_missing("program with role 'data_collection' in airoa_metadata.programs")?;
        let data_collection_git_info = data_collection_program
            .source
            .git
            .clone()
            .or_missing("git info for data_collection program in airoa_metadata")?;

        // Primitive actions are from labels, SHT is from episode label
        let (primitive_action, short_horizon_task) = if !airoa_metadata.episode.label.is_empty() {
            (
                airoa_metadata.labels.clone(),
                vec![airoa_metadata.episode.label.clone()],
            )
        } else if lerobot_tasks.tasks.len() > 1 {
            (
                lerobot_tasks
                    .tasks
                    .iter()
                    .take(lerobot_tasks.tasks.len().saturating_sub(1))
                    .map(|task| task.task.clone())
                    .collect(),
                vec![
                    lerobot_tasks
                        .tasks
                        .last()
                        .map(|task| task.task.clone())
                        .unwrap_or_default(),
                ],
            )
        } else {
            let first_task = lerobot_tasks.tasks.first().or_missing(
                "at least one task in lerobot_tasks (required to define tasks in episode metadata)",
            )?;
            (vec![first_task.task.clone()], vec![])
        };

        // Get bag filename from airoa_metadata.files
        let bag_file_name = airoa_metadata
            .files
            .iter()
            .find(|file| {
                file.file_type == "rosbag"
                    || file.file_type == "rosbag2"
                    || file.file_type == "mcap"
            })
            .map(|file| file.name.clone())
            .or_missing("file with type 'rosbag' or 'rosbag2' in airoa_metadata.files")?;

        // Location from environment
        let location_name = airoa_metadata.environment.site.clone();

        // Robot ID from robot
        let hsr_id = airoa_metadata.robot.id.clone();

        Ok(Episodes {
            episode_index: 0,
            episode_id: format_episode_id(&airoa_metadata.uuid, None),
            tasks: primitive_action.clone(),
            length: df.height(),
            bag_path: bag_file_name,
            version: version.to_string(),
            location_name,
            interface: interface_program.name.clone(),
            git_hash: data_collection_git_info.hash.clone(),
            git_branch: data_collection_git_info.branch.clone(),
            interface_git_hash: interface_git_info.hash.clone(),
            interface_git_branch: interface_git_info.branch.clone(),
            pipeline_git_hash: "".to_string(),
            pipeline_git_branch: "".to_string(),
            label: episode_label.to_string(),
            hsr_id,
            task_type: default_task_type.to_string(),
            task_success: last_segment_success,
            short_horizon_task,
            primitive_action,
            success_short_horizon_task: last_segment_success,
            uuid: airoa_metadata.uuid.to_string(),
            metadata: airoa_metadata.clone(),
        })
    }

    /// Build episode metadata for PA mode.
    /// In PA mode, each segment is a separate episode with its own task.
    #[allow(clippy::too_many_arguments)]
    pub fn build_episode_for_pa(
        &self,
        df: &DataFrame,
        airoa_metadata: &MetadataV2_0,
        lerobot_tasks: &LeRobotTasksVec,
        segment_success: bool,
        version: &str,
        episode_label: &str,
        task_type: &str,
        episode_index: usize,
        source_segment_index: usize,
        task_name: &str,
    ) -> Result<Episodes, StageError> {
        // Find interface (teleoperation) program
        let interface_program = airoa_metadata
            .programs
            .iter()
            .find(|p| p.role == "interface" || p.role == "teleoperation")
            .or_missing(
                "program with role 'interface' or 'teleoperation' in airoa_metadata.programs \
                 (required for git source info in episode metadata)",
            )?;
        let interface_git_info = interface_program
            .source
            .git
            .clone()
            .or_missing("git info for interface program in airoa_metadata")?;

        // Find data_collection program
        let data_collection_program = airoa_metadata
            .programs
            .iter()
            .find(|p| p.role == "data_collection" || p.role == "data_capture")
            .or_missing("program with role 'data_collection' in airoa_metadata.programs")?;
        let data_collection_git_info = data_collection_program
            .source
            .git
            .clone()
            .or_missing("git info for data_collection program in airoa_metadata")?;

        // Get bag filename from airoa_metadata.files
        let bag_file_name = airoa_metadata
            .files
            .iter()
            .find(|file| {
                file.file_type == "rosbag"
                    || file.file_type == "rosbag2"
                    || file.file_type == "mcap"
            })
            .map(|file| file.name.clone())
            .or_missing("file with type 'rosbag' or 'rosbag2' in airoa_metadata.files")?;

        // Location from environment
        let location_name = airoa_metadata.environment.site.clone();

        // Robot ID from robot
        let hsr_id = airoa_metadata.robot.id.clone();

        // In PA mode, each episode has a single primitive action
        let primitive_action = vec![task_name.to_string()];

        // Find SHT (composite task) if any - use the last label if multiple exist
        let short_horizon_task = if lerobot_tasks.tasks.len() > 1 {
            vec![
                lerobot_tasks
                    .tasks
                    .last()
                    .map(|task| task.task.clone())
                    .unwrap_or_default(),
            ]
        } else {
            vec![]
        };

        Ok(Episodes {
            episode_index,
            episode_id: format_episode_id(&airoa_metadata.uuid, Some(source_segment_index)),
            tasks: primitive_action.clone(),
            length: df.height(),
            bag_path: bag_file_name,
            version: version.to_string(),
            location_name,
            interface: interface_program.name.clone(),
            git_hash: data_collection_git_info.hash.clone(),
            git_branch: data_collection_git_info.branch.clone(),
            interface_git_hash: interface_git_info.hash.clone(),
            interface_git_branch: interface_git_info.branch.clone(),
            pipeline_git_hash: "".to_string(),
            pipeline_git_branch: "".to_string(),
            label: episode_label.to_string(),
            hsr_id,
            task_type: task_type.to_string(),
            task_success: segment_success,
            short_horizon_task,
            primitive_action,
            success_short_horizon_task: segment_success,
            uuid: airoa_metadata.uuid.to_string(),
            metadata: airoa_metadata.clone(),
        })
    }

    /// Update info.json for PA mode with cumulative statistics.
    #[allow(clippy::too_many_arguments)]
    pub fn update_info_for_pa(
        &mut self,
        df: &DataFrame,
        _video_stats: &HashMap<String, VideoStats>,
        outdir: &Utf8Path,
        airoa_metadata: &MetadataV2_0,
        data_path_template: &str,
        video_path_template: &str,
        version: &str,
        total_episodes: usize,
        total_frames: usize,
        total_videos: usize,
    ) -> Result<LeRobotTasksVec, StageError> {
        self.metadata.info.codebase_version = version.to_string();
        self.metadata.info.robot_type = airoa_metadata.robot.robot_type.clone();
        self.metadata.info.total_episodes = total_episodes;
        self.metadata.info.total_frames = total_frames;
        self.metadata.info.total_tasks = self.metadata.tasks.task_to_task_index.len();
        self.metadata.info.total_videos = total_videos;
        self.metadata.info.total_chunks = 1;
        self.metadata.info.chunks_size = 1000;
        self.metadata.info.fps = self.fps;
        self.metadata.info.splits =
            IndexMap::from([("train".to_string(), format!("0:{}", total_episodes))]);
        self.metadata.info.data_path = data_path_template.to_string();
        self.metadata.info.video_path = video_path_template.to_string();

        // Build features from DataFrame columns (excludes internal and standard columns)
        self.metadata.info.features = self.build_features_from_dataframe(df)?;

        // Add Video/Image features from TopicFeatureMap (not present in DataFrame)
        self.add_video_image_features()?;

        // Add standard LeRobot fields at the end for better readability
        self.add_standard_features(df)?;

        self.metadata.info.save(outdir)?;
        let mut lerobot_tasks = LeRobotTasksVec::from(self.metadata.tasks.clone());
        lerobot_tasks.save(outdir)?;
        Ok(lerobot_tasks)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod info_tests {
    use std::collections::HashMap;

    use camino::Utf8PathBuf;
    use polars::df;
    use tempfile::tempdir;

    use super::*;
    use crate::encode::video_encoder::VideoEncoderConfig;
    use crate::schema::TopicFeatureMap;
    use crate::schema::metadata::v2_0::{
        Device, EnvType, Environment, Episode, File, GitSource, Program, Robot, Runner, RunnerType,
        Segment, Source,
    };
    use crate::transform::lerobot_v21::lerobot_dataset_metadata::LeRobotMetadata;

    fn sample_metadata(robot_type: &str) -> MetadataV2_0 {
        MetadataV2_0 {
            schema: "https://example.com/schema.json".to_string(),
            schema_version: "2.0".to_string(),
            uuid: "test-uuid".to_string(),
            robot: Robot {
                uri: None,
                robot_type: robot_type.to_string(),
                id: "robot-001".to_string(),
                checksum: None,
            },
            files: vec![File {
                file_type: "rosbag2".to_string(),
                name: "data.mcap".to_string(),
                checksum: None,
            }],
            environment: Environment {
                env_type: EnvType::RealWorld,
                site: "lab".to_string(),
                location: None,
            },
            runner: Runner {
                runner_type: RunnerType::Operator,
                organization: "airoa".to_string(),
                name: "tester".to_string(),
            },
            devices: vec![Device {
                role: "controller".to_string(),
                device_type: "vr".to_string(),
                id: "dev-001".to_string(),
            }],
            programs: vec![Program {
                role: "interface".to_string(),
                name: "teleop".to_string(),
                source: Source {
                    git: Some(GitSource {
                        uri: "https://example.com/repo.git".to_string(),
                        hash: "abc123".to_string(),
                        branch: "main".to_string(),
                        tag: None,
                    }),
                },
            }],
            episode: Episode {
                start_time: 0.0,
                end_time: 1.0,
                success: true,
                label: String::new(),
            },
            labels: Vec::new(),
            segments: vec![Segment {
                start_time: 0.0,
                end_time: 1.0,
                label_idx: 0,
                success: true,
            }],
        }
    }

    #[test]
    fn update_info_uses_robot_type_from_metadata_v2() {
        let mut metadata = LeRobotMetadata::default();
        let topic_feature_map = TopicFeatureMap { map: Vec::new() };
        let video_config = VideoEncoderConfig::default();
        let mut composer = MetadataComposer::new(
            &mut metadata,
            &topic_feature_map,
            None,
            None,
            &video_config,
            10,
        );
        let df = df! { "value" => [1_i32, 2, 3] }.unwrap();
        let airoa_metadata = sample_metadata("yubi");
        let temp_dir = tempdir().unwrap();
        let outdir = Utf8PathBuf::from_path_buf(temp_dir.path().to_path_buf()).unwrap();

        composer
            .update_info(
                &df,
                &HashMap::new(),
                &outdir,
                &airoa_metadata,
                "data/chunk-{episode_chunk:03d}/episode_{episode_index:06d}.parquet",
                "videos/chunk-{episode_chunk:03d}/{video_key}/episode_{episode_index:06d}.mp4",
                "v2.1",
                0,
            )
            .unwrap();

        assert_eq!(composer.metadata.info.robot_type, "yubi");
    }
}

/// Infer DType from a Polars Series.
///
/// # Errors
///
/// Returns `StageError::InvalidData` if a List type has no inner dtype.
fn infer_dtype_from_series(series: &Series) -> StageResult<DType> {
    let dtype = series.dtype();
    match dtype {
        DataType::List(_) => {
            // For List types, use the inner dtype
            let inner = dtype
                .inner_dtype()
                .ok_or_else(|| StageError::invalid("List type must have inner dtype"))?;
            Ok(DType::from(inner))
        }
        _ => Ok(DType::from(dtype)),
    }
}

/// Infer shape from a Polars Series.
///
/// # Errors
///
/// Returns `StageError::InvalidData` if the column is List but cannot be cast.
fn infer_shape_from_series(series: &Series) -> StageResult<Vec<usize>> {
    let dtype = series.dtype();
    match dtype {
        DataType::List(_) => {
            // For List types, get the length of the first element
            let list = series.list().map_err(|e| {
                StageError::invalid(format!("column dtype is List but list() failed: {:?}", e))
            })?;
            Ok(list
                .get(0)
                .map(|inner| vec![inner.len()])
                .unwrap_or_else(|| vec![0]))
        }
        _ => Ok(vec![1]),
    }
}

/// Register labels as tasks in LeRobot metadata.
///
/// If a label is already registered, it is skipped.
pub(crate) fn register_labels(metadata: &mut LeRobotMetadata, labels: &[String]) {
    for label in labels {
        if metadata.get_task_index(label).is_none() {
            metadata.add_task(label.clone());
        }
    }
}

/// Get task index from episode label (for composite/SHT task).
///
/// Returns `None` if the episode has no label.
pub(crate) fn episode_task_index(metadata: &LeRobotMetadata, episode: &Episode) -> Option<i64> {
    if episode.label.is_empty() {
        None
    } else {
        metadata
            .get_task_index(&episode.label)
            .map(|idx| *idx as i64)
    }
}

/// Find label by index.
///
/// # Errors
/// Returns `StageError::InvalidData` if the index is out of bounds.
pub(crate) fn find_label(labels: &[String], idx: usize) -> Result<&String, StageError> {
    labels
        .get(idx)
        .ok_or_else(|| StageError::invalid(format!("label index {} not found", idx)))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    use crate::encode::video_encoder::{CodecConfig, ResizeConfig, VideoEncoderConfig, X264Preset};
    use crate::schema::{TopicFeatureMap, TopicFeatureMapEntry};

    #[test]
    fn add_video_image_features_applies_resize_only_to_video_entries() {
        let mut metadata = LeRobotMetadata::default();
        let topic_feature_map = TopicFeatureMap {
            map: vec![
                TopicFeatureMapEntry::Video {
                    topic: "/camera/video".to_string(),
                    feature: "observation.image.video".to_string(),
                    names: None,
                    description: None,
                },
                TopicFeatureMapEntry::Image {
                    topic: "/camera/still".to_string(),
                    feature: "observation.image.still".to_string(),
                    names: None,
                    description: None,
                },
            ],
        };
        let image_topic_shapes = HashMap::from([
            ("/camera/video".to_string(), ImageShape::new(480, 640, 3)),
            ("/camera/still".to_string(), ImageShape::new(480, 640, 3)),
        ]);
        let video_config = VideoEncoderConfig::new(10)
            .set_resize(Some(ResizeConfig {
                width: 320,
                height: 240,
            }))
            .set_codec_config(CodecConfig::H264 {
                threads: None,
                preset: X264Preset::Fast,
                tune: vec![],
            });

        let mut composer = MetadataComposer::new(
            &mut metadata,
            &topic_feature_map,
            None,
            Some(&image_topic_shapes),
            &video_config,
            10,
        );

        composer.add_video_image_features().unwrap();

        let video_feature = composer
            .metadata
            .info
            .features
            .get("observation.image.video")
            .unwrap();
        assert_eq!(video_feature.shape, vec![240, 320, 3]);
        assert_eq!(video_feature.video_info.as_ref().unwrap().codec, "h264");

        let image_feature = composer
            .metadata
            .info
            .features
            .get("observation.image.still")
            .unwrap();
        assert_eq!(image_feature.shape, vec![480, 640, 3]);
        assert!(image_feature.video_info.is_none());
    }

    #[test]
    fn add_video_image_features_uses_configured_resize_shape_without_source_shape() {
        let mut metadata = LeRobotMetadata::default();
        let topic_feature_map = TopicFeatureMap {
            map: vec![TopicFeatureMapEntry::Video {
                topic: "/camera/video".to_string(),
                feature: "observation.image.video".to_string(),
                names: None,
                description: None,
            }],
        };
        let video_config = VideoEncoderConfig::new(10).set_resize(Some(ResizeConfig {
            width: 320,
            height: 240,
        }));

        let mut composer = MetadataComposer::new(
            &mut metadata,
            &topic_feature_map,
            None,
            None,
            &video_config,
            10,
        );

        composer.add_video_image_features().unwrap();

        let video_feature = composer
            .metadata
            .info
            .features
            .get("observation.image.video")
            .unwrap();
        assert_eq!(video_feature.shape, vec![240, 320, 3]);
    }
}
