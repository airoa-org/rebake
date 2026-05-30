"""Convert ROS 2 bag files to LeRobot format.

This example demonstrates the rebake Context-free API, operating directly on
Arrow Tables without requiring a Context object.

Usage:
    uv run python examples/rosbag_to_lerobot.py \
        --rosbag-path /path/to/recording.mcap \
        --output ./lerobot_output \
        --robot-model ../config/robot_model/hsr2.yaml
"""

from __future__ import annotations

import argparse
from pathlib import Path
from typing import Any

import pyarrow as pa

from rebake.common import PyImageFrame
from rebake.enrich import (
    FramePair,
    TfBufferEnricherConfig,
    TfChainEnricherConfig,
)
from rebake.ingest import Rosbag2IngestorConfig
from rebake.synchronize import ZeroOrderHoldTimeSynchronizerConfig
from rebake.transform import LeRobotV21TransformerConfig


FPS: int = 10

HSR2_FRAME_PAIRS: list[FramePair] = [
    FramePair(source="base_footprint", target="hand_left_left_finger_tip_frame"),
    FramePair(source="base_footprint", target="hand_left_right_finger_tip_frame"),
    FramePair(source="base_footprint", target="hand_right_left_finger_tip_frame"),
    FramePair(source="base_footprint", target="hand_right_right_finger_tip_frame"),
]


def run_pipeline(rosbag_path: str, output_dir: str, robot_model_path: str) -> None:
    """Run the complete data pipeline.

    Pipeline stages:
    1. Ingest rosbag -> Arrow Tables
    2. Build TF buffer
    3. Synchronize to target FPS
    4. Compute TF chains
    5. Transform to LeRobot format (includes video encoding)
    """
    # Stage 1: Ingest rosbag -> Arrow Tables
    ingestor = Rosbag2IngestorConfig().build()
    topics: dict[str, pa.Table]
    metadata: dict[str, Any]
    image_data: dict[str, list[PyImageFrame]]
    topics, metadata, image_data, _ = ingestor.ingest(rosbag_path)

    # Stage 2: Build TF buffer
    tf_buffer_enricher = TfBufferEnricherConfig().build()
    tf_buffer: pa.Table = tf_buffer_enricher.enrich(
        tf_data=topics["/tf"],
        tf_static_data=topics.get("/tf_static"),
    )
    topics["/tf_buffer"] = tf_buffer

    # Stage 3: Synchronize to target FPS
    synchronizer = ZeroOrderHoldTimeSynchronizerConfig(fps=FPS).build()
    topics = synchronizer.synchronize(topics)

    # Stage 4: Compute TF chains
    tf_chain_enricher = TfChainEnricherConfig(frame_pairs=HSR2_FRAME_PAIRS).build()
    tf_chain: pa.Table = tf_chain_enricher.enrich(topics["/tf_buffer"])
    topics["/tf_chain"] = tf_chain

    # Stage 5: Transform to LeRobot format (includes video encoding)
    lerobot_transformer = LeRobotV21TransformerConfig(
        outdir=output_dir,
        robot_model=robot_model_path,
    ).build()
    lerobot_transformer.transform(topics, metadata, fps=FPS, image_data=image_data)


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Convert ROS 2 bag files to LeRobot format"
    )
    parser.add_argument(
        "--rosbag-path", required=True, help="Path to ROS 2 bag file (.mcap)"
    )
    parser.add_argument(
        "--output-dir",
        default="./lerobot_output",
        help="Output directory for LeRobot format",
    )
    parser.add_argument(
        "--robot-model",
        default="../config/robot_model/hsr2.yaml",
        help="Path to robot model YAML file",
    )
    args = parser.parse_args()

    rosbag_path = Path(args.rosbag_path)
    if not rosbag_path.exists():
        raise FileNotFoundError(f"Rosbag file not found: {rosbag_path}")

    robot_model_path = Path(args.robot_model)
    if not robot_model_path.exists():
        raise FileNotFoundError(f"Robot model file not found: {robot_model_path}")

    run_pipeline(str(rosbag_path), args.output_dir, str(robot_model_path))


if __name__ == "__main__":
    main()
