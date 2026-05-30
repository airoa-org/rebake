from __future__ import annotations

from pathlib import Path

import numpy as np
import pyarrow as pa
import pyarrow.parquet as pq
import pytest
from rebake.analysis import (
    TOPIC_METRICS_SCHEMA,
    compute_topic_metrics_from_export_bundle,
    compute_topic_timing_metrics,
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
            "end_time": 4.0,
            "success": True,
            "label": "episode",
        },
        "labels": ["pick"],
        "segments": [
            {
                "start_time": 1.0,
                "end_time": 4.0,
                "label_idx": 0,
                "success": True,
            },
        ],
    }


def test_compute_topic_timing_metrics_summarizes_windowed_topic() -> None:
    metrics = compute_topic_timing_metrics(
        timestamps_ns=[0, 100_000_000, 200_000_000, 450_000_000, 700_000_000],
        episode_start_ns=0,
        episode_end_ns=500_000_000,
    )

    assert metrics["message_count"] == 4
    assert metrics["first_message_timestamp_ns"] == 0
    assert metrics["last_message_timestamp_ns"] == 450_000_000
    assert metrics["observed_topic_span_s"] == 0.45
    assert metrics["episode_coverage_ratio"] == 0.9
    assert metrics["observed_hz"] == (3 / 0.45)
    assert metrics["median_topic_interval_ms"] == 100.0
    assert metrics["max_topic_interval_ms"] == 250.0
    assert metrics["max_interval_over_median"] == 2.5


def test_compute_topic_timing_metrics_returns_zero_coverage_for_empty_window() -> None:
    metrics = compute_topic_timing_metrics(
        timestamps_ns=[0, 50, 100],
        episode_start_ns=1_000,
        episode_end_ns=2_000,
    )

    assert metrics == {
        "message_count": 0,
        "first_message_timestamp_ns": None,
        "last_message_timestamp_ns": None,
        "observed_topic_span_s": 0.0,
        "episode_coverage_ratio": 0.0,
        "observed_hz": None,
        "median_topic_interval_ms": None,
        "topic_interval_cv": None,
        "max_topic_interval_ms": None,
        "max_interval_over_median": None,
    }


def test_compute_topic_timing_metrics_accepts_numpy_arrays() -> None:
    metrics = compute_topic_timing_metrics(
        timestamps_ns=np.array([0, 100_000_000, 200_000_000], dtype=np.int64),
        episode_start_ns=0,
        episode_end_ns=250_000_000,
    )

    assert metrics["message_count"] == 3
    assert metrics["median_topic_interval_ms"] == 100.0


def test_compute_topic_metrics_from_export_bundle_reads_export_layout(
    tmp_path: Path,
) -> None:
    bundle_dir = tmp_path / "bundle"
    parquet_dir = bundle_dir / "parquet"
    parquet_dir.mkdir(parents=True)

    pq.write_table(metadata_to_arrow(_metadata_dict()), parquet_dir / "_metadata.parquet")
    pq.write_table(
        pa.table(
            {
                "topic_name": ["/required/a", "/optional/b"],
                "message_type": [
                    "sensor_msgs/msg/JointState",
                    "std_msgs/msg/String",
                ],
            }
        ),
        parquet_dir / "_topic_type_map.parquet",
    )
    pq.write_table(
        pa.table({"timestamp_ns": [1_000_000_000, 2_000_000_000, 4_000_000_000]}),
        parquet_dir / "required__a.parquet",
    )
    pq.write_table(
        pa.table({"timestamp_ns": [2_000_000_000, 3_000_000_000]}),
        parquet_dir / "optional__b.parquet",
    )

    table = compute_topic_metrics_from_export_bundle(
        bundle_dir,
        required_topics=["/required/a", "/missing"],
        required_topic_profile="test-default",
    )

    assert table.schema == TOPIC_METRICS_SCHEMA
    rows = table.to_pylist()
    assert [row["topic_name"] for row in rows] == ["/optional/b", "/required/a"]
    assert rows[0]["is_required_topic"] is False
    assert rows[0]["required_topic_count"] == 2
    assert rows[0]["required_topic_profile"] == "test-default"
    assert rows[1]["is_required_topic"] is True
    assert rows[1]["message_count"] == 3
    assert rows[1]["episode_coverage_ratio"] == pytest.approx(1.0)
    assert rows[1]["max_topic_interval_ms"] == pytest.approx(2000.0)
