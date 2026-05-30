"""Depth video encoder configuration for compressedDepth topics.

Provides configuration types for encoding 16-bit depth frames into video files.
For lossy codecs, depth values are quantized via Q10Clip4 (16-bit → 10-bit)
and packed into P010LE format. FFV1 lossless encodes raw gray16le.

Example:
    >>> from rebake.encode import DepthVideoConfig, DepthCodecConfig
    >>> config = DepthVideoConfig(
    ...     depth_max_mm=4092,
    ...     fps=30,
    ...     codec_config=DepthCodecConfig.av1(crf=4, preset=4),
    ... )
"""

from __future__ import annotations

from pydantic import BaseModel, ConfigDict

from .. import _internal

# Re-export from internal module
DepthCodecConfig = _internal.encode.DepthCodecConfig
"""Codec-specific configuration for depth video encoding.

Create codec configurations using static methods:

Software encoders:
- ``DepthCodecConfig.av1(crf=4, preset=4)``: AV1 via SVT-AV1 (default, no HW required)

Hardware encoders (VA-API, requires AMD VCN or Intel QSV):
- ``DepthCodecConfig.h265_vaapi(qp=18)``: HEVC VA-API
- ``DepthCodecConfig.av1_vaapi(global_quality=35)``: AV1 VA-API (VCN 4.0+)

Hardware encoders (NVIDIA NVENC):
- ``DepthCodecConfig.h265_nvenc(qp=10)``: HEVC NVENC
- ``DepthCodecConfig.av1_nvenc(qp=20)``: AV1 NVENC
  ``b_frames`` and ``rc_lookahead`` can be set for archive-oriented
  compression. ``b_frames`` defaults to 0 for frame-indexed packaging; when it
  is above 0, rebake uses FFmpeg/NVENC ``b_ref_mode=middle`` internally.

Lossless:
- ``DepthCodecConfig.ffv1()``: FFV1 lossless (large files, MKV container)

Example:
    >>> config = DepthVideoConfig(
    ...     codec_config=DepthCodecConfig.av1(crf=4, preset=4),
    ... )
"""


class DepthVideoConfig(BaseModel):
    """Configuration for depth video encoding.

    Controls how depth frames (16-bit grayscale from ROS compressedDepth)
    are compressed into video files. For lossy codecs, depth values are
    quantized via Q10Clip4 (16-bit → 10-bit) before encoding.

    Attributes:
        depth_max_mm: Maximum depth in millimeters for Q10Clip4 quantization.
            Pixels with depth > depth_max_mm are clipped to 0 (invalid).
            Ignored when codec is FFV1 (lossless). Default is 4092.
        fps: Frames per second for the output video. Default is 30.
        codec_config: Codec-specific configuration. Default is AV1 with
            CRF=4 and preset=4. Use ``DepthCodecConfig.av1()``,
            ``DepthCodecConfig.h265_vaapi()``, etc. to create custom
            configurations.

    Example:
        >>> config = DepthVideoConfig(
        ...     depth_max_mm=4092,
        ...     fps=30,
        ...     codec_config=DepthCodecConfig.av1(crf=4, preset=4),
        ... )
    """

    model_config = ConfigDict(arbitrary_types_allowed=True)
    depth_max_mm: int = 4092
    fps: int = 30
    codec_config: DepthCodecConfig | None = None

    def _to_inner(self) -> _internal.encode.DepthVideoConfig:
        """Convert to internal Rust config object."""
        return _internal.encode.DepthVideoConfig(
            self.depth_max_mm,
            self.fps,
            self.codec_config,
        )
