"""Ingestor for ROS 1 bag files."""

from __future__ import annotations

from typing import Any

import pyarrow as pa
from pydantic import BaseModel, ConfigDict

from .. import _internal
from ..common import PyImageFrame
from ..core.context import Context
from ..exceptions import IngestError


class Rosbag1IngestorConfig(BaseModel):
    """Configuration for the ROS 1 bag file ingestor.

    This config creates an ingestor that reads ROS 1 bag files (.bag)
    and converts them to structured data in the Context.

    Args:
        require_metadata: Whether to require meta.json (airoa metadata).
            Defaults to True. Set to False for testing or non-airoa rosbags.

    Examples:
        ```python
        config = Rosbag1IngestorConfig()
        ingestor = config.build()
        topics, metadata, image_data, type_map = ingestor.ingest("/path/to/data.bag")

        # For testing without metadata:
        config = Rosbag1IngestorConfig(require_metadata=False)
        ```
    """

    model_config = ConfigDict(arbitrary_types_allowed=True)

    require_metadata: bool = True

    def build(self) -> Rosbag1Ingestor:
        """Create a Rosbag1Ingestor from this config.

        Returns:
            A new Rosbag1Ingestor instance.
        """
        return Rosbag1Ingestor(self)

    def _to_inner(self) -> _internal.ingest.PyRosbag1IngestorConfig:
        """Convert to internal Rust config object."""
        return _internal.ingest.PyRosbag1IngestorConfig(
            require_metadata=self.require_metadata
        )


class Rosbag1Ingestor:
    """Reads ROS 1 bag files and converts them to structured data.

    This ingestor parses ROS 1 bag files (.bag format) and extracts
    all messages into PyArrow RecordBatches. Each ROS topic becomes
    a separate entry in the Context dataset.

    The ingestor handles:

    - Standard ROS message types
    - Compressed images (stored separately as image data)
    - TF transforms

    Examples:
        ```python
        config = Rosbag1IngestorConfig()
        ingestor = config.build()
        topics, metadata, image_data, type_map = ingestor.ingest(
            "/path/to/recording.bag"
        )
        # Check what topics were loaded
        print(topics.keys())
        # dict_keys(['/camera/image', '/joint_states', '/tf'])
        ```
    """

    def __init__(self, config: Rosbag1IngestorConfig):
        """Create a new Rosbag1Ingestor.

        Args:
            config: The configuration for this ingestor.
        """
        self.config = config
        self._inner = _internal.ingest.PyRosbag1Ingestor(config._to_inner())

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
            context.set_rosbag_path("/path/to/data.bag")
            context = ingestor.run(context)
            ```
        """
        if context.rosbag_path is None:
            raise ValueError(
                "Context.rosbag_path must be set before running Rosbag1Ingestor"
            )
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
        """Load a ROS 1 bag file and return Arrow Tables directly.

        This is a Context-free API for ingesting rosbag files.
        It returns the data as Arrow Tables along with metadata, image data,
        and the topic-to-message-type mapping.

        Args:
            rosbag_path: Path to the ROS 1 bag file (.bag).

        Returns:
            A tuple of (topics, metadata, image_data, topic_message_type_map) where:

            - topics: Dictionary mapping topic names to Arrow Tables
            - metadata: Airoa metadata as a Python dictionary
            - image_data: Dictionary mapping topic names to lists of PyImageFrame
            - topic_message_type_map: Dictionary mapping topic names to ROS message
              types (e.g., "/joint_states" -> "sensor_msgs/JointState")

        Examples:
            ```python
            ingestor = Rosbag1IngestorConfig().build()
            topics, metadata, image_data, type_map = ingestor.ingest(
                "/data/recording.bag"
            )
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

        metadata = context.get_airoa_metadata() or {}
        image_data = context.get_image_data() or {}
        topic_message_type_map = context.get_topic_message_type_map() or {}

        return topics, metadata, image_data, topic_message_type_map
