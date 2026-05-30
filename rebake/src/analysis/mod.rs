//! Reusable analysis kernels for timestamp-based topic metrics.

pub mod segments;
pub mod timestamps;
pub mod topics;

pub use segments::{
    SegmentMetricsRow, SegmentRelativeMetricsRow, compute_segment_metrics,
    compute_segment_relative_metrics,
};
pub use timestamps::{
    IntervalStats, compute_coverage_ratio, compute_interval_stats, compute_message_intervals_ms,
    compute_observed_hz, slice_timestamps_to_window,
};
pub use topics::{TopicTimingMetrics, compute_topic_timing_metrics};

#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
/// Errors returned by the timestamp analysis helpers.
pub enum AnalysisError {
    #[error("end_ns must be greater than or equal to start_ns")]
    InvalidInclusiveWindow,
    #[error("end_ns must be greater than start_ns")]
    InvalidStrictWindow,
    #[error("segment {segment_index} has end_time <= start_time")]
    InvalidSegmentWindow { segment_index: usize },
    #[error(
        "segment {segment_index} references label_idx {label_idx}, but labels has length {labels_len}"
    )]
    SegmentLabelIndexOutOfBounds {
        segment_index: usize,
        label_idx: usize,
        labels_len: usize,
    },
    #[error("metadata contained a non-finite or out-of-range timestamp value")]
    InvalidTimestampValue,
}
