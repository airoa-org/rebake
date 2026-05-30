"""Video encoder for converting image frames to video files."""

from __future__ import annotations

from typing import Any

from pydantic import BaseModel, ConfigDict, Field, model_serializer

from .. import _internal
from ..common import PyImageFrame
from ..core.context import Context

# Re-export from internal module
ScalingFlag = _internal.encode.PyScalingFlag
"""Scaling algorithm for video encoding.

Available options:

- FastBilinear: Fast but lower quality bilinear scaling.
- Bilinear: Standard bilinear scaling.
- Bicubic: Higher quality bicubic scaling (default).
- Lanczos: High quality Lanczos scaling.
- And more... (see FFmpeg documentation for details)
"""

CodecConfig = _internal.encode.PyCodecConfig
"""Codec-specific configuration.

Create codec configurations using static methods:

Software encoders:
- ``CodecConfig.av1(...)``: AV1 codec (default, best compression)
- ``CodecConfig.h264(...)``: H.264 codec (faster decoding)
- ``CodecConfig.h265(...)``: H.265/HEVC codec (good compression)

Hardware encoders (VA-API, requires AMD VCN or Intel QSV):
- ``CodecConfig.h264_vaapi(...)``: H.264 VA-API hardware encoding
- ``CodecConfig.h265_vaapi(...)``: H.265 VA-API hardware encoding
- ``CodecConfig.av1_vaapi(...)``: AV1 VA-API hardware encoding (VCN 4.0+)

Hardware encoders (NVIDIA NVENC):
- ``CodecConfig.h264_nvenc(...)``: H.264 NVENC hardware encoding
- ``CodecConfig.h265_nvenc(...)``: H.265 NVENC hardware encoding
- ``CodecConfig.av1_nvenc(...)``: AV1 NVENC hardware encoding

Use ``is_vaapi_available()`` to check if hardware encoding is available.

Example:
    >>> # Default canonical AV1
    >>> config = VideoEncoderConfig()
    >>> # H.264 with custom settings
    >>> config = VideoEncoderConfig(
    ...     codec_config=CodecConfig.h264(threads=4, preset=X264Preset.Fast)
    ... )
    >>> # H.264 VA-API hardware encoding with tuned general-purpose defaults.
    >>> if is_vaapi_available():
    ...     config = VideoEncoderConfig(codec_config=CodecConfig.h264_vaapi())
"""

X264Preset = _internal.encode.PyX264Preset
"""x264/x265 encoder preset for speed vs compression tradeoff.

Available presets (fastest to slowest):
Ultrafast, Superfast, Veryfast, Faster, Fast, Medium (default), Slow, Slower, Veryslow
"""

X264Tune = _internal.encode.PyX264Tune
"""x264 (H.264) encoder tuning options.

PSY tunings (mutually exclusive): Film, Animation, Grain, StillImage, Psnr, Ssim
Non-PSY tunings (can combine with one PSY): FastDecode, ZeroLatency
"""

X265Tune = _internal.encode.PyX265Tune
"""x265 (H.265/HEVC) encoder tuning options.

PSY tunings (mutually exclusive): Psnr, Ssim, Grain, Animation
Non-PSY tunings (can combine with one PSY): FastDecode, ZeroLatency
"""

NvencPreset = _internal.encode.PyNvencPreset
"""NVIDIA NVENC preset (P1 fastest through P7 slowest/best compression)."""

NvencTune = _internal.encode.PyNvencTune
"""NVIDIA NVENC tuning mode: Hq, Ll, or Ull."""


def is_vaapi_available() -> bool:
    """Check if VA-API hardware acceleration is available.

    Returns True if the VA-API device (/dev/dri/renderD128) exists on the system.
    This can be used to determine whether to use hardware or software encoding.

    Returns:
        bool: True if VA-API is available, False otherwise.

    Example:
        >>> from rebake.encode import is_vaapi_available, CodecConfig
        >>> if is_vaapi_available():
        ...     config = CodecConfig.h264_vaapi()
        ...     print("Using VA-API hardware encoding")
        ... else:
        ...     config = CodecConfig.h264()
        ...     print("Falling back to software encoding")
    """
    return _internal.encode.py_is_vaapi_available()


def validate_video_config_json(config_json: str, *, preflight: bool = False) -> str:
    """Validate and normalize a video encoder config JSON string.

    This is the canonical entrypoint for config validation from Python.
    It delegates parsing, semantic validation, and optional capability
    checks to Rust, then returns compact JSON after serde normalization.

    Args:
        config_json: JSON string matching ``VideoEncoderConfig``.
        preflight: When True, also verifies FFmpeg encoder availability and
            hardware device visibility for VA-API/NVENC codecs.

    Returns:
        Compact JSON string.
    """
    return _internal.encode.py_validate_video_config_json(config_json, preflight)


class VideoMetadata(BaseModel):
    """Canonical metadata for a single encoded video.

    Attributes:
        media_type: Logical media class such as ``"rgb"`` or ``"depth"``.
        codec_family: Encoded codec family such as ``"av1"`` or ``"h264"``.
        encoder_name: Concrete encoder implementation such as ``"libsvtav1"``
            or ``"av1_vaapi"``.
        pix_fmt: Output pixel format such as ``"yuv420p"``.
        width: Encoded frame width in pixels.
        height: Encoded frame height in pixels.
        fps: Encoded frame rate.
        encoding_config_json: Compact JSON for the validated encoder config.
    """

    model_config = ConfigDict(frozen=True)
    media_type: str
    codec_family: str
    encoder_name: str
    pix_fmt: str
    width: int
    height: int
    fps: int
    encoding_config_json: str


class VideoArtifact(BaseModel):
    """Encoded video file plus its canonical metadata."""

    model_config = ConfigDict(frozen=True)
    video_path: str
    metadata: VideoMetadata


def build_video_metadata(
    config_json: str,
    *,
    width: int,
    height: int,
) -> VideoMetadata:
    """Build canonical video metadata from a video config JSON string.

    This is the canonical Python entrypoint for turning a validated
    ``VideoEncoderConfig`` JSON string plus encoded dimensions into the
    semantic metadata persisted by downstream systems.

    Args:
        config_json: JSON string matching ``VideoEncoderConfig``.
            This may be raw config JSON or the compact JSON returned by
            ``validate_video_config_json()``.
        width: Encoded frame width in pixels.
        height: Encoded frame height in pixels.

    Returns:
        Typed video metadata.
    """
    metadata_json = _internal.encode.py_build_video_metadata_json(
        config_json,
        width,
        height,
    )
    return VideoMetadata.model_validate_json(metadata_json)


def build_video_artifact(
    config_json: str,
    *,
    video_path: str,
    width: int,
    height: int,
) -> VideoArtifact:
    """Build a typed video artifact from config JSON, path, and dimensions."""
    artifact_json = _internal.encode.py_build_video_artifact_json(
        config_json,
        video_path,
        width,
        height,
    )
    return VideoArtifact.model_validate_json(artifact_json)


class VideoEncoderConfig(BaseModel):
    """Configuration for the video encoder.

    This config creates an encoder that converts image frames to
    MP4 video files. Supports AV1 (default), H.264, and H.265 codecs.
    This config only contains encoding-related settings; the output
    location is determined by the Context.

    Attributes:
        fps: Frames per second for the output video. Default is 100.
        gop: Group of Pictures size (keyframe interval). Default is 20.
        crf: Constant Rate Factor for quality. Lower is better quality
            but larger file size. Default is "34".
        scaling: Scaling algorithm to use. Default is Bicubic.
        codec_config: Codec-specific configuration. Default is AV1 with
            auto-detect settings. Use ``CodecConfig.av1()``, ``CodecConfig.h264()``,
            or ``CodecConfig.h265()`` to create custom configurations.

    Video Output Location:
        Videos are saved to ``{video_cache_dir}/{uuid}/{topic}.mp4``, where:

        - ``video_cache_dir`` is obtained from the Context (either directly set
          via ``context.set_video_cache_dir()`` or defaults to ``./video_cache``)
        - ``uuid`` is obtained from ``airoa_metadata`` in the Context

    This class provides two methods for processing data:

    - ``run(context)``: Process data within a Context object.
    - ``encode(image_data, video_cache_dir, uuid)``: Process image
      frames directly and return typed video artifacts.

    Example:
        >>> # Default canonical AV1 configuration
        >>> config = VideoEncoderConfig()
        >>> encoder = config.build()
        >>> # Using H.264 for faster decoding
        >>> config = VideoEncoderConfig(
        ...     codec_config=CodecConfig.h264(threads=4, preset=X264Preset.Fast)
        ... )
        >>> encoder = config.build()
    """

    model_config = ConfigDict(arbitrary_types_allowed=True)
    fps: int = 100
    gop: int = 20
    crf: str = "34"
    scaling: ScalingFlag = Field(default_factory=lambda: ScalingFlag.Bicubic)
    codec_config: CodecConfig | None = None

    @model_serializer
    def _serialize(self) -> dict:
        """Serialize via Rust serde for correct PyO3 type handling.

        ``scaling`` (ScalingFlag) and ``codec_config`` (CodecConfig) are
        Rust-native PyO3 types that Pydantic cannot serialize natively.
        Delegating to ``_to_inner().to_dict()`` uses Rust's serde
        implementation as the single source of truth, ensuring the JSON
        output always matches what Rust serde expects for deserialization.
        """
        return self._to_inner().to_dict()

    def build(self) -> VideoEncoder:
        """Create a VideoEncoder from this config.

        Returns:
            A new VideoEncoder instance.
        """
        return VideoEncoder(self)

    def _to_inner(self) -> _internal.encode.VideoEncoderConfig:
        """Convert to internal Rust config object."""
        return _internal.encode.VideoEncoderConfig(
            self.fps,
            self.gop,
            self.crf,
            self.scaling,
            self.codec_config,
        )

    def output_dimensions(self, width: int, height: int) -> tuple[int, int]:
        """Resolve encoded dimensions for a source frame."""
        if width <= 0 or height <= 0:
            raise ValueError("output dimensions require positive width and height")

        config_dict = self._to_inner().to_dict()
        resize = config_dict.get("resize")
        if resize is None:
            return width, height
        return int(resize["width"]), int(resize["height"])


class VideoEncoder:
    """Converts image frames to MP4 video files.

    This encoder takes image data from the Context and creates
    MP4 video files using FFmpeg. Videos are saved to
    ``{video_cache_dir}/{uuid}/{topic}.mp4``, where:

    - ``video_cache_dir`` is obtained from the Context
    - ``uuid`` is obtained from ``airoa_metadata`` in the Context

    The encoder uses AV1 codec (libsvtav1) with configurable quality
    settings. Output files are named based on the topic name (e.g.,
    "/camera/image" becomes "{uuid}/camera/image.mp4").

    This class provides two methods for processing data:

    - ``run(context)``: Process data within a Context object.
    - ``encode(image_data, video_cache_dir, uuid)``: Process image
      frames directly and return typed video artifacts.

    Note:
        The ``run()`` method requires:

        - ``airoa_metadata`` to be present in the context (loaded by
          Rosbag2Ingestor with meta.json)
        - Either ``video_cache_dir`` or ``output_dir`` set in the context

        The ``encode()`` method is the convenience API that accepts
        ``video_cache_dir`` and ``uuid`` as explicit parameters.

    Example:
        >>> config = VideoEncoderConfig()
        >>> encoder = config.build()
        >>> # Using run() with Context
        >>> context.set_video_cache_dir("./video_cache")
        >>> context = encoder.run(context)
        >>> # Using encode() directly with image data
        >>> artifacts = encoder.encode(image_data, "./video_cache", "my-uuid")
    """

    def __init__(self, config: VideoEncoderConfig):
        """Create a new VideoEncoder.

        Args:
            config: The configuration for this encoder.
        """
        self.config = config
        self._inner = _internal.encode.VideoEncoder(config._to_inner())

    def run(self, context: Context) -> Context:
        """Run the encoder on the given context.

        This converts all image data in the context to video files.
        Videos are saved to ``{video_cache_dir}/{uuid}/{topic}.mp4``.

        Args:
            context: The context containing image data to encode. Must have:

                - ``airoa_metadata`` (loaded by Rosbag2Ingestor with meta.json)
                - Either ``video_cache_dir`` or ``output_dir`` set in the context.
                  If only ``output_dir`` is set, videos are saved to
                  ``{output_dir}/video_cache/{uuid}/{topic}.mp4``.

        Returns:
            The context with video paths added.

        Example:
            >>> config = VideoEncoderConfig()
            >>> encoder = config.build()
            >>> # Option 1: set video_cache_dir explicitly
            >>> context.set_video_cache_dir("./video_cache")
            >>> context = encoder.run(context)
            >>> # Option 2: use output_dir (common in Orchestrator pipelines)
            >>> context.set_output_dir("./output")
            >>> context = encoder.run(context)  # saves to ./output/video_cache/{uuid}/
        """
        self._inner.run(context.inner)
        return context

    def _canonical_config_json(self) -> str:
        """Return canonical compact JSON for this encoder configuration."""
        return validate_video_config_json(
            self.config.model_dump_json(exclude_none=True)
        )

    def encode(
        self,
        image_data: dict[str, list[PyImageFrame]],
        video_cache_dir: str,
        uuid: str,
    ) -> dict[str, VideoArtifact]:
        """Encode image frames and return typed video artifacts.

        This is the primary convenience API for direct encoding without the
        full Context-based pipeline. The returned artifacts include both the
        local encoded path and canonical metadata derived from the validated
        encoder config.

        Args:
            image_data: Dictionary mapping topic names to lists of PyImageFrame.
            video_cache_dir: Base directory for video cache files.
            uuid: Unique identifier for this dataset.

        Returns:
            Dictionary mapping topic names to typed video artifacts.
        """
        if not image_data:
            return {}

        context = Context()
        context.set_video_cache_dir(video_cache_dir)
        context.set_image_data(image_data)
        context.set_airoa_metadata(_create_minimal_metadata(uuid))
        context = self.run(context)
        local_paths = context.video_paths or {}
        config_json = self._canonical_config_json()

        artifacts: dict[str, VideoArtifact] = {}
        for topic, frames in image_data.items():
            if not frames:
                continue
            shape = getattr(frames[0], "shape", None)
            if shape is None:
                raise ValueError(
                    f"Image frame shape is required to build video artifact metadata: {topic}"
                )
            video_path = local_paths.get(topic)
            if video_path is None:
                raise RuntimeError(f"Encoded video path missing for topic: {topic}")
            output_width, output_height = self.config.output_dimensions(
                shape.width,
                shape.height,
            )
            artifacts[topic] = build_video_artifact(
                config_json,
                video_path=video_path,
                width=output_width,
                height=output_height,
            )

        return artifacts


def _create_minimal_metadata(uuid: str) -> dict[str, Any]:
    """Create minimal airoa metadata with the given UUID.

    This creates the minimum required metadata structure for VideoEncoder
    to extract the UUID for directory naming.
    """
    return {
        "uuid": uuid,
        "version": "1.3",
        "files": [],
        "context": {"entities": [], "components": []},
        "run": {
            "total_time_s": 0.0,
            "instructions": [],
            "segments": [],
            "episode_label": "",
        },
    }
