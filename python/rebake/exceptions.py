"""Exception classes for rebake.

This module defines the exception hierarchy for rebake. All exceptions
inherit from RebakeError, which allows catching all rebake-related
errors with a single except clause if desired.

Exception Hierarchy:
    RebakeError (base)
    ├── IngestError      - ROS bag reading errors
    ├── SynchronizeError - Time synchronization errors
    ├── EnrichError      - Data enrichment errors
    ├── EncodeError      - Video encoding errors
    ├── DecodeError      - Video decoding errors
    └── TransformError   - Output transformation errors

Usage:
    >>> from rebake.exceptions import RebakeError, IngestError
    >>>
    >>> try:
    ...     context = ingestor.run(context)
    ... except IngestError as e:
    ...     print(f"Failed to ingest: {e}")
    ... except RebakeError as e:
    ...     print(f"Rebake error: {e}")
"""

from __future__ import annotations


class RebakeError(Exception):
    """Base exception for all rebake errors.

    All rebake exceptions inherit from this class, allowing you to
    catch any rebake-related error with a single except clause.
    """

    pass


class IngestError(RebakeError):
    """Error during ROS bag ingestion.

    Raised when reading or parsing ROS bag files fails.
    Common causes include missing files, corrupted bags,
    or unsupported message types.
    """

    pass


class SynchronizeError(RebakeError):
    """Error during time synchronization.

    Raised when synchronizing data across topics fails.
    Common causes include empty datasets or invalid fps values.
    """

    pass


class EnrichError(RebakeError):
    """Error during data enrichment.

    Raised when enrichment operations (transforms, deltas, etc.) fail.
    Common causes include missing required columns or invalid configurations.
    """

    pass


class EncodeError(RebakeError):
    """Error during video encoding.

    Raised when encoding image data to video fails.
    Common causes include invalid image data, codec issues,
    or filesystem errors.
    """

    pass


class DecodeError(RebakeError):
    """Error during video decoding.

    Raised when decoding video back to image frames fails.
    Common causes include missing video files, corrupted data,
    or codec issues.
    """

    pass


class TransformError(RebakeError):
    """Error during output transformation.

    Raised when transforming data to output formats (e.g., LeRobot) fails.
    Common causes include missing required data, invalid configurations,
    or filesystem errors.
    """

    pass


__all__ = [
    "RebakeError",
    "IngestError",
    "SynchronizeError",
    "EnrichError",
    "EncodeError",
    "DecodeError",
    "TransformError",
]
