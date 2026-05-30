"""Integration tests for the ingest pipeline.

These tests verify the complete flow from ROS bag reading
through synchronization using the Context-based API (run()).
"""

from __future__ import annotations

from pathlib import Path

import pytest

from rebake.core import Context
from rebake.ingest import Rosbag2IngestorConfig
from rebake.synchronize import ZeroOrderHoldTimeSynchronizerConfig


class TestIngestPipeline:
    """Tests for the complete ingest pipeline using Context-based API."""

    def test_ingest_then_sync(self, test_mcap_path: Path, test_ingestor) -> None:
        """Verify ingest followed by synchronization works."""
        # Ingest using Context-based API
        context = Context()
        context.set_rosbag_path(str(test_mcap_path))
        context = test_ingestor.run(context)

        # Sync
        sync = ZeroOrderHoldTimeSynchronizerConfig(fps=30).build()
        context = sync.run(context)

        # Verify
        assert context.fps == 30
        topics = context.dataset_topics()
        assert len(topics) > 0

    def test_pipeline_preserves_all_data_topics(
        self, test_mcap_path: Path, test_ingestor
    ) -> None:
        """Verify pipeline preserves all data topics through stages."""
        # Ingest using Context-based API
        context = Context()
        context.set_rosbag_path(str(test_mcap_path))
        context = test_ingestor.run(context)
        original_topics = set(context.dataset_topics())

        # Sync
        sync = ZeroOrderHoldTimeSynchronizerConfig(fps=30).build()
        context = sync.run(context)

        # Verify all topics still present
        final_topics = set(context.dataset_topics())
        assert original_topics == final_topics

    def test_pipeline_preserves_image_data(
        self, test_mcap_path: Path, test_ingestor
    ) -> None:
        """Verify pipeline preserves image data through stages."""
        # Ingest using Context-based API
        context = Context()
        context.set_rosbag_path(str(test_mcap_path))
        context = test_ingestor.run(context)
        original_image_data = context.get_image_data()
        assert original_image_data is not None, "Expected image data after ingest"

        original_image_topics = set(original_image_data.keys())

        # Sync
        sync = ZeroOrderHoldTimeSynchronizerConfig(fps=30).build()
        context = sync.run(context)

        # Verify image data still present
        final_image_data = context.get_image_data()
        assert final_image_data is not None, "Image data lost after sync"

        final_image_topics = set(final_image_data.keys())
        assert original_image_topics == final_image_topics

    def test_synchronized_data_columns_preserved(
        self, test_mcap_path: Path, test_ingestor
    ) -> None:
        """Verify essential data columns are preserved through synchronization."""
        # Ingest using Context-based API
        context = Context()
        context.set_rosbag_path(str(test_mcap_path))
        context = test_ingestor.run(context)

        # Get original column names for joint_states
        original_batch = context.get_record_batch("/joint_states")
        original_columns = set(original_batch.schema.names)

        # Sync
        sync = ZeroOrderHoldTimeSynchronizerConfig(fps=30).build()
        context = sync.run(context)

        # Verify essential columns preserved (sync may add/modify some columns)
        synced_batch = context.get_record_batch("/joint_states")
        synced_columns = set(synced_batch.schema.names)

        # Essential columns that must be preserved
        essential_columns = {"name", "position", "velocity", "effort"}
        preserved_essential = essential_columns & original_columns

        for col in preserved_essential:
            assert col in synced_columns, f"Essential column '{col}' was lost"


class TestIngestorErrorHandling:
    """Tests for ingestor error handling."""

    def test_ingest_nonexistent_file_raises(self, tmp_path: Path) -> None:
        """Verify ingesting non-existent file raises error."""
        ingestor = Rosbag2IngestorConfig().build()
        context = Context()
        context.set_rosbag_path(str(tmp_path / "nonexistent.mcap"))

        # Rust panics are raised as pyo3_runtime.PanicException
        # which is a subclass of BaseException, not Exception
        with pytest.raises(BaseException):
            ingestor.run(context)

    def test_ingest_without_rosbag_path_raises(self) -> None:
        """Verify ingesting without rosbag_path raises error."""
        ingestor = Rosbag2IngestorConfig().build()
        context = Context()

        with pytest.raises(ValueError, match="rosbag_path must be set"):
            ingestor.run(context)
