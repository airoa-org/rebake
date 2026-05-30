#!/usr/bin/env python3
"""Reindex ROS bag files that are missing the index header.

This script requires a ROS 1 environment where the `rosbag` Python module
and its CLI utilities are available (e.g., via RobotStack/Micromamba).
"""

from __future__ import annotations

import argparse
from pathlib import Path
from typing import Iterator, Tuple, Union

import rosbag
from rosbag.bag import ROSBagException, ROSBagUnindexedException
from rosbag import rosbag_main

BAGPath = Union[str, Path]


def iter_bag_files(root: Path) -> Iterator[Path]:
    """Yield all *.bag files under the root directory while skipping backup bags."""
    for path in root.rglob("*.bag"):
        if path.name.endswith(".orig.bag"):
            continue
        yield path


def bag_index_state(path: BAGPath) -> Tuple[bool, str]:
    """Return (has_index, error_message)."""
    try:
        with rosbag.Bag(str(path), "r"):
            pass
    except ROSBagUnindexedException:
        return False, ""
    except ROSBagException as exc:
        return False, f"ROS bag error: {exc}"
    except OSError as exc:
        return False, f"I/O error: {exc}"
    return True, ""


def reindex(path: BAGPath) -> bool:
    """Run the `rosbag reindex` command and return True on success."""
    try:
        rosbag_main.reindex_cmd([str(path)])
    except SystemExit as exc:
        return exc.code == 0
    return True


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Reindex ROS bag files that are missing index metadata."
    )
    parser.add_argument("directory", type=Path, help="Directory to scan for bag files")
    args = parser.parse_args()

    root = args.directory.resolve()
    if not root.exists():
        parser.error(f"{root} does not exist")
    if not root.is_dir():
        parser.error(f"{root} is not a directory")

    unindexed = []
    for path in iter_bag_files(root):
        has_index, error = bag_index_state(path)
        if error:
            print(f"[skip] {path} ({error})")
            continue
        if not has_index:
            unindexed.append(path)

    if not unindexed:
        print(f"No unindexed bag files found under {root}")
        return

    print(f"Reindexing {len(unindexed)} bag file(s) under {root}")
    for bag in unindexed:
        print(f"[reindex] {bag}")
        success = reindex(bag)
        status = "done" if success else "failed"
        print(f"  -> {status}")


if __name__ == "__main__":
    main()

