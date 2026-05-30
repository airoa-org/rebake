"""Topic-level convenience wrappers built on top of timestamp primitives."""

from __future__ import annotations

import math
from pathlib import Path
from typing import Any, Mapping, Sequence

import numpy.typing as npt
import pyarrow as pa

from .. import _internal
from .export_bundle import (
    load_export_metadata,
    load_export_topic_type_map,
    load_topic_timestamps,
    resolve_parquet_dir,
)

NANOSECONDS_PER_SECOND = 1_000_000_000

TOPIC_METRICS_SCHEMA = pa.schema(
    [
        pa.field("recording_uuid", pa.string(), nullable=False),
        pa.field("topic_name", pa.string(), nullable=False),
        pa.field("message_type", pa.string(), nullable=False),
        pa.field("required_topic_profile", pa.string(), nullable=True),
        pa.field("required_topic_count", pa.int64(), nullable=True),
        pa.field("is_required_topic", pa.bool_(), nullable=False),
        pa.field("message_count", pa.int64(), nullable=False),
        pa.field("first_message_timestamp_ns", pa.int64(), nullable=True),
        pa.field("last_message_timestamp_ns", pa.int64(), nullable=True),
        pa.field("observed_topic_span_s", pa.float64(), nullable=False),
        pa.field("episode_coverage_ratio", pa.float64(), nullable=False),
        pa.field("observed_hz", pa.float64(), nullable=True),
        pa.field("median_topic_interval_ms", pa.float64(), nullable=True),
        pa.field("topic_interval_cv", pa.float64(), nullable=True),
        pa.field("max_topic_interval_ms", pa.float64(), nullable=True),
        pa.field("max_interval_over_median", pa.float64(), nullable=True),
    ]
)


def compute_topic_timing_metrics(
    timestamps_ns: npt.ArrayLike,
    episode_start_ns: int,
    episode_end_ns: int,
) -> dict[str, float | int | None]:
    """Compute generic timing metrics for one logical topic.

    Args:
        timestamps_ns: One-dimensional sequence of nanosecond timestamps for a
            single logical topic.
        episode_start_ns: Inclusive episode start in nanoseconds.
        episode_end_ns: Inclusive episode end in nanoseconds.

    Returns:
        A dictionary containing message count, observed span, coverage, and
        interval-stability metrics for the topic within the episode window.
    """

    return _internal.analysis.compute_topic_timing_metrics(
        timestamps_ns,
        episode_start_ns,
        episode_end_ns,
    )


def compute_topic_metrics(
    topic_timestamps_ns: Mapping[str, Any],
    metadata: Mapping[str, Any],
    topic_type_map: Mapping[str, str],
    required_topics: Sequence[str] = (),
    required_topic_profile: str | None = None,
) -> pa.Table:
    """Compute topic-level timing metrics for one recording.

    Args:
        topic_timestamps_ns: Mapping from topic name to nanosecond timestamps.
        metadata: Airoa metadata dictionary with an ``episode`` window.
        topic_type_map: Mapping from topic name to ROS message type.
        required_topics: Topics used by downstream episode/segment quality checks.
        required_topic_profile: Optional label for the required-topic policy.

    Returns:
        A ``pyarrow.Table`` matching ``TOPIC_METRICS_SCHEMA``.
    """

    recording_uuid = _recording_uuid(metadata)
    episode = _episode(metadata)
    episode_start_ns = _seconds_to_nanoseconds(episode["start_time"])
    episode_end_ns = _seconds_to_nanoseconds(episode["end_time"])
    required_topic_set = set(required_topics)
    required_topic_count = len(required_topic_set) if required_topics else None

    rows = []
    for topic_name, message_type in sorted(topic_type_map.items()):
        metrics = compute_topic_timing_metrics(
            topic_timestamps_ns.get(topic_name, []),
            episode_start_ns,
            episode_end_ns,
        )
        rows.append(
            {
                "recording_uuid": recording_uuid,
                "topic_name": topic_name,
                "message_type": message_type,
                "required_topic_profile": required_topic_profile,
                "required_topic_count": required_topic_count,
                "is_required_topic": topic_name in required_topic_set,
                **metrics,
            }
        )

    return pa.Table.from_pylist(rows, schema=TOPIC_METRICS_SCHEMA)


def compute_topic_metrics_from_export_bundle(
    bundle_dir: str | Path,
    required_topics: Sequence[str] = (),
    required_topic_profile: str | None = None,
) -> pa.Table:
    """Load one exported bundle and compute topic metrics for all exported topics.

    The function accepts either the bundle root directory or the nested
    ``parquet/`` directory directly.
    """

    parquet_dir = resolve_parquet_dir(bundle_dir)
    metadata = load_export_metadata(parquet_dir)
    topic_type_map = load_export_topic_type_map(parquet_dir)
    topic_timestamps_ns = {
        topic_name: load_topic_timestamps(parquet_dir, topic_name)
        for topic_name in topic_type_map
    }
    return compute_topic_metrics(
        topic_timestamps_ns,
        metadata,
        topic_type_map,
        required_topics,
        required_topic_profile,
    )


def _recording_uuid(metadata: Mapping[str, Any]) -> str:
    uuid = metadata.get("uuid")
    if not isinstance(uuid, str) or not uuid:
        raise ValueError("metadata must contain a non-empty string uuid")
    return uuid


def _episode(metadata: Mapping[str, Any]) -> Mapping[str, Any]:
    episode = metadata.get("episode")
    if not isinstance(episode, Mapping):
        raise ValueError("metadata must contain an episode object")
    return episode


def _seconds_to_nanoseconds(seconds: Any) -> int:
    seconds_f64 = float(seconds)
    if not math.isfinite(seconds_f64):
        raise ValueError("episode timestamps must be finite")
    return int(round(seconds_f64 * NANOSECONDS_PER_SECOND))
