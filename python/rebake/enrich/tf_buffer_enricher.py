"""TF Buffer enricher for building transform buffers."""

from __future__ import annotations

import pyarrow as pa
from pydantic import BaseModel, ConfigDict

from .. import _internal
from ..core.context import Context


class TfBufferEnricherConfig(BaseModel):
    """Configuration for the TF Buffer enricher.

    This enricher processes /tf and optional /tf_static messages and builds
    a transform buffer. The buffer stores all transforms indexed by
    time, which is needed by TfChainEnricher to compute transform chains.

    You should run this enricher before TfChainEnricher.

    Example:
        >>> config = TfBufferEnricherConfig()
        >>> enricher = config.build()
        >>> # Using run() with Context
        >>> context = enricher.run(context)
        >>> # Using enrich() with Arrow Tables
        >>> tf_buffer = enricher.enrich(tf_data, tf_static_data)
    """

    model_config = ConfigDict(arbitrary_types_allowed=True)

    def build(self) -> TfBufferEnricher:
        """Create a TfBufferEnricher from this config.

        Returns:
            A new TfBufferEnricher instance.
        """
        return TfBufferEnricher(self)

    def _to_inner(self) -> _internal.enrich.PyTfBufferEnricherConfig:
        """Convert to internal Rust config object."""
        return _internal.enrich.PyTfBufferEnricherConfig()


class TfBufferEnricher:
    """Builds a transform buffer from TF messages.

    This enricher reads /tf and optional /tf_static topics from the dataset
    and creates a time-indexed buffer of all transforms. This buffer
    is stored in the Context and used by TfChainEnricher.

    The TF buffer allows looking up the transform between any two
    frames at any point in time.

    This class provides two methods for processing data:

    - ``run(context)``: Process data within a Context object.
    - ``enrich(tf_data, ...)``: Process Arrow Tables directly.

    Example:
        >>> config = TfBufferEnricherConfig()
        >>> enricher = config.build()
        >>> # Using run() with Context
        >>> context = enricher.run(context)
        >>> # Using enrich() with Arrow Tables
        >>> tf_buffer = enricher.enrich(tf_data, tf_static_data)
    """

    def __init__(self, config: TfBufferEnricherConfig):
        """Create a new TfBufferEnricher.

        Args:
            config: The configuration for this enricher.
        """
        self.config = config
        self._inner = _internal.enrich.PyTfBufferEnricher(config._to_inner())

    def run(self, context: Context) -> Context:
        """Run the enricher on the given context.

        This reads TF topics from the context and builds the transform buffer.

        Args:
            context: The context containing TF data.
                Required topics: /tf
                Optional topics: /tf_static

        Returns:
            The context with /tf_buffer topic added.

        Example:
            >>> context = enricher.run(context)
        """
        self._inner.run(context.inner)
        return context

    def enrich(
        self,
        tf_data: pa.Table,
        tf_static_data: pa.Table | None = None,
    ) -> pa.Table:
        """Enrich TF data to build a transform buffer.

        Args:
            tf_data: Arrow Table containing /tf topic data.
                Must have columns: timestamp_ns, transforms.
            tf_static_data: Optional Arrow Table containing /tf_static data.

        Returns:
            Arrow Table containing the TF buffer with columns:

            - timestamp_ns: The timestamp in nanoseconds
            - transforms: List of transform structs with child_frame_id,
              header, transform, and is_fresh fields

        Example:
            >>> tf_buffer = enricher.enrich(tf_data, tf_static_data)
        """
        tables: dict[str, pa.Table] = {"/tf": tf_data}
        if tf_static_data is not None:
            tables["/tf_static"] = tf_static_data

        context = Context.from_tables(tables)
        context = self.run(context)

        return pa.Table.from_batches([context.get_record_batch("/tf_buffer")])
