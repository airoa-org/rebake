use std::collections::HashMap;

use polars::prelude::{
    AsofJoin, AsofStrategy, ChunkAgg, ChunkedArray, Column, DataFrame, IntoLazy, LazyFrame,
    NamedFrom, Series, UInt64Type, col, df, lit,
};

use crate::core::error::{OptionExt, PolarsExt, ResultExt, StageError, StageResult};
use crate::core::stage::{Context, Stage};

pub const SYNCHED_TIMESTAMP_COL: &str = "synched_timestamp_ns";
pub const ORIGINAL_TIMESTAMP_COL: &str = "timestamp_ns";
pub const IS_FRESH_COL: &str = "is_fresh";

/// Type alias for topic frames mapping topic names to lazy frames.
///
/// Used by [`TimeSynchronizer`] implementations to store and manipulate
/// topic data during synchronization.
pub type TopicFrames = HashMap<String, LazyFrame>;

/// Type alias for timestamp index mapping topic names to their timestamp arrays.
///
/// Used by [`TimeSynchronizer`] implementations to track timestamps
/// for efficient time-based lookups.
pub type TimestampIndex = HashMap<String, ChunkedArray<UInt64Type>>;

/// Time synchronizer trait for aligning topic frames to a unified timeline.
///
/// # Contract
///
/// The following accessor methods require prior initialization via `set_*()`:
///
/// - `topic_frames()` / `topic_frames_mut()` → requires `set_topic_frames()` first
/// - `timestamp_index()` / `timestamp_index_mut()` → requires `set_timestamp_index()` first
///
/// The default `synchronize()` implementation guarantees this initialization order.
/// Direct calls to accessor methods without prior `set_*()` will panic with a
/// descriptive message.
///
/// Implementors should use `expect()` with clear messages in accessor methods:
/// ```ignore
/// fn topic_frames(&self) -> &TopicFrames {
///     self.topic_frames.as_ref()
///         .expect("topic_frames must be set via set_topic_frames() before access")
/// }
/// ```
pub trait TimeSynchronizer {
    fn name(&self) -> &'static str;
    fn fps(&self) -> u32;

    /// Returns a reference to topic frames.
    ///
    /// # Panics
    ///
    /// Panics if `set_topic_frames()` was not called before this method.
    fn topic_frames(&self) -> &TopicFrames;

    /// Returns a mutable reference to topic frames.
    ///
    /// # Panics
    ///
    /// Panics if `set_topic_frames()` was not called before this method.
    fn topic_frames_mut(&mut self) -> &mut TopicFrames;

    /// Returns a reference to the timestamp index.
    ///
    /// # Panics
    ///
    /// Panics if `set_timestamp_index()` was not called before this method.
    fn timestamp_index(&self) -> &TimestampIndex;

    /// Returns a mutable reference to the timestamp index.
    ///
    /// # Panics
    ///
    /// Panics if `set_timestamp_index()` was not called before this method.
    fn timestamp_index_mut(&mut self) -> &mut TimestampIndex;

    fn set_topic_frames(&mut self, frames: HashMap<String, LazyFrame>);
    fn set_timestamp_index(&mut self, index: HashMap<String, ChunkedArray<UInt64Type>>);

    fn synchronize(
        &mut self,
        frames: HashMap<String, LazyFrame>,
    ) -> StageResult<HashMap<String, LazyFrame>> {
        self.set_topic_frames(frames);
        self.set_timestamp_index(Self::collect_timestamp_index(self.topic_frames())?);
        let timeline = self.timeline()?;
        let frames = std::mem::take(self.topic_frames_mut());
        let (synched, _excluded) = self.align_topics(frames, &timeline)?;
        *self.topic_frames_mut() = synched.clone();

        Ok(synched)
    }

    fn timeline(&self) -> StageResult<SyncTimeline> {
        let (start, end) = self.time_bounds()?;
        let table = self.timeline_table(start, end)?;
        SyncTimeline::new(table, start, end)
    }

    fn align_topics(
        &self,
        frames: TopicFrames,
        timeline: &SyncTimeline,
    ) -> StageResult<(TopicFrames, TopicFrames)> {
        let mut synched = HashMap::with_capacity(frames.len());
        let mut excluded = HashMap::with_capacity(frames.len());

        for (path, frame) in frames {
            let (synched_frame, excluded_frame) = self.align_topic_frame(timeline, &frame)?;
            synched.insert(path.clone(), synched_frame);
            excluded.insert(path, excluded_frame);
        }

        Ok((synched, excluded))
    }

    fn align_topic_frame(
        &self,
        timeline: &SyncTimeline,
        frame: &LazyFrame,
    ) -> StageResult<(LazyFrame, LazyFrame)> {
        let source = self.collect_source(frame)?;
        let aligned = self.align_topic(timeline.table(), timeline.timestamps(), &source)?;
        let aligned = append_is_fresh_column(aligned)?;
        let excluded = self.excluded_rows(&source, timeline, &aligned)?;
        Ok((aligned.lazy(), excluded.lazy()))
    }

    fn collect_source(&self, frame: &LazyFrame) -> StageResult<DataFrame> {
        frame
            .clone()
            .collect()
            .with_context("failed to collect source frame for synchronization")
    }

    fn align_topic(
        &self,
        target_df: &DataFrame,
        target_column: &Column,
        source_df: &DataFrame,
    ) -> StageResult<DataFrame>;

    fn timeline_table(&self, start_time: u64, end_time: u64) -> StageResult<DataFrame> {
        df! {
            SYNCHED_TIMESTAMP_COL => Self::target_timestamps(start_time, end_time, self.fps())
        }
        .with_context("failed to create timeline DataFrame")
    }

    fn time_bounds(&self) -> StageResult<(u64, u64)> {
        let mut start = u64::MIN;
        let mut end = u64::MIN;

        for timestamps in self.timestamp_index().values() {
            let min = timestamps.min().ok_or_else(|| {
                StageError::invalid("empty timestamp array - no data to synchronize")
            })?;
            let max = timestamps.max().ok_or_else(|| {
                StageError::invalid("empty timestamp array - no data to synchronize")
            })?;
            start = start.max(min);
            end = end.max(max);
        }

        Ok((start, end))
    }

    fn target_timestamps(start_time: u64, end_time: u64, fps: u32) -> Vec<u64> {
        let base_step = 1_000_000_000u64 / fps as u64;
        let mut timestamps = Vec::new();
        let mut current = start_time;

        while current <= end_time {
            timestamps.push(current);
            current += base_step;
        }

        timestamps
    }

    fn collect_timestamp_index(frames: &TopicFrames) -> StageResult<TimestampIndex> {
        frames
            .iter()
            .map(|(path, frame)| {
                let collected = frame
                    .clone()
                    .select([polars::prelude::col(ORIGINAL_TIMESTAMP_COL)])
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

    fn excluded_rows(
        &self,
        source_df: &DataFrame,
        timeline: &SyncTimeline,
        _aligned_df: &DataFrame,
    ) -> StageResult<DataFrame> {
        source_df
            .clone()
            .lazy()
            .filter(
                col(ORIGINAL_TIMESTAMP_COL)
                    .lt(lit(timeline.start()))
                    .or(col(ORIGINAL_TIMESTAMP_COL).gt(lit(timeline.end()))),
            )
            .collect()
            .with_context("failed to collect excluded rows outside timeline bounds")
    }
}

impl<T> Stage for T
where
    T: TimeSynchronizer + Send + Sync,
{
    fn name(&self) -> &'static str {
        self.name()
    }

    fn run(&mut self, mut context: Context) -> Result<Context, StageError> {
        let dataset = context.dataset.take().or_missing("dataset in context")?;

        self.set_topic_frames(dataset);
        self.set_timestamp_index(Self::collect_timestamp_index(self.topic_frames())?);
        let timeline = self.timeline()?;
        let frames = std::mem::take(self.topic_frames_mut());
        let (synched, _excluded) = self.align_topics(frames, &timeline)?;
        *self.topic_frames_mut() = synched.clone();

        context.set_dataset(synched);
        context.set_fps(self.fps() as usize);
        Ok(context)
    }
}

pub(crate) fn join_asof_with_strategy(
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
        .with_context("asof join failed during time synchronization")
}

/// Represents a synchronized timeline for time alignment.
///
/// This struct holds the target timestamps at the specified FPS,
/// along with the time bounds of the original data. It is created
/// by [`TimeSynchronizer::timeline()`] and used internally during
/// the synchronization process.
///
/// The timeline contains:
/// - A DataFrame with `synched_timestamp_ns` column at regular intervals
/// - The start and end timestamps from the source data
pub struct SyncTimeline {
    table: DataFrame,
    timestamps: Column,
    start: u64,
    end: u64,
}

impl SyncTimeline {
    fn new(table: DataFrame, start: u64, end: u64) -> StageResult<Self> {
        let timestamps = table
            .column(SYNCHED_TIMESTAMP_COL)
            .or_invalid(&format!(
                "timeline table missing '{SYNCHED_TIMESTAMP_COL}' column"
            ))?
            .clone();
        Ok(Self {
            table,
            timestamps,
            start,
            end,
        })
    }

    fn table(&self) -> &DataFrame {
        &self.table
    }

    fn timestamps(&self) -> &Column {
        &self.timestamps
    }

    fn start(&self) -> u64 {
        self.start
    }

    fn end(&self) -> u64 {
        self.end
    }
}

fn append_is_fresh_column(mut df: DataFrame) -> StageResult<DataFrame> {
    let column = df.column(ORIGINAL_TIMESTAMP_COL).or_invalid(&format!(
        "DataFrame missing '{ORIGINAL_TIMESTAMP_COL}' column for is_fresh computation"
    ))?;
    let timestamps = column.u64().or_invalid(&format!(
        "'{ORIGINAL_TIMESTAMP_COL}' column is not u64 for is_fresh computation"
    ))?;

    let mut prev: Option<u64> = None;
    let mut flags = Vec::with_capacity(timestamps.len());

    for timestamp in timestamps.clone().into_iter() {
        match timestamp {
            Some(current) => {
                let is_fresh = prev != Some(current);
                flags.push(is_fresh);
                prev = Some(current);
            }
            None => flags.push(false),
        }
    }

    let series = Series::new(IS_FRESH_COL.into(), flags);
    df.with_column(series).or_invalid(&format!(
        "failed to add '{IS_FRESH_COL}' column to DataFrame"
    ))?;
    Ok(df)
}
