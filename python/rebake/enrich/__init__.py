"""Enrichers that add new data columns based on existing data.

This module provides enrichers that process existing data and add new
columns or topics to the Context. Common uses include computing
transforms, calculating deltas (changes), and extracting commands.

Enrichers serve two main purposes:
- Feature engineering: Create features that help VLA model training
  (e.g., delta actions, end-effector poses)
- Query simplification: Pre-compute values that would need complex
  queries if stored in a data warehouse like Iceberg

Available enrichers:
- TfBufferEnricher: Builds a TF buffer from /tf and /tf_static messages.
- TfChainEnricher: Computes transform chains between coordinate frames.
- DeltaJointPositionEnricher: Calculates changes in joint positions.
- DeltaTransformEnricher: Calculates changes in transforms.
- HeadCommandEnricher: Extracts head command data.
- HandCommandEnricher: Extracts hand command data.
- UuidEnricher: Adds rosbag_uuid column from airoa metadata.
- ShiftEnricher: Shifts column values by N steps for temporal offset.

Example:
    >>> from rebake.enrich import TfChainEnricherConfig, FramePair
    >>> config = TfChainEnricherConfig(frame_pairs=[
    ...     FramePair(source="base_link", target="hand_palm_link")
    ... ])
    >>> enricher = config.build()
    >>> context = enricher.run(context)
"""

from .delta_joint_position_enricher import (
    DeltaJointPositionEnricher,
    DeltaJointPositionEnricherConfig,
)
from .delta_transform_enricher import (
    DeltaTransformEnricher,
    DeltaTransformEnricherConfig,
)
from .hand_command_enricher import (
    HandCommandEnricher,
    HandCommandEnricherConfig,
)
from .head_command_enricher import (
    HeadCommandEnricher,
    HeadCommandEnricherConfig,
)
from .tf_buffer_enricher import (
    TfBufferEnricher,
    TfBufferEnricherConfig,
)
from .tf_chain_enricher import (
    FramePair,
    TfChainEnricher,
    TfChainEnricherConfig,
)
from .shift_enricher import (
    ShiftEnricher,
    ShiftEnricherConfig,
)
from .uuid_enricher import (
    UuidEnricher,
    UuidEnricherConfig,
)

__all__ = [
    "DeltaJointPositionEnricher",
    "DeltaJointPositionEnricherConfig",
    "DeltaTransformEnricher",
    "DeltaTransformEnricherConfig",
    "HandCommandEnricher",
    "HandCommandEnricherConfig",
    "HeadCommandEnricher",
    "HeadCommandEnricherConfig",
    "TfBufferEnricher",
    "TfBufferEnricherConfig",
    "FramePair",
    "TfChainEnricher",
    "TfChainEnricherConfig",
    "ShiftEnricher",
    "ShiftEnricherConfig",
    "UuidEnricher",
    "UuidEnricherConfig",
]
