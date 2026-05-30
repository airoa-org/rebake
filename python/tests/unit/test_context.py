"""Unit tests for the Context class.

These tests verify Context functionality without requiring
ROS bag files, using in-memory PyArrow data instead.
"""

from __future__ import annotations

import pyarrow as pa
import pytest
from rebake.core import Context
from rebake.encode import VideoArtifact, VideoMetadata


class TestContextCreation:
    """Tests for Context creation and initialization."""

    def test_create_empty_context(self) -> None:
        """Verify that an empty Context can be created."""
        context = Context()

        assert context.rosbag_path is None
        assert context.output_dir is None
        assert context.fps is None

    def test_from_tables_single_topic(self) -> None:
        """Verify Context can be created from a single PyArrow table."""
        table = pa.table(
            {
                "timestamp_ns": [1000000000, 1100000000, 1200000000],
                "value": [1.0, 2.0, 3.0],
            }
        )

        context = Context.from_tables({"/sensor/data": table})

        topics = context.dataset_topics()
        assert "/sensor/data" in topics

    def test_from_tables_multiple_topics(self) -> None:
        """Verify Context can be created from multiple PyArrow tables."""
        table_a = pa.table(
            {
                "timestamp_ns": [1000000000, 1100000000],
                "value_a": [10, 20],
            }
        )
        table_b = pa.table(
            {
                "timestamp_ns": [1000000000, 1100000000],
                "value_b": [100.0, 200.0],
            }
        )

        context = Context.from_tables(
            {
                "/topic_a": table_a,
                "/topic_b": table_b,
            }
        )

        topics = set(context.dataset_topics())
        assert topics == {"/topic_a", "/topic_b"}


class TestContextProperties:
    """Tests for Context property access."""

    def test_set_and_get_rosbag_path(self) -> None:
        """Verify rosbag_path can be set and retrieved."""
        context = Context()

        context.set_rosbag_path("/path/to/test.mcap")

        assert context.rosbag_path == "/path/to/test.mcap"

    def test_set_and_get_output_dir(self) -> None:
        """Verify output_dir can be set and retrieved."""
        context = Context()

        context.set_output_dir("/path/to/output")

        assert context.output_dir == "/path/to/output"

    def test_set_and_get_fps(self) -> None:
        """Verify fps can be set and retrieved."""
        context = Context()

        context.set_fps(30)

        assert context.fps == 30

    def test_fps_can_be_none(self) -> None:
        """Verify fps can be set to None."""
        context = Context()
        context.set_fps(30)

        context.set_fps(None)

        assert context.fps is None

    def test_set_and_get_bundle_root(self) -> None:
        """Verify bundle_root can be set and retrieved."""
        context = Context()

        context.set_bundle_root("/tmp/bundle")

        assert context.bundle_root == "/tmp/bundle"

    def test_set_video_registry_resolves_relative_video_paths(self) -> None:
        """Canonical video registry should drive the compatibility video_paths view."""
        context = Context()
        context.set_video_registry(
            {
                "/camera/image": VideoArtifact(
                    video_path="videos/camera.mp4",
                    metadata=VideoMetadata(
                        media_type="rgb",
                        codec_family="av1",
                        encoder_name="libsvtav1",
                        pix_fmt="yuv420p",
                        width=640,
                        height=480,
                        fps=30,
                        encoding_config_json="{}",
                    ),
                )
            },
            bundle_root="/tmp/bundle",
        )

        assert context.video_paths == {"/camera/image": "/tmp/bundle/videos/camera.mp4"}


class TestContextDataAccess:
    """Tests for Context data access methods."""

    def test_get_record_batch(self) -> None:
        """Verify data can be retrieved as RecordBatch."""
        table = pa.table(
            {
                "timestamp_ns": [1000000000, 1100000000],
                "value": [1.0, 2.0],
            }
        )
        context = Context.from_tables({"/test": table})

        batch = context.get_record_batch("/test")

        assert batch.num_rows == 2
        assert "timestamp_ns" in batch.schema.names
        assert "value" in batch.schema.names

    def test_set_record_batch(self) -> None:
        """Verify data can be set from a RecordBatch."""
        context = Context()
        batch = pa.record_batch(
            {
                "timestamp_ns": [1000000000],
                "value": [42.0],
            }
        )

        context.set_record_batch("/new_topic", batch)

        assert "/new_topic" in context.dataset_topics()
        retrieved = context.get_record_batch("/new_topic")
        assert retrieved.num_rows == 1

    def test_to_record_batches(self) -> None:
        """Verify all data can be exported as RecordBatches."""
        table_a = pa.table({"a": [1, 2]})
        table_b = pa.table({"b": [3, 4]})
        context = Context.from_tables({"/a": table_a, "/b": table_b})

        batches = context.to_record_batches()

        assert len(batches) == 2
        assert "/a" in batches
        assert "/b" in batches

    def test_get_nonexistent_topic_raises(self) -> None:
        """Verify accessing non-existent topic raises an error."""
        context = Context()

        with pytest.raises(RuntimeError):
            context.get_record_batch("/nonexistent")


class TestContextImageData:
    """Tests for Context image data handling."""

    def test_image_data_initially_none(self) -> None:
        """Verify image_data is None for new Context."""
        context = Context()

        assert context.get_image_data() is None

    def test_set_and_get_image_data(self) -> None:
        """Verify image data can be set and retrieved."""
        from rebake import _internal

        context = Context()
        PyImageFrame = _internal.common.PyImageFrame
        PyImageShape = _internal.common.PyImageShape

        shape = PyImageShape(64, 64, 3)
        frame = PyImageFrame(0, "png", list(b"test_data"), shape)

        context.set_image_data({"/camera/image": [frame]})

        image_data = context.get_image_data()
        assert image_data is not None
        assert "/camera/image" in image_data
        assert len(image_data["/camera/image"]) == 1

    def test_clear_image_data(self) -> None:
        """Verify image data can be cleared by setting to None."""
        from rebake import _internal

        context = Context()
        PyImageFrame = _internal.common.PyImageFrame

        frame = PyImageFrame(0, "png", list(b"data"), None)
        context.set_image_data({"/camera": [frame]})

        context.set_image_data(None)

        assert context.get_image_data() is None

    def test_set_and_get_depth_data(self) -> None:
        """Verify depth data can be set and retrieved."""
        from rebake import _internal

        context = Context()
        PyDepthFrame = _internal.common.PyDepthFrame

        frame = PyDepthFrame(0, "png", list(b"depth_data"), "16UC1; png compressed")

        context.set_depth_data({"/camera/depth": [frame]})

        depth_data = context.get_depth_data()
        assert depth_data is not None
        assert "/camera/depth" in depth_data
        assert len(depth_data["/camera/depth"]) == 1
        assert depth_data["/camera/depth"][0].ros_format == "16UC1; png compressed"

    def test_clear_depth_data(self) -> None:
        """Verify depth data can be cleared by setting to None."""
        from rebake import _internal

        context = Context()
        PyDepthFrame = _internal.common.PyDepthFrame

        frame = PyDepthFrame(0, "png", list(b"data"), None)
        context.set_depth_data({"/camera/depth": [frame]})

        context.set_depth_data(None)

        assert context.get_depth_data() is None
