"""Time synchronizers for aligning data from different topics.

This module provides synchronizers that align data from different ROS topics
to a common timeline. This is important because sensors often publish data
at different rates.

Available synchronizers:
- ZeroOrderHoldTimeSynchronizer: Resamples to a fixed frame rate using
  zero-order hold (keeps the last known value).
- NearestNeighborTimeSynchronizer: Picks the closest data point for
  each timestamp.

Example:
    >>> from rebake.synchronize import ZeroOrderHoldTimeSynchronizerConfig
    >>> config = ZeroOrderHoldTimeSynchronizerConfig(fps=30)
    >>> synchronizer = config.build()
    >>> context = synchronizer.run(context)
"""

from .nearest_neighbor_time_synchronizer import (
    NearestNeighborTimeSynchronizer,
    NearestNeighborTimeSynchronizerConfig,
)
from .zero_order_hold_time_synchronizer import (
    ZeroOrderHoldTimeSynchronizer,
    ZeroOrderHoldTimeSynchronizerConfig,
)

__all__ = [
    "NearestNeighborTimeSynchronizer",
    "NearestNeighborTimeSynchronizerConfig",
    "ZeroOrderHoldTimeSynchronizer",
    "ZeroOrderHoldTimeSynchronizerConfig",
]
