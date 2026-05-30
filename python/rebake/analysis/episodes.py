"""Episode-level quality metrics derived from topic metrics."""

from __future__ import annotations

import math
from pathlib import Path
from typing import Any, Mapping, Sequence

import pyarrow as pa

from .export_bundle import load_export_metadata, resolve_parquet_dir
from .topics import (
    TOPIC_METRICS_SCHEMA,
    compute_topic_metrics_from_export_bundle,
)

EPISODE_METRICS_SCHEMA = pa.schema(
    [
        pa.field("recording_uuid", pa.string(), nullable=False),
        pa.field("episode_label", pa.string(), nullable=False),
        pa.field("episode_success", pa.bool_(), nullable=False),
        pa.field("episode_duration_s", pa.float64(), nullable=False),
        pa.field("has_all_required_topics", pa.bool_(), nullable=False),
        pa.field("minimum_required_episode_coverage_ratio", pa.float64(), nullable=True),
        pa.field("worst_topic_max_interval_ms", pa.float64(), nullable=True),
        pa.field("worst_topic_max_interval_over_median", pa.float64(), nullable=True),
        pa.field("worst_topic_interval_cv", pa.float64(), nullable=True),
    ]
)

EPISODE_RELATIVE_METRICS_SCHEMA = pa.schema(
    [
        pa.field("recording_uuid", pa.string(), nullable=False),
        pa.field("episode_label", pa.string(), nullable=False),
        pa.field("episode_duration_s", pa.float64(), nullable=False),
        pa.field("episode_label_group_size", pa.int64(), nullable=False),
        pa.field(
            "episode_duration_s_percentile_rank_within_label",
            pa.float64(),
            nullable=False,
        ),
    ]
)


def compute_episode_metrics(
    metadata: Mapping[str, Any],
    topic_metrics: pa.Table | Sequence[Mapping[str, Any]],
    required_topics: Sequence[str] = (),
) -> pa.Table:
    """Aggregate one recording's topic metrics into one episode metrics row."""

    topic_rows = _topic_metrics_rows(topic_metrics)
    required_rows = [row for row in topic_rows if row["is_required_topic"]]
    required_topic_count = (
        len(set(required_topics)) if required_topics else _required_topic_count(topic_rows)
    )
    missing_required_count = max(required_topic_count - len(required_rows), 0)

    has_all_required_topics = (
        missing_required_count == 0
        and all(int(row["message_count"]) > 0 for row in required_rows)
    )
    minimum_required_episode_coverage_ratio = _minimum_required_coverage(
        required_rows,
        missing_required_count,
        required_topic_count,
    )

    episode = _episode(metadata)
    episode_duration_s = float(episode["end_time"]) - float(episode["start_time"])
    if not math.isfinite(episode_duration_s):
        raise ValueError("episode duration must be finite")
    if episode_duration_s < 0:
        raise ValueError("episode.end_time must be greater than or equal to start_time")

    return pa.Table.from_pylist(
        [
            {
                "recording_uuid": _recording_uuid(metadata),
                "episode_label": _episode_label(episode),
                "episode_success": bool(episode.get("success", False)),
                "episode_duration_s": episode_duration_s,
                "has_all_required_topics": has_all_required_topics,
                "minimum_required_episode_coverage_ratio": minimum_required_episode_coverage_ratio,
                "worst_topic_max_interval_ms": _max_non_null(
                    required_rows,
                    "max_topic_interval_ms",
                ),
                "worst_topic_max_interval_over_median": _max_non_null(
                    required_rows,
                    "max_interval_over_median",
                ),
                "worst_topic_interval_cv": _max_non_null(
                    required_rows,
                    "topic_interval_cv",
                ),
            }
        ],
        schema=EPISODE_METRICS_SCHEMA,
    )


def compute_episode_metrics_from_export_bundle(
    bundle_dir: str | Path,
    required_topics: Sequence[str] = (),
    required_topic_profile: str | None = None,
    topic_metrics: pa.Table | None = None,
) -> pa.Table:
    """Load one exported bundle and compute one episode metrics row."""

    parquet_dir = resolve_parquet_dir(bundle_dir)
    metadata = load_export_metadata(parquet_dir)
    if topic_metrics is None:
        topic_metrics = compute_topic_metrics_from_export_bundle(
            parquet_dir,
            required_topics,
            required_topic_profile,
        )
    return compute_episode_metrics(metadata, topic_metrics, required_topics)


def compute_episode_relative_metrics(
    episode_metrics: pa.Table | Sequence[Mapping[str, Any]],
) -> pa.Table:
    """Compute duration percentile ranks within each ``episode_label`` cohort."""

    rows = (
        episode_metrics.to_pylist()
        if isinstance(episode_metrics, pa.Table)
        else [dict(row) for row in episode_metrics]
    )
    label_positions: dict[str, list[int]] = {}
    for index, row in enumerate(rows):
        label_positions.setdefault(str(row["episode_label"]), []).append(index)

    output_rows: list[dict[str, Any] | None] = [None] * len(rows)
    for positions in label_positions.values():
        percentile_ranks = _compute_percentile_ranks(
            [float(rows[position]["episode_duration_s"]) for position in positions]
        )
        for position, percentile_rank in zip(positions, percentile_ranks, strict=True):
            row = rows[position]
            output_rows[position] = {
                "recording_uuid": row["recording_uuid"],
                "episode_label": row["episode_label"],
                "episode_duration_s": row["episode_duration_s"],
                "episode_label_group_size": len(positions),
                "episode_duration_s_percentile_rank_within_label": percentile_rank,
            }

    return pa.Table.from_pylist(
        [row for row in output_rows if row is not None],
        schema=EPISODE_RELATIVE_METRICS_SCHEMA,
    )


def _topic_metrics_rows(
    topic_metrics: pa.Table | Sequence[Mapping[str, Any]],
) -> list[dict[str, Any]]:
    if isinstance(topic_metrics, pa.Table):
        if topic_metrics.schema != TOPIC_METRICS_SCHEMA:
            topic_metrics = topic_metrics.cast(TOPIC_METRICS_SCHEMA)
        return topic_metrics.to_pylist()
    return [dict(row) for row in topic_metrics]


def _required_topic_count(topic_rows: Sequence[Mapping[str, Any]]) -> int:
    counts = {
        int(row["required_topic_count"])
        for row in topic_rows
        if row.get("required_topic_count") is not None
    }
    if len(counts) > 1:
        raise ValueError("topic metrics contain inconsistent required_topic_count values")
    return next(iter(counts), 0)


def _minimum_required_coverage(
    required_rows: Sequence[Mapping[str, Any]],
    missing_required_count: int,
    required_topic_count: int,
) -> float | None:
    if required_topic_count == 0:
        return None
    if missing_required_count > 0:
        return 0.0
    values = [
        float(row["episode_coverage_ratio"])
        for row in required_rows
        if row.get("episode_coverage_ratio") is not None
    ]
    return min(values) if values else 0.0


def _max_non_null(rows: Sequence[Mapping[str, Any]], key: str) -> float | None:
    values = [float(row[key]) for row in rows if row.get(key) is not None]
    return max(values) if values else None


def _episode(metadata: Mapping[str, Any]) -> Mapping[str, Any]:
    episode = metadata.get("episode")
    if not isinstance(episode, Mapping):
        raise ValueError("metadata must contain an episode object")
    return episode


def _recording_uuid(metadata: Mapping[str, Any]) -> str:
    uuid = metadata.get("uuid")
    if not isinstance(uuid, str) or not uuid:
        raise ValueError("metadata must contain a non-empty string uuid")
    return uuid


def _episode_label(episode: Mapping[str, Any]) -> str:
    label = episode.get("label")
    if not isinstance(label, str):
        raise ValueError("metadata.episode.label must be a string")
    return label


def _compute_percentile_ranks(values: Sequence[float]) -> list[float]:
    if not values:
        return []
    if len(values) == 1:
        return [0.5]

    indexed_values = sorted(enumerate(values), key=lambda item: item[1])
    ranks = [0.0] * len(values)
    start_index = 0
    while start_index < len(indexed_values):
        value = indexed_values[start_index][1]
        end_index = start_index
        while (
            end_index + 1 < len(indexed_values)
            and indexed_values[end_index + 1][1] == value
        ):
            end_index += 1

        percentile_rank = ((start_index + end_index) / 2.0) / (
            len(indexed_values) - 1
        )
        for position in range(start_index, end_index + 1):
            ranks[indexed_values[position][0]] = percentile_rank
        start_index = end_index + 1

    return ranks
