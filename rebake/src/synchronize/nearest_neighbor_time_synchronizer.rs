use std::collections::HashMap;

use polars::datatypes::{DataType, PlSmallStr};
use polars::prelude::{
    AsofStrategy, ChunkedArray, Column, DataFrame, DataFrameJoinOps, Expr, IntoLazy, JoinArgs,
    JoinType, LazyFrame, SortMultipleOptions, UInt64Type, col, when,
};
use serde::{Deserialize, Serialize};

use crate::core::error::{PolarsExt, ResultExt, StageResult};
use crate::core::stage::{Stage, StageConfig};
use crate::synchronize::time_synchronizer::{
    ORIGINAL_TIMESTAMP_COL, SYNCHED_TIMESTAMP_COL, TimeSynchronizer, TimestampIndex, TopicFrames,
    join_asof_with_strategy,
};

/// Configuration for the `NearestNeighborTimeSynchronizer`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NearestNeighborTimeSynchronizerConfig {
    /// The target frame rate for the synchronized output timeline.
    fps: u32,
}

impl NearestNeighborTimeSynchronizerConfig {
    pub fn new(fps: u32) -> Self {
        Self { fps }
    }
}

#[typetag::serde(name = "NearestNeighborTimeSynchronizerConfig")]
impl StageConfig for NearestNeighborTimeSynchronizerConfig {
    fn build(&self) -> Box<dyn Stage> {
        Box::new(NearestNeighborTimeSynchronizer::new(self.fps))
    }
}

/// A time synchronizer that aligns multiple topics to a uniform timeline using a nearest neighbor strategy.
///
/// This stage creates a new, uniform timeline based on the `fps` parameter. It then resamples
/// each topic's DataFrame by finding the message with the timestamp closest to each point in the
/// new timeline.
///
/// To achieve this, it performs both a backward and a forward as-of join and then selects the
/// candidate (either backward or forward) with the smaller time difference from the target
/// synchronized timestamp. This ensures that each point on the new timeline is matched with the
/// absolute nearest neighbor from the original data.
///
/// # Preconditions
///
/// - `dataset`: **Required** (all topics as LazyFrame)
///
/// # Postconditions
///
/// - `dataset`: **Guaranteed** (all topics with `synched_timestamp_ns` column added)
/// - `fps`: **Guaranteed** (set from config)
///
/// # Errors
///
/// - `StageError::MissingData`: `dataset` not set in context
pub struct NearestNeighborTimeSynchronizer {
    fps: u32,
    topic_frames: Option<TopicFrames>,
    timestamp_index: Option<TimestampIndex>,
}

impl NearestNeighborTimeSynchronizer {
    pub fn new(fps: u32) -> Self {
        Self {
            fps,
            topic_frames: None,
            timestamp_index: None,
        }
    }

    fn append_suffix_except(data: &mut DataFrame, suffix: &str, skip: &str) -> StageResult<()> {
        let names: Vec<String> = data
            .get_column_names()
            .iter()
            .map(|name| name.to_string())
            .collect();

        for name in names {
            if name == skip {
                continue;
            }
            let renamed = format!("{}{}", name, suffix);
            data.rename(&name, PlSmallStr::from(renamed.as_str()))
                .or_invalid(&format!("failed to rename column '{name}' to '{renamed}'"))?;
        }
        Ok(())
    }

    fn squared_time_difference(target_col: &str, candidate_col: &str, alias: &str) -> Expr {
        let diff = col(target_col).cast(DataType::Int64) - col(candidate_col).cast(DataType::Int64);
        (diff.clone() * diff).alias(alias)
    }

    fn choose_nearest(combined: DataFrame, base_columns: &[String]) -> StageResult<DataFrame> {
        let diff_backward = Self::squared_time_difference(
            SYNCHED_TIMESTAMP_COL,
            ORIGINAL_TIMESTAMP_COL,
            "diff_backward_sq",
        );
        let forward_column = format!("{}_forward", ORIGINAL_TIMESTAMP_COL);
        let diff_forward = Self::squared_time_difference(
            SYNCHED_TIMESTAMP_COL,
            forward_column.as_str(),
            "diff_forward_sq",
        );

        let prefer_forward = col(forward_column.as_str()).is_not_null().and(
            col("diff_backward_sq")
                .is_null()
                .or(col("diff_forward_sq").lt(col("diff_backward_sq"))),
        );

        let column_switch_exprs: Vec<Expr> = base_columns
            .iter()
            .map(|name| Self::resolve_value_expr(name, prefer_forward.clone()))
            .collect();

        let select_exprs = Self::select_projection(base_columns);

        combined
            .lazy()
            .with_columns(vec![diff_backward, diff_forward])
            .with_columns(column_switch_exprs)
            .select(select_exprs)
            .collect()
            .with_context("failed to choose nearest neighbor candidates")
    }

    fn resolve_value_expr(column_name: &str, prefer_forward: Expr) -> Expr {
        let forward_name = format!("{}_forward", column_name);
        when(prefer_forward)
            .then(col(&forward_name))
            .otherwise(col(column_name))
            .alias(column_name)
    }

    fn select_projection(base_columns: &[String]) -> Vec<Expr> {
        let mut select_exprs = Vec::with_capacity(base_columns.len() + 1);
        select_exprs.push(col(SYNCHED_TIMESTAMP_COL));
        for name in base_columns {
            select_exprs.push(col(name));
        }
        select_exprs
    }

    fn combine_candidates(backward: DataFrame, forward: DataFrame) -> StageResult<DataFrame> {
        backward
            .join(
                &forward,
                [SYNCHED_TIMESTAMP_COL],
                [SYNCHED_TIMESTAMP_COL],
                JoinArgs::new(JoinType::Left),
                None,
            )
            .or_invalid("failed to join backward and forward candidates")
    }
}

impl TimeSynchronizer for NearestNeighborTimeSynchronizer {
    fn name(&self) -> &'static str {
        "nearest_neighbor_time_synchronizer"
    }

    fn fps(&self) -> u32 {
        self.fps
    }

    /// # Contract
    ///
    /// This method is only called during synchronization after `set_topic_frames` has been called.
    /// The synchronize trait method guarantees topic_frames is Some before calling accessor methods.
    fn topic_frames(&self) -> &TopicFrames {
        // CONTRACT: synchronize trait guarantees set_topic_frames called before this accessor
        #[allow(clippy::expect_used)]
        self.topic_frames
            .as_ref()
            .expect("topic_frames must be set before calling accessor - contract violation")
    }

    /// # Contract
    ///
    /// This method is only called during synchronization after `set_topic_frames` has been called.
    fn topic_frames_mut(&mut self) -> &mut TopicFrames {
        // CONTRACT: synchronize trait guarantees set_topic_frames called before this accessor
        #[allow(clippy::expect_used)]
        self.topic_frames
            .as_mut()
            .expect("topic_frames must be set before calling accessor - contract violation")
    }

    /// # Contract
    ///
    /// This method is only called during synchronization after `set_timestamp_index` has been called.
    /// The synchronize trait method guarantees timestamp_index is Some before calling accessor methods.
    fn timestamp_index(&self) -> &TimestampIndex {
        // CONTRACT: synchronize trait guarantees set_timestamp_index called before this accessor
        #[allow(clippy::expect_used)]
        self.timestamp_index
            .as_ref()
            .expect("timestamp_index must be set before calling accessor - contract violation")
    }

    /// # Contract
    ///
    /// This method is only called during synchronization after `set_timestamp_index` has been called.
    fn timestamp_index_mut(&mut self) -> &mut TimestampIndex {
        // CONTRACT: synchronize trait guarantees set_timestamp_index called before this accessor
        #[allow(clippy::expect_used)]
        self.timestamp_index
            .as_mut()
            .expect("timestamp_index must be set before calling accessor - contract violation")
    }

    fn set_topic_frames(&mut self, frames: HashMap<String, LazyFrame>) {
        self.topic_frames = Some(frames);
    }

    fn set_timestamp_index(&mut self, index: HashMap<String, ChunkedArray<UInt64Type>>) {
        self.timestamp_index = Some(index);
    }

    fn collect_source(&self, lazy_frame: &LazyFrame) -> StageResult<DataFrame> {
        let collected = lazy_frame
            .clone()
            .collect()
            .with_context("failed to collect source frame for nearest neighbor synchronization")?;
        collected
            .sort([ORIGINAL_TIMESTAMP_COL], SortMultipleOptions::default())
            .or_invalid(&format!(
                "failed to sort source frame by '{ORIGINAL_TIMESTAMP_COL}'"
            ))
    }

    fn align_topic(
        &self,
        target_df: &DataFrame,
        target_column: &Column,
        source_df: &DataFrame,
    ) -> StageResult<DataFrame> {
        let base_columns: Vec<String> = source_df
            .get_column_names()
            .iter()
            .map(|name| name.to_string())
            .collect();

        let backward =
            join_asof_with_strategy(target_df, target_column, source_df, AsofStrategy::Backward)?;
        let mut forward =
            join_asof_with_strategy(target_df, target_column, source_df, AsofStrategy::Forward)?;
        Self::append_suffix_except(&mut forward, "_forward", SYNCHED_TIMESTAMP_COL)?;

        let combined = Self::combine_candidates(backward, forward)?;
        Self::choose_nearest(combined, &base_columns)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::synchronize::time_synchronizer::{
        IS_FRESH_COL, ORIGINAL_TIMESTAMP_COL, SYNCHED_TIMESTAMP_COL,
    };
    use polars::{df, prelude::IntoLazy};
    use std::collections::HashMap;

    #[test]
    fn nearest_neighbor_syncs_each_topic() {
        let topic1_name = "topic1".to_string();
        let topic2_name = "topic2".to_string();
        let topic3_name = "topic3".to_string();

        let topic1_df = df! {
            ORIGINAL_TIMESTAMP_COL => &[
                400_000_000_u64,
                650_000_000,
                900_000_000,
                1_200_000_000,
                1_900_000_000,
                2_050_000_000,
            ],
            "value" => &[9_i32, 15, 21, 26, 30, 34],
        }
        .unwrap();

        let topic2_df = df! {
            ORIGINAL_TIMESTAMP_COL => &[
                450_000_000_u64,
                680_000_000,
                940_000_000,
                1_250_000_000,
                1_820_000_000,
                2_030_000_000,
            ],
            "value" => &[41_i32, 44, 50, 53, 57, 63],
        }
        .unwrap();

        let topic3_df = df! {
            ORIGINAL_TIMESTAMP_COL => &[
                500_000_000_u64,
                730_000_000,
                980_000_000,
                1_260_000_000,
                1_880_000_000,
                2_010_000_000,
            ],
            "value" => &[5_i32, 9, 14, 18, 23, 27],
        }
        .unwrap();

        let mut frames = HashMap::new();
        frames.insert(topic1_name.clone(), topic1_df.lazy());
        frames.insert(topic2_name.clone(), topic2_df.lazy());
        frames.insert(topic3_name.clone(), topic3_df.lazy());

        let mut synchronizer = NearestNeighborTimeSynchronizer::new(4);
        synchronizer.set_topic_frames(frames.clone());
        synchronizer.set_timestamp_index(
            NearestNeighborTimeSynchronizer::collect_timestamp_index(synchronizer.topic_frames())
                .unwrap(),
        );

        let (start_time, end_time) = synchronizer.time_bounds().unwrap();
        assert_eq!(start_time, 500_000_000);
        assert_eq!(end_time, 2_050_000_000);

        let synced = synchronizer.synchronize(frames).unwrap();
        assert_eq!(synced.len(), 3);

        let expected_synched = [
            500_000_000,
            750_000_000,
            1_000_000_000,
            1_250_000_000,
            1_500_000_000,
            1_750_000_000,
            2_000_000_000,
        ];

        assert_topic(
            &synced,
            &topic1_name,
            &expected_synched,
            &[9, 15, 21, 26, 26, 30, 34],
            &[true, true, true, true, false, true, true],
        );
        assert_topic(
            &synced,
            &topic2_name,
            &expected_synched,
            &[41, 44, 50, 53, 53, 57, 63],
            &[true, true, true, true, false, true, true],
        );
        assert_topic(
            &synced,
            &topic3_name,
            &expected_synched,
            &[5, 9, 14, 18, 18, 23, 27],
            &[true, true, true, true, false, true, true],
        );
    }

    fn assert_topic(
        synced: &HashMap<String, LazyFrame>,
        topic: &String,
        expected_synched: &[u64],
        expected_values: &[i32],
        expected_fresh: &[bool],
    ) {
        let df = synced.get(topic).unwrap().clone().collect().unwrap();
        let synched: Vec<u64> = df
            .column(SYNCHED_TIMESTAMP_COL)
            .unwrap()
            .u64()
            .unwrap()
            .into_iter()
            .map(|v| v.unwrap())
            .collect();
        assert_eq!(synched.as_slice(), expected_synched);

        let values: Vec<i32> = df
            .column("value")
            .unwrap()
            .i32()
            .unwrap()
            .into_iter()
            .map(|v| v.unwrap())
            .collect();
        assert_eq!(values, expected_values);

        let is_fresh: Vec<bool> = df
            .column(IS_FRESH_COL)
            .unwrap()
            .bool()
            .unwrap()
            .into_iter()
            .map(|v| v.unwrap())
            .collect();
        assert_eq!(is_fresh, expected_fresh);
    }
}
