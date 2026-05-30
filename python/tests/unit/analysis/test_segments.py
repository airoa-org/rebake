from __future__ import annotations

from pathlib import Path

import pyarrow as pa
import pyarrow.parquet as pq
import pytest
from rebake.analysis import (
    SEGMENT_METRICS_SCHEMA,
    SEGMENT_RELATIVE_METRICS_SCHEMA,
    compute_segment_metrics,
    compute_segment_metrics_from_export_bundle,
    compute_segment_relative_metrics,
)
from rebake.core import metadata_to_arrow


def _metadata_dict() -> dict:
    return {
        "$schema": "https://example.com/schema.json",
        "schema_version": "2.0",
        "uuid": "uuid-123",
        "robot": {
            "uri": None,
            "type": "test_robot",
            "id": "robot-1",
            "checksum": None,
        },
        "files": [{"type": "mcap", "name": "recording.mcap", "checksum": None}],
        "environment": {"type": "real_world", "site": "lab", "location": None},
        "runner": {
            "type": "operator",
            "organization": "airoa",
            "name": "operator",
        },
        "devices": [{"role": "controller", "type": "joystick", "id": "joy-1"}],
        "programs": [{"role": "data_collection", "name": "rebake-cli", "source": {"git": None}}],
        "episode": {
            "start_time": 1.0,
            "end_time": 7.0,
            "success": True,
            "label": "episode",
        },
        "labels": ["pick", "place"],
        "segments": [
            {
                "start_time": 1.0,
                "end_time": 4.0,
                "label_idx": 0,
                "success": True,
            },
            {
                "start_time": 4.0,
                "end_time": 7.0,
                "label_idx": 1,
                "success": False,
            },
        ],
    }


def test_compute_segment_metrics_returns_expected_arrow_table() -> None:
    table = compute_segment_metrics(
        topic_timestamps_ns={
            "/required/a": [
                1_000_000_000,
                2_000_000_000,
                3_000_000_000,
                4_000_000_000,
                6_000_000_000,
            ],
            "/required/b": [2_000_000_000, 3_000_000_000, 6_000_000_000],
        },
        metadata=_metadata_dict(),
        required_topics=["/required/a", "/required/b"],
    )

    assert table.schema == SEGMENT_METRICS_SCHEMA
    assert table.num_rows == 2

    rows = table.to_pylist()
    assert rows[0]["recording_uuid"] == "uuid-123"
    assert rows[0]["segment_label"] == "pick"
    assert rows[0]["segment_success"] is True
    assert rows[0]["segment_duration_s"] == 3.0
    assert rows[0]["has_all_required_topics"] is True
    assert rows[0]["minimum_required_segment_coverage_ratio"] == pytest.approx(1.0 / 3.0)
    assert rows[0]["worst_topic_max_interval_ms"] == pytest.approx(1000.0)
    assert rows[0]["worst_topic_max_interval_over_median"] == pytest.approx(1.0)

    assert rows[1]["segment_label"] == "place"
    assert rows[1]["segment_success"] is False
    assert rows[1]["minimum_required_segment_coverage_ratio"] == pytest.approx(0.0)
    assert rows[1]["worst_topic_max_interval_ms"] == pytest.approx(2000.0)
    assert rows[1]["worst_topic_max_interval_over_median"] == pytest.approx(1.0)


def test_compute_segment_relative_metrics_returns_tie_aware_percentiles() -> None:
    segment_metrics = pa.Table.from_pylist(
        [
            {
                "recording_uuid": "uuid-1",
                "segment_index": 0,
                "segment_label": "pick",
                "segment_success": True,
                "segment_duration_s": 2.0,
                "has_all_required_topics": True,
                "minimum_required_segment_coverage_ratio": 1.0,
                "worst_topic_max_interval_ms": 100.0,
                "worst_topic_max_interval_over_median": 1.0,
            },
            {
                "recording_uuid": "uuid-2",
                "segment_index": 0,
                "segment_label": "pick",
                "segment_success": True,
                "segment_duration_s": 4.0,
                "has_all_required_topics": True,
                "minimum_required_segment_coverage_ratio": 1.0,
                "worst_topic_max_interval_ms": 100.0,
                "worst_topic_max_interval_over_median": 1.0,
            },
            {
                "recording_uuid": "uuid-3",
                "segment_index": 0,
                "segment_label": "pick",
                "segment_success": True,
                "segment_duration_s": 4.0,
                "has_all_required_topics": True,
                "minimum_required_segment_coverage_ratio": 1.0,
                "worst_topic_max_interval_ms": 100.0,
                "worst_topic_max_interval_over_median": 1.0,
            },
            {
                "recording_uuid": "uuid-4",
                "segment_index": 0,
                "segment_label": "place",
                "segment_success": True,
                "segment_duration_s": 3.0,
                "has_all_required_topics": True,
                "minimum_required_segment_coverage_ratio": 1.0,
                "worst_topic_max_interval_ms": 100.0,
                "worst_topic_max_interval_over_median": 1.0,
            },
        ],
        schema=SEGMENT_METRICS_SCHEMA,
    )

    relative = compute_segment_relative_metrics(segment_metrics)

    assert relative.schema == SEGMENT_RELATIVE_METRICS_SCHEMA
    assert relative.num_rows == 4

    rows = relative.to_pylist()
    assert rows[0]["segment_duration_s"] == 2.0
    assert rows[0]["segment_duration_s_percentile_rank_within_label"] == pytest.approx(
        0.0
    )
    assert rows[1]["segment_duration_s"] == 4.0
    assert rows[1]["segment_duration_s_percentile_rank_within_label"] == pytest.approx(
        0.75
    )
    assert rows[2]["segment_duration_s_percentile_rank_within_label"] == pytest.approx(
        0.75
    )
    assert rows[3]["segment_label_group_size"] == 1
    assert rows[3]["segment_duration_s_percentile_rank_within_label"] == pytest.approx(
        0.5
    )


def test_compute_segment_metrics_from_export_bundle_reads_export_layout(
    tmp_path: Path,
) -> None:
    bundle_dir = tmp_path / "bundle"
    parquet_dir = bundle_dir / "parquet"
    parquet_dir.mkdir(parents=True)

    metadata_table = metadata_to_arrow(_metadata_dict())
    pq.write_table(metadata_table, parquet_dir / "_metadata.parquet")
    pq.write_table(
        pa.table(
            {
                "topic_name": ["/required/a"],
                "message_type": ["sensor_msgs/msg/JointState"],
            }
        ),
        parquet_dir / "_topic_type_map.parquet",
    )
    pq.write_table(
        pa.table({"timestamp_ns": [1_000_000_000, 2_000_000_000, 3_000_000_000]}),
        parquet_dir / "required__a.parquet",
    )

    table = compute_segment_metrics_from_export_bundle(
        bundle_dir,
        required_topics=["/required/a", "/missing"],
    )

    assert table.schema == SEGMENT_METRICS_SCHEMA
    rows = table.to_pylist()
    assert rows[0]["has_all_required_topics"] is False
    assert rows[0]["minimum_required_segment_coverage_ratio"] == pytest.approx(0.0)
