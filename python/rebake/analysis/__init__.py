"""Small analysis helpers that are reusable across rebake workflows.

The public surface intentionally stays small:

- low-level timestamp primitives for slicing and interval analysis
- topic-level convenience wrappers built on top of those primitives
- episode- and segment-level quality metrics derived from canonical metadata
- export-bundle convenience helpers that read rebake's intermediate format

The package does not know about workflow orchestration or quality-policy
decisions.
"""

from .episodes import (
    EPISODE_METRICS_SCHEMA,
    EPISODE_RELATIVE_METRICS_SCHEMA,
    compute_episode_metrics,
    compute_episode_metrics_from_export_bundle,
    compute_episode_relative_metrics,
)
from .segments import (
    SEGMENT_METRICS_SCHEMA,
    SEGMENT_RELATIVE_METRICS_SCHEMA,
    compute_segment_metrics,
    compute_segment_metrics_from_export_bundle,
    compute_segment_relative_metrics,
)
from .timestamps import (
    compute_coverage_ratio,
    compute_interval_stats,
    compute_message_intervals_ms,
    compute_observed_hz,
    slice_timestamps_to_window,
)
from .topics import (
    TOPIC_METRICS_SCHEMA,
    compute_topic_metrics,
    compute_topic_metrics_from_export_bundle,
    compute_topic_timing_metrics,
)

__all__ = [
    "slice_timestamps_to_window",
    "compute_message_intervals_ms",
    "compute_coverage_ratio",
    "compute_observed_hz",
    "compute_interval_stats",
    "compute_topic_timing_metrics",
    "TOPIC_METRICS_SCHEMA",
    "compute_topic_metrics",
    "compute_topic_metrics_from_export_bundle",
    "EPISODE_METRICS_SCHEMA",
    "EPISODE_RELATIVE_METRICS_SCHEMA",
    "compute_episode_metrics",
    "compute_episode_metrics_from_export_bundle",
    "compute_episode_relative_metrics",
    "SEGMENT_METRICS_SCHEMA",
    "SEGMENT_RELATIVE_METRICS_SCHEMA",
    "compute_segment_metrics",
    "compute_segment_metrics_from_export_bundle",
    "compute_segment_relative_metrics",
]
