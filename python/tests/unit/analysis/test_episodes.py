from __future__ import annotations

from pathlib import Path

import pyarrow as pa
import pyarrow.parquet as pq
import pytest
from rebake.analysis import (
    EPISODE_METRICS_SCHEMA,
    EPISODE_RELATIVE_METRICS_SCHEMA,
    compute_episode_metrics_from_export_bundle,
    compute_episode_relative_metrics,
    compute_topic_metrics_from_export_bundle,
)
from rebake.core import metadata_to_arrow


def _metadata_dict(
    *,
    uuid: str = "uuid-123",
    label: str = "episode",
    start_time: float = 1.0,
    end_time: float = 4.0,
    success: bool = True,
) -> dict:
    return {
        "$schema": "https://example.com/schema.json",
        "schema_version": "2.0",
        "uuid": uuid,
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
            "start_time": start_time,
            "end_time": end_time,
            "success": success,
            "label": label,
        },
        "labels": ["pick"],
        "segments": [
            {
                "start_time": start_time,
                "end_time": end_time,
                "label_idx": 0,
                "success": success,
            },
        ],
    }


def _write_bundle(tmp_path: Path) -> Path:
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
    return bundle_dir


def test_compute_episode_metrics_from_export_bundle_aggregates_topic_metrics(
    tmp_path: Path,
) -> None:
    bundle_dir = _write_bundle(tmp_path)
    topic_metrics = compute_topic_metrics_from_export_bundle(
        bundle_dir,
        required_topics=["/required/a"],
    )

    table = compute_episode_metrics_from_export_bundle(
        bundle_dir,
        required_topics=["/required/a"],
        topic_metrics=topic_metrics,
    )

    assert table.schema == EPISODE_METRICS_SCHEMA
    rows = table.to_pylist()
    assert len(rows) == 1
    row = rows[0]
    assert row["recording_uuid"] == "uuid-123"
    assert row["episode_label"] == "episode"
    assert row["episode_success"] is True
    assert row["episode_duration_s"] == 3.0
    assert row["has_all_required_topics"] is True
    assert row["minimum_required_episode_coverage_ratio"] == pytest.approx(1.0)
    assert row["worst_topic_max_interval_ms"] == pytest.approx(2000.0)
    assert row["worst_topic_max_interval_over_median"] == pytest.approx(4 / 3)
    assert row["worst_topic_interval_cv"] == pytest.approx(1 / 3)


def test_compute_episode_metrics_treats_missing_required_topic_as_missing(
    tmp_path: Path,
) -> None:
    bundle_dir = _write_bundle(tmp_path)

    table = compute_episode_metrics_from_export_bundle(
        bundle_dir,
        required_topics=["/required/a", "/missing"],
    )

    row = table.to_pylist()[0]
    assert row["has_all_required_topics"] is False
    assert row["minimum_required_episode_coverage_ratio"] == 0.0


def test_compute_episode_relative_metrics_uses_tie_aware_percentiles() -> None:
    episode_metrics = pa.Table.from_pylist(
        [
            {
                "recording_uuid": "uuid-1",
                "episode_label": "task-a",
                "episode_success": True,
                "episode_duration_s": 2.0,
                "has_all_required_topics": True,
                "minimum_required_episode_coverage_ratio": 1.0,
                "worst_topic_max_interval_ms": 100.0,
                "worst_topic_max_interval_over_median": 1.0,
                "worst_topic_interval_cv": 0.1,
            },
            {
                "recording_uuid": "uuid-2",
                "episode_label": "task-a",
                "episode_success": True,
                "episode_duration_s": 4.0,
                "has_all_required_topics": True,
                "minimum_required_episode_coverage_ratio": 1.0,
                "worst_topic_max_interval_ms": 100.0,
                "worst_topic_max_interval_over_median": 1.0,
                "worst_topic_interval_cv": 0.1,
            },
            {
                "recording_uuid": "uuid-3",
                "episode_label": "task-a",
                "episode_success": True,
                "episode_duration_s": 4.0,
                "has_all_required_topics": True,
                "minimum_required_episode_coverage_ratio": 1.0,
                "worst_topic_max_interval_ms": 100.0,
                "worst_topic_max_interval_over_median": 1.0,
                "worst_topic_interval_cv": 0.1,
            },
            {
                "recording_uuid": "uuid-4",
                "episode_label": "task-b",
                "episode_success": True,
                "episode_duration_s": 3.0,
                "has_all_required_topics": True,
                "minimum_required_episode_coverage_ratio": 1.0,
                "worst_topic_max_interval_ms": 100.0,
                "worst_topic_max_interval_over_median": 1.0,
                "worst_topic_interval_cv": 0.1,
            },
        ],
        schema=EPISODE_METRICS_SCHEMA,
    )

    relative = compute_episode_relative_metrics(episode_metrics)

    assert relative.schema == EPISODE_RELATIVE_METRICS_SCHEMA
    rows = relative.to_pylist()
    assert rows[0]["episode_duration_s"] == 2.0
    assert rows[0]["episode_duration_s_percentile_rank_within_label"] == pytest.approx(
        0.0
    )
    assert rows[1]["episode_duration_s"] == 4.0
    assert rows[1]["episode_duration_s_percentile_rank_within_label"] == pytest.approx(
        0.75
    )
    assert rows[2]["episode_duration_s_percentile_rank_within_label"] == pytest.approx(
        0.75
    )
    assert rows[3]["episode_label_group_size"] == 1
    assert rows[3]["episode_duration_s_percentile_rank_within_label"] == pytest.approx(
        0.5
    )
