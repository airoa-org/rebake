from __future__ import annotations

import json
from typing import TYPE_CHECKING, Any

import pyarrow as pa

from .. import _internal

if TYPE_CHECKING:
    from ..encode import VideoArtifact


class Context:
    """A container that holds all data as it moves through the pipeline.

    The Context is the main data structure in rebake. It stores:

    - Dataset: Structured data as PyArrow RecordBatches, organized by topic name
    - Image data: Raw image frames from camera topics
    - Metadata: File paths and settings like output directory and frame rate

    Each pipeline stage reads from the Context, processes the data,
    and writes results back to the Context.

    Examples:
        ```python
        context = Context()
        context.set_rosbag_path("/path/to/data.bag")
        # Pass context through pipeline stages
        context = ingestor.run(context)
        context = synchronizer.run(context)
        ```
    """

    def __init__(self, _inner: Any = None):
        """Create a new Context.

        Args:
            _inner: Internal Rust context object. Usually you should not
                set this directly. Use the default constructor or
                factory methods like `from_tables()` instead.
        """
        if _inner is None:
            self._inner = _internal.core.PyContext()
        else:
            self._inner = _inner

    @property
    def inner(self) -> _internal.core.PyContext:
        """Get the internal Rust context object.

        Returns:
            The underlying PyContext object from Rust.

        Note:
            This is mainly for internal use. Most users do not need
            to access this directly.
        """
        return self._inner

    @classmethod
    def from_tables(cls, tables: dict[str, pa.Table]) -> Context:
        """Create a Context from a dictionary of PyArrow Tables.

        This method converts PyArrow Tables to the internal format
        used by rebake. Each table is converted to a single RecordBatch.

        Args:
            tables: A dictionary where keys are topic names (like "/camera/image")
                and values are PyArrow Tables containing the data.

        Returns:
            A new Context containing the provided data.

        Examples:
            ```python
            import pyarrow as pa
            table = pa.table({"timestamp": [1, 2, 3], "value": [10, 20, 30]})
            context = Context.from_tables({"/sensor/data": table})
            ```
        """
        batches = {}
        for k, v in tables.items():
            batches[k] = v.combine_chunks().to_batches()[0]

        context = _internal.core.PyContext.from_record_batches(batches)
        return cls(context)

    @property
    def rosbag_path(self) -> str | None:
        """Get the path to the ROS bag file.

        Returns:
            The file path as a string, or None if not set.
        """
        return self._inner.rosbag_path

    def set_rosbag_path(self, value: str) -> None:
        """Set the path to the ROS bag file.

        This must be set before running an Ingestor stage.

        Args:
            value: The file path to the ROS bag file.

        Examples:
            ```python
            context = Context()
            context.set_rosbag_path("/data/recording.bag")
            ```
        """
        self._inner.rosbag_path = value

    @property
    def bundle_root(self) -> str | None:
        """Get the root directory used to resolve relative video artifact paths."""
        return self._inner.bundle_root

    def set_bundle_root(self, value: str | None) -> None:
        """Set the root directory used to resolve relative video artifact paths."""
        self._inner.bundle_root = value

    @property
    def output_dir(self) -> str | None:
        """Get the output directory path.

        Returns:
            The directory path as a string, or None if not set.
        """
        return self._inner.output_dir

    def set_output_dir(self, value: str) -> None:
        """Set the output directory path.

        This is used by stages that produce final output, such as
        LeRobotV21Transformer which writes Parquet and video files.

        Args:
            value: The directory path for output files.

        Examples:
            ```python
            context.set_output_dir("./lerobot_output")
            ```
        """
        self._inner.output_dir = value

    @property
    def video_cache_dir(self) -> str | None:
        """Get the video cache directory path.

        Returns:
            The directory path as a string, or None if not set.
        """
        return self._inner.video_cache_dir

    def set_video_cache_dir(self, value: str) -> None:
        """Set the video cache directory path.

        This directory is used by VideoEncoder to store intermediate
        video files. These files are separate from the final LeRobot
        output and can be used for Iceberg storage.

        Args:
            value: The directory path for video cache files.

        Examples:
            ```python
            context.set_video_cache_dir("./video_cache")
            ```
        """
        self._inner.video_cache_dir = value

    def dataset_topics(self) -> list[str]:
        """Get the list of topic names in the dataset.

        Returns:
            A list of topic names (like ["/camera/image", "/joint_states"]).

        Examples:
            ```python
            topics = context.dataset_topics()
            print(topics)
            # ['/camera/image', '/joint_states', '/tf']
            ```
        """
        return self._inner.dataset_topics()

    def get_record_batch(self, topic: str) -> pa.RecordBatch:
        """Get data for a specific topic as a PyArrow RecordBatch.

        Args:
            topic: The topic name to get data for.

        Returns:
            A PyArrow RecordBatch containing the topic data.

        Raises:
            RuntimeError: If the dataset is empty or the topic is not found.

        Examples:
            ```python
            batch = context.get_record_batch("/joint_states")
            print(batch.schema)
            ```
        """
        return self._inner.get_record_batch(topic)

    def set_record_batch(self, topic: str, batch: pa.RecordBatch) -> None:
        """Set data for a specific topic from a PyArrow RecordBatch.

        Args:
            topic: The topic name to set data for.
            batch: A PyArrow RecordBatch containing the data.

        Examples:
            ```python
            import pyarrow as pa
            batch = pa.record_batch({"value": [1, 2, 3]})
            context.set_record_batch("/my_topic", batch)
            ```
        """
        self._inner.set_record_batch(topic, batch)

    def to_record_batches(self) -> dict[str, pa.RecordBatch]:
        """Get all data as a dictionary of PyArrow RecordBatches.

        Returns:
            A dictionary where keys are topic names and values are
            PyArrow RecordBatches.

        Examples:
            ```python
            batches = context.to_record_batches()
            for topic, batch in batches.items():
                print(f"{topic}: {len(batch)} rows")
            ```
        """
        return self._inner.to_record_batches()

    def get_image_data(
        self,
    ) -> dict[str, list[_internal.common.PyImageFrame]] | None:
        """Get the image data stored in the context.

        Image data is stored separately from the main dataset.
        Each topic maps to a list of image frames.

        Returns:
            A dictionary where keys are topic names and values are
            lists of image frames. Returns None if no image data exists.

        Examples:
            ```python
            image_data = context.get_image_data()
            if image_data:
                for topic, frames in image_data.items():
                    print(f"{topic}: {len(frames)} frames")
            ```
        """
        return self._inner.get_image_data()

    def set_image_data(
        self, data: dict[str, list[_internal.common.PyImageFrame]] | None
    ) -> None:
        """Set the image data in the context.

        Args:
            data: A dictionary where keys are topic names and values
                are lists of image frames. Pass None to clear image data.

        Examples:
            ```python
            context.set_image_data({"/camera/image": frames})
            ```
        """
        self._inner.set_image_data(data)

    def get_depth_data(
        self,
    ) -> dict[str, list[_internal.common.PyDepthFrame]] | None:
        """Get the depth data stored in the context.

        Depth data is stored separately from the main dataset.
        Each topic maps to a list of depth frames.

        Returns:
            A dictionary where keys are topic names and values are
            lists of depth frames. Returns None if no depth data exists.

        Examples:
            ```python
            depth_data = context.get_depth_data()
            if depth_data:
                for topic, frames in depth_data.items():
                    print(f"{topic}: {len(frames)} frames")
            ```
        """
        return self._inner.get_depth_data()

    def set_depth_data(
        self, data: dict[str, list[_internal.common.PyDepthFrame]] | None
    ) -> None:
        """Set the depth data in the context.

        Args:
            data: A dictionary where keys are topic names and values
                are lists of depth frames. Pass None to clear depth data.

        Examples:
            ```python
            context.set_depth_data({"/camera/depth": frames})
            ```
        """
        self._inner.set_depth_data(data)

    def set_video_registry(
        self,
        video_registry: dict[str, VideoArtifact],
        *,
        bundle_root: str | None = None,
    ) -> None:
        """Set the canonical video registry used for lazy frame loading.

        Args:
            video_registry: Mapping from topic names to typed ``VideoArtifact``
                values. Each artifact carries both the path and canonical
                encoding metadata.
            bundle_root: Optional root directory used to resolve relative
                artifact paths.
        """
        if bundle_root is not None:
            self._inner.bundle_root = bundle_root

        payload = {
            topic: artifact.model_dump(mode="json")
            for topic, artifact in video_registry.items()
        }
        self._inner.set_video_registry_json(json.dumps(payload))

    @property
    def fps(self) -> int | None:
        """Get the frame rate (frames per second).

        This is set by time synchronization stages.

        Returns:
            The frame rate as an integer, or None if not set.
        """
        return self._inner.fps

    def set_fps(self, value: int | None) -> None:
        """Set the frame rate (frames per second).

        Args:
            value: The frame rate as an integer, or None to clear.
        """
        self._inner.fps = value

    def get_airoa_metadata(self) -> dict[str, Any] | None:
        """Get the airoa metadata (from meta.json).

        The metadata contains information about the rosbag recording,
        including UUID plus either legacy V1.3 fields or canonical V2.0
        fields, depending on what is currently stored in the Context.

        Returns:
            A dictionary containing the airoa metadata, or None if not set.

        Examples:
            ```python
            metadata = context.get_airoa_metadata()
            if metadata:
                print(f"UUID: {metadata['uuid']}")
                print(f"Version: {metadata['version']}")
            ```
        """
        json_str = self._inner.get_airoa_metadata_json()
        if json_str is None:
            return None
        return json.loads(json_str)

    def set_airoa_metadata(
        self, metadata: dict[str, Any] | _internal.core.MetadataV2_0
    ) -> None:
        """Set the airoa metadata.

        Accepts either a typed ``MetadataV2_0`` instance (recommended) or a
        dictionary (back-compat path; also supports the legacy V1.3 schema).
        Schema constraints are validated at this boundary — invalid metadata
        is rejected with ``ValueError`` before any downstream stage sees it.

        Args:
            metadata: Either a ``MetadataV2_0`` instance or a dict containing
                the metadata. V1.3 dicts are also accepted for back-compat.

        Examples:
            ```python
            from rebake.core import MetadataV2_0, Episode, File

            m = MetadataV2_0(
                episode=Episode(label="pick and place"),
                files=[File(name="bag.mcap")],
            )
            context.set_airoa_metadata(m)
            ```
        """
        if isinstance(metadata, _internal.core.MetadataV2_0):
            self._inner.set_airoa_metadata(metadata)
        else:
            json_str = json.dumps(metadata, ensure_ascii=False)
            self._inner.set_airoa_metadata_json(json_str)

    def get_metadata_record_batch(self) -> pa.RecordBatch:
        """Get the airoa metadata as an Arrow RecordBatch.

        This method returns the metadata in a format that preserves
        the full nested structure, making it suitable for writing
        to Iceberg tables or Parquet files.

        The returned RecordBatch contains a single row with columns:
        - uuid: string
        - version: string
        - files: list<struct<type, name>>
        - context: struct<entities: list<...>, components: list<...>>
        - run: struct<total_time_s, instructions, segments, episode_label>
        - schema: string

        Returns:
            A PyArrow RecordBatch with a single row containing the metadata.

        Raises:
            RuntimeError: If no metadata is available in the context.

        Examples:
            ```python
            batch = context.get_metadata_record_batch()
            print(batch.schema)  # Shows nested structure
            table = pa.Table.from_batches([batch])
            # Write to Iceberg or Parquet
            ```
        """
        return self._inner.get_metadata_record_batch()

    @property
    def video_paths(self) -> dict[str, str] | None:
        """Get resolved video paths derived from the context's video registry.

        This is a compatibility view over rebake's canonical ``video_registry``.
        Paths are resolved against the current bundle root when available.

        Returns:
            A dictionary where keys are topic names (e.g., "/camera/image")
            and values are video file paths, or None if not set.

        Examples:
            ```python
            paths = context.video_paths
            if paths:
                for topic, path in paths.items():
                    print(f"{topic}: {path}")
            ```
        """
        return self._inner.get_video_paths()

    def get_topic_message_type_map(self) -> dict[str, str] | None:
        """Get the mapping from topic names to ROS message types.

        This mapping is populated by the Ingestor stages when reading
        ROS bag files. It maps each topic name to its corresponding
        ROS message type (e.g., "/joint_states" -> "sensor_msgs/msg/JointState").

        Returns:
            A dictionary mapping topic names to message types,
            or None if no mapping is available.

        Examples:
            ```python
            type_map = context.get_topic_message_type_map()
            if type_map:
                for topic, msg_type in type_map.items():
                    print(f"{topic}: {msg_type}")
                # Output:
                # /joint_states: sensor_msgs/msg/JointState
                # /tf: tf2_msgs/msg/TFMessage
            ```
        """
        return self._inner.get_topic_message_type_map()

    def set_topic_message_type_map(self, map: dict[str, str] | None) -> None:
        """Set the mapping from topic names to ROS message types.

        This is typically used when reconstructing a Context from
        external sources like Iceberg tables.

        Args:
            map: A dictionary mapping topic names to message types,
                or None to clear the mapping.

        Examples:
            ```python
            context.set_topic_message_type_map({
                "/joint_states": "sensor_msgs/msg/JointState",
                "/tf": "tf2_msgs/msg/TFMessage",
            })
            ```
        """
        self._inner.set_topic_message_type_map(map)

    def save_to_parquet(self, output_dir: str) -> None:
        """Save the context dataset to Parquet files.

        This mirrors Orchestrator's save_context(), saving each topic's
        data as a separate Parquet file.

        Args:
            output_dir: Directory to save Parquet files to.

        Output structure:
            {output_dir}/{topic}.parquet

        Examples:
            ```python
            context.save_to_parquet("./parquet_output")
            # Creates: ./parquet_output/joint_states.parquet
            #          ./parquet_output/camera/image.parquet
            ```

        Raises:
            RuntimeError: If I/O fails or Parquet writing fails.
        """
        self._inner.save_to_parquet(output_dir)
