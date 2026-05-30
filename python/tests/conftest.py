"""Pytest configuration and shared fixtures for rebake tests."""

from __future__ import annotations

from pathlib import Path

import pytest

from tests.fixtures.mcap_generator import (
    McapGeneratorConfig,
    find_mcap_file,
    generate_test_mcap,
)


@pytest.fixture(scope="session")
def test_mcap_dir(tmp_path_factory: pytest.TempPathFactory) -> Path:
    """Generate a test MCAP file once per test session.

    This fixture creates a minimal MCAP file with:
    - /joint_states (sensor_msgs/msg/JointState)
    - /camera/image_raw (sensor_msgs/msg/Image)
    - /camera/camera_info (sensor_msgs/msg/CameraInfo)
    - /tf (tf2_msgs/msg/TFMessage)

    Returns:
        Path to the directory containing the MCAP file.
    """
    output_dir = tmp_path_factory.mktemp("test_bag")
    config = McapGeneratorConfig(
        num_frames=10,
        fps=30,
        image_width=64,
        image_height=64,
        joint_names=["joint1", "joint2", "joint3"],
        base_frame="base_link",
        child_frames=["hand_link", "camera_link"],
    )
    return generate_test_mcap(output_dir, config)


@pytest.fixture(scope="session")
def test_mcap_path(test_mcap_dir: Path) -> Path:
    """Get the path to the .mcap file within the test bag directory.

    Returns:
        Path to the .mcap file.
    """
    return find_mcap_file(test_mcap_dir)


@pytest.fixture(scope="session")
def test_ingestor():
    """Create an ingestor configured for testing (no metadata required).

    Returns:
        A Rosbag2Ingestor that doesn't require meta.json.
    """
    from rebake.ingest import Rosbag2IngestorConfig

    return Rosbag2IngestorConfig(require_metadata=False).build()
