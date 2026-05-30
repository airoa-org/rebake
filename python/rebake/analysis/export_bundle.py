"""Helpers for reading rebake Parquet/MP4 export bundle metadata."""

from __future__ import annotations

from pathlib import Path
from typing import Any

import pyarrow.parquet as pq


def resolve_parquet_dir(bundle_dir: str | Path) -> Path:
    """Resolve either an export bundle root or its nested ``parquet/`` directory."""

    root = Path(bundle_dir)
    if not root.exists():
        raise FileNotFoundError(f"bundle path does not exist: {root}")

    if _looks_like_parquet_dir(root):
        return root

    parquet_dir = root / "parquet"
    if _looks_like_parquet_dir(parquet_dir):
        return parquet_dir

    raise FileNotFoundError(f"could not find exported parquet bundle under: {root}")


def load_export_metadata(parquet_dir: Path) -> dict[str, Any]:
    """Load the single metadata row from ``_metadata.parquet``."""

    metadata_table = pq.read_table(parquet_dir / "_metadata.parquet")
    rows = metadata_table.to_pylist()
    if len(rows) != 1:
        raise ValueError(
            f"_metadata.parquet must contain exactly one row, found {len(rows)}"
        )
    return dict(rows[0])


def load_export_topic_type_map(parquet_dir: Path) -> dict[str, str]:
    """Load ``topic_name -> message_type`` from ``_topic_type_map.parquet``."""

    topic_type_table = pq.read_table(
        parquet_dir / "_topic_type_map.parquet",
        columns=["topic_name", "message_type"],
    )
    topic_type_map: dict[str, str] = {}
    for row in topic_type_table.to_pylist():
        topic_name = row["topic_name"]
        message_type = row["message_type"]
        if isinstance(topic_name, str) and isinstance(message_type, str):
            topic_type_map[topic_name] = message_type
    return topic_type_map


def load_topic_timestamps(parquet_dir: Path, topic_name: str) -> list[int]:
    """Load non-null ``timestamp_ns`` values for one topic parquet."""

    topic_path = parquet_dir / f"{topic_name_to_flat_file_stem(topic_name)}.parquet"
    topic_table = pq.read_table(topic_path, columns=["timestamp_ns"])
    timestamps: list[int] = []
    for value in topic_table.column("timestamp_ns").to_pylist():
        if value is None:
            raise ValueError(f"{topic_path} contains null timestamp_ns values")
        timestamps.append(int(value))
    return timestamps


def topic_name_to_flat_file_stem(topic_name: str) -> str:
    """Map a ROS topic name to rebake's flat parquet file stem."""

    return topic_name.lstrip("/").replace("/", "__")


def _looks_like_parquet_dir(path: Path) -> bool:
    return (
        path / "_metadata.parquet"
    ).exists() and (path / "_topic_type_map.parquet").exists()
