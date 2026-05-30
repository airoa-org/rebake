"""Zero Order Hold time synchronizer."""

from __future__ import annotations

import pyarrow as pa
from pydantic import BaseModel, ConfigDict

from .. import _internal
from ..core.context import Context


class ZeroOrderHoldTimeSynchronizerConfig(BaseModel):
    """Configuration for the Zero Order Hold time synchronizer.

    Zero Order Hold (ZOH) resamples all topics to a fixed frame rate.
    For each output timestamp, it uses the most recent value that was
    available at that time. This is useful when you need data at a
    consistent frame rate for machine learning models.

    Attributes:
        fps: The output frame rate in frames per second (Hz).

    Example:
        >>> config = ZeroOrderHoldTimeSynchronizerConfig(fps=30)
        >>> synchronizer = config.build()
        >>> # Using run() with Context
        >>> context = synchronizer.run(context)
        >>> # Using synchronize() with Arrow Tables
        >>> synced = synchronizer.synchronize({"/topic1": table1, "/topic2": table2})
    """

    model_config = ConfigDict(arbitrary_types_allowed=True)
    fps: int

    def build(self) -> ZeroOrderHoldTimeSynchronizer:
        """Create a ZeroOrderHoldTimeSynchronizer from this config.

        Returns:
            A new ZeroOrderHoldTimeSynchronizer instance.
        """
        return ZeroOrderHoldTimeSynchronizer(self)

    def _to_inner(self) -> _internal.synchronize.PyZeroOrderHoldTimeSynchronizerConfig:
        """Convert to internal Rust config object."""
        return _internal.synchronize.PyZeroOrderHoldTimeSynchronizerConfig(self.fps)


class ZeroOrderHoldTimeSynchronizer:
    """Synchronizes data using Zero Order Hold method.

    This synchronizer creates a uniform time grid based on the specified
    frame rate. The time range starts from the latest start time across
    all topics, so timestamps before any topic has data are skipped.
    For each timestamp in the grid, it finds the most recent value
    from each topic.

    This class provides two methods for processing data:

    - ``run(context)``: Process data within a Context object.
    - ``synchronize(topics)``: Process Arrow Tables directly.

    This method is good for:

    - Creating fixed frame rate data for VLA training
    - Aligning sensors with different update rates

    Example:
        >>> config = ZeroOrderHoldTimeSynchronizerConfig(fps=30)
        >>> synchronizer = config.build()
        >>> # Using run() with Context
        >>> context = synchronizer.run(context)
        >>> # Using synchronize() with Arrow Tables
        >>> synced = synchronizer.synchronize({"/topic1": table1, "/topic2": table2})
    """

    def __init__(self, config: ZeroOrderHoldTimeSynchronizerConfig):
        """Create a new ZeroOrderHoldTimeSynchronizer.

        Args:
            config: The configuration for this synchronizer.
        """
        self.config = config
        self._inner = _internal.synchronize.PyZeroOrderHoldTimeSynchronizer(
            config._to_inner()
        )

    def run(self, context: Context) -> Context:
        """Run the synchronizer on the given context.

        This resamples all topics in the dataset to the configured
        frame rate using zero order hold interpolation.

        Args:
            context: The context containing data to synchronize.

        Returns:
            The context with synchronized data.

        Example:
            >>> context = synchronizer.run(context)
        """
        self._inner.run(context.inner)
        return context

    def synchronize(
        self,
        topics: dict[str, pa.Table],
    ) -> dict[str, pa.Table]:
        """Synchronize Arrow Tables to a fixed frame rate.

        Args:
            topics: Dictionary mapping topic names to Arrow Tables.
                Each table must have a ``timestamp_ns`` column (UInt64).

        Returns:
            Dictionary mapping topic names to synchronized Arrow Tables.
            Each output table has additional columns:

            - synched_timestamp_ns: The synchronized timestamp
            - is_fresh: Whether this row contains fresh data (not held)

        Example:
            >>> topics = {"/joint_states": js_table, "/tf_buffer": tf_table}
            >>> synced = synchronizer.synchronize(topics)
        """
        context = Context.from_tables(topics)
        context = self.run(context)

        return {
            topic: pa.Table.from_batches([context.get_record_batch(topic)])
            for topic in topics
        }
