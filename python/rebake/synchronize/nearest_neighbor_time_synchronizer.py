"""Nearest Neighbor time synchronizer."""

from __future__ import annotations

import pyarrow as pa
from pydantic import BaseModel, ConfigDict

from .. import _internal
from ..core.context import Context


class NearestNeighborTimeSynchronizerConfig(BaseModel):
    """Configuration for the Nearest Neighbor time synchronizer.

    Nearest Neighbor resamples all topics to a fixed frame rate.
    For each output timestamp, it picks the data point that is
    closest in time. Unlike Zero Order Hold, this method can pick
    future values if they are closer than past values.

    Attributes:
        fps: The output frame rate in frames per second (Hz).

    Example:
        >>> config = NearestNeighborTimeSynchronizerConfig(fps=30)
        >>> synchronizer = config.build()
        >>> # Using run() with Context
        >>> context = synchronizer.run(context)
        >>> # Using synchronize() with Arrow Tables
        >>> synced = synchronizer.synchronize({"/topic1": table1, "/topic2": table2})
    """

    model_config = ConfigDict(arbitrary_types_allowed=True)
    fps: int

    def build(self) -> NearestNeighborTimeSynchronizer:
        """Create a NearestNeighborTimeSynchronizer from this config.

        Returns:
            A new NearestNeighborTimeSynchronizer instance.
        """
        return NearestNeighborTimeSynchronizer(self)

    def _to_inner(
        self,
    ) -> _internal.synchronize.PyNearestNeighborTimeSynchronizerConfig:
        """Convert to internal Rust config object."""
        return _internal.synchronize.PyNearestNeighborTimeSynchronizerConfig(self.fps)


class NearestNeighborTimeSynchronizer:
    """Synchronizes data using Nearest Neighbor method.

    This synchronizer creates a uniform time grid based on the specified
    frame rate. For each timestamp in the grid, it finds the data point
    from each topic that is closest in time (either before or after).

    This class provides two methods for processing data:

    - ``run(context)``: Process data within a Context object.
    - ``synchronize(topics)``: Process Arrow Tables directly.

    This method is good for:

    - Preserving original sensor values as much as possible
    - Cases where future data is acceptable
    - When you want the most accurate value for each timestamp

    Example:
        >>> config = NearestNeighborTimeSynchronizerConfig(fps=30)
        >>> synchronizer = config.build()
        >>> # Using run() with Context
        >>> context = synchronizer.run(context)
        >>> # Using synchronize() with Arrow Tables
        >>> synced = synchronizer.synchronize({"/topic1": table1, "/topic2": table2})
    """

    def __init__(self, config: NearestNeighborTimeSynchronizerConfig):
        """Create a new NearestNeighborTimeSynchronizer.

        Args:
            config: The configuration for this synchronizer.
        """
        self.config = config
        self._inner = _internal.synchronize.PyNearestNeighborTimeSynchronizer(
            config._to_inner()
        )

    def run(self, context: Context) -> Context:
        """Run the synchronizer on the given context.

        This resamples all topics in the dataset to the configured
        frame rate using nearest neighbor interpolation.

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
            - is_fresh: Whether this row contains fresh data

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
