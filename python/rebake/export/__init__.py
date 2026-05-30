"""Export stages for structured data output.

This module provides stages for exporting Context data to structured formats
suitable for data lake ingestion.

Available exporters:
- ParquetVideoExporter: Exports data to structured Parquet + Video format.

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

from .parquet_video_exporter import (
    ParquetVideoExporter,
    ParquetVideoExporterConfig,
)

__all__ = [
    "ParquetVideoExporter",
    "ParquetVideoExporterConfig",
]
