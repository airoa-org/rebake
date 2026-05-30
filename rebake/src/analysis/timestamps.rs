use super::AnalysisError;

const NANOSECONDS_PER_SECOND: f64 = 1_000_000_000.0;
const NANOSECONDS_PER_MILLISECOND: f64 = 1_000_000.0;

#[derive(Debug, Clone, PartialEq)]
/// Summary statistics for a sequence of message intervals.
pub struct IntervalStats {
    pub median_interval_ms: Option<f64>,
    pub interval_cv: Option<f64>,
    pub max_interval_ms: Option<f64>,
    pub max_interval_over_median: Option<f64>,
}

/// Return sorted timestamps that fall inside an inclusive window.
pub fn slice_timestamps_to_window(
    timestamps_ns: &[i64],
    start_ns: i64,
    end_ns: i64,
) -> Result<Vec<i64>, AnalysisError> {
    if end_ns < start_ns {
        return Err(AnalysisError::InvalidInclusiveWindow);
    }

    let timestamps = sorted_timestamps_ns(timestamps_ns);
    Ok(timestamps
        .into_iter()
        .filter(|timestamp| *timestamp >= start_ns && *timestamp <= end_ns)
        .collect())
}

/// Compute adjacent message intervals in milliseconds.
pub fn compute_message_intervals_ms(timestamps_ns: &[i64]) -> Vec<f64> {
    let timestamps = sorted_timestamps_ns(timestamps_ns);
    if timestamps.len() < 2 {
        return Vec::new();
    }

    timestamps
        .windows(2)
        .map(|window| (window[1] - window[0]) as f64 / NANOSECONDS_PER_MILLISECOND)
        .collect()
}

/// Measure how much of the window is covered by the observed timestamp span.
pub fn compute_coverage_ratio(
    timestamps_ns: &[i64],
    start_ns: i64,
    end_ns: i64,
) -> Result<f64, AnalysisError> {
    if end_ns <= start_ns {
        return Err(AnalysisError::InvalidStrictWindow);
    }

    let windowed = slice_timestamps_to_window(timestamps_ns, start_ns, end_ns)?;
    if windowed.is_empty() {
        return Ok(0.0);
    }

    let observed_span_ns = windowed[windowed.len() - 1] - windowed[0];
    let window_span_ns = end_ns - start_ns;
    let ratio = observed_span_ns as f64 / window_span_ns as f64;
    Ok(ratio.clamp(0.0, 1.0))
}

/// Estimate the observed message rate in Hertz.
pub fn compute_observed_hz(timestamps_ns: &[i64]) -> Option<f64> {
    let timestamps = sorted_timestamps_ns(timestamps_ns);
    if timestamps.len() < 2 {
        return None;
    }

    let observed_span_ns = timestamps[timestamps.len() - 1] - timestamps[0];
    if observed_span_ns <= 0 {
        return None;
    }

    let observed_span_s = observed_span_ns as f64 / NANOSECONDS_PER_SECOND;
    Some((timestamps.len() - 1) as f64 / observed_span_s)
}

/// Compute the interval summary used by topic timing metrics.
pub fn compute_interval_stats(intervals_ms: &[f64]) -> IntervalStats {
    if intervals_ms.is_empty() {
        return IntervalStats {
            median_interval_ms: None,
            interval_cv: None,
            max_interval_ms: None,
            max_interval_over_median: None,
        };
    }

    let median_interval_ms = median_interval_ms(intervals_ms);
    let max_interval_ms = max_interval_ms(intervals_ms);

    IntervalStats {
        median_interval_ms,
        interval_cv: interval_cv(intervals_ms),
        max_interval_ms,
        max_interval_over_median: max_interval_over_median(intervals_ms),
    }
}

/// Return a sorted copy of the timestamp sequence.
fn sorted_timestamps_ns(values: &[i64]) -> Vec<i64> {
    let mut timestamps = values.to_vec();
    timestamps.sort_unstable();
    timestamps
}

/// Compute the median interval in milliseconds.
fn median_interval_ms(intervals_ms: &[f64]) -> Option<f64> {
    if intervals_ms.is_empty() {
        return None;
    }

    Some(median(intervals_ms))
}

/// Compute the coefficient of variation for the interval sequence.
fn interval_cv(intervals_ms: &[f64]) -> Option<f64> {
    if intervals_ms.is_empty() {
        return None;
    }

    let mean_interval_ms = intervals_ms.iter().sum::<f64>() / intervals_ms.len() as f64;
    if mean_interval_ms <= 0.0 {
        return None;
    }

    if intervals_ms.len() == 1 {
        return Some(0.0);
    }

    let variance = intervals_ms
        .iter()
        .map(|interval| {
            let delta = *interval - mean_interval_ms;
            delta * delta
        })
        .sum::<f64>()
        / intervals_ms.len() as f64;
    Some(variance.sqrt() / mean_interval_ms)
}

/// Return the largest interval in milliseconds.
fn max_interval_ms(intervals_ms: &[f64]) -> Option<f64> {
    intervals_ms.iter().copied().reduce(f64::max)
}

/// Return the largest interval divided by the median interval.
fn max_interval_over_median(intervals_ms: &[f64]) -> Option<f64> {
    let max_interval_ms = max_interval_ms(intervals_ms)?;
    let median_interval_ms = median_interval_ms(intervals_ms)?;
    if median_interval_ms <= 0.0 {
        return None;
    }

    Some(max_interval_ms / median_interval_ms)
}

/// Compute the median of a non-empty numeric slice.
fn median(values: &[f64]) -> f64 {
    let mut sorted = values.to_vec();
    sorted.sort_by(|left, right| left.total_cmp(right));

    let middle = sorted.len() / 2;
    if sorted.len() % 2 == 1 {
        sorted[middle]
    } else {
        (sorted[middle - 1] + sorted[middle]) / 2.0
    }
}

#[cfg(test)]
mod tests {
    use super::{
        compute_coverage_ratio, compute_interval_stats, compute_message_intervals_ms,
        compute_observed_hz, slice_timestamps_to_window,
    };

    #[test]
    fn slice_timestamps_filters_inclusively_and_sorts() {
        let result = slice_timestamps_to_window(&[300, 100, 200, 400, 500], 200, 400).unwrap();
        assert_eq!(result, vec![200, 300, 400]);
    }

    #[test]
    fn compute_message_intervals_returns_adjacent_deltas() {
        let result = compute_message_intervals_ms(&[0, 100_000_000, 450_000_000]);
        assert_eq!(result, vec![100.0, 350.0]);
    }

    #[test]
    fn compute_coverage_ratio_uses_observed_span() {
        let ratio = compute_coverage_ratio(&[0, 100_000_000, 450_000_000], 0, 500_000_000).unwrap();
        assert_eq!(ratio, 0.9);
    }

    #[test]
    fn compute_observed_hz_uses_message_density() {
        let hz = compute_observed_hz(&[0, 100_000_000, 200_000_000, 300_000_000]).unwrap();
        assert_eq!(hz, 10.0);
    }

    #[test]
    fn compute_interval_stats_matches_python_semantics() {
        let stats = compute_interval_stats(&[100.0, 100.0, 300.0]);
        assert_eq!(stats.median_interval_ms, Some(100.0));
        assert_eq!(stats.max_interval_ms, Some(300.0));
        assert_eq!(stats.max_interval_over_median, Some(3.0));
        assert_eq!(stats.interval_cv, Some(0.565_685_424_949_238_1));
    }
}
