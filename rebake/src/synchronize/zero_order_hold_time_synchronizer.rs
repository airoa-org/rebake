use std::collections::HashMap;

use polars::prelude::{AsofStrategy, ChunkedArray, Column, DataFrame, LazyFrame, UInt64Type};

use crate::core::error::StageResult;
use crate::core::stage::{Stage, StageConfig};
use crate::synchronize::time_synchronizer::{
    TimeSynchronizer, TimestampIndex, TopicFrames, join_asof_with_strategy,
};
use serde::{Deserialize, Serialize};

/// Configuration for the `ZeroOrderHoldTimeSynchronizer`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ZeroOrderHoldTimeSynchronizerConfig {
    /// The target frame rate for the synchronized output timeline.
    pub fps: u32,
}

impl ZeroOrderHoldTimeSynchronizerConfig {
    pub fn new(fps: u32) -> Self {
        Self { fps }
    }
}

#[typetag::serde(name = "ZeroOrderHoldTimeSynchronizerConfig")]
impl StageConfig for ZeroOrderHoldTimeSynchronizerConfig {
    fn build(&self) -> Box<dyn Stage> {
        Box::new(ZeroOrderHoldTimeSynchronizer::new(self.fps))
    }
}

/// A time synchronizer that aligns multiple topics to a uniform timeline using a zero-order hold strategy.
///
/// This stage first creates a new, uniform timeline with a frequency determined by the `fps`
/// parameter in the `ZeroOrderHoldTimeSynchronizerConfig`. It then resamples each topic's DataFrame
/// onto this new timeline.
///
/// The resampling is performed using a backward as-of join (`AsofStrategy::Backward`). This means
/// for each timestamp in the new timeline, the synchronizer finds the most recent message from the
/// original topic that occurred at or before that timestamp.
///
/// The output is a new dataset where all topics share the same synchronized timestamps, making it
/// suitable for creating fixed-frequency datasets for machine learning.
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
pub struct ZeroOrderHoldTimeSynchronizer {
    fps: u32,
    topic_frames: Option<TopicFrames>,
    timestamp_index: Option<TimestampIndex>,
}

impl ZeroOrderHoldTimeSynchronizer {
    pub fn new(fps: u32) -> Self {
        Self {
            fps,
            topic_frames: None,
            timestamp_index: None,
        }
    }
}

impl TimeSynchronizer for ZeroOrderHoldTimeSynchronizer {
    fn name(&self) -> &'static str {
        "zero_order_hold_time_synchronizer"
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

    fn align_topic(
        &self,
        target_df: &DataFrame,
        target_column: &Column,
        source_df: &DataFrame,
    ) -> StageResult<DataFrame> {
        join_asof_with_strategy(target_df, target_column, source_df, AsofStrategy::Backward)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::core::stage::{Context, StageError};
    use crate::synchronize::time_synchronizer::{
        IS_FRESH_COL, ORIGINAL_TIMESTAMP_COL, SYNCHED_TIMESTAMP_COL,
    };
    use polars::{df, prelude::IntoLazy};
    use std::collections::HashMap;

    #[test]
    fn zero_order_hold_syncs_each_topic() {
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

        let mut synchronizer = ZeroOrderHoldTimeSynchronizer::new(4);
        synchronizer.set_topic_frames(frames.clone());
        synchronizer.set_timestamp_index(
            ZeroOrderHoldTimeSynchronizer::collect_timestamp_index(synchronizer.topic_frames())
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
            &[9, 15, 21, 26, 26, 26, 30],
            &[true, true, true, true, false, false, true],
        );
        assert_topic(
            &synced,
            &topic2_name,
            &expected_synched,
            &[41, 44, 50, 53, 53, 53, 57],
            &[true, true, true, true, false, false, true],
        );
        assert_topic(
            &synced,
            &topic3_name,
            &expected_synched,
            &[5, 9, 14, 14, 18, 18, 23],
            &[true, true, true, false, true, false, true],
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

    /// Normal case: synchronizes dataset and sets synched_timestamp_ns column and fps
    #[test]
    fn test_synchronize_sets_synched_timestamp_and_fps() {
        let config = ZeroOrderHoldTimeSynchronizerConfig { fps: 10 };
        let mut stage = config.build();

        let df = df! {
            "timestamp_ns" => [1_000_000_000u64, 1_100_000_000u64, 1_200_000_000u64],
            "value" => [1.0, 2.0, 3.0]
        }
        .unwrap()
        .lazy();

        let mut dataset = HashMap::new();
        dataset.insert("/test_topic".to_string(), df);

        let context = Context {
            dataset: Some(dataset),
            ..Default::default()
        };

        let result = stage.run(context);

        assert!(result.is_ok());
        let ctx = result.unwrap();
        assert_eq!(ctx.fps, Some(10));

        let dataset = ctx.dataset.unwrap();
        let topic_df = dataset
            .get("/test_topic")
            .unwrap()
            .clone()
            .collect()
            .unwrap();
        assert!(topic_df.column("synched_timestamp_ns").is_ok());
    }

    /// Edge case: synchronization succeeds even with sparse data (non-uniform intervals)
    #[test]
    fn test_synchronize_handles_sparse_data() {
        let config = ZeroOrderHoldTimeSynchronizerConfig { fps: 10 };
        let mut stage = config.build();

        let df = df! {
            "timestamp_ns" => [1_000_000_000u64, 1_500_000_000u64, 3_000_000_000u64],
            "value" => [1.0, 2.0, 3.0]
        }
        .unwrap()
        .lazy();

        let mut dataset = HashMap::new();
        dataset.insert("/sparse_topic".to_string(), df);

        let context = Context {
            dataset: Some(dataset),
            ..Default::default()
        };

        let result = stage.run(context);

        assert!(result.is_ok());
        let ctx = result.unwrap();
        assert!(ctx.dataset.is_some());
    }

    /// Error case: returns MissingData error when dataset is not set
    #[test]
    fn test_synchronize_missing_dataset_returns_missing_data_error() {
        let config = ZeroOrderHoldTimeSynchronizerConfig { fps: 10 };
        let mut stage = config.build();

        let context = Context::default();

        let result = stage.run(context);

        assert!(result.is_err());
        let err = result.err().unwrap();
        assert!(matches!(err, StageError::MissingData(_)));
    }
}
