"""UUID enricher that adds rosbag_uuid column to all topics."""

from __future__ import annotations

from typing import Any

import pyarrow as pa
from pydantic import BaseModel, ConfigDict

from .. import _internal
from ..core.context import Context
from ..exceptions import EnrichError


class UuidEnricherConfig(BaseModel):
    """Configuration for the UUID enricher.

    This enricher adds a `rosbag_uuid` column to all topics in the dataset.
    The UUID is read from the airoa metadata (meta.json) that was loaded
    during ingestion.

    This class provides two methods for processing data:

    - ``run(context)``: Process data within a Context object.
    - ``enrich(table, metadata)``: Process Arrow Tables directly.

    Example:
        >>> config = UuidEnricherConfig()
        >>> enricher = config.build()
        >>> # Using run() with Context
        >>> context = enricher.run(context)
        >>> # Using enrich() with Arrow Tables
        >>> enriched = enricher.enrich(table, metadata)
    """

    model_config = ConfigDict(arbitrary_types_allowed=True)

    def build(self) -> UuidEnricher:
        """Create a UuidEnricher from this config.

        Returns:
            A new UuidEnricher instance.
        """
        return UuidEnricher(self)

    def _to_inner(self) -> _internal.enrich.PyUuidEnricherConfig:
        """Convert to internal Rust config object."""
        return _internal.enrich.PyUuidEnricherConfig()


class UuidEnricher:
    """Adds rosbag_uuid column to all topics in the dataset.

    This enricher reads the UUID from the airoa metadata (meta.json) that was
    loaded by the Ingestor, and adds it as a column to every topic's DataFrame.

    This enables tracking which rosbag each record came from when multiple
    rosbags are processed and stored together (e.g., in Iceberg tables).

    This class provides two methods for processing data:

    - ``run(context)``: Process data within a Context object.
    - ``enrich(table, metadata)``: Process Arrow Tables directly.

    Example:
        >>> config = UuidEnricherConfig()
        >>> enricher = config.build()
        >>> # Using run() with Context
        >>> context = enricher.run(context)
        >>> # Using enrich() with Arrow Tables
        >>> enriched = enricher.enrich(table, metadata)
    """

    def __init__(self, config: UuidEnricherConfig):
        """Create a new UuidEnricher.

        Args:
            config: The configuration for this enricher.
        """
        self.config = config
        self._inner = _internal.enrich.PyUuidEnricher(config._to_inner())

    def run(self, context: Context) -> Context:
        """Run the enricher on the given context.

        Args:
            context: The context to process.

        Returns:
            The context with rosbag_uuid column added to all topics.

        Raises:
            EnrichError: If the enrichment fails (e.g., no metadata available).

        Example:
            >>> context = enricher.run(context)
        """
        try:
            self._inner.run(context.inner)
        except Exception as e:
            raise EnrichError(str(e)) from e
        return context

    def enrich(self, table: pa.Table, metadata: dict[str, Any]) -> pa.Table:
        """Add rosbag_uuid column to the table.

        Args:
            table: Arrow Table to enrich.
            metadata: Airoa metadata dictionary containing the 'uuid' field.

        Returns:
            Arrow Table with rosbag_uuid column added.

        Raises:
            EnrichError: If the metadata does not contain a 'uuid' field.

        Example:
            >>> enriched = enricher.enrich(table, metadata)
        """
        uuid = metadata.get("uuid")
        if uuid is None:
            raise EnrichError("metadata does not contain 'uuid' field")

        num_rows = table.num_rows
        uuid_array = pa.array([uuid] * num_rows, type=pa.string())
        return table.append_column("rosbag_uuid", uuid_array)
