"""Convert ROS 2 bag files to LeRobot format.

This example demonstrates the rebake Context-based API, where a Context object
is passed through the pipeline stages.

Usage:
    uv run python examples/rosbag_to_lerobot_with_context.py \
        --rosbag-path /path/to/recording.mcap \
        --output ./lerobot_output \
        --robot-model ../config/robot_model/hsr2.yaml
"""

from __future__ import annotations

import argparse
from pathlib import Path

from rebake.core import Context
from rebake.encode import ScalingFlag, VideoEncoderConfig
from rebake.enrich import (
    FramePair,
    TfBufferEnricherConfig,
    TfChainEnricherConfig,
)
from rebake.ingest import Rosbag2IngestorConfig
from rebake.synchronize import ZeroOrderHoldTimeSynchronizerConfig
from rebake.transform import LeRobotV21TransformerConfig


FPS = 10

HSR2_FRAME_PAIRS = [
    FramePair(source="base_footprint", target="hand_left_left_finger_tip_frame"),
    FramePair(source="base_footprint", target="hand_left_right_finger_tip_frame"),
    FramePair(source="base_footprint", target="hand_right_left_finger_tip_frame"),
    FramePair(source="base_footprint", target="hand_right_right_finger_tip_frame"),
]


def run_pipeline(rosbag_path: str, output_dir: str, robot_model_path: str) -> None:
    """Run the complete data pipeline.

    Pipeline stages:
    1. Ingest rosbag
    2. Build TF buffer
    3. Compute TF chains
    4. Synchronize to target FPS
    5. Transform to LeRobot format (includes video encoding)
    """
    video_config = VideoEncoderConfig(
        fps=FPS,
        gop=2,
        crf="30",
        scaling=ScalingFlag.Bicubic,
        thread_count=8,
    )

    # Stage 1: Ingest rosbag
    context = Context()
    context.set_rosbag_path(rosbag_path)
    ingestor = Rosbag2IngestorConfig().build()
    context = ingestor.run(context)

    # Stage 2: Build TF buffer
    tf_buffer_enricher = TfBufferEnricherConfig().build()
    context = tf_buffer_enricher.run(context)

    # Stage 3: Compute TF chains
    tf_chain_enricher = TfChainEnricherConfig(frame_pairs=HSR2_FRAME_PAIRS).build()
    context = tf_chain_enricher.run(context)

    # Stage 4: Synchronize to target FPS
    synchronizer = ZeroOrderHoldTimeSynchronizerConfig(fps=FPS).build()
    context = synchronizer.run(context)

    # Stage 5: Transform to LeRobot format
    lerobot_transformer = LeRobotV21TransformerConfig(
        outdir=output_dir,
        robot_model=robot_model_path,
        video_config=video_config,
    ).build()
    context = lerobot_transformer.run(context)


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
