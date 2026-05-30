"""MCAP file generator for testing rebake with rosbag2 format.

This module generates minimal MCAP files containing ROS2 messages
for testing the rebake pipeline without requiring actual robot data.

Uses the rosbags library for CDR serialization and MCAP writing.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from pathlib import Path
from typing import Optional

import numpy as np
from numpy.typing import NDArray
from rosbags.rosbag2 import Writer
from rosbags.rosbag2.writer import StoragePlugin
from rosbags.typesys import Stores, get_typestore


@dataclass
class McapGeneratorConfig:
    """Configuration for MCAP test file generation.

    Args:
        num_frames: Number of frames/messages to generate per topic.
        fps: Simulated frame rate (determines timestamp intervals).
        image_width: Width of generated images in pixels.
        image_height: Height of generated images in pixels.
        joint_names: List of joint names for JointState messages.
        base_frame: Base coordinate frame for TF messages.
        child_frames: Child frames for TF messages.
        include_camera_info: Whether to include CameraInfo messages.
            CameraInfo contains FixedSizeList arrays (k, r, p matrices).
        include_image: Whether to include Image messages.
    """

    num_frames: int = 10
    fps: int = 30
    image_width: int = 64
    image_height: int = 64
    joint_names: list[str] = field(
        default_factory=lambda: ["joint1", "joint2", "joint3"]
    )
    base_frame: str = "base_link"
    child_frames: list[str] = field(
        default_factory=lambda: ["hand_link", "camera_link"]
    )
    include_camera_info: bool = True
    include_image: bool = True


def generate_test_mcap(
    output_path: Path,
    config: Optional[McapGeneratorConfig] = None,
) -> Path:
    """Generate a minimal MCAP file for testing.

    Creates an MCAP file with the following topics:
    - /joint_states (sensor_msgs/msg/JointState)
    - /camera/image_raw (sensor_msgs/msg/Image) - if include_image=True
    - /camera/camera_info (sensor_msgs/msg/CameraInfo) - if include_camera_info=True
    - /tf (tf2_msgs/msg/TFMessage)

    Args:
        output_path: Directory where the MCAP file will be created.
            The actual file will be at output_path/metadata.yaml and
            output_path/<filename>.mcap (rosbag2 format).
        config: Configuration for data generation. Uses defaults if None.

    Returns:
        Path to the created MCAP directory.

    Example:
        >>> from pathlib import Path
        >>> mcap_path = generate_test_mcap(Path("/tmp/test_bag"))
        >>> # Use with rebake
        >>> context.set_rosbag_path(str(mcap_path / "test_0.mcap"))
    """
    if config is None:
        config = McapGeneratorConfig()

    # Get ROS2 type store (Humble)
    typestore = get_typestore(Stores.ROS2_HUMBLE)

    # Get message types
    JointState = typestore.types["sensor_msgs/msg/JointState"]
    Image = typestore.types["sensor_msgs/msg/Image"]
    CameraInfo = typestore.types["sensor_msgs/msg/CameraInfo"]
    TFMessage = typestore.types["tf2_msgs/msg/TFMessage"]
    TransformStamped = typestore.types["geometry_msgs/msg/TransformStamped"]
    Transform = typestore.types["geometry_msgs/msg/Transform"]
    Vector3 = typestore.types["geometry_msgs/msg/Vector3"]
    Quaternion = typestore.types["geometry_msgs/msg/Quaternion"]
    Header = typestore.types["std_msgs/msg/Header"]
    Time = typestore.types["builtin_interfaces/msg/Time"]

    # rosbags Writer expects a non-existing path for the bag directory
    output_path = Path(output_path)
    if output_path.exists():
        # Create a subdirectory for the actual bag
        output_path = output_path / "test_bag"
    output_path.parent.mkdir(parents=True, exist_ok=True)

    # Calculate time interval in nanoseconds
    interval_ns = 1_000_000_000 // config.fps
    start_time_ns = 1_000_000_000  # Start at 1 second

    with Writer(output_path, version=9, storage_plugin=StoragePlugin.MCAP) as writer:
        # Register connections (topics)
        conn_joint = writer.add_connection(
            "/joint_states",
            JointState.__msgtype__,
            typestore=typestore,
        )
        conn_image = None
        if config.include_image:
            conn_image = writer.add_connection(
                "/camera/image_raw",
                Image.__msgtype__,
                typestore=typestore,
            )
        conn_camera_info = None
        if config.include_camera_info:
            conn_camera_info = writer.add_connection(
                "/camera/camera_info",
                CameraInfo.__msgtype__,
                typestore=typestore,
            )
        conn_tf = writer.add_connection(
            "/tf",
            TFMessage.__msgtype__,
            typestore=typestore,
        )

        # Generate messages for each frame
        for i in range(config.num_frames):
            timestamp_ns = start_time_ns + i * interval_ns
            sec = timestamp_ns // 1_000_000_000
            nanosec = timestamp_ns % 1_000_000_000

            stamp = Time(sec=sec, nanosec=nanosec)
            header = Header(stamp=stamp, frame_id="base_link")

            # JointState message
            num_joints = len(config.joint_names)
            joint_msg = JointState(
                header=header,
                name=np.array(config.joint_names),
                position=np.array(
                    [0.1 * i * (j + 1) for j in range(num_joints)], dtype=np.float64
                ),
                velocity=np.zeros(num_joints, dtype=np.float64),
                effort=np.zeros(num_joints, dtype=np.float64),
            )
            writer.write(
                conn_joint,
                timestamp_ns,
                typestore.serialize_cdr(joint_msg, joint_msg.__msgtype__),
            )

            # Image message (simple RGB image)
            if conn_image is not None:
                image_data = _generate_test_image(
                    config.image_width, config.image_height, frame_index=i
                )
                image_msg = Image(
                    header=Header(stamp=stamp, frame_id="camera_link"),
                    height=config.image_height,
                    width=config.image_width,
                    encoding="rgb8",
                    is_bigendian=0,
                    step=config.image_width * 3,
                    data=image_data,
                )
                writer.write(
                    conn_image,
                    timestamp_ns,
                    typestore.serialize_cdr(image_msg, image_msg.__msgtype__),
                )

            # CameraInfo message
            if conn_camera_info is not None:
                RegionOfInterest = typestore.types["sensor_msgs/msg/RegionOfInterest"]
                camera_info_msg = CameraInfo(
                    header=Header(stamp=stamp, frame_id="camera_link"),
                    height=config.image_height,
                    width=config.image_width,
                    distortion_model="plumb_bob",
                    d=np.array([0.0, 0.0, 0.0, 0.0, 0.0], dtype=np.float64),
                    k=np.array(
                        [500.0, 0.0, 32.0, 0.0, 500.0, 32.0, 0.0, 0.0, 1.0],
                        dtype=np.float64,
                    ),
                    r=np.array(
                        [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0], dtype=np.float64
                    ),
                    p=np.array(
                        [
                            500.0,
                            0.0,
                            32.0,
                            0.0,
                            0.0,
                            500.0,
                            32.0,
                            0.0,
                            0.0,
                            0.0,
                            1.0,
                            0.0,
                        ],
                        dtype=np.float64,
                    ),
                    binning_x=0,
                    binning_y=0,
                    roi=RegionOfInterest(
                        x_offset=0, y_offset=0, height=0, width=0, do_rectify=False
                    ),
                )
                writer.write(
                    conn_camera_info,
                    timestamp_ns,
                    typestore.serialize_cdr(
                        camera_info_msg, camera_info_msg.__msgtype__
                    ),
                )

            # TF message
            transforms = []
            for j, child_frame in enumerate(config.child_frames):
                transform = TransformStamped(
                    header=Header(stamp=stamp, frame_id=config.base_frame),
                    child_frame_id=child_frame,
                    transform=Transform(
                        translation=Vector3(
                            x=0.1 * (j + 1),
                            y=0.0,
                            z=0.5 + 0.01 * i,
                        ),
                        rotation=Quaternion(x=0.0, y=0.0, z=0.0, w=1.0),
                    ),
                )
                transforms.append(transform)

            tf_msg = TFMessage(transforms=np.array(transforms))
            writer.write(
                conn_tf,
                timestamp_ns,
                typestore.serialize_cdr(tf_msg, tf_msg.__msgtype__),
            )

    return output_path


def _generate_test_image(
    width: int, height: int, frame_index: int
) -> NDArray[np.uint8]:
    """Generate a simple test image with varying colors.

    Creates an RGB image where the color varies based on frame index
    to make it easy to verify correct ordering.

    Args:
        width: Image width in pixels.
        height: Image height in pixels.
        frame_index: Frame number for color variation.

    Returns:
        Raw RGB data as numpy array (width * height * 3 bytes).
    """
    # Create a simple gradient that changes with frame index
    r = (frame_index * 25) % 256
    g = (frame_index * 10) % 256
    b = 128

    pixels = np.zeros((height, width, 3), dtype=np.uint8)
    for y in range(height):
        for x in range(width):
            # Add some spatial variation
            pixels[y, x, 0] = (r + x) % 256
            pixels[y, x, 1] = (g + y) % 256
            pixels[y, x, 2] = b

    return pixels.flatten()


def find_mcap_file(bag_path: Path) -> Path:
    """Find the .mcap file within a rosbag2 directory.

    Args:
        bag_path: Path to the rosbag2 directory.

    Returns:
        Path to the .mcap file.

    Raises:
        FileNotFoundError: If no .mcap file is found.
    """
    mcap_files = list(bag_path.glob("*.mcap"))
    if not mcap_files:
        raise FileNotFoundError(f"No .mcap file found in {bag_path}")
    return mcap_files[0]
