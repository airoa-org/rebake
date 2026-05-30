use std::collections::HashMap;

use polars::prelude::*;

use crate::core::error::{OptionExt, PolarsExt};
use crate::core::stage::StageError;
use crate::synchronize::time_synchronizer::SYNCHED_TIMESTAMP_COL;

pub struct TimelineFormatter;

impl TimelineFormatter {
    pub fn derive_frame_spacing(dataset: &HashMap<String, LazyFrame>) -> Result<f64, StageError> {
        let first_frame = dataset
            .values()
            .next()
            .or_missing("topic frame in dataset")?;
        let binding = first_frame
            .clone()
            .select(&[col(SYNCHED_TIMESTAMP_COL)])
            .collect()?;
        let timestamps = binding
            .column(SYNCHED_TIMESTAMP_COL)?
            .u64()
            .or_invalid(&format!("{} column must be UInt64", SYNCHED_TIMESTAMP_COL))?
            .clone();

        let first = timestamps
            .get(0)
            .ok_or_else(|| StageError::invalid("no timestamps in dataset"))?;
        let second = timestamps.get(1).ok_or_else(|| {
            StageError::invalid("need at least two timestamps to compute spacing")
        })?;
        let delta_ns = second
            .checked_sub(first)
            .ok_or_else(|| StageError::invalid("timestamp overflow when computing delta"))?;
        Ok(delta_ns as f64 / 1_000_000_000.0)
    }

    /// Append timeline axes for SHT mode (single episode).
    /// - `index`: global frame index (0..N for the entire dataset)
    /// - `frame_index`: same as index for single episode
    /// - `episode_index`: always 0
    /// - `timestamp`: frame_index * frame_spacing
    pub fn append_axes(df: &mut DataFrame, frame_spacing: f64) -> Result<(), StageError> {
        let frame_count = df.height();
        let frame_index = (0..frame_count as i64).collect::<Vec<_>>();
        let episode_index = vec![0_i64; frame_count];
        let timestamps = (0..frame_count)
            .map(|i| i as f64 * frame_spacing)
            .collect::<Vec<_>>();

        let additional_df = df! {
            "index" => &frame_index,
            "frame_index" => &frame_index,
            "episode_index" => &episode_index,
            "timestamp" => &timestamps,
        }?;

        let updated = df.hstack(additional_df.get_columns())?;
        *df = updated;
        Ok(())
    }

    /// Append timeline axes for PA mode (multiple episodes).
    /// - `index`: global frame index across all episodes (global_frame_offset + local_index)
    /// - `frame_index`: episode-local frame index (0..N for each episode)
    /// - `episode_index`: the episode index for all frames in this episode
    /// - `timestamp`: frame_index * frame_spacing (resets for each episode)
    pub fn append_axes_for_episode(
        df: &mut DataFrame,
        frame_spacing: f64,
        episode_index: usize,
        global_frame_offset: usize,
    ) -> Result<(), StageError> {
        let frame_count = df.height();

        // frame_index: local to this episode (0, 1, 2, ...)
        let frame_index = (0..frame_count as i64).collect::<Vec<_>>();

        // index: global across all episodes (global_frame_offset + local_index)
        let index = (0..frame_count)
            .map(|i| (global_frame_offset + i) as i64)
            .collect::<Vec<_>>();

        // episode_index: same for all frames in this episode
        let episode_index_col = vec![episode_index as i64; frame_count];

        // timestamp: resets for each episode (0.0, 0.1, 0.2, ...)
        let timestamps = (0..frame_count)
            .map(|i| i as f64 * frame_spacing)
            .collect::<Vec<_>>();

        let additional_df = df! {
            "index" => &index,
            "frame_index" => &frame_index,
            "episode_index" => &episode_index_col,
            "timestamp" => &timestamps,
        }?;

        let updated = df.hstack(additional_df.get_columns())?;
        *df = updated;
        Ok(())
    }

    pub fn remove_image_index_columns(df: &mut DataFrame) -> Result<(), StageError> {
        let columns: Vec<String> = df
            .get_column_names()
            .iter()
            .filter(|name| name.starts_with("image_index_"))
            .map(|name| name.to_string())
            .collect();
        for name in columns {
            df.drop_in_place(&name)?;
        }
        Ok(())
    }
}

/// Compute the minimum and maximum synched timestamp across all frames in the dataset.
///
/// Returns `None` if no valid timestamps are found.
pub(crate) fn synched_timestamp_range(
    dataset: &HashMap<String, LazyFrame>,
) -> PolarsResult<Option<(u64, u64)>> {
    let mut min_ts: Option<u64> = None;
    let mut max_ts: Option<u64> = None;

    for frame in dataset.values() {
        let stats = frame
            .clone()
            .select([
                col(SYNCHED_TIMESTAMP_COL).min().alias("min_ts"),
                col(SYNCHED_TIMESTAMP_COL).max().alias("max_ts"),
            ])
            .collect()?;

        if stats.height() == 0 {
            continue;
        }

        if let Some(value) = extract_u64(&stats, "min_ts") {
            min_ts = match min_ts {
                Some(current) => Some(current.min(value)),
                None => Some(value),
            };
        }

        if let Some(value) = extract_u64(&stats, "max_ts") {
            max_ts = match max_ts {
                Some(current) => Some(current.max(value)),
                None => Some(value),
            };
        }
    }

    Ok(match (min_ts, max_ts) {
        (Some(min), Some(max)) => Some((min, max)),
        _ => None,
    })
}

fn extract_u64(stats: &DataFrame, column: &str) -> Option<u64> {
    stats
        .column(column)
        .ok()
        .and_then(|series| series.u64().ok())
        .and_then(|chunked| chunked.get(0))
}
