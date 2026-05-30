"""Delta Joint Position enricher for computing joint position changes."""

from __future__ import annotations

import pyarrow as pa
from pydantic import BaseModel, ConfigDict

from .. import _internal
from ..core.context import Context


class DeltaJointPositionEnricherConfig(BaseModel):
    """Configuration for the Delta Joint Position enricher.

    This enricher calculates the change in joint positions between
    consecutive timestamps. This is useful for learning action
    policies where the action is the change in joint position.

    Attributes:
        topic_names: List of topic names containing joint state data.
            Usually this is ["/robot/joint_states"] or similar.

    Example:
        >>> config = DeltaJointPositionEnricherConfig(
        ...     topic_names=["/hsrb/joint_states"]
        ... )
        >>> enricher = config.build()
        >>> # Using run() with Context
        >>> context = enricher.run(context)
        >>> # Using enrich() with Arrow Tables
        >>> enriched = enricher.enrich(joint_states)
    """

    model_config = ConfigDict(arbitrary_types_allowed=True)
    topic_names: list[str]

    def build(self) -> DeltaJointPositionEnricher:
        """Create a DeltaJointPositionEnricher from this config.

        Returns:
            A new DeltaJointPositionEnricher instance.
        """
        return DeltaJointPositionEnricher(self)

    def _to_inner(self) -> _internal.enrich.PyDeltaJointPositionEnricherConfig:
        """Convert to internal Rust config object."""
        return _internal.enrich.PyDeltaJointPositionEnricherConfig(self.topic_names)


class DeltaJointPositionEnricher:
    """Calculates changes in joint positions over time.

    This enricher adds delta (change) columns to joint state topics.
    For each joint position column, it adds a corresponding delta
    column showing the change from the previous timestamp.

    The delta values are useful for:

    - Learning action policies (action = position change)
    - Detecting motion
    - Computing velocities

    This class provides two methods for processing data:

    - ``run(context)``: Process data within a Context object.
    - ``enrich(joint_states)``: Process Arrow Tables directly.

    Example:
        >>> config = DeltaJointPositionEnricherConfig(
        ...     topic_names=["/hsrb/joint_states"]
        ... )
        >>> enricher = config.build()
        >>> # Using run() with Context
        >>> context = enricher.run(context)
        >>> # Using enrich() with Arrow Tables
        >>> enriched = enricher.enrich(joint_states)
    """

    def __init__(self, config: DeltaJointPositionEnricherConfig):
        """Create a new DeltaJointPositionEnricher.

        Args:
            config: The configuration for this enricher.
        """
        self.config = config
        self._inner = _internal.enrich.PyDeltaJointPositionEnricher(config._to_inner())

    def run(self, context: Context) -> Context:
        """Run the enricher on the given context.

        This adds delta columns to the specified joint state topics.

        Args:
            context: The context containing joint state data.

        Returns:
            The context with delta columns added.

        Example:
            >>> context = enricher.run(context)
        """
        self._inner.run(context.inner)
        return context

    def enrich(self, joint_states: pa.Table) -> pa.Table:
        """Enrich joint state data with delta columns.

        Args:
            joint_states: Arrow Table containing joint state data.
                Must have columns: timestamp_ns, position (list of floats).

        Returns:
            Arrow Table with additional delta columns for each
            joint position.

        Example:
            >>> enriched = enricher.enrich(joint_states)
        """
        topic_name = self.config.topic_names[0]
        context = Context.from_tables({topic_name: joint_states})
        context = self.run(context)

        return pa.Table.from_batches([context.get_record_batch(topic_name)])
