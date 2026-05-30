"""Decoders for extracting data from encoded formats.

This module provides decoders that extract data from encoded formats.
Currently, it includes video decoding to extract image frames.

Available decoders:
- VideoDecoder: Extracts image frames from MP4 video files.

Example:
    >>> from rebake.decode import VideoDecoderConfig
    >>> config = VideoDecoderConfig()
    >>> decoder = config.build()
    >>> context = decoder.run(context)
"""

from .video_decoder import (
    VideoDecoder,
    VideoDecoderConfig,
)

__all__ = [
    "VideoDecoder",
    "VideoDecoderConfig",
]
