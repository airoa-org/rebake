use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::{AnalysisError, compute_topic_timing_metrics};
use crate::schema::metadata::MetadataV2_0;

const NANOSECONDS_PER_SECOND: f64 = 1_000_000_000.0;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
/// Segment-scoped timing quality metrics derived from canonical metadata.
pub struct SegmentMetricsRow {
    pub recording_uuid: String,
    pub segment_index: usize,
    pub segment_label: String,
    pub segment_success: bool,
    pub segment_duration_s: f64,
    pub has_all_required_topics: bool,
    pub minimum_required_segment_coverage_ratio: Option<f64>,
    pub worst_topic_max_interval_ms: Option<f64>,
    pub worst_topic_max_interval_over_median: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
/// Segment-level duration rank within the same segment label cohort.
pub struct SegmentRelativeMetricsRow {
    pub recording_uuid: String,
    pub segment_index: usize,
    pub segment_label: String,
    pub segment_label_group_size: usize,
    pub segment_duration_s_percentile_rank_within_segment_label: f64,
}

/// Compute per-segment timing quality metrics from metadata and topic timestamps.
pub fn compute_segment_metrics(
    metadata: &MetadataV2_0,
    topic_timestamps_ns: &HashMap<String, Vec<i64>>,
    required_topics: &[String],
) -> Result<Vec<SegmentMetricsRow>, AnalysisError> {
    let mut rows = Vec::with_capacity(metadata.segments.len());

    for (segment_index, segment) in metadata.segments.iter().enumerate() {
        if segment.end_time <= segment.start_time {
            return Err(AnalysisError::InvalidSegmentWindow { segment_index });
        }

        let segment_label = metadata.labels.get(segment.label_idx).ok_or(
            AnalysisError::SegmentLabelIndexOutOfBounds {
                segment_index,
                label_idx: segment.label_idx,
                labels_len: metadata.labels.len(),
            },
        )?;

        let segment_start_ns = seconds_to_nanoseconds(segment.start_time)?;
        let segment_end_ns = seconds_to_nanoseconds(segment.end_time)?;
        let segment_duration_s = segment.end_time - segment.start_time;

        let mut minimum_required_segment_coverage_ratio = None;
        let mut worst_topic_max_interval_ms = None;
        let mut worst_topic_max_interval_over_median = None;
        let mut has_all_required_topics = true;

        for topic_name in required_topics {
            let topic_timestamps = topic_timestamps_ns
                .get(topic_name)
                .map(Vec::as_slice)
                .unwrap_or(&[]);
            let topic_metrics =
                compute_topic_timing_metrics(topic_timestamps, segment_start_ns, segment_end_ns)?;

            if topic_metrics.message_count == 0 {
                has_all_required_topics = false;
            }
            minimum_required_segment_coverage_ratio = min_optional_f64(
                minimum_required_segment_coverage_ratio,
                Some(topic_metrics.episode_coverage_ratio),
            );
            worst_topic_max_interval_ms = max_optional_f64(
                worst_topic_max_interval_ms,
                topic_metrics.max_topic_interval_ms,
            );
            worst_topic_max_interval_over_median = max_optional_f64(
                worst_topic_max_interval_over_median,
                topic_metrics.max_interval_over_median,
            );
        }

        rows.push(SegmentMetricsRow {
            recording_uuid: metadata.uuid.clone(),
            segment_index,
            segment_label: segment_label.clone(),
            segment_success: segment.success,
            segment_duration_s,
            has_all_required_topics,
            minimum_required_segment_coverage_ratio,
            worst_topic_max_interval_ms,
            worst_topic_max_interval_over_median,
        });
    }

    Ok(rows)
}

/// Compute segment-duration percentile ranks within each segment label cohort.
pub fn compute_segment_relative_metrics(
    segment_metrics: &[SegmentMetricsRow],
) -> Vec<SegmentRelativeMetricsRow> {
    let mut label_positions: HashMap<&str, Vec<usize>> = HashMap::new();
    for (index, row) in segment_metrics.iter().enumerate() {
        label_positions
            .entry(row.segment_label.as_str())
            .or_default()
            .push(index);
    }

    let mut output_rows: Vec<Option<SegmentRelativeMetricsRow>> = vec![None; segment_metrics.len()];

    for positions in label_positions.values() {
        let percentile_ranks = compute_percentile_ranks(
            &positions
                .iter()
                .map(|position| segment_metrics[*position].segment_duration_s)
                .collect::<Vec<_>>(),
        );
        for (position, percentile_rank) in positions.iter().zip(percentile_ranks.iter()) {
            let row = &segment_metrics[*position];
            output_rows[*position] = Some(SegmentRelativeMetricsRow {
                recording_uuid: row.recording_uuid.clone(),
                segment_index: row.segment_index,
                segment_label: row.segment_label.clone(),
                segment_label_group_size: positions.len(),
                segment_duration_s_percentile_rank_within_segment_label: *percentile_rank,
            });
        }
    }

    output_rows
        .into_iter()
        .map(Option::unwrap)
        .collect::<Vec<_>>()
}

fn seconds_to_nanoseconds(seconds: f64) -> Result<i64, AnalysisError> {
    if !seconds.is_finite() {
        return Err(AnalysisError::InvalidTimestampValue);
    }

    let nanoseconds = seconds * NANOSECONDS_PER_SECOND;
    if nanoseconds < i64::MIN as f64 || nanoseconds > i64::MAX as f64 {
        return Err(AnalysisError::InvalidTimestampValue);
    }

    Ok(nanoseconds.round() as i64)
}

fn min_optional_f64(current: Option<f64>, candidate: Option<f64>) -> Option<f64> {
    match (current, candidate) {
        (Some(left), Some(right)) => Some(left.min(right)),
        (Some(left), None) => Some(left),
        (None, Some(right)) => Some(right),
        (None, None) => None,
    }
}

fn max_optional_f64(current: Option<f64>, candidate: Option<f64>) -> Option<f64> {
    match (current, candidate) {
        (Some(left), Some(right)) => Some(left.max(right)),
        (Some(left), None) => Some(left),
        (None, Some(right)) => Some(right),
        (None, None) => None,
    }
}

fn compute_percentile_ranks(values: &[f64]) -> Vec<f64> {
    if values.is_empty() {
        return Vec::new();
    }
    if values.len() == 1 {
        return vec![0.5];
    }

    let mut indexed_values = values.iter().copied().enumerate().collect::<Vec<_>>();
    indexed_values.sort_by(|left, right| left.1.total_cmp(&right.1));

    let mut ranks = vec![0.0; values.len()];
    let mut start_index = 0;
    while start_index < indexed_values.len() {
        let value = indexed_values[start_index].1;
        let mut end_index = start_index;
        while end_index + 1 < indexed_values.len() && indexed_values[end_index + 1].1 == value {
            end_index += 1;
        }

        let percentile_rank =
            ((start_index + end_index) as f64 / 2.0) / (indexed_values.len() - 1) as f64;
        for position in start_index..=end_index {
            ranks[indexed_values[position].0] = percentile_rank;
        }

        start_index = end_index + 1;
    }

    ranks
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::{compute_segment_metrics, compute_segment_relative_metrics};
    use crate::schema::metadata::v2_0::{
        Device, EnvType, Environment, Episode, File, MetadataV2_0, Program, Robot, Runner,
        RunnerType, Segment, Source,
    };

    fn test_metadata() -> MetadataV2_0 {
        MetadataV2_0 {
            schema: "https://example.com/schema.json".to_string(),
            schema_version: "2.0".to_string(),
            uuid: "uuid-123".to_string(),
            robot: Robot {
                uri: None,
                robot_type: "test_robot".to_string(),
                id: "robot-1".to_string(),
                checksum: None,
            },
            files: vec![File {
                file_type: "mcap".to_string(),
                name: "recording.mcap".to_string(),
                checksum: None,
            }],
            environment: Environment {
                env_type: EnvType::RealWorld,
                site: "lab".to_string(),
                location: None,
            },
            runner: Runner {
                runner_type: RunnerType::Operator,
                organization: "airoa".to_string(),
                name: "operator".to_string(),
            },
            devices: vec![Device {
                role: "controller".to_string(),
                device_type: "joystick".to_string(),
                id: "joy-1".to_string(),
            }],
            programs: vec![Program {
                role: "data_collection".to_string(),
                name: "rebake-cli".to_string(),
                source: Source { git: None },
            }],
            episode: Episode {
                start_time: 1.0,
                end_time: 7.0,
                success: true,
                label: "episode".to_string(),
            },
            labels: vec!["pick".to_string(), "place".to_string()],
            segments: vec![
                Segment {
                    start_time: 1.0,
                    end_time: 4.0,
                    label_idx: 0,
                    success: true,
                },
                Segment {
                    start_time: 4.0,
                    end_time: 7.0,
                    label_idx: 1,
                    success: false,
                },
            ],
        }
    }

    #[test]
    fn compute_segment_metrics_aggregates_required_topic_quality() {
        let metadata = test_metadata();
        let mut timestamps = HashMap::new();
        timestamps.insert(
            "/required/a".to_string(),
            vec![
                1_000_000_000,
                2_000_000_000,
                3_000_000_000,
                4_000_000_000,
                6_000_000_000,
            ],
        );
        timestamps.insert(
            "/required/b".to_string(),
            vec![2_000_000_000, 3_000_000_000, 6_000_000_000],
        );
        let required_topics = vec!["/required/a".to_string(), "/required/b".to_string()];

        let rows = compute_segment_metrics(&metadata, &timestamps, &required_topics).unwrap();

        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].recording_uuid, "uuid-123");
        assert_eq!(rows[0].segment_index, 0);
        assert_eq!(rows[0].segment_label, "pick");
        assert!(rows[0].segment_success);
        assert_eq!(rows[0].segment_duration_s, 3.0);
        assert!(rows[0].has_all_required_topics);
        assert_eq!(
            rows[0].minimum_required_segment_coverage_ratio,
            Some(1.0 / 3.0)
        );
        assert_eq!(rows[0].worst_topic_max_interval_ms, Some(1_000.0));
        assert_eq!(rows[0].worst_topic_max_interval_over_median, Some(1.0));

        assert_eq!(rows[1].segment_label, "place");
        assert!(!rows[1].segment_success);
        assert_eq!(rows[1].segment_duration_s, 3.0);
        assert!(rows[1].has_all_required_topics);
        assert_eq!(rows[1].minimum_required_segment_coverage_ratio, Some(0.0));
        assert_eq!(rows[1].worst_topic_max_interval_ms, Some(2_000.0));
        assert_eq!(rows[1].worst_topic_max_interval_over_median, Some(1.0));
    }

    #[test]
    fn compute_segment_metrics_treats_missing_required_topics_as_missing_data() {
        let metadata = test_metadata();
        let required_topics = vec!["/missing".to_string()];

        let rows = compute_segment_metrics(&metadata, &HashMap::new(), &required_topics).unwrap();

        assert_eq!(rows.len(), 2);
        assert!(!rows[0].has_all_required_topics);
        assert_eq!(rows[0].minimum_required_segment_coverage_ratio, Some(0.0));
        assert_eq!(rows[0].worst_topic_max_interval_ms, None);
        assert_eq!(rows[0].worst_topic_max_interval_over_median, None);
    }

    #[test]
    fn compute_segment_metrics_rejects_out_of_bounds_label_index() {
        let mut metadata = test_metadata();
        metadata.segments[0].label_idx = 99;

        let error = compute_segment_metrics(&metadata, &HashMap::new(), &[]).unwrap_err();

        assert_eq!(
            error.to_string(),
            "segment 0 references label_idx 99, but labels has length 2"
        );
    }

    #[test]
    fn compute_segment_relative_metrics_uses_tie_aware_percentile_ranks() {
        let rows = vec![
            super::SegmentMetricsRow {
                recording_uuid: "uuid-1".to_string(),
                segment_index: 0,
                segment_label: "pick".to_string(),
                segment_success: true,
                segment_duration_s: 2.0,
                has_all_required_topics: true,
                minimum_required_segment_coverage_ratio: Some(1.0),
                worst_topic_max_interval_ms: Some(100.0),
                worst_topic_max_interval_over_median: Some(1.0),
            },
            super::SegmentMetricsRow {
                recording_uuid: "uuid-2".to_string(),
                segment_index: 0,
                segment_label: "pick".to_string(),
                segment_success: true,
                segment_duration_s: 4.0,
                has_all_required_topics: true,
                minimum_required_segment_coverage_ratio: Some(1.0),
                worst_topic_max_interval_ms: Some(100.0),
                worst_topic_max_interval_over_median: Some(1.0),
            },
            super::SegmentMetricsRow {
                recording_uuid: "uuid-3".to_string(),
                segment_index: 0,
                segment_label: "pick".to_string(),
                segment_success: true,
                segment_duration_s: 4.0,
                has_all_required_topics: true,
                minimum_required_segment_coverage_ratio: Some(1.0),
                worst_topic_max_interval_ms: Some(100.0),
                worst_topic_max_interval_over_median: Some(1.0),
            },
            super::SegmentMetricsRow {
                recording_uuid: "uuid-4".to_string(),
                segment_index: 0,
                segment_label: "place".to_string(),
                segment_success: true,
                segment_duration_s: 3.0,
                has_all_required_topics: true,
                minimum_required_segment_coverage_ratio: Some(1.0),
                worst_topic_max_interval_ms: Some(100.0),
                worst_topic_max_interval_over_median: Some(1.0),
            },
        ];

        let relative_rows = compute_segment_relative_metrics(&rows);

        assert_eq!(relative_rows.len(), 4);
        assert_eq!(
            relative_rows[0].segment_duration_s_percentile_rank_within_segment_label,
            0.0
        );
        assert_eq!(
            relative_rows[1].segment_duration_s_percentile_rank_within_segment_label,
            0.75
        );
        assert_eq!(
            relative_rows[2].segment_duration_s_percentile_rank_within_segment_label,
            0.75
        );
        assert_eq!(relative_rows[3].segment_label_group_size, 1);
        assert_eq!(
            relative_rows[3].segment_duration_s_percentile_rank_within_segment_label,
            0.5
        );
    }
}
