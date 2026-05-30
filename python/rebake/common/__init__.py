"""Common data structures used across the rebake library.

This module provides data structures for handling image frames and shapes.
The preferred names are ``ImageFrame`` and ``ImageShape``, which follow
the naming convention used throughout rebake's Python API.

Example:
    >>> from rebake.common import ImageFrame, ImageShape
    >>> shape = ImageShape(480, 640, 3)
    >>> frame = ImageFrame(0, "jpg", image_bytes, shape)
"""

from __future__ import annotations

from .. import _internal

# Internal FFI types (kept for backward compatibility)
PyImageFrame = _internal.common.PyImageFrame
PyImageShape = _internal.common.PyImageShape
PyDepthFrame = _internal.common.PyDepthFrame

# Public API aliases (preferred names, consistent with other modules)
ImageFrame = PyImageFrame
"""A single image frame with metadata.

Stores image data as bytes along with frame index and format info.
This class supports pickle serialization for Dagster compatibility.

Attributes:
    index: Frame index in the sequence.
    extension: File extension (e.g., "png", "jpg").
    bytes: Raw image data.
    shape: Optional image dimensions (ImageShape).

Example:
    >>> from rebake.common import ImageFrame, ImageShape
    >>> shape = ImageShape(480, 640, 3)
    >>> frame = ImageFrame(0, "jpg", list(image_bytes), shape)
    >>> print(frame.index, frame.extension)
    0 jpg
"""

ImageShape = PyImageShape
"""Shape information for an image (height, width, channels).

Attributes:
    height: Image height in pixels.
    width: Image width in pixels.
    channels: Number of color channels (e.g., 3 for RGB).

Example:
    >>> from rebake.common import ImageShape
    >>> shape = ImageShape(480, 640, 3)
    >>> print(shape.height, shape.width, shape.channels)
    480 640 3
"""

DepthFrame = PyDepthFrame
"""A single depth frame with metadata.

Stores depth data as bytes along with frame index and format info.
This class supports pickle serialization for Dagster compatibility.

Attributes:
    index: Frame index in the sequence.
    extension: File extension (e.g., "png").
    bytes: Raw depth data.
    ros_format: Optional original ROS CompressedImage format string.

Example:
    >>> from rebake.common import DepthFrame
    >>> frame = DepthFrame(0, "png", list(depth_bytes))
    >>> print(frame.index, frame.extension)
    0 png
"""

__all__ = [
    # Preferred public API
    "ImageFrame",
    "ImageShape",
    "DepthFrame",
    # Backward compatibility (FFI names)
    "PyImageFrame",
    "PyImageShape",
    "PyDepthFrame",
]
