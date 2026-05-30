use std::collections::HashMap;

use polars::prelude::*;

use crate::core::error::{OptionExt, PolarsExt, ResultExt, StageError, StageResult};
use crate::core::stage::{Context, Stage, StageConfig};
use crate::synchronize::time_synchronizer::{ORIGINAL_TIMESTAMP_COL, SYNCHED_TIMESTAMP_COL};
use serde::{Deserialize, Serialize};

/// Configuration for the `TimestampMergeTimeSynchronizer`.
///
/// This is currently a placeholder as the synchronizer does not require any parameters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct TimestampMergeTimeSynchronizerConfig {}

impl TimestampMergeTimeSynchronizerConfig {
    pub fn new() -> Self {
        Self::default()
    }
}

#[typetag::serde(name = "TimestampMergeTimeSynchronizerConfig")]
impl StageConfig for TimestampMergeTimeSynchronizerConfig {
    fn build(&self) -> Box<dyn Stage> {
        Box::new(TimestampMergeTimeSynchronizer::new())
    }
}

/// A time synchronizer that aligns multiple topics to a non-uniform timeline created
/// by merging all unique timestamps from all topics.
///
/// This stage first collects every unique timestamp from all topics in the dataset.
/// It then creates a new, unified timeline by sorting these unique timestamps. This results
/// in a non-uniform timeline where each point corresponds to an actual message timestamp
/// from at least one of the original topics.
///
/// Finally, it resamples each topic's DataFrame onto this new merged timeline using a
/// zero-order hold (backward as-of join) strategy. This ensures that at any given
/// timestamp in the new timeline, every topic has a value, which is the last known value
/// from that topic.
///
/// # Preconditions
///
/// - `dataset`: **Required** (all topics as LazyFrame)
///
/// # Postconditions
///
/// - `dataset`: **Guaranteed** (all topics with `synched_timestamp_ns` column added)
///
/// Note: Unlike `ZeroOrderHoldTimeSynchronizer` and `NearestNeighborTimeSynchronizer`,
/// this synchronizer does NOT set `fps` since it produces a non-uniform timeline.
///
/// # Errors
///
/// - [`StageError::MissingData`]: `dataset` not set in context
#[derive(Default)]
pub struct TimestampMergeTimeSynchronizer {
    dataset: Option<HashMap<String, LazyFrame>>,
}

impl TimestampMergeTimeSynchronizer {
    pub fn new() -> Self {
        Self::default()
    }

    fn time_bounds(
        timestamps: &HashMap<String, ChunkedArray<UInt64Type>>,
    ) -> StageResult<(u64, u64)> {
        let mut start = u64::MIN;
        let mut end = u64::MIN;

        for ts in timestamps.values() {
            let min = ts.min().ok_or_else(|| {
                StageError::invalid("empty timestamp array - no data to synchronize")
            })?;
            let max = ts.max().ok_or_else(|| {
                StageError::invalid("empty timestamp array - no data to synchronize")
            })?;
            start = start.max(min);
            end = end.max(max);
        }
        Ok((start, end))
    }

    fn collect_timestamp_index(
        dataset: &HashMap<String, LazyFrame>,
    ) -> StageResult<HashMap<String, ChunkedArray<UInt64Type>>> {
        dataset
            .iter()
            .map(|(path, frame)| {
                let collected = frame
                    .clone()
                    .select([col(ORIGINAL_TIMESTAMP_COL)])
                    .collect()
                    .with_context(format!(
                        "failed to collect timestamp column for topic '{path}'"
                    ))?;
                let column = collected
                    .column(ORIGINAL_TIMESTAMP_COL)
                    .or_invalid(&format!(
                        "missing '{ORIGINAL_TIMESTAMP_COL}' column in topic '{path}'"
                    ))?;
                let timestamps = column.u64().or_invalid(&format!(
                    "'{ORIGINAL_TIMESTAMP_COL}' column is not u64 in topic '{path}'"
                ))?;
                Ok((path.clone(), timestamps.clone()))
            })
            .collect()
    }
}

impl Stage for TimestampMergeTimeSynchronizer {
    fn name(&self) -> &'static str {
        "timeline_merge_time_synchronizer"
    }

    fn run(&mut self, mut context: Context) -> Result<Context, StageError> {
        self.dataset = Some(context.dataset.take().or_missing("dataset in context")?);
        // INVARIANT: dataset was just set on the line above, so it's guaranteed to be Some
        #[allow(clippy::expect_used)]
        let dataset = self
            .dataset
            .as_ref()
            .expect("dataset was just set - contract violation");

        let all_timestamps = Self::collect_timestamp_index(dataset)?;
        let (start_time, end_time) = Self::time_bounds(&all_timestamps)?;

        let merged_timestamps: UInt64Chunked = all_timestamps
            .into_values()
            .reduce(|mut accumulated_timestamps, timestamps| {
                let _ = accumulated_timestamps.append(&timestamps);
                accumulated_timestamps
            })
            .ok_or_else(|| StageError::invalid("no timestamps found in dataset"))?
            .sort(false);
        let merged_timestamps_df = df! {
            SYNCHED_TIMESTAMP_COL => merged_timestamps.into_series()
        }
        .with_context("failed to create merged timestamps DataFrame")?
        .lazy()
        .filter(
            col(SYNCHED_TIMESTAMP_COL)
                .gt_eq(lit(start_time))
                .and(col(SYNCHED_TIMESTAMP_COL).lt_eq(lit(end_time))),
        )
        .collect()
        .with_context("failed to filter merged timestamps by time bounds")?;

        let mut synched_dfs = HashMap::new();
        for (path, lf) in dataset {
            let original_df = lf.clone().collect().with_context(format!(
                "failed to collect topic '{path}' for synchronization"
            ))?;
            let target_column = merged_timestamps_df
                .column(SYNCHED_TIMESTAMP_COL)
                .or_invalid(&format!(
                    "merged timestamps missing '{SYNCHED_TIMESTAMP_COL}' column"
                ))?;
            let synched_df = join_asof_with_strategy(
                &merged_timestamps_df,
                target_column,
                &original_df,
                AsofStrategy::Backward,
            )?;
            synched_dfs.insert(path.clone(), synched_df.lazy());
        }

        context.dataset = Some(synched_dfs);
        Ok(context)
    }
}

fn join_asof_with_strategy(
    target_df: &DataFrame,
    target_column: &Column,
    source_df: &DataFrame,
    strategy: AsofStrategy,
) -> StageResult<DataFrame> {
    let source_timestamp = source_df
        .column(ORIGINAL_TIMESTAMP_COL)
        .or_invalid(&format!(
            "source DataFrame missing '{ORIGINAL_TIMESTAMP_COL}' column for asof join"
        ))?;
    target_df
        ._join_asof(
            source_df,
            target_column.as_materialized_series(),
            source_timestamp.as_materialized_series(),
            strategy,
            None,
            None,
            None,
            true,
            true,
            false,
        )
        .with_context("asof join failed during timestamp merge synchronization")
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::core::stage::{Context, Stage};
    use polars::{df, prelude::IntoLazy};

    fn build_topic_frame(timestamps: &[u64], column: &str, values: &[i32]) -> LazyFrame {
        df! {
            ORIGINAL_TIMESTAMP_COL => timestamps,
            column => values,
        }
        .unwrap()
        .lazy()
    }

    #[test]
    fn merges_timestamps_and_backfills_latest_values() {
        let topic_a = build_topic_frame(&[100, 200, 400], "value_a", &[1, 2, 4]);
        let topic_b = build_topic_frame(&[150, 250, 350], "value_b", &[10, 20, 30]);
        let dataset = HashMap::from([
            ("/topic_a".to_string(), topic_a),
            ("/topic_b".to_string(), topic_b),
        ]);

        let mut synchronizer = TimestampMergeTimeSynchronizer::new();
        let context = Context::new(dataset);
        let context = synchronizer.run(context).unwrap();
        let result = context.dataset.expect("synchronized dataset must exist");

        assert_eq!(result.len(), 2, "all topics should remain present");

        let expected_timeline = vec![150_u64, 200, 250, 350, 400];

        let topic_a_frame = result
            .get("/topic_a")
            .expect("topic_a should be synchronized")
            .clone()
            .collect()
            .unwrap();
        let expected_topic_a = df! {
            SYNCHED_TIMESTAMP_COL => &expected_timeline,
            ORIGINAL_TIMESTAMP_COL => &[100_u64, 200, 200, 200, 400],
            "value_a" => &[1_i32, 2, 2, 2, 4],
        }
        .unwrap();
        polars_testing::assert_dataframe_equal!(&topic_a_frame, &expected_topic_a);

        let topic_b_frame = result
            .get("/topic_b")
            .expect("topic_b should be synchronized")
            .clone()
            .collect()
            .unwrap();
        let expected_topic_b = df! {
            SYNCHED_TIMESTAMP_COL => &expected_timeline,
            ORIGINAL_TIMESTAMP_COL => &[150_u64, 150, 250, 350, 350],
            "value_b" => &[10_i32, 10, 20, 30, 30],
        }
        .unwrap();
        polars_testing::assert_dataframe_equal!(&topic_b_frame, &expected_topic_b);
    }
}
