use std::collections::HashMap;
use std::fs;
use std::io::{self, BufRead, BufWriter, Write};

use camino::{Utf8Path, Utf8PathBuf};
use indexmap::IndexMap;
use serde::de::DeserializeOwned;
use serde_json::json;
use tracing::{debug, info, warn};

use crate::core::error::{StageError, StageResult};
use crate::transform::lerobot_v21::{DType, Info, LeRobotTask, LeRobotTasksVec};

use super::parquet::process_parquet_files;
use super::validation::validate_sources;

/// Configuration for a dataset merge operation.
pub struct MergeConfig {
    /// Path to a directory containing multiple LeRobot dataset subdirectories.
    /// Each subdirectory must contain `meta/info.json` to be recognized as a dataset.
    pub source_dir: Utf8PathBuf,
    /// Path to the output merged dataset directory.
    pub output: Utf8PathBuf,
    /// Override for chunks_size. If `None`, uses the value from the first source dataset.
    pub chunks_size: Option<usize>,
}

/// A loaded source dataset with all metadata parsed and ready for merging.
pub struct SourceDataset {
    pub path: Utf8PathBuf,
    pub info: Info,
    pub tasks: Vec<LeRobotTask>,
    /// Episode entries from `meta/episodes.jsonl` as opaque JSON values.
    /// Using `serde_json::Value` instead of the `Episodes` struct to support
    /// arbitrary episode schemas (the `Episodes` struct is HSR-specific).
    pub episodes: Vec<serde_json::Value>,
    /// Episode statistics from `meta/episodes_stats.jsonl` as opaque JSON values.
    pub episode_stats: Vec<serde_json::Value>,
}

/// Mapping information for a single episode during merge.
#[derive(Debug)]
pub struct EpisodeMapping {
    /// Index of the source dataset in the sources list (0-based).
    pub source_index: usize,
    /// Original episode index within the source dataset.
    pub old_episode_index: usize,
    /// New global episode index in the merged dataset.
    pub new_episode_index: usize,
    /// Number of frames in this episode.
    pub frame_count: usize,
    /// Cumulative frame count before this episode (used for global `index` remapping).
    pub global_frame_offset: usize,
}

/// Task deduplication and remapping information.
pub struct TaskRemapping {
    /// Per-source task index remapping: `per_source_map[source_idx][old_index] = new_index`.
    /// Uses `i64` to match the Parquet column type.
    pub per_source_map: Vec<HashMap<i64, i64>>,
    /// The deduplicated global task list (ordering not guaranteed; sorted at write time).
    pub global_tasks: Vec<LeRobotTask>,
}

/// Discover LeRobot dataset directories inside a parent directory.
///
/// Scans immediate subdirectories of `parent_dir` for those containing
/// `meta/info.json`. Returns the discovered paths sorted alphabetically
/// for deterministic merge ordering.
///
/// Returns an empty list if `parent_dir` does not exist.
pub fn discover_datasets(parent_dir: &Utf8Path) -> StageResult<Vec<Utf8PathBuf>> {
    if !parent_dir.exists() {
        return Ok(Vec::new());
    }

    let mut dataset_paths = Vec::new();

    let entries = fs::read_dir(parent_dir.as_std_path())
        .map_err(|e| StageError::io(format!("failed to read directory {}", parent_dir), e))?;

    for entry in entries {
        let entry = entry.map_err(|e| StageError::io("failed to read dir entry".to_string(), e))?;

        if !entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false) {
            continue;
        }

        let path = Utf8PathBuf::from_path_buf(entry.path())
            .map_err(|p| StageError::invalid(format!("non-UTF-8 path: {}", p.display())))?;

        if path.join("meta/info.json").exists() {
            dataset_paths.push(path);
        }
    }

    dataset_paths.sort();
    Ok(dataset_paths)
}

/// Merge multiple LeRobot v2.1 datasets into a single unified dataset.
///
/// Discovers all dataset directories under `config.source_dir`, then:
/// 1. Loads and parses all source datasets
/// 2. Validates compatibility (FPS, features, version)
/// 3. Deduplicates tasks across datasets
/// 4. Builds episode mapping with renumbered indices
/// 5. Processes parquet files with column remapping
/// 6. Copies video files with new paths
/// 7. Merges metadata (episodes.jsonl, episodes_stats.jsonl, tasks.jsonl)
/// 8. Generates merged info.json
pub fn merge_datasets(config: &MergeConfig) -> StageResult<u32> {
    let dataset_paths = discover_datasets(&config.source_dir)?;

    if dataset_paths.len() < 2 {
        return Err(StageError::invalid(format!(
            "expected at least 2 dataset directories in {}, found {}",
            config.source_dir,
            dataset_paths.len()
        )));
    }

    let num_datasets = dataset_paths.len() as u32;

    info!(
        source_dir = %config.source_dir,
        datasets = num_datasets,
        output = %config.output,
        "starting dataset merge"
    );

    // 1. Loads and parses all source datasets
    let sources = load_sources(&dataset_paths)?;

    // 2. Validates compatibility (FPS, features, version)
    validate_sources(&sources)?;

    let chunks_size = config.chunks_size.unwrap_or(sources[0].info.chunks_size);

    // 3. Deduplicates tasks across datasets
    let task_remapping = deduplicate_tasks(&sources)?;

    // 4. Builds episode mapping with renumbered indices
    let mappings = build_episode_mappings(&sources)?;
    let total_frames: usize = mappings.iter().map(|m| m.frame_count).sum();

    // 5. Processes parquet files with column remapping
    process_parquet_files(chunks_size, config, &sources, &mappings, &task_remapping)?;

    // 6. Copies video files with new paths
    let total_videos = copy_video_files(chunks_size, config, &sources, &mappings)?;

    // 7. Merges metadata (episodes.jsonl, episodes_stats.jsonl, tasks.jsonl)
    merge_jsonl_metadata(config, &sources, &mappings, "episodes.jsonl", None, |src| {
        &src.episodes
    })?;
    merge_jsonl_metadata(
        config,
        &sources,
        &mappings,
        "episodes_stats.jsonl",
        Some(&task_remapping),
        |src| &src.episode_stats,
    )?;

    let mut tasks_vec = LeRobotTasksVec {
        tasks: task_remapping.global_tasks.clone(),
    };
    tasks_vec.save(&config.output)?;

    // 8. Generates merged info.json
    write_merged_info(
        chunks_size,
        config,
        &sources,
        &mappings,
        &task_remapping,
        total_videos,
        total_frames,
    )?;

    info!(
        total_episodes = mappings.len(),
        total_frames,
        total_videos,
        output = %config.output,
        "merge completed successfully"
    );

    Ok(num_datasets)
}

/// Load all source datasets from the given paths.
///
/// For each path, reads `meta/info.json`, `meta/tasks.jsonl`, `meta/episodes.jsonl`,
/// and `meta/episodes_stats.jsonl`.
fn load_sources(paths: &[Utf8PathBuf]) -> StageResult<Vec<SourceDataset>> {
    let mut sources = Vec::with_capacity(paths.len());

    for path in paths {
        debug!(path = %path, "loading source dataset");

        let info = load_info(path)?;
        let tasks: Vec<LeRobotTask> = load_jsonl(path, "tasks.jsonl")?;
        let episodes: Vec<serde_json::Value> = load_jsonl(path, "episodes.jsonl")?;
        let episode_stats: Vec<serde_json::Value> = load_jsonl(path, "episodes_stats.jsonl")?;

        sources.push(SourceDataset {
            path: path.clone(),
            info,
            tasks,
            episodes,
            episode_stats,
        });
    }

    Ok(sources)
}

/// Load and parse `meta/info.json` from a dataset directory.
fn load_info(dataset_path: &Utf8Path) -> StageResult<Info> {
    let path = dataset_path.join("meta/info.json");
    let content = fs::read_to_string(path.as_std_path())
        .map_err(|e| StageError::io(format!("failed to read {}", path), e))?;
    let info: Info = serde_json::from_str(&content)?;
    Ok(info)
}

/// Load a JSONL file as a vector of deserialized items.
fn load_jsonl<T: DeserializeOwned>(dataset_path: &Utf8Path, filename: &str) -> StageResult<Vec<T>> {
    let path = dataset_path.join("meta").join(filename);
    if !path.exists() {
        return Ok(vec![]);
    }
    let file = fs::File::open(path.as_std_path())
        .map_err(|e| StageError::io(format!("failed to open {}", path), e))?;
    let reader = io::BufReader::new(file);
    let mut entries = Vec::new();
    for line in reader.lines() {
        let line =
            line.map_err(|e| StageError::io(format!("failed to read line from {}", filename), e))?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let entry: T = serde_json::from_str(trimmed)?;
        entries.push(entry);
    }
    Ok(entries)
}

/// Deduplicate tasks across all source datasets and build index remapping tables.
///
/// Tasks are deduplicated by their string content (the `task` field).
/// Each unique task string is assigned a global index starting from 0.
fn deduplicate_tasks(sources: &[SourceDataset]) -> StageResult<TaskRemapping> {
    let mut task_to_global_index: HashMap<String, usize> = HashMap::new();
    let mut per_source_map: Vec<HashMap<i64, i64>> = Vec::with_capacity(sources.len());
    let mut next_global_index: usize = 0;

    for source in sources {
        let mut source_map = HashMap::new();
        for task in &source.tasks {
            let global_index = *task_to_global_index
                .entry(task.task.clone())
                .or_insert_with(|| {
                    let idx = next_global_index;
                    next_global_index += 1;
                    idx
                });
            source_map.insert(task.task_index as i64, global_index as i64);
        }
        per_source_map.push(source_map);
    }

    let global_tasks: Vec<LeRobotTask> = task_to_global_index
        .into_iter()
        .map(|(task, index)| LeRobotTask {
            task_index: index,
            task,
        })
        .collect();

    Ok(TaskRemapping {
        per_source_map,
        global_tasks,
    })
}

/// Build the episode mapping with renumbered indices and cumulative frame offsets.
fn build_episode_mappings(sources: &[SourceDataset]) -> StageResult<Vec<EpisodeMapping>> {
    let mut mappings = Vec::new();
    let mut next_episode_index: usize = 0;
    let mut global_frame_offset: usize = 0;

    for (source_idx, source) in sources.iter().enumerate() {
        for ep in &source.episodes {
            let old_index = ep["episode_index"].as_u64().ok_or_else(|| {
                StageError::invalid(format!(
                    "missing or non-integer episode_index in episodes.jsonl of source {}",
                    source.path
                ))
            })? as usize;

            let frame_count = ep["length"].as_u64().ok_or_else(|| {
                StageError::invalid(format!(
                    "missing or non-integer length for episode {} in source {}",
                    old_index, source.path
                ))
            })? as usize;

            mappings.push(EpisodeMapping {
                source_index: source_idx,
                old_episode_index: old_index,
                new_episode_index: next_episode_index,
                frame_count,
                global_frame_offset,
            });

            next_episode_index += 1;
            global_frame_offset += frame_count;
        }
    }

    Ok(mappings)
}

/// Copy video files from source datasets to the output directory with new episode indices.
///
/// Videos are not re-encoded; they are copied as-is with updated file paths.
/// Returns the total number of video files copied.
fn copy_video_files(
    chunks_size: usize,
    config: &MergeConfig,
    sources: &[SourceDataset],
    mappings: &[EpisodeMapping],
) -> StageResult<usize> {
    let video_features: Vec<String> = sources[0]
        .info
        .features
        .iter()
        .filter(|(_, f)| f.dtype == DType::Video)
        .map(|(name, _)| name.clone())
        .collect();

    if video_features.is_empty() {
        debug!("no video features found, skipping video copy");
        return Ok(0);
    }

    let mut total_copied = 0;

    for mapping in mappings {
        let source = &sources[mapping.source_index];
        let old_chunk = mapping.old_episode_index / source.info.chunks_size;
        let new_chunk = mapping.new_episode_index / chunks_size;

        for feature_name in &video_features {
            let src_video = source.path.join(format!(
                "videos/chunk-{:03}/{}/episode_{:06}.mp4",
                old_chunk, feature_name, mapping.old_episode_index
            ));
            let dst_video = config.output.join(format!(
                "videos/chunk-{:03}/{}/episode_{:06}.mp4",
                new_chunk, feature_name, mapping.new_episode_index
            ));

            if src_video.exists() {
                if let Some(parent) = dst_video.parent() {
                    fs::create_dir_all(parent.as_std_path()).map_err(|e| {
                        StageError::io(format!("failed to create directory {}", parent), e)
                    })?;
                }
                fs::copy(src_video.as_std_path(), dst_video.as_std_path()).map_err(|e| {
                    StageError::io(
                        format!("failed to copy video {} -> {}", src_video, dst_video),
                        e,
                    )
                })?;
                total_copied += 1;
            } else {
                warn!(
                    path = %src_video,
                    feature = %feature_name,
                    episode = mapping.old_episode_index,
                    "video file not found, skipping"
                );
            }
        }
    }

    debug!(total_copied, "video file copy complete");
    Ok(total_copied)
}

/// Look up an episode entry by `episode_index` from a JSON array.
///
/// Tries direct indexing first (O(1) when episodes are in order), then falls back
/// to linear search. Returns `None` if the episode is not found.
fn find_episode_entry(
    entries: &[serde_json::Value],
    episode_index: usize,
) -> Option<&serde_json::Value> {
    entries
        .get(episode_index)
        .filter(|e| e["episode_index"].as_u64() == Some(episode_index as u64))
        .or_else(|| {
            entries
                .iter()
                .find(|e| e["episode_index"].as_u64() == Some(episode_index as u64))
        })
}

/// Merge a JSONL metadata file (episodes.jsonl or episodes_stats.jsonl) from all
/// source datasets with renumbered episode indices.
///
/// The `get_entries` closure selects which field on `SourceDataset` to use,
/// avoiding near-duplicate functions for episodes vs stats.
fn merge_jsonl_metadata(
    config: &MergeConfig,
    sources: &[SourceDataset],
    mappings: &[EpisodeMapping],
    filename: &str,
    task_remapping: Option<&TaskRemapping>,
    get_entries: fn(&SourceDataset) -> &Vec<serde_json::Value>,
) -> StageResult<()> {
    let meta_dir = config.output.join("meta");
    fs::create_dir_all(meta_dir.as_std_path())
        .map_err(|e| StageError::io(format!("failed to create directory {}", meta_dir), e))?;

    let file_path = meta_dir.join(filename);
    let file = fs::File::create(file_path.as_std_path())
        .map_err(|e| StageError::io(format!("failed to create {}", file_path), e))?;
    let mut writer = BufWriter::new(file);

    for mapping in mappings {
        let source = &sources[mapping.source_index];
        let entries = get_entries(source);

        let entry = match find_episode_entry(entries, mapping.old_episode_index) {
            Some(e) => e,
            None => {
                // episodes_stats.jsonl may be missing entries; episodes.jsonl must not be.
                if filename == "episodes.jsonl" {
                    return Err(StageError::missing(format!(
                        "episode_index {} not found in {} of source {}",
                        mapping.old_episode_index, filename, source.path
                    )));
                }
                continue;
            }
        };

        let mut merged = entry.clone();
        merged["episode_index"] =
            serde_json::Value::Number(serde_json::Number::from(mapping.new_episode_index));
        if filename == "episodes_stats.jsonl" {
            let task_map =
                task_remapping.map(|remapping| &remapping.per_source_map[mapping.source_index]);
            remap_episode_stats_metadata(&mut merged, mapping, task_map);
        }

        serde_json::to_writer(&mut writer, &merged)?;
        writer.write_all(b"\n")?;
    }

    writer.flush()?;
    Ok(())
}

fn remap_episode_stats_metadata(
    entry: &mut serde_json::Value,
    mapping: &EpisodeMapping,
    task_map: Option<&HashMap<i64, i64>>,
) {
    let Some(stats) = entry
        .get_mut("stats")
        .and_then(serde_json::Value::as_object_mut)
    else {
        return;
    };

    set_constant_numeric_stat(stats, "episode_index", mapping.new_episode_index as i64);
    remap_index_stat(stats, mapping);

    if let Some(task_map) = task_map {
        for key in [
            "task_index",
            "primitive_action_index",
            "short_horizon_task_index",
        ] {
            remap_constant_task_stat(stats, key, task_map);
        }
    }
}

fn set_constant_numeric_stat(
    stats: &mut serde_json::Map<String, serde_json::Value>,
    key: &str,
    value: i64,
) {
    let count = stats
        .get(key)
        .and_then(|stat| stat.get("count"))
        .cloned()
        .unwrap_or_else(|| json!([1]));
    stats.insert(
        key.to_string(),
        json!({
            "min": [value],
            "max": [value],
            "mean": [value as f64],
            "std": [0.0],
            "count": count,
        }),
    );
}

fn remap_index_stat(
    stats: &mut serde_json::Map<String, serde_json::Value>,
    mapping: &EpisodeMapping,
) {
    let Some(old_index_stat) = stats.get("index").cloned() else {
        return;
    };
    let count = old_index_stat
        .get("count")
        .cloned()
        .unwrap_or_else(|| json!([mapping.frame_count]));
    let std = stats
        .get("frame_index")
        .and_then(|stat| stat.get("std"))
        .or_else(|| old_index_stat.get("std"))
        .cloned()
        .unwrap_or_else(|| json!([0.0]));

    let min = mapping.global_frame_offset as i64;
    let max = mapping
        .global_frame_offset
        .saturating_add(mapping.frame_count.saturating_sub(1)) as i64;
    let mean = if mapping.frame_count == 0 {
        mapping.global_frame_offset as f64
    } else {
        mapping.global_frame_offset as f64 + (mapping.frame_count - 1) as f64 / 2.0
    };

    stats.insert(
        "index".to_string(),
        json!({
            "min": [min],
            "max": [max],
            "mean": [mean],
            "std": std,
            "count": count,
        }),
    );
}

fn remap_constant_task_stat(
    stats: &mut serde_json::Map<String, serde_json::Value>,
    key: &str,
    task_map: &HashMap<i64, i64>,
) {
    let Some(old_stat) = stats.get(key) else {
        return;
    };
    let Some(old_value) = constant_stat_i64(old_stat) else {
        return;
    };
    let new_value = if old_value < 0 {
        old_value
    } else {
        task_map.get(&old_value).copied().unwrap_or(old_value)
    };
    set_constant_numeric_stat(stats, key, new_value);
}

fn constant_stat_i64(stat: &serde_json::Value) -> Option<i64> {
    let min = value_as_i64(stat.get("min")?.as_array()?.first()?)?;
    let max = value_as_i64(stat.get("max")?.as_array()?.first()?)?;
    let mean = stat.get("mean")?.as_array()?.first()?.as_f64()?;
    if min == max && (mean - min as f64).abs() < f64::EPSILON {
        Some(min)
    } else {
        None
    }
}

fn value_as_i64(value: &serde_json::Value) -> Option<i64> {
    if let Some(value) = value.as_i64() {
        return Some(value);
    }
    let value = value.as_f64()?;
    let rounded = value.round();
    if (value - rounded).abs() < f64::EPSILON {
        Some(rounded as i64)
    } else {
        None
    }
}

/// Generate and write the merged `meta/info.json` with updated totals and splits.
fn write_merged_info(
    chunks_size: usize,
    config: &MergeConfig,
    sources: &[SourceDataset],
    mappings: &[EpisodeMapping],
    task_remapping: &TaskRemapping,
    total_videos: usize,
    total_frames: usize,
) -> StageResult<()> {
    let reference = &sources[0].info;
    let total_episodes = mappings.len();
    let total_chunks = if total_episodes == 0 {
        0
    } else {
        total_episodes.div_ceil(chunks_size)
    };

    let merged_info = Info {
        codebase_version: reference.codebase_version.clone(),
        robot_type: reference.robot_type.clone(),
        total_episodes,
        total_frames,
        total_tasks: task_remapping.global_tasks.len(),
        total_videos,
        total_chunks,
        chunks_size,
        fps: reference.fps,
        splits: IndexMap::from([("train".to_string(), format!("0:{}", total_episodes))]),
        data_path: reference.data_path.clone(),
        video_path: reference.video_path.clone(),
        features: reference.features.clone(),
    };

    merged_info.save(&config.output)?;
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use serde_json::json;

    use crate::transform::lerobot_v21::Info;

    use super::*;

    fn make_task_source(tasks: Vec<(usize, &str)>) -> SourceDataset {
        SourceDataset {
            path: "/test".into(),
            info: Info::default(),
            tasks: tasks
                .into_iter()
                .map(|(idx, name)| LeRobotTask {
                    task_index: idx,
                    task: name.to_string(),
                })
                .collect(),
            episodes: vec![],
            episode_stats: vec![],
        }
    }

    #[test]
    fn test_deduplicate_identical_tasks() {
        let sources = vec![
            make_task_source(vec![(0, "pick"), (1, "place")]),
            make_task_source(vec![(0, "pick"), (1, "place")]),
        ];
        let remapping = deduplicate_tasks(&sources).unwrap();
        assert_eq!(remapping.global_tasks.len(), 2);
        assert_eq!(remapping.per_source_map[0][&0], 0);
        assert_eq!(remapping.per_source_map[0][&1], 1);
        assert_eq!(remapping.per_source_map[1][&0], 0);
        assert_eq!(remapping.per_source_map[1][&1], 1);
    }

    #[test]
    fn test_deduplicate_disjoint_tasks() {
        let sources = vec![
            make_task_source(vec![(0, "pick"), (1, "place")]),
            make_task_source(vec![(0, "push"), (1, "pull")]),
        ];
        let remapping = deduplicate_tasks(&sources).unwrap();
        assert_eq!(remapping.global_tasks.len(), 4);
        assert_eq!(remapping.per_source_map[1][&0], 2);
        assert_eq!(remapping.per_source_map[1][&1], 3);
    }

    #[test]
    fn test_deduplicate_partial_overlap() {
        let sources = vec![
            make_task_source(vec![(0, "pick"), (1, "place")]),
            make_task_source(vec![(0, "place"), (1, "push")]),
        ];
        let remapping = deduplicate_tasks(&sources).unwrap();
        assert_eq!(remapping.global_tasks.len(), 3);
        assert_eq!(remapping.per_source_map[0][&0], 0);
        assert_eq!(remapping.per_source_map[0][&1], 1);
        assert_eq!(remapping.per_source_map[1][&0], 1);
        assert_eq!(remapping.per_source_map[1][&1], 2);
    }

    #[test]
    fn test_task_global_index_assignment() {
        let sources = vec![
            make_task_source(vec![(0, "a"), (1, "b")]),
            make_task_source(vec![(0, "b"), (1, "c")]),
        ];
        let remapping = deduplicate_tasks(&sources).unwrap();

        let task_map: HashMap<&str, usize> = remapping
            .global_tasks
            .iter()
            .map(|t| (t.task.as_str(), t.task_index))
            .collect();
        assert_eq!(task_map["a"], 0);
        assert_eq!(task_map["b"], 1);
        assert_eq!(task_map["c"], 2);
    }

    #[test]
    fn test_build_episode_mappings() {
        let sources = vec![
            SourceDataset {
                path: "/a".into(),
                info: Default::default(),
                tasks: vec![],
                episodes: vec![
                    json!({"episode_index": 0, "length": 100}),
                    json!({"episode_index": 1, "length": 200}),
                ],
                episode_stats: vec![],
            },
            SourceDataset {
                path: "/b".into(),
                info: Default::default(),
                tasks: vec![],
                episodes: vec![json!({"episode_index": 0, "length": 150})],
                episode_stats: vec![],
            },
        ];
        let mappings = build_episode_mappings(&sources).unwrap();
        assert_eq!(mappings.len(), 3);
        assert_eq!(mappings[0].new_episode_index, 0);
        assert_eq!(mappings[0].global_frame_offset, 0);
        assert_eq!(mappings[1].new_episode_index, 1);
        assert_eq!(mappings[1].global_frame_offset, 100);
        assert_eq!(mappings[2].new_episode_index, 2);
        assert_eq!(mappings[2].global_frame_offset, 300);
    }

    #[test]
    fn test_build_episode_mappings_rejects_missing_episode_index() {
        let sources = vec![SourceDataset {
            path: "/bad".into(),
            info: Default::default(),
            tasks: vec![],
            episodes: vec![json!({"length": 100})],
            episode_stats: vec![],
        }];
        let err = build_episode_mappings(&sources).unwrap_err();
        assert!(err.to_string().contains("episode_index"));
    }

    #[test]
    fn test_build_episode_mappings_rejects_missing_length() {
        let sources = vec![SourceDataset {
            path: "/bad".into(),
            info: Default::default(),
            tasks: vec![],
            episodes: vec![json!({"episode_index": 0})],
            episode_stats: vec![],
        }];
        let err = build_episode_mappings(&sources).unwrap_err();
        assert!(err.to_string().contains("length"));
    }

    #[test]
    fn test_find_episode_entry_direct_index() {
        let entries = vec![
            json!({"episode_index": 0, "data": "a"}),
            json!({"episode_index": 1, "data": "b"}),
        ];
        let found = find_episode_entry(&entries, 1).unwrap();
        assert_eq!(found["data"], "b");
    }

    #[test]
    fn test_find_episode_entry_fallback_search() {
        // Out-of-order entries — direct index won't match, must fall back
        let entries = vec![
            json!({"episode_index": 5, "data": "x"}),
            json!({"episode_index": 0, "data": "y"}),
        ];
        let found = find_episode_entry(&entries, 0).unwrap();
        assert_eq!(found["data"], "y");
    }

    #[test]
    fn test_find_episode_entry_missing() {
        let entries = vec![json!({"episode_index": 0})];
        assert!(find_episode_entry(&entries, 99).is_none());
    }

    #[test]
    fn test_merge_jsonl_metadata_renumbers() {
        let tmpdir = tempfile::tempdir().unwrap();
        let output = Utf8PathBuf::from(tmpdir.path().to_str().unwrap());
        let config = MergeConfig {
            source_dir: "/unused".into(),
            output: output.clone(),
            chunks_size: None,
        };
        let sources = vec![
            SourceDataset {
                path: "/a".into(),
                info: Default::default(),
                tasks: vec![],
                episodes: vec![
                    json!({"episode_index": 0, "length": 100, "tasks": ["pick"]}),
                    json!({"episode_index": 1, "length": 200, "tasks": ["place"]}),
                ],
                episode_stats: vec![],
            },
            SourceDataset {
                path: "/b".into(),
                info: Default::default(),
                tasks: vec![],
                episodes: vec![json!({"episode_index": 0, "length": 150, "tasks": ["push"]})],
                episode_stats: vec![],
            },
        ];
        let mappings = vec![
            EpisodeMapping {
                source_index: 0,
                old_episode_index: 0,
                new_episode_index: 0,
                frame_count: 100,
                global_frame_offset: 0,
            },
            EpisodeMapping {
                source_index: 0,
                old_episode_index: 1,
                new_episode_index: 1,
                frame_count: 200,
                global_frame_offset: 100,
            },
            EpisodeMapping {
                source_index: 1,
                old_episode_index: 0,
                new_episode_index: 2,
                frame_count: 150,
                global_frame_offset: 300,
            },
        ];

        merge_jsonl_metadata(&config, &sources, &mappings, "episodes.jsonl", None, |s| {
            &s.episodes
        })
        .unwrap();

        let content = fs::read_to_string(output.join("meta/episodes.jsonl")).unwrap();
        let lines: Vec<serde_json::Value> = content
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();

        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0]["episode_index"], 0);
        assert_eq!(lines[0]["tasks"], json!(["pick"]));
        assert_eq!(lines[1]["episode_index"], 1);
        assert_eq!(lines[1]["tasks"], json!(["place"]));
        assert_eq!(lines[2]["episode_index"], 2);
        assert_eq!(lines[2]["tasks"], json!(["push"]));
    }

    #[test]
    fn test_remap_episode_stats_metadata_updates_remapped_standard_columns() {
        let mut entry = json!({
            "episode_index": 0,
            "episode_id": "uuid:0",
            "stats": {
                "episode_index": {
                    "min": [0.0],
                    "max": [0.0],
                    "mean": [0.0],
                    "std": [0.0],
                    "count": [4]
                },
                "frame_index": {
                    "min": [0.0],
                    "max": [3.0],
                    "mean": [1.5],
                    "std": [1.11803398875],
                    "count": [4]
                },
                "index": {
                    "min": [0.0],
                    "max": [3.0],
                    "mean": [1.5],
                    "std": [1.11803398875],
                    "count": [4]
                },
                "task_index": {
                    "min": [1.0],
                    "max": [1.0],
                    "mean": [1.0],
                    "std": [0.0],
                    "count": [4]
                },
                "action": {
                    "min": [0.0],
                    "max": [1.0],
                    "mean": [0.5],
                    "std": [0.5],
                    "count": [4]
                }
            }
        });
        let mapping = EpisodeMapping {
            source_index: 0,
            old_episode_index: 0,
            new_episode_index: 3,
            frame_count: 4,
            global_frame_offset: 10,
        };
        let task_map = HashMap::from([(1_i64, 5_i64)]);

        remap_episode_stats_metadata(&mut entry, &mapping, Some(&task_map));

        assert_eq!(entry["episode_id"], "uuid:0");
        assert_eq!(entry["stats"]["episode_index"]["min"], json!([3]));
        assert_eq!(entry["stats"]["episode_index"]["max"], json!([3]));
        assert_eq!(entry["stats"]["episode_index"]["mean"], json!([3.0]));
        assert_eq!(entry["stats"]["index"]["min"], json!([10]));
        assert_eq!(entry["stats"]["index"]["max"], json!([13]));
        assert_eq!(entry["stats"]["index"]["mean"], json!([11.5]));
        assert_eq!(entry["stats"]["task_index"]["min"], json!([5]));
        assert_eq!(entry["stats"]["action"]["mean"], json!([0.5]));
    }
}
