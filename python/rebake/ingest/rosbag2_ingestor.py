"""Ingestor for ROS 2 bag files."""

from __future__ import annotations

import json
from typing import Any

import pyarrow as pa
from pydantic import BaseModel, ConfigDict

from .. import _internal
from ..common import PyImageFrame
from ..core.context import Context
from ..exceptions import IngestError


def read_metadata(rosbag_path: str) -> dict[str, Any]:
    """Read only the metadata from a rosbag without full ingestion.

    This function reads the `meta.json` file from the parent directory of the
    rosbag path. It is useful for extracting the UUID before deciding whether
    to perform expensive full ingestion.

    Args:
        rosbag_path: Path to the .mcap file. The meta.json is expected
            to be in the parent directory.

    Returns:
        Canonical V2.0 metadata dictionary. V1.3 input is converted to V2.0
        before being returned.

    Raises:
        IngestError: If the metadata cannot be read or parsed.

    Example:
        >>> metadata = read_metadata("/data/recording.mcap")
        >>> print(metadata["uuid"])
        'd006f9f5-1234-5678-abcd-ef0123456789'

        >>> # Check if already processed before full ingestion
        >>> if metadata["uuid"] in existing_uuids:
        ...     print("Already processed, skipping")
        ... else:
        ...     topics, metadata, image_data, type_map = ingestor.ingest(rosbag_path)
    """
    try:
        json_str = _internal.ingest.py_read_metadata(rosbag_path)
        return json.loads(json_str)
    except Exception as e:
        raise IngestError(str(e)) from e


class Rosbag2IngestorConfig(BaseModel):
    """Configuration for the ROS 2 bag file ingestor.

    This config creates an ingestor that reads ROS 2 bag files (.mcap)
    and converts them to structured data in the Context.

    Args:
        require_metadata: Whether to require meta.json (airoa metadata).
            Defaults to True. Set to False for testing or non-airoa rosbags.

    Examples:
        ```python
        config = Rosbag2IngestorConfig()
        ingestor = config.build()
        context = ingestor.ingest("/path/to/data.mcap")

        # For testing without metadata:
        config = Rosbag2IngestorConfig(require_metadata=False)
        ```
    """

    model_config = ConfigDict(arbitrary_types_allowed=True)

    require_metadata: bool = True

    def build(self) -> Rosbag2Ingestor:
        """Create a Rosbag2Ingestor from this config.

        Returns:
            A new Rosbag2Ingestor instance.
        """
        return Rosbag2Ingestor(self)

    def _to_inner(self) -> _internal.ingest.PyRosbag2IngestorConfig:
        """Convert to internal Rust config object."""
        return _internal.ingest.PyRosbag2IngestorConfig(require_metadata=self.require_metadata)


class Rosbag2Ingestor:
    """Reads ROS 2 bag files and converts them to structured data.

    This ingestor parses ROS 2 bag files (.mcap format) and extracts
    all messages into PyArrow RecordBatches. Each ROS topic becomes
    a separate entry in the Context dataset.

    The ingestor handles:

    - Standard ROS 2 message types
    - Images (sensor_msgs/Image and sensor_msgs/CompressedImage)
    - TF transforms

    Examples:
        ```python
        config = Rosbag2IngestorConfig()
        ingestor = config.build()
        context = ingestor.ingest("/path/to/recording.mcap")
        # Check what topics were loaded
        print(context.dataset_topics())
        # ['/camera/image', '/joint_states', '/tf']
        ```
    """

    def __init__(self, config: Rosbag2IngestorConfig):
        """Create a new Rosbag2Ingestor.

        Args:
            config: The configuration for this ingestor.
        """
        self.config = config
        self._inner = _internal.ingest.PyRosbag2Ingestor(config._to_inner())

    def run(self, context: Context) -> Context:
        """Run the ingestor on the given context.

        The context must have `rosbag_path` set before calling this method.
        After running, the context will contain all data from the bag file.

        Args:
            context: The context to process. Must have rosbag_path set.

        Returns:
            The same context, now containing the ingested data.

        Raises:
            ValueError: If context.rosbag_path is not set.
            IngestError: If the bag file cannot be read.

        Examples:
            ```python
            context = Context()
            context.set_rosbag_path("/path/to/data.mcap")
            context = ingestor.run(context)
            ```
        """
        if context.rosbag_path is None:
            raise ValueError("Context.rosbag_path must be set before running Rosbag2Ingestor")

        try:
            self._inner.run(context.inner)
        except Exception as e:
            raise IngestError(str(e)) from e
        return context

    def ingest(
        self, rosbag_path: str
    ) -> tuple[
        dict[str, pa.Table],
        dict[str, Any],
        dict[str, list[PyImageFrame]],
        dict[str, str],
    ]:
        """Load a ROS 2 bag file and return Arrow Tables directly.

        This is a Context-free API for ingesting rosbag files.
        It returns the data as Arrow Tables along with metadata, image data,
        and the topic-to-message-type mapping.

        Args:
            rosbag_path: Path to the ROS 2 bag file (.mcap).

        Returns:
            A tuple of (topics, metadata, image_data, topic_message_type_map) where:

            - topics: Dictionary mapping topic names to Arrow Tables
            - metadata: Airoa metadata as a Python dictionary
            - image_data: Dictionary mapping topic names to lists of PyImageFrame
            - topic_message_type_map: Dictionary mapping topic names to ROS message
              types (e.g., "/joint_states" -> "sensor_msgs/msg/JointState")

        Examples:
            ```python
            ingestor = Rosbag2IngestorConfig().build()
            topics, metadata, image_data, type_map = ingestor.ingest(
                "/data/recording.mcap"
            )

            # topics is dict[str, pa.Table]
            print(topics.keys())

            # metadata is dict with uuid, run, context, etc.
            print(metadata["uuid"])

            # image_data is dict[str, list[PyImageFrame]]
            print(len(image_data["/camera/image"]))

            # type_map shows the ROS message type for each topic
            for topic, msg_type in type_map.items():
                print(f"{topic}: {msg_type}")
            # /joint_states: sensor_msgs/msg/JointState
            # /tf: tf2_msgs/msg/TFMessage
            ```
        """
        context = Context()
        context.set_rosbag_path(rosbag_path)
        context = self.run(context)

        # Convert RecordBatches to Tables
        topics = {
            topic: pa.Table.from_batches([batch])
            for topic, batch in context.to_record_batches().items()
        }

        # Get metadata as dict
        metadata = context.get_airoa_metadata() or {}

        # Get image data
        image_data = context.get_image_data() or {}

        # Get topic to message type mapping
        topic_message_type_map = context.get_topic_message_type_map() or {}

        return topics, metadata, image_data, topic_message_type_map
