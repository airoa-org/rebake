"""Core components for the rebake pipeline.

This module provides the core data structures used throughout
the rebake pipeline.

Main classes:
- Context: Container that holds all data as it moves through stages.

Functions:
- metadata_to_arrow: Convert metadata dict to Arrow RecordBatch.
- parse_metadata_as_v2_0: Parse metadata and return a Rust-backed V2.0 object.
- normalize_metadata_to_v2_0: Normalize metadata dict to canonical V2.0.
"""

from __future__ import annotations

import json
from typing import TYPE_CHECKING, Any

from .. import _internal
from .context import Context

if TYPE_CHECKING:
    import pyarrow as pa


EnvType = _internal.core.EnvType
RunnerType = _internal.core.RunnerType
Robot = _internal.core.Robot
File = _internal.core.File
Environment = _internal.core.Environment
Runner = _internal.core.Runner
Device = _internal.core.Device
GitSource = _internal.core.GitSource
Source = _internal.core.Source
Program = _internal.core.Program
Episode = _internal.core.Episode
Segment = _internal.core.Segment
MetadataV2_0 = _internal.core.MetadataV2_0


def metadata_to_arrow(metadata: dict[str, object]) -> "pa.Table":
    """Convert metadata dictionary to an Arrow Table.

    This function converts an Airoa metadata dictionary to an Arrow Table,
    preserving the full nested structure including lists of structs.

    Args:
        metadata: Airoa metadata as a Python dictionary.

    Returns:
        Arrow Table containing the metadata (single row).

    Examples:
        ```python
        metadata = {"uuid": "...", "version": "1.3", ...}
        table = metadata_to_arrow(metadata)
        ```
    """
    metadata_json = json.dumps(metadata)
    import pyarrow as pa

    batch = _internal.core.metadata_to_arrow(metadata_json)
    return pa.Table.from_batches([batch])


def parse_metadata_as_v2_0(metadata_json: str) -> MetadataV2_0:
    """Parse metadata and return a canonical V2.0 object.

    Args:
        metadata_json: Metadata JSON string in V1.3 or V2.0 format.

    Returns:
        Rust-backed metadata object normalized to canonical V2.0.
    """
    return _internal.core.parse_metadata_as_v2_0(metadata_json)


def normalize_metadata_to_v2_0(metadata: dict[str, Any]) -> dict[str, Any]:
    """Normalize metadata to canonical V2.0.

    Accepts either V1.3 or V2.0 metadata and returns a Python dictionary
    serialized from rebake's Rust-side V2.0 schema.

    Args:
        metadata: Airoa metadata as a Python dictionary.

    Returns:
        Canonical V2.0 metadata as a Python dictionary.
    """
    metadata_json = json.dumps(metadata)
    normalized_json = _internal.core.normalize_metadata_json_to_v2_0(metadata_json)
    return json.loads(normalized_json)


__all__ = [
    "Context",
    "Device",
    "EnvType",
    "Environment",
    "Episode",
    "File",
    "GitSource",
    "MetadataV2_0",
    "Program",
    "Robot",
    "Runner",
    "RunnerType",
    "Segment",
    "Source",
    "metadata_to_arrow",
    "normalize_metadata_to_v2_0",
    "parse_metadata_as_v2_0",
]
