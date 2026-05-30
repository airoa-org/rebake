//! Dataset I/O operations for LeRobot format.
//!
//! # Overview
//!
//! Handles writing episode data files and statistics to disk in the
//! LeRobot v2.1 directory structure.
//!
//! # Responsibilities
//!
//! - Owns: Writing parquet files (`data/chunk-*/episode_*.parquet`) and episode statistics (`meta/episodes_stats.jsonl`)
//! - Does not own: Metadata composition (see [`super::metadata`])

use std::collections::HashMap;
use std::fs;
use std::io::{BufWriter, Write};

use camino::Utf8Path;
use polars::prelude::*;
use serde_json::{Value, json};
use tracing::warn;

use crate::core::stage::StageError;
use crate::transform::lerobot_v21::video::VideoStats;

/// Writer for episode data files in a LeRobot dataset.
///
/// Handles writing:
/// - Episode parquet files (`data/chunk-*/episode_*.parquet`)
/// - Episode statistics (`meta/episodes_stats.jsonl`)
///
/// Note: Other metadata files are handled by different components:
/// - `Episodes` writes `meta/episodes.jsonl`
/// - `MetadataComposer` writes `meta/info.json`
pub struct DatasetWriter;

impl DatasetWriter {
    pub fn write_parquet(
        outdir: &Utf8Path,
        chunk_id: &str,
        episode_id: &str,
        df: &mut DataFrame,
    ) -> Result<(), StageError> {
        let output_path = outdir
            .join("data")
            .join(format!("chunk-{}", chunk_id))
            .join(format!("episode_{}.parquet", episode_id));

        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent.as_std_path())?;
        }

        let mut file = fs::File::create(output_path.as_std_path())?;
        ParquetWriter::new(&mut file).finish(df)?;
        file.flush()?;

        Ok(())
    }

    /// Write episode statistics for a single episode (SHT mode).
    ///
    /// Creates or overwrites `meta/episodes_stats.jsonl` with a single line
    /// containing statistics for the given episode.
    ///
    /// # Arguments
    /// * `df` - DataFrame containing the episode data
    /// * `outdir` - Output directory for the LeRobot dataset
    /// * `video_stats` - Video statistics for image features
    pub fn write_episode_stats(
        df: &DataFrame,
        outdir: &Utf8Path,
        video_stats: &HashMap<String, VideoStats>,
        episode_id: &str,
    ) -> Result<(), StageError> {
        let stats_map = build_stats_map(df, video_stats);

        let episode_index = df
            .column("episode_index")
            .ok()
            .and_then(|c| c.i64().ok()?.get(0))
            .unwrap_or(0);

        let record = json!({
            "episode_index": episode_index,
            "episode_id": episode_id,
            "stats": stats_map,
        });

        let meta_dir = outdir.join("meta");
        fs::create_dir_all(meta_dir.as_std_path())?;
        let file_path = meta_dir.join("episodes_stats.jsonl");
        let file = fs::File::create(file_path.as_std_path())?;
        let mut writer = BufWriter::new(file);

        serde_json::to_writer(&mut writer, &record)?;
        writer.write_all(b"\n")?;
        writer.flush()?;
        Ok(())
    }

    /// Write episode statistics for multiple episodes (PA mode).
    ///
    /// Creates or overwrites `meta/episodes_stats.jsonl` with one line per episode.
    /// This mirrors the behavior of `Episodes::save_all` for `episodes.jsonl`.
    ///
    /// # Arguments
    /// * `episodes` - Slice of tuples containing (episode_index, episode_id, DataFrame, video_stats) for each episode
    /// * `outdir` - Output directory for the LeRobot dataset
    pub fn write_episode_stats_all(
        episodes: &[(usize, &str, &DataFrame, &HashMap<String, VideoStats>)],
        outdir: &Utf8Path,
    ) -> Result<(), StageError> {
        let meta_dir = outdir.join("meta");
        fs::create_dir_all(meta_dir.as_std_path())?;
        let file_path = meta_dir.join("episodes_stats.jsonl");
        let file = fs::File::create(file_path.as_std_path())?;
        let mut writer = BufWriter::new(file);

        for (episode_index, episode_id, df, video_stats) in episodes {
            let stats_map = build_stats_map(df, video_stats);

            let record = json!({
                "episode_index": episode_index,
                "episode_id": episode_id,
                "stats": stats_map,
            });
            serde_json::to_writer(&mut writer, &record)?;
            writer.write_all(b"\n")?;
        }

        writer.flush()?;
        Ok(())
    }
}

/// Build statistics map for a single episode.
///
/// Computes min, max, mean, std, and count for each numeric column in the DataFrame,
/// and includes video statistics for image features.
fn build_stats_map(
    df: &DataFrame,
    video_stats: &HashMap<String, VideoStats>,
) -> serde_json::Map<String, Value> {
    let mut stats_map = serde_json::Map::new();

    for column in df.get_columns() {
        if let Some(stats) = video_stats.get::<str>(column.name().as_ref()) {
            stats_map.insert(column.name().to_string(), video_stats_to_json(stats));
            continue;
        }

        let base_series = match column.as_series() {
            Some(series) => series,
            None => continue,
        };

        if let Some((min, max, mean, std, count)) = summarize_numeric_series(base_series) {
            let mut column_stats = json!({
                "min": min,
                "max": max,
                "mean": mean,
                "std": std,
                "count": [count],
            });
            adjust_image_fresh_stats(base_series.name(), &mut column_stats, video_stats);
            stats_map.insert(base_series.name().to_string(), column_stats);
            continue;
        }
        let materialized = base_series.clone();

        if materialized.is_empty() || materialized.null_count() == materialized.len() {
            continue;
        }

        if let Some((min, max, mean, std, count)) = summarize_list_series(&materialized) {
            let mut column_stats = json!({
                "min": min,
                "max": max,
                "mean": mean,
                "std": std,
                "count": [count],
            });
            adjust_image_fresh_stats(base_series.name(), &mut column_stats, video_stats);
            stats_map.insert(base_series.name().to_string(), column_stats);
        }
    }

    for (feature, stats) in video_stats {
        stats_map
            .entry(feature.clone())
            .or_insert_with(|| video_stats_to_json(stats));
    }

    stats_map
}

fn video_stats_to_json(stats: &VideoStats) -> Value {
    json!({
        "min": channel_tensor(&stats.min),
        "max": channel_tensor(&stats.max),
        "mean": channel_tensor(&stats.mean),
        "std": channel_tensor(&stats.std),
        "count": [stats.frame_count],
    })
}

fn channel_tensor(values: &[f64]) -> Value {
    Value::Array(
        values
            .iter()
            .map(|v| Value::Array(vec![Value::Array(vec![Value::from(*v)])]))
            .collect(),
    )
}

fn summarize_numeric_series(series: &Series) -> Option<(Value, Value, Value, Value, u64)> {
    use polars::prelude::DataType::*;

    let dtype = series.dtype();
    let supported = matches!(
        dtype,
        Boolean
            | UInt8
            | UInt16
            | UInt32
            | UInt64
            | Int8
            | Int16
            | Int32
            | Int64
            | Float32
            | Float64
    );
    if !supported {
        return None;
    }

    let non_null = series.drop_nulls();
    if non_null.is_empty() {
        return None;
    }

    let as_f64 = match dtype {
        Boolean => non_null.cast(&UInt8).ok()?.cast(&Float64).ok()?,
        UInt8 | UInt16 | UInt32 | UInt64 | Int8 | Int16 | Int32 | Int64 | Float32 | Float64 => {
            non_null.cast(&Float64).ok()?
        }
        _ => {
            debug_assert!(
                false,
                "dtype {:?} should have been filtered by supported check",
                dtype
            );
            warn!(
                column = %series.name(),
                dtype = ?dtype,
                "Unexpected dtype in summarize_numeric_series, skipping"
            );
            return None;
        }
    };

    let values = as_f64.f64().ok()?;
    let count = values.len() as u64;

    let mut min = f64::INFINITY;
    let mut max = f64::NEG_INFINITY;
    let mut sum = 0.0;
    let mut sum_sq = 0.0;

    for value in values.into_no_null_iter() {
        let v = value;
        min = min.min(v);
        max = max.max(v);
        sum += v;
        sum_sq += v * v;
    }

    let mean = sum / count as f64;
    let variance = (sum_sq / count as f64) - mean.powi(2);
    let std = variance.max(0.0).sqrt();

    let (min_value, max_value) = if matches!(dtype, Boolean) {
        (wrap_scalar_bool(min > 0.0), wrap_scalar_bool(max > 0.0))
    } else {
        (wrap_scalar_number(min), wrap_scalar_number(max))
    };

    Some((
        min_value,
        max_value,
        wrap_scalar_number(mean),
        wrap_scalar_number(std),
        count,
    ))
}

fn wrap_scalar_number(value: f64) -> Value {
    Value::Array(vec![Value::from(value)])
}

fn wrap_scalar_bool(value: bool) -> Value {
    Value::Array(vec![Value::from(value)])
}

#[derive(Copy, Clone, PartialEq, Eq)]
enum ListElementKind {
    Boolean,
    Numeric,
}

fn summarize_list_series(series: &Series) -> Option<(Value, Value, Value, Value, u64)> {
    use polars::prelude::DataType::*;

    let list = series.list().ok()?;
    let mut template_len: Option<usize> = None;
    let mut element_kind: Option<ListElementKind> = None;
    let mut min_vec = Vec::<f64>::new();
    let mut max_vec = Vec::<f64>::new();
    let mut sum_vec = Vec::<f64>::new();
    let mut sum_sq_vec = Vec::<f64>::new();
    let mut count: u64 = 0;

    for opt_inner in list.into_iter() {
        let inner = match opt_inner {
            Some(series) => series,
            None => continue,
        };

        if inner.null_count() > 0 {
            return None;
        }

        let len = inner.len();
        if len == 0 {
            continue;
        }

        let len_usize = len;
        if let Some(expected) = template_len {
            if expected != len_usize {
                return None;
            }
        } else {
            template_len = Some(len_usize);
            min_vec = vec![f64::INFINITY; len_usize];
            max_vec = vec![f64::NEG_INFINITY; len_usize];
            sum_vec = vec![0.0; len_usize];
            sum_sq_vec = vec![0.0; len_usize];
        }

        let kind = match inner.dtype() {
            Boolean => ListElementKind::Boolean,
            UInt8 | UInt16 | UInt32 | UInt64 | Int8 | Int16 | Int32 | Int64 | Float32 | Float64 => {
                ListElementKind::Numeric
            }
            _ => return None,
        };

        if let Some(existing) = element_kind {
            if existing != kind {
                return None;
            }
        } else {
            element_kind = Some(kind);
        }

        let cast = match kind {
            ListElementKind::Boolean => inner.cast(&UInt8).ok()?.cast(&Float64).ok()?,
            ListElementKind::Numeric => inner.cast(&Float64).ok()?,
        };
        let values = cast.f64().ok()?;

        for (idx, value) in values.into_no_null_iter().enumerate() {
            min_vec[idx] = min_vec[idx].min(value);
            max_vec[idx] = max_vec[idx].max(value);
            sum_vec[idx] += value;
            sum_sq_vec[idx] += value * value;
        }

        count += 1;
    }

    let len = template_len?;
    if count == 0 {
        return None;
    }

    let mut mean_vec = vec![0.0; len];
    let mut std_vec = vec![0.0; len];
    for idx in 0..len {
        let mean = sum_vec[idx] / count as f64;
        let variance = (sum_sq_vec[idx] / count as f64) - mean.powi(2);
        mean_vec[idx] = mean;
        std_vec[idx] = variance.max(0.0).sqrt();
    }

    let is_bool = matches!(element_kind, Some(ListElementKind::Boolean));
    let min_value = if is_bool {
        Value::Array(min_vec.iter().map(|v| Value::from(*v > 0.0)).collect())
    } else {
        Value::Array(min_vec.iter().map(|v| Value::from(*v)).collect())
    };
    let max_value = if is_bool {
        Value::Array(max_vec.iter().map(|v| Value::from(*v > 0.0)).collect())
    } else {
        Value::Array(max_vec.iter().map(|v| Value::from(*v)).collect())
    };
    let mean_value = Value::Array(mean_vec.iter().map(|v| Value::from(*v)).collect());
    let std_value = Value::Array(std_vec.iter().map(|v| Value::from(*v)).collect());

    Some((min_value, max_value, mean_value, std_value, count))
}

fn adjust_image_fresh_stats(
    column_name: &str,
    column_stats: &mut Value,
    video_stats: &HashMap<String, VideoStats>,
) {
    let Some(feature_name) = column_name.strip_suffix(".is_fresh") else {
        return;
    };
    let Some(stats) = video_stats.get(feature_name) else {
        return;
    };
    let channels = stats.min.len();
    if channels == 0 {
        return;
    }
    let Some(object) = column_stats.as_object_mut() else {
        return;
    };
    for key in ["min", "max", "mean", "std"] {
        if let Some(value) = object.get_mut(key) {
            *value = replicate_scalar_to_channels(value, channels);
        }
    }
}

fn replicate_scalar_to_channels(value: &Value, channels: usize) -> Value {
    match value {
        Value::Array(arr) if arr.len() == channels => value.clone(),
        Value::Array(arr) if arr.len() == 1 => {
            let elem = arr[0].clone();
            replicate_scalar_element(elem, channels)
        }
        Value::Array(arr) if arr.is_empty() => replicate_scalar_element(Value::Null, channels),
        other => replicate_scalar_element(other.clone(), channels),
    }
}

fn replicate_scalar_element(elem: Value, channels: usize) -> Value {
    Value::Array(
        (0..channels)
            .map(|_| Value::Array(vec![Value::Array(vec![elem.clone()])]))
            .collect(),
    )
}
