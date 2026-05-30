"""Ingestors for reading ROS bag files.

This module provides ingestors that read ROS bag files and convert
them to structured data in the Context.

Available ingestors:
- Rosbag1Ingestor: For ROS 1 bag files (.bag)
- Rosbag2Ingestor: For ROS 2 bag files (.mcap)

Functions:
- create_ingestor: Create an appropriate ingestor based on file extension
- collect_rosbags: Collect rosbags from a path
- read_metadata: Read metadata from a rosbag without full ingestion

Example:
    >>> from rebake.ingest import create_ingestor, read_metadata
    >>>
    >>> # Quick metadata check
    >>> metadata = read_metadata("/path/to/data.mcap")
    >>> print(metadata["uuid"])
    >>>
    >>> # Full ingestion (automatically selects correct ingestor)
    >>> ingestor = create_ingestor("/path/to/data.bag")
    >>> topics, metadata, image_data, type_map = ingestor.ingest("/path/to/data.bag")
"""

from __future__ import annotations

from pathlib import Path

from .rosbag1_ingestor import (
    Rosbag1Ingestor,
    Rosbag1IngestorConfig,
)
from .rosbag2_ingestor import (
    Rosbag2Ingestor,
    Rosbag2IngestorConfig,
    read_metadata,
)
from .utils import collect_rosbags


def create_ingestor(
    rosbag_path: str | Path,
    require_metadata: bool = True,
) -> Rosbag1Ingestor | Rosbag2Ingestor:
    """Create an appropriate ingestor based on file extension.

    This factory function automatically selects the correct ingestor
    (ROS 1 or ROS 2) based on the rosbag file extension.

    Args:
        rosbag_path: Path to the rosbag file (.bag or .mcap).
        require_metadata: Whether to require meta.json. Defaults to True.

    Returns:
        Rosbag1Ingestor for .bag files, Rosbag2Ingestor for .mcap files.

    Raises:
        ValueError: If the file extension is not supported.
    """
    path = Path(rosbag_path)
    extension = path.suffix.lower()

    if extension == ".bag":
        return Rosbag1IngestorConfig(require_metadata=require_metadata).build()
    elif extension == ".mcap":
        return Rosbag2IngestorConfig(require_metadata=require_metadata).build()
    else:
        raise ValueError(
            f"Unsupported rosbag file extension: {extension}. "
            f"Expected .bag (ROS 1) or .mcap (ROS 2)."
        )


__all__ = [
    "Rosbag1Ingestor",
    "Rosbag1IngestorConfig",
    "Rosbag2Ingestor",
    "Rosbag2IngestorConfig",
    "collect_rosbags",
    "create_ingestor",
    "read_metadata",
]
