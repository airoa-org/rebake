"""Shift enricher for creating temporal offsets in data columns.

This enricher creates a new topic with shifted column values, which is essential
for creating action labels in VLA model training where "action = future observation".
The source topic is preserved unchanged.
"""

from __future__ import annotations

from typing import Literal

import pyarrow as pa
from pydantic import BaseModel, ConfigDict

from .. import _internal
from ..core.context import Context


class ShiftEnricherConfig(BaseModel):
    """Configuration for the Shift enricher.

    This enricher reads from a source topic, shifts its column values by a
    specified number of steps, and writes the result to a new output topic.
    The source topic is preserved unchanged, allowing both state (original)
    and action (shifted) data to coexist.

    Time metadata columns (``synched_timestamp_ns``, ``timestamp_ns``,
    ``is_fresh``) are excluded from shifting.

    Attributes:
        source_topic: Source topic to read data from. This topic is not modified.
        output_topic: Output topic name for the shifted data.
        shift_steps: Number of steps to shift. Positive = future direction,
            negative = past direction, 0 = no-op.
        fill_strategy: Strategy for filling null values created by shifting.
            ``"edge"`` (default) uses forward/backward fill.
            ``"zero"`` fills numeric scalars with 0, falls back to edge for others.

    Example:
        >>> config = ShiftEnricherConfig(
        ...     source_topic="/joint_states",
        ...     output_topic="/joint_states/action",
        ...     shift_steps=1,
        ...     fill_strategy="edge",
        ... )
        >>> enricher = config.build()
        >>> context = enricher.run(context)
    """

    model_config = ConfigDict(arbitrary_types_allowed=True)
    source_topic: str
    output_topic: str
    shift_steps: int
    fill_strategy: Literal["edge", "zero"] = "edge"

    def build(self) -> ShiftEnricher:
        """Create a ShiftEnricher from this config.

        Returns:
            A new ShiftEnricher instance.
        """
        return ShiftEnricher(self)

    def _to_inner(self) -> _internal.enrich.PyShiftEnricherConfig:
        """Convert to internal Rust config object."""
        return _internal.enrich.PyShiftEnricherConfig(
            self.source_topic,
            self.output_topic,
            self.shift_steps,
            self.fill_strategy,
        )


class ShiftEnricher:
    """Creates a new topic with shifted column values for temporal offset.

    This enricher reads from a source topic, shifts all data columns
    (excluding time metadata) by the specified number of steps, and
    writes the result to a new output topic. The source topic is
    preserved unchanged.

    This is primarily used for VLA model training where actions
    correspond to future observations. By creating a shifted topic,
    you can use both state (original) and action (shifted) data
    simultaneously.

    Time metadata columns (``synched_timestamp_ns``, ``timestamp_ns``,
    ``is_fresh``) are preserved and not shifted.

    Example:
        >>> config = ShiftEnricherConfig(
        ...     source_topic="/joint_states",
        ...     output_topic="/joint_states/action",
        ...     shift_steps=1,
        ... )
        >>> enricher = config.build()
        >>> context = enricher.run(context)
    """

    def __init__(self, config: ShiftEnricherConfig):
        """Create a new ShiftEnricher.

        Args:
            config: The configuration for this enricher.
        """
        self.config = config
        self._inner = _internal.enrich.PyShiftEnricher(config._to_inner())

    def run(self, context: Context) -> Context:
        """Run the enricher on the given context.

        This reads from the source topic, shifts all data columns by the
        configured number of steps, and inserts the result as a new output
        topic. The source topic is preserved unchanged.

        Args:
            context: The context containing data to shift.

        Returns:
            The context with the new shifted output topic added.

        Example:
            >>> context = enricher.run(context)
        """
        self._inner.run(context.inner)
        return context

    def enrich(self, table: pa.Table) -> pa.Table:
        """Shift column values in an Arrow Table.

        Convenience method that wraps the table in a Context as the source
        topic, runs the enricher, and extracts the output topic result.

        Args:
            table: Arrow Table containing data to shift.

        Returns:
            Arrow Table with shifted column values.

        Example:
            >>> shifted = enricher.enrich(joint_states)
        """
        source_topic = self.config.source_topic
        output_topic = self.config.output_topic
        context = Context.from_tables({source_topic: table})
        context = self.run(context)
        return pa.Table.from_batches([context.get_record_batch(output_topic)])
