"""Unit tests for the synchronize() and enrich() methods.

These tests verify the Arrow-based API that allows processing data
without using the Context object.
"""

from __future__ import annotations

import pyarrow as pa
import pytest

from rebake.synchronize import (
    NearestNeighborTimeSynchronizerConfig,
    ZeroOrderHoldTimeSynchronizerConfig,
)


class TestZeroOrderHoldSynchronizerSynchronize:
    """Tests for ZeroOrderHoldTimeSynchronizer.synchronize() method."""

    def test_synchronizes_multiple_topics(self) -> None:
        """Verify synchronize() synchronizes multiple topics to same timestamps."""
        topic1 = pa.table(
            {
                "timestamp_ns": pa.array(
                    [400_000_000, 650_000_000, 900_000_000], type=pa.uint64()
                ),
                "value": pa.array([10, 20, 30], type=pa.int32()),
            }
        )
        topic2 = pa.table(
            {
                "timestamp_ns": pa.array(
                    [450_000_000, 680_000_000, 940_000_000], type=pa.uint64()
                ),
                "value": pa.array([100, 200, 300], type=pa.int32()),
            }
        )

        synchronizer = ZeroOrderHoldTimeSynchronizerConfig(fps=4).build()
        synced = synchronizer.synchronize({"/topic1": topic1, "/topic2": topic2})

        # Both topics should have the same number of rows
        assert synced["/topic1"].num_rows == synced["/topic2"].num_rows

    def test_returns_arrow_tables(self) -> None:
        """Verify synchronize() returns PyArrow Tables."""
        topic = pa.table(
            {
                "timestamp_ns": pa.array([100_000_000, 200_000_000], type=pa.uint64()),
                "value": pa.array([1, 2], type=pa.int32()),
            }
        )

        synchronizer = ZeroOrderHoldTimeSynchronizerConfig(fps=10).build()
        synced = synchronizer.synchronize({"/topic": topic})

        assert isinstance(synced["/topic"], pa.Table)

    def test_adds_synched_timestamp_column(self) -> None:
        """Verify synchronized data has synched_timestamp_ns column."""
        topic = pa.table(
            {
                "timestamp_ns": pa.array(
                    [100_000_000, 200_000_000, 300_000_000], type=pa.uint64()
                ),
                "value": pa.array([1, 2, 3], type=pa.int32()),
            }
        )

        synchronizer = ZeroOrderHoldTimeSynchronizerConfig(fps=10).build()
        synced = synchronizer.synchronize({"/topic": topic})

        assert "synched_timestamp_ns" in synced["/topic"].schema.names

    def test_adds_is_fresh_column(self) -> None:
        """Verify synchronized data has is_fresh column."""
        topic = pa.table(
            {
                "timestamp_ns": pa.array(
                    [100_000_000, 200_000_000, 300_000_000], type=pa.uint64()
                ),
                "value": pa.array([1, 2, 3], type=pa.int32()),
            }
        )

        synchronizer = ZeroOrderHoldTimeSynchronizerConfig(fps=10).build()
        synced = synchronizer.synchronize({"/topic": topic})

        assert "is_fresh" in synced["/topic"].schema.names


class TestNearestNeighborSynchronizerSynchronize:
    """Tests for NearestNeighborTimeSynchronizer.synchronize() method."""

    def test_synchronizes_multiple_topics(self) -> None:
        """Verify synchronize() synchronizes multiple topics."""
        topic1 = pa.table(
            {
                "timestamp_ns": pa.array(
                    [400_000_000, 650_000_000, 900_000_000], type=pa.uint64()
                ),
                "value": pa.array([10, 20, 30], type=pa.int32()),
            }
        )
        topic2 = pa.table(
            {
                "timestamp_ns": pa.array(
                    [450_000_000, 680_000_000, 940_000_000], type=pa.uint64()
                ),
                "value": pa.array([100, 200, 300], type=pa.int32()),
            }
        )

        synchronizer = NearestNeighborTimeSynchronizerConfig(fps=4).build()
        synced = synchronizer.synchronize({"/topic1": topic1, "/topic2": topic2})

        assert synced["/topic1"].num_rows == synced["/topic2"].num_rows

    def test_returns_dict_of_tables(self) -> None:
        """Verify synchronize() returns dict of PyArrow Tables."""
        topic = pa.table(
            {
                "timestamp_ns": pa.array([100_000_000, 200_000_000], type=pa.uint64()),
                "value": pa.array([1, 2], type=pa.int32()),
            }
        )

        synchronizer = NearestNeighborTimeSynchronizerConfig(fps=10).build()
        synced = synchronizer.synchronize({"/topic": topic})

        assert isinstance(synced, dict)
        assert "/topic" in synced


class TestArrowPolarsInterop:
    """Tests for Arrow/Polars interoperability with synchronize()."""

    def test_polars_to_arrow_roundtrip(self) -> None:
        """Verify Polars DataFrames can be converted and used with synchronize()."""
        pytest.importorskip("polars")
        import polars as pl

        # Create Polars DataFrame
        df = pl.DataFrame(
            {
                "timestamp_ns": [100_000_000, 200_000_000, 300_000_000],
                "value": [1, 2, 3],
            }
        ).cast({"timestamp_ns": pl.UInt64, "value": pl.Int32})

        # Convert to Arrow
        arrow_table = df.to_arrow()

        # Use with synchronize()
        synchronizer = ZeroOrderHoldTimeSynchronizerConfig(fps=10).build()
        synced = synchronizer.synchronize({"/topic": arrow_table})

        # Convert back to Polars
        result_df = pl.from_arrow(synced["/topic"])

        assert isinstance(result_df, pl.DataFrame)
        assert "synched_timestamp_ns" in result_df.columns

    def test_synchronize_works_with_arrow_directly(self) -> None:
        """Verify synchronize() works with PyArrow Tables directly (no Polars needed)."""
        # Create pure Arrow table
        table = pa.table(
            {
                "timestamp_ns": pa.array([100_000_000, 200_000_000], type=pa.uint64()),
                "data": pa.array([1.0, 2.0], type=pa.float64()),
            }
        )

        synchronizer = ZeroOrderHoldTimeSynchronizerConfig(fps=5).build()
        result = synchronizer.synchronize({"/sensor": table})

        assert isinstance(result["/sensor"], pa.Table)
