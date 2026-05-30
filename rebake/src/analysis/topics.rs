use super::AnalysisError;
use super::timestamps::{
    compute_coverage_ratio, compute_interval_stats, compute_message_intervals_ms,
    compute_observed_hz, slice_timestamps_to_window,
};

const NANOSECONDS_PER_SECOND: f64 = 1_000_000_000.0;

#[derive(Debug, Clone, PartialEq)]
/// Generic timing metrics for one logical topic inside an episode window.
pub struct TopicTimingMetrics {
    pub message_count: usize,
    pub first_message_timestamp_ns: Option<i64>,
    pub last_message_timestamp_ns: Option<i64>,
    pub observed_topic_span_s: f64,
    pub episode_coverage_ratio: f64,
    pub observed_hz: Option<f64>,
    pub median_topic_interval_ms: Option<f64>,
    pub topic_interval_cv: Option<f64>,
    pub max_topic_interval_ms: Option<f64>,
    pub max_interval_over_median: Option<f64>,
}

/// Compute generic timing metrics for one topic inside an episode window.
pub fn compute_topic_timing_metrics(
    timestamps_ns: &[i64],
    episode_start_ns: i64,
    episode_end_ns: i64,
) -> Result<TopicTimingMetrics, AnalysisError> {
    let windowed = slice_timestamps_to_window(timestamps_ns, episode_start_ns, episode_end_ns)?;
    let message_count = windowed.len();

    let first_message_timestamp_ns = windowed.first().copied();
    let last_message_timestamp_ns = windowed.last().copied();
    let observed_topic_span_s = match (first_message_timestamp_ns, last_message_timestamp_ns) {
        (Some(first), Some(last)) => (last - first) as f64 / NANOSECONDS_PER_SECOND,
        _ => 0.0,
    };

    let interval_stats = compute_interval_stats(&compute_message_intervals_ms(&windowed));

    Ok(TopicTimingMetrics {
        message_count,
        first_message_timestamp_ns,
        last_message_timestamp_ns,
        observed_topic_span_s,
        episode_coverage_ratio: compute_coverage_ratio(
            &windowed,
            episode_start_ns,
            episode_end_ns,
        )?,
        observed_hz: compute_observed_hz(&windowed),
        median_topic_interval_ms: interval_stats.median_interval_ms,
        topic_interval_cv: interval_stats.interval_cv,
        max_topic_interval_ms: interval_stats.max_interval_ms,
        max_interval_over_median: interval_stats.max_interval_over_median,
    })
}

#[cfg(test)]
mod tests {
    use super::compute_topic_timing_metrics;

    #[test]
    fn compute_topic_timing_metrics_summarizes_windowed_topic() {
        let metrics = compute_topic_timing_metrics(
            &[0, 100_000_000, 200_000_000, 450_000_000, 700_000_000],
            0,
            500_000_000,
        )
        .unwrap();

        assert_eq!(metrics.message_count, 4);
        assert_eq!(metrics.first_message_timestamp_ns, Some(0));
        assert_eq!(metrics.last_message_timestamp_ns, Some(450_000_000));
        assert_eq!(metrics.observed_topic_span_s, 0.45);
        assert_eq!(metrics.episode_coverage_ratio, 0.9);
        assert_eq!(metrics.observed_hz, Some(3.0 / 0.45));
        assert_eq!(metrics.median_topic_interval_ms, Some(100.0));
        assert_eq!(metrics.max_topic_interval_ms, Some(250.0));
        assert_eq!(metrics.max_interval_over_median, Some(2.5));
    }
}
