"""Transformers for converting data to specific output formats.

This module provides transformers that convert processed data to
specific output formats for VLA training.

Available transformers:
- LeRobotV21Transformer: Outputs data in LeRobot v2.1 format.

Example:
    >>> from rebake.transform import LeRobotV21TransformerConfig
    >>> config = LeRobotV21TransformerConfig(
    ...     outdir="./lerobot_output",
    ...     robot_model="./robot_model.yaml",
    ... )
    >>> transformer = config.build()
    >>> context = transformer.run(context)
"""

from .lerobot_v21_transformer import (
    LeRobotV21Transformer,
    LeRobotV21TransformerConfig,
)
from ..encode import VideoEncoderConfig, ScalingFlag

__all__ = [
    "LeRobotV21Transformer",
    "LeRobotV21TransformerConfig",
    "VideoEncoderConfig",
    "ScalingFlag",
]
