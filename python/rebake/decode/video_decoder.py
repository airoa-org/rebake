"""Video decoder for extracting image frames from video files."""

from __future__ import annotations

import json
import warnings
from typing import TYPE_CHECKING

from pydantic import BaseModel, ConfigDict

from .. import _internal
from ..common import PyImageFrame
from ..core.context import Context

if TYPE_CHECKING:
    from ..encode.video_encoder import VideoArtifact


class VideoDecoderConfig(BaseModel):
    """Configuration for the video decoder.

    This config creates a decoder that extracts image frames from
    MP4 video files and stores them as image data in the Context.

    The decoder automatically processes all video files described by
    rebake's internal video registry in the ``Context``. No configuration is needed.

    This class provides two methods for processing data:

    - ``run(context)``: Process data within a Context object.
    - ``decode_registry(video_registry, bundle_root=None)``: Canonical API for
      decoding typed video artifacts.
    - ``decode_rgb_paths(video_paths)``: Compatibility API for decoding RGB video files directly.

    Example:
        >>> config = VideoDecoderConfig()
        >>> decoder = config.build()
        >>> # Using run() with Context (decodes all registered videos)
        >>> context = decoder.run(context)
        >>> # Using decode_registry() with typed artifacts
        >>> image_data = decoder.decode_registry(video_registry, bundle_root="/tmp/bundle")
    """

    model_config = ConfigDict(arbitrary_types_allowed=True)

    def build(self) -> VideoDecoder:
        """Create a VideoDecoder from this config.

        Returns:
            A new VideoDecoder instance.
        """
        return VideoDecoder(self)

    def _to_inner(self) -> _internal.decode.VideoDecoderConfig:
        """Convert to internal Rust config object."""
        return _internal.decode.VideoDecoderConfig()


class VideoDecoder:
    """Extracts image frames from MP4 video files.

    This decoder reads all registered video files from the ``Context`` and
    extracts individual frames as image data. The frames are stored
    in the Context and can be used by other stages.

    This is useful when you need to re-process video data or when
    you want to convert videos back to image frames.

    This class provides two methods for processing data:

    - ``run(context)``: Process data within a Context object.
    - ``decode_registry(video_registry, bundle_root=None)``: Canonical API for
      decoding typed video artifacts.
    - ``decode_rgb_paths(video_paths)``: Compatibility API for decoding RGB video files directly.

    Example:
        >>> config = VideoDecoderConfig()
        >>> decoder = config.build()
        >>> # Using run() with Context (decodes all videos)
        >>> context = decoder.run(context)
        >>> # Using decode_registry() with typed artifacts
        >>> image_data = decoder.decode_registry(video_registry, bundle_root="/tmp/bundle")
    """

    def __init__(self, config: VideoDecoderConfig):
        """Create a new VideoDecoder.

        Args:
            config: The configuration for this decoder.
        """
        self.config = config
        self._inner = _internal.decode.VideoDecoder(config._to_inner())

    def run(self, context: Context) -> Context:
        """Run the decoder on the given context.

        This extracts frames from all registered video files in the context.

        Args:
            context: The context with registered videos to decode.

        Returns:
            The context with extracted image data.

        Example:
            >>> context = decoder.run(context)
        """
        self._inner.run(context.inner)
        return context

    def decode_registry(
        self,
        video_registry: dict[str, "VideoArtifact"],
        *,
        bundle_root: str | None = None,
    ) -> dict[str, list[PyImageFrame]]:
        """Decode typed video artifacts to image frames.

        This is the canonical convenience API. Unlike path-only helpers, it
        preserves media semantics such as ``media_type`` and depth encoding
        metadata, so it works for both RGB and depth videos.

        Args:
            video_registry: Mapping from topic names to typed ``VideoArtifact`` objects.
            bundle_root: Optional root directory used to resolve relative artifact paths.

        Returns:
            Dictionary mapping topic names to lists of PyImageFrame.
            Each frame contains PNG-encoded image bytes.

        Example:
            >>> image_data = decoder.decode_registry(video_registry, bundle_root="/tmp/bundle")
        """
        payload = {
            topic: artifact.model_dump(mode="json")
            for topic, artifact in video_registry.items()
        }
        return self._inner.decode_video_registry_json(
            json.dumps(payload),
            bundle_root,
        )

    def decode_rgb_paths(
        self,
        video_paths: dict[str, str],
    ) -> dict[str, list[PyImageFrame]]:
        """Decode RGB video files to image frames.

        This is a compatibility helper for path-only callers. It only supports
        RGB video files because path dictionaries do not carry the metadata
        needed for depth decode.

        Args:
            video_paths: Dictionary mapping topic names to RGB video file paths.

        Returns:
            Dictionary mapping topic names to lists of PyImageFrame.
            Each frame contains PNG-encoded image bytes.

        Example:
            >>> image_data = decoder.decode_rgb_paths(video_paths)
        """
        return self._inner.decode_rgb_paths(video_paths)

    def decode(
        self,
        video_paths: dict[str, str],
    ) -> dict[str, list[PyImageFrame]]:
        """Deprecated alias for ``decode_rgb_paths()``.

        Use ``decode_registry()`` for canonical typed decode or
        ``decode_rgb_paths()`` when you only have RGB path dictionaries.
        """
        warnings.warn(
            "VideoDecoder.decode() is deprecated; use decode_registry() for typed video artifacts "
            "or decode_rgb_paths() for RGB path dictionaries.",
            DeprecationWarning,
            stacklevel=2,
        )
        return self.decode_rgb_paths(video_paths)
