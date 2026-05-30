"""Dataset merging operations.

Merge multiple LeRobot v2.1 datasets into a single unified dataset.
Handles episode renumbering, task deduplication, parquet column remapping,
video file copying, and metadata consolidation.

Example:
    >>> from rebake.merge import discover_datasets, merge_datasets
    >>>
    >>> datasets = discover_datasets(source_dir="/data/datasets")
    >>> if len(datasets) >= 2:
    ...     merged_count = merge_datasets(
    ...         source_dir="/data/datasets",
    ...         output="/data/merged",
    ...     )
"""

from .. import _internal

discover_datasets = _internal.merge.discover_datasets
merge_datasets = _internal.merge.merge_datasets

__all__ = ["discover_datasets", "merge_datasets"]
