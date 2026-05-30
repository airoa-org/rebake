# rebake.synchronize

Time synchronization algorithms for aligning data from multiple topics.

## Zero-Order Hold Synchronizer

Holds the last known value until a new value arrives. This is the recommended synchronizer for most use cases.

::: rebake.synchronize.ZeroOrderHoldTimeSynchronizerConfig
    options:
      members:
        - fps
        - build

::: rebake.synchronize.ZeroOrderHoldTimeSynchronizer
    options:
      members:
        - run

## Nearest Neighbor Synchronizer

Selects the sample nearest to each target timestamp.

::: rebake.synchronize.NearestNeighborTimeSynchronizerConfig
    options:
      members:
        - fps
        - build

::: rebake.synchronize.NearestNeighborTimeSynchronizer
    options:
      members:
        - run
