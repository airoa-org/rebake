"""Encoders for converting data to various formats.

This module provides encoders that convert data to different output
formats.

Available encoders:
- ImageEncoder: Saves image frames to individual files.
- VideoEncoder: Converts image frames to MP4 video files.
- DepthVideoConfig: Encodes depth frames to video (via rebake-cli pipeline).
- build_video_metadata: Builds canonical metadata for encoded RGB videos.
- build_video_artifact: Builds a typed encoded video artifact from config + path.

Video codecs:
- Software: AV1 (default), H.264, H.265
- Hardware (VA-API): H.264, H.265, AV1 (requires AMD VCN or Intel QSV)
- Hardware (NVIDIA NVENC): H.264, H.265, AV1

Depth video codecs:
- Software: AV1 via SVT-AV1 (default), FFV1 (lossless)
- Hardware (VA-API): HEVC, AV1
- Hardware (NVIDIA NVENC): HEVC, AV1

Example:
    >>> from rebake.encode import ImageEncoderConfig, VideoEncoderConfig
    >>> # Save images to files
    >>> image_encoder = ImageEncoderConfig().build()
    >>> context = image_encoder.run(context)
    >>> # Convert to video
    >>> video_encoder = VideoEncoderConfig().build()
    >>> context = video_encoder.run(context)

VA-API hardware encoding example:
    >>> from rebake.encode import (
    ...     VideoEncoderConfig, CodecConfig, is_vaapi_available
    ... )
    >>> if is_vaapi_available():
    ...     config = VideoEncoderConfig(codec_config=CodecConfig.h264_vaapi())
    ... else:
    ...     config = VideoEncoderConfig(codec_config=CodecConfig.h264())

Depth video example:
    >>> from rebake.encode import DepthVideoConfig, DepthCodecConfig
    >>> config = DepthVideoConfig(
    ...     depth_max_mm=4092,
    ...     codec_config=DepthCodecConfig.av1(crf=4, preset=4),
    ... )
"""

from .depth_video_encoder import DepthCodecConfig, DepthVideoConfig
from .image_encoder import ImageEncoder, ImageEncoderConfig
from .video_encoder import (
    CodecConfig,
    NvencPreset,
    NvencTune,
    ScalingFlag,
    VideoArtifact,
    VideoEncoder,
    VideoEncoderConfig,
    VideoMetadata,
    X264Preset,
    X264Tune,
    X265Tune,
    build_video_artifact,
    build_video_metadata,
    is_vaapi_available,
    validate_video_config_json,
)

__all__ = [
    # Image
    "ImageEncoder",
    "ImageEncoderConfig",
    # Video (RGB)
    "VideoEncoder",
    "VideoEncoderConfig",
    "VideoMetadata",
    "VideoArtifact",
    "ScalingFlag",
    "CodecConfig",
    "X264Preset",
    "X264Tune",
    "X265Tune",
    "NvencPreset",
    "NvencTune",
    "build_video_metadata",
    "build_video_artifact",
    # Video (Depth)
    "DepthCodecConfig",
    "DepthVideoConfig",
    # VA-API
    "is_vaapi_available",
    "validate_video_config_json",
]
