"""Delta Transform enricher for computing transform changes."""

from __future__ import annotations

from typing import Literal

import pyarrow as pa
from pydantic import BaseModel, ConfigDict

from .. import _internal
from ..core.context import Context

DeltaReferenceFrame = Literal["source_frame", "previous_target_frame"]


class DeltaTransformEnricherConfig(BaseModel):
    """Configuration for the Delta Transform enricher.

    This enricher calculates the change in transforms (position and
    orientation) between consecutive timestamps. This is useful for
    learning action policies where the action is the change in pose.

    Attributes:
        topic_names: List of topic names containing transform data.
            Usually this is ["/tf_chain"] after running TfChainEnricher.
        delta_reference_frame: Reference frame for translation deltas.
            Use "previous_target_frame" for body-frame action deltas.
            Use "source_frame" for source-frame coordinate component deltas.

    Example:
        >>> config = DeltaTransformEnricherConfig(
        ...     topic_names=["/tf_chain"],
        ...     delta_reference_frame="previous_target_frame",
        ... )
        >>> enricher = config.build()
        >>> # Using run() with Context
        >>> context = enricher.run(context)
        >>> # Using enrich() with Arrow Tables
        >>> enriched = enricher.enrich(tf_chain)
    """

    model_config = ConfigDict(arbitrary_types_allowed=True)
    topic_names: list[str]
    delta_reference_frame: DeltaReferenceFrame

    def build(self) -> DeltaTransformEnricher:
        """Create a DeltaTransformEnricher from this config.

        Returns:
            A new DeltaTransformEnricher instance.
        """
        return DeltaTransformEnricher(self)

    def _to_inner(self) -> _internal.enrich.PyDeltaTransformEnricherConfig:
        """Convert to internal Rust config object."""
        return _internal.enrich.PyDeltaTransformEnricherConfig(
            self.topic_names,
            self.delta_reference_frame,
        )


class DeltaTransformEnricher:
    """Calculates changes in transforms over time.

    This enricher adds delta (change) columns to transform topics.
    For position, it calculates the difference. For orientation
    (quaternion), it calculates the relative rotation.

    The delta values are useful for:

    - Learning action policies (action = pose change)
    - Computing velocities
    - Detecting motion

    This class provides two methods for processing data:

    - ``run(context)``: Process data within a Context object.
    - ``enrich(transform_data)``: Process Arrow Tables directly.

    Example:
        >>> config = DeltaTransformEnricherConfig(
        ...     topic_names=["/tf_chain"],
        ...     delta_reference_frame="previous_target_frame",
        ... )
        >>> enricher = config.build()
        >>> # Using run() with Context
        >>> context = enricher.run(context)
        >>> # Using enrich() with Arrow Tables
        >>> enriched = enricher.enrich(tf_chain)
    """

    def __init__(self, config: DeltaTransformEnricherConfig):
        """Create a new DeltaTransformEnricher.

        Args:
            config: The configuration for this enricher.
        """
        self.config = config
        self._inner = _internal.enrich.PyDeltaTransformEnricher(config._to_inner())

    def run(self, context: Context) -> Context:
        """Run the enricher on the given context.

        This adds delta columns to the specified transform topics.

        Args:
            context: The context containing transform data.

        Returns:
            The context with delta columns added.

        Example:
            >>> context = enricher.run(context)
        """
        self._inner.run(context.inner)
        return context

    def enrich(self, transform_data: pa.Table) -> pa.Table:
        """Enrich transform data with delta columns.

        Args:
            transform_data: Arrow Table containing transform data.
                Must have columns: timestamp_ns, x, y, z, qx, qy, qz, qw.

        Returns:
            Arrow Table with additional delta columns for position
            and orientation changes.

        Example:
            >>> enriched = enricher.enrich(tf_chain)
        """
        topic_name = self.config.topic_names[0]
        context = Context.from_tables({topic_name: transform_data})
        context = self.run(context)

        return pa.Table.from_batches([context.get_record_batch(topic_name)])
