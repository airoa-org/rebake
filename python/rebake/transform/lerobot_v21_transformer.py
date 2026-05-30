"""LeRobot v2.1 transformer for outputting ML-ready datasets."""

from __future__ import annotations

from typing import Any

import pyarrow as pa
from pydantic import BaseModel, ConfigDict

from .. import _internal
from ..common import PyImageFrame
from ..core.context import Context
from ..encode import VideoArtifact, VideoEncoderConfig


class LeRobotV21TransformerConfig(BaseModel):
    """Configuration for the LeRobot v2.1 transformer.

    This transformer converts synchronized data into the LeRobot v2.1
    dataset format. LeRobot is a format designed for robot learning
    that includes Parquet files for structured data and MP4 videos
    for image observations.

    The output includes:

    - data/chunk-XXX/episode_XXXXXX.parquet: Episode data
    - videos/chunk-XXX/observation.image.*/episode_XXXXXX.mp4: Videos
    - meta/info.json: Dataset metadata
    - meta/episodes.jsonl: Episode information
    - meta/episodes_stats.jsonl: Episode statistics
    - meta/tasks.jsonl: Task definitions

    Attributes:
        outdir: Output directory for the LeRobot dataset.
        robot_model: Robot model configuration. Either:
            - list[dict]: Inline TopicFeatureMap entries defining
              ROS topic -> LeRobot feature mappings.
            - str: Path to a YAML file containing the mappings.
        video_config: Optional video encoder configuration. Accepts either:
            - ``VideoEncoderConfig``: A Pydantic model instance (programmatic use).
            - ``dict``: A raw dictionary (from YAML config). Passed through to
              Rust serde as JSON, which handles deserialization of all fields
              including ``scaling`` (ScalingFlag) and ``codec_config`` (CodecConfig).
            - ``None``: Use default video settings.

    This class provides two methods for processing data:

    - ``run(context)``: Process data within a Context object.
    - ``transform(topics, metadata, ...)``: Process Arrow Tables directly.

    Example:
        >>> # Programmatic construction with VideoEncoderConfig
        >>> config = LeRobotV21TransformerConfig(
        ...     outdir="./lerobot_output",
        ...     robot_model=[
        ...         {"type": "Parquet", "topic": "/joint_states",
        ...          "field": "/position", "feature": "observation.state"},
        ...     ],
        ...     video_config=VideoEncoderConfig(fps=10, crf="23"),
        ... )
        >>> # Dict construction (from YAML config)
        >>> config = LeRobotV21TransformerConfig(
        ...     outdir="./lerobot_output",
        ...     robot_model=[...],
        ...     video_config={"fps": 10, "crf": "23", "codec_config": {"H264": {"threads": 4}}},
        ... )
        >>> transformer = config.build()
    """

    model_config = ConfigDict(arbitrary_types_allowed=True)

    outdir: str
    robot_model: list[dict] | str
    video_config: VideoEncoderConfig | dict | None = None

    def build(self) -> LeRobotV21Transformer:
        """Create a LeRobotV21Transformer from this config.

        Returns:
            A new LeRobotV21Transformer instance.
        """
        return LeRobotV21Transformer(self)

    def _to_inner(self) -> _internal.transform.PyLeRobotV21TransformerConfig:
        """Convert to internal Rust config object."""
        return _internal.transform.PyLeRobotV21TransformerConfig(
            self.model_dump_json(exclude_none=True)
        )


class LeRobotV21Transformer:
    """Transforms data into LeRobot v2.1 dataset format.

    This transformer takes synchronized and enriched data from the
    Context and outputs a complete LeRobot dataset. The dataset
    can be used directly with the LeRobot library for robot learning.

    The transformer:

    1. Maps ROS topics to LeRobot features using the robot model
    2. Encodes image data as MP4 videos
    3. Writes episode data as Parquet files
    4. Generates metadata files

    This class provides two methods for processing data:

    - ``run(context)``: Process data within a Context object.
    - ``transform(topics, metadata, ...)``: Process Arrow Tables directly.

    Example:
        >>> config = LeRobotV21TransformerConfig(
        ...     outdir="./lerobot_output",
        ...     robot_model=[
        ...         {"type": "Parquet", "topic": "/joint_states",
        ...          "field": "/position", "feature": "observation.state"},
        ...     ],
        ... )
        >>> transformer = config.build()
        >>> # Using run() with Context
        >>> context = transformer.run(context)
        >>> # Using transform() with Arrow Tables
        >>> transformer.transform(topics, metadata, fps=30, video_registry=video_registry)
    """

    def __init__(self, config: LeRobotV21TransformerConfig):
        """Create a new LeRobotV21Transformer.

        Args:
            config: The configuration for this transformer.
        """
        self.config = config
        self._inner = _internal.transform.PyLeRobotV21Transformer(config._to_inner())

    def run(self, context: Context) -> Context:
        """Run the transformer on the given context.

        This converts the synchronized data to LeRobot format and
        writes all output files to the configured output directory.

        Args:
            context: The context with synchronized data to transform.

        Returns:
            The context after transformation.

        Example:
            >>> context = transformer.run(context)
        """
        self._inner.run(context.inner)
        return context

    def transform(
        self,
        topics: dict[str, pa.Table],
        metadata: dict[str, Any],
        fps: int,
        video_registry: dict[str, VideoArtifact] | None = None,
        bundle_root: str | None = None,
        image_data: dict[str, list[PyImageFrame]] | None = None,
    ) -> None:
        """Transform synchronized data to LeRobot format.

        This is a Context-free API for transforming data to LeRobot format.
        It's designed to be used when you have Arrow Tables from external
        sources (e.g., Iceberg).

        Args:
            topics: Dictionary mapping topic names to Arrow Tables.
                Each table should contain synchronized data.
            metadata: Airoa metadata dictionary. Must include 'uuid',
                'run.segments', and 'run.instructions'.
            fps: Frame rate for the output videos and data.
            video_registry: Dictionary mapping topic names to typed
                ``VideoArtifact`` objects. Use this when videos have already
                been encoded and saved. The transformer will read frames from
                these files on-demand without decoding the full source videos
                up front.
            bundle_root: Optional root directory used to resolve relative
                video artifact paths in ``video_registry``.
            image_data: Dictionary mapping topic names to lists of PyImageFrame.
                Use this when image frames are available in memory.
                This keeps all frame data in memory and should only be used
                when the caller already owns decoded frames.

        Note:
            ``image_data`` and ``video_registry`` are mutually exclusive.
            Use ``video_registry`` for the normal lazy-decode path and reserve
            ``image_data`` for callers that intentionally already hold frames
            in memory.

        Example:
            >>> # Option 1: Using video_registry (reads from video files lazily)
            >>> transformer.transform(
            ...     topics, metadata, fps=30,
            ...     video_registry=video_registry
            ... )
            >>> # Option 2: Using image_data (in-memory frames)
            >>> transformer.transform(
            ...     topics, metadata, fps=30,
            ...     image_data=image_data
            ... )
        """
        context = Context.from_tables(topics)
        context.set_airoa_metadata(metadata)
        context.set_fps(fps)

        if image_data is not None and video_registry is not None:
            raise ValueError(
                "image_data and video_registry are mutually exclusive; pass only one input source"
            )

        if image_data is not None:
            context.set_image_data(image_data)
        elif video_registry is not None:
            context.set_video_registry(video_registry, bundle_root=bundle_root)

        self.run(context)
