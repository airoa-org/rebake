"""Utility functions for rosbag file discovery."""

from __future__ import annotations

from pathlib import Path

from ..exceptions import IngestError


def collect_rosbags(path: Path | str) -> list[Path]:
    """Collect rosbags from a path, excluding backups.

    Supports both ROS 1 (.bag) and ROS 2 (.mcap) formats. Backup files
    created by rosbag tools (e.g., `.orig.bag` from `rosbag reindex`)
    are automatically excluded.

    Args:
        path: Path to a single rosbag or a directory containing
            rosbags. Directories are searched recursively.

    Returns:
        Sorted list of rosbag paths. When given a single file,
        returns a list containing only that file.

    Raises:
        IngestError: If the path does not exist, if a single file has
            an unsupported extension, if a single file is a backup file,
            or if no rosbags are found in a directory.

    Examples:
        ```python
        from rebake.ingest import collect_rosbags

        # Single file
        rosbags = collect_rosbags("/data/recording.mcap")
        # [Path('/data/recording.mcap')]

        # Directory (recursive)
        rosbags = collect_rosbags("/data/recordings/")
        # [Path('/data/recordings/day1/rec1.mcap'),
        #  Path('/data/recordings/day2/rec2.mcap')]
        ```
    """
    path = Path(path)

    if not path.exists():
        raise IngestError(f"Path does not exist: {path}")

    if path.is_file():
        if not _is_rosbag(path):
            raise IngestError(f"Unsupported file extension: {path.suffix}")
        if _is_backup(path):
            raise IngestError(f"Backup file not allowed: {path.name}")
        return [path]

    rosbags = [p for p in path.rglob("*") if _is_rosbag(p) and not _is_backup(p)]

    if not rosbags:
        raise IngestError(f"No rosbags found in: {path}")

    return sorted(rosbags)


def _is_rosbag(path: Path) -> bool:
    return path.suffix.lower() in (".bag", ".mcap")


def _is_backup(path: Path) -> bool:
    return path.name.endswith((".orig.bag", ".orig.mcap"))
