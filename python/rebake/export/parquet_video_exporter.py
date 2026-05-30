"""ParquetVideoExporter for structured data export."""

from __future__ import annotations

from pydantic import BaseModel, ConfigDict

from .. import _internal
from ..core.context import Context
from ..encode.video_encoder import VideoEncoderConfig


class ParquetVideoExporterConfig(BaseModel):
    """Configuration for the ParquetVideoExporter stage.

    This stage exports Context data to a structured directory format:
    - Parquet files for each topic
    - Encoded video files (if image_data exists)
    - Registry files (_metadata.parquet, _topic_type_map.parquet, _video_registry.parquet)

    Output Structure:
        {output_dir}/{uuid}/
          parquet/
            {topic}.parquet           # Topic data
            _metadata.parquet         # Airoa metadata
            _topic_type_map.parquet   # Topic name to message type mapping
            _video_registry.parquet   # Topic name to video path mapping
          videos/
            {topic}.mp4               # Encoded video files

    Attributes:
        output_dir: Root output directory. UUID subdirectories are created automatically.
        video_config: Optional video encoder configuration. If not provided,
                      defaults to ``VideoEncoderConfig()`` (AV1, fps=100, gop=20, crf=34).

    Example:
        >>> # Simple usage with defaults
        >>> config = ParquetVideoExporterConfig(output_dir="/data/output")

        >>> # With custom video settings
        >>> from rebake.encode import VideoEncoderConfig, CodecConfig
        >>> video_config = VideoEncoderConfig(fps=60, codec_config=CodecConfig.h264())
        >>> config = ParquetVideoExporterConfig(
        ...     output_dir="/data/output",
        ...     video_config=video_config
        ... )
    """

    model_config = ConfigDict(arbitrary_types_allowed=True)
    output_dir: str
    video_config: VideoEncoderConfig | None = None

    def build(self) -> ParquetVideoExporter:
        """Create a ParquetVideoExporter from this config.

        Returns:
            A new ParquetVideoExporter instance.
        """
        return ParquetVideoExporter(self)

    def _to_inner(self) -> _internal.export.ParquetVideoExporterConfig:
        """Convert to internal Rust config object."""
        inner_video_config = None
        if self.video_config is not None:
            inner_video_config = self.video_config._to_inner()
        return _internal.export.ParquetVideoExporterConfig(
            self.output_dir,
            inner_video_config,
        )


class ParquetVideoExporter:
    """Exports Context data to structured Parquet + Video format.

    This is a terminal stage that creates the following directory structure:

        {output_dir}/{uuid}/
          parquet/
            {topic}.parquet           # Topic data
            _metadata.parquet         # Airoa metadata
            _topic_type_map.parquet   # Topic name to message type mapping
            _video_registry.parquet   # Topic name to video path mapping
          videos/
            {topic}.mp4               # Encoded video files

    Preconditions:
        - dataset: Required - HashMap of topic names to LazyFrames
        - airoa_metadata: Required - Metadata containing UUID
        - topic_message_type_map: Required - Topic to message type mapping
        - image_data: Optional - If present, videos will be encoded

    Postconditions:
        - output_dir: Set to {config.output_dir}/{uuid}
        - bundle_root: Set to the exported bundle root
        - video_registry: Set if videos were written

    Example:
        >>> from rebake.ingest import Rosbag2Ingestor, Rosbag2IngestorConfig
        >>> from rebake.enrich import UuidEnricher, UuidEnricherConfig
        >>> from rebake.export import ParquetVideoExporter, ParquetVideoExporterConfig
        >>>
        >>> # Build pipeline
        >>> ingestor = Rosbag2IngestorConfig(require_metadata=True).build()
        >>> enricher = UuidEnricherConfig().build()
        >>> exporter = ParquetVideoExporterConfig("/data/output").build()
        >>>
        >>> # Run pipeline
        >>> context = Context()
        >>> context.set_rosbag_path("/path/to/rosbag.mcap")
        >>> context = ingestor.run(context)
        >>> context = enricher.run(context)
        >>> context = exporter.run(context)
        >>> print(f"Exported to: {context.output_dir}")
    """

    def __init__(self, config: ParquetVideoExporterConfig):
        """Create a new ParquetVideoExporter.

        Args:
            config: The configuration for this exporter.
        """
        self.config = config
        self._inner = _internal.export.ParquetVideoExporter(config._to_inner())

    def run(self, context: Context) -> Context:
        """Run the exporter on the given context.

        Exports all data to the structured directory format.
        Creates Parquet files for each topic, metadata files,
        and encodes videos if image_data is present.

        Args:
            context: The context containing data to export. Must have:
                - dataset (required)
                - airoa_metadata (required)
                - topic_message_type_map (required)
                - image_data (optional - videos encoded if present)

        Returns:
            The context with output_dir, bundle_root, and video registry set.

        Raises:
            RuntimeError: If required preconditions are not met or I/O fails.
        """
        self._inner.run(context.inner)
        return context
