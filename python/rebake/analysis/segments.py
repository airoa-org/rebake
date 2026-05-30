"""Segment-level quality metrics built on top of rebake's timing kernels."""

from __future__ import annotations

import json
from pathlib import Path
from typing import Any, Mapping, Sequence

import pyarrow as pa

from .. import _internal
from .export_bundle import (
    load_export_metadata,
    load_export_topic_type_map,
    load_topic_timestamps,
    resolve_parquet_dir,
)

SEGMENT_METRICS_SCHEMA = pa.schema(
    [
        pa.field("recording_uuid", pa.string(), nullable=False),
        pa.field("segment_index", pa.int64(), nullable=False),
        pa.field("segment_label", pa.string(), nullable=False),
        pa.field("segment_success", pa.bool_(), nullable=False),
        pa.field("segment_duration_s", pa.float64(), nullable=False),
        pa.field("has_all_required_topics", pa.bool_(), nullable=False),
        pa.field("minimum_required_segment_coverage_ratio", pa.float64(), nullable=True),
        pa.field("worst_topic_max_interval_ms", pa.float64(), nullable=True),
        pa.field("worst_topic_max_interval_over_median", pa.float64(), nullable=True),
    ]
)

SEGMENT_RELATIVE_METRICS_SCHEMA = pa.schema(
    [
        pa.field("recording_uuid", pa.string(), nullable=False),
        pa.field("segment_index", pa.int64(), nullable=False),
        pa.field("segment_label", pa.string(), nullable=False),
        pa.field("segment_duration_s", pa.float64(), nullable=False),
        pa.field("segment_label_group_size", pa.int64(), nullable=False),
        pa.field(
            "segment_duration_s_percentile_rank_within_label",
            pa.float64(),
            nullable=False,
        ),
    ]
)


def compute_segment_metrics(
    topic_timestamps_ns: Mapping[str, Any],
    metadata: Mapping[str, Any],
    required_topics: Sequence[str],
) -> pa.Table:
    """Compute one recording's segment-level timing metrics.

    Args:
        topic_timestamps_ns: Mapping from topic name to nanosecond timestamp sequence.
        metadata: Airoa metadata dictionary. V1.3 and V2.0 are both accepted.
        required_topics: Required topics used to evaluate each segment window.

    Returns:
        A ``pyarrow.Table`` matching ``SEGMENT_METRICS_SCHEMA``.
    """

    rows = _internal.analysis.compute_segment_metrics(
        topic_timestamps_ns,
        json.dumps(dict(metadata), ensure_ascii=False),
        list(required_topics),
    )
    return pa.Table.from_pylist(rows, schema=SEGMENT_METRICS_SCHEMA)


def compute_segment_relative_metrics(
    segment_metrics: pa.Table | Sequence[Mapping[str, Any]],
) -> pa.Table:
    """Compute duration percentile ranks within each ``segment_label`` cohort."""

    rows = (
        segment_metrics.to_pylist()
        if isinstance(segment_metrics, pa.Table)
        else [dict(row) for row in segment_metrics]
    )
    relative_rows = _internal.analysis.compute_segment_relative_metrics(
        json.dumps(rows, ensure_ascii=False)
    )
    relative_rows = [
        {
            "recording_uuid": row["recording_uuid"],
            "segment_index": row["segment_index"],
            "segment_label": row["segment_label"],
            "segment_duration_s": rows[index]["segment_duration_s"],
            "segment_label_group_size": row["segment_label_group_size"],
            "segment_duration_s_percentile_rank_within_label": row[
                "segment_duration_s_percentile_rank_within_segment_label"
            ],
        }
        for index, row in enumerate(relative_rows)
    ]
    return pa.Table.from_pylist(
        relative_rows,
        schema=SEGMENT_RELATIVE_METRICS_SCHEMA,
    )


def compute_segment_metrics_from_export_bundle(
    bundle_dir: str | Path,
    required_topics: Sequence[str],
) -> pa.Table:
    """Load one exported bundle and compute its segment metrics.

    The function accepts either the bundle root directory or the nested
    ``parquet/`` directory directly.
    """

    parquet_dir = resolve_parquet_dir(bundle_dir)
    metadata = load_export_metadata(parquet_dir)
    available_topics = set(load_export_topic_type_map(parquet_dir))
    topic_timestamps_ns = {
        topic_name: load_topic_timestamps(parquet_dir, topic_name)
        if topic_name in available_topics
        else []
        for topic_name in required_topics
    }
    return compute_segment_metrics(topic_timestamps_ns, metadata, required_topics)
