from __future__ import annotations

import numpy as np
from rebake.analysis import (
    compute_coverage_ratio,
    compute_interval_stats,
    compute_message_intervals_ms,
    compute_observed_hz,
    slice_timestamps_to_window,
)


def test_slice_timestamps_to_window_filters_inclusively_and_sorts() -> None:
    result = slice_timestamps_to_window(
        [300, 100, 200, 400, 500],
        start_ns=200,
        end_ns=400,
    )

    assert np.array_equal(result, np.array([200, 300, 400], dtype=np.int64))


def test_compute_message_intervals_ms_returns_adjacent_deltas() -> None:
    intervals = compute_message_intervals_ms([0, 100_000_000, 450_000_000])

    assert np.allclose(intervals, np.array([100.0, 350.0]))


def test_timestamp_primitives_accept_numpy_arrays() -> None:
    timestamps = np.array([0, 100_000_000, 450_000_000], dtype=np.int64)

    windowed = slice_timestamps_to_window(timestamps, start_ns=0, end_ns=200_000_000)
    intervals = compute_message_intervals_ms(timestamps)

    assert np.array_equal(windowed, np.array([0, 100_000_000], dtype=np.int64))
    assert np.allclose(intervals, np.array([100.0, 350.0]))


def test_compute_coverage_ratio_uses_observed_span_within_window() -> None:
    ratio = compute_coverage_ratio(
        [0, 100_000_000, 450_000_000],
        start_ns=0,
        end_ns=500_000_000,
    )

    assert ratio == 0.9


def test_compute_coverage_ratio_returns_zero_for_empty_window() -> None:
    ratio = compute_coverage_ratio(
        [10, 20, 30],
        start_ns=100,
        end_ns=200,
    )

    assert ratio == 0.0


def test_compute_observed_hz_returns_none_for_single_message() -> None:
    assert compute_observed_hz([100]) is None


def test_compute_observed_hz_uses_message_density() -> None:
    hz = compute_observed_hz([0, 100_000_000, 200_000_000, 300_000_000])

    assert hz == 10.0


def test_compute_interval_stats_returns_expected_summary() -> None:
    stats = compute_interval_stats([100.0, 100.0, 300.0])

    assert stats["median_interval_ms"] == 100.0
    assert stats["max_interval_ms"] == 300.0
    assert stats["max_interval_over_median"] == 3.0
    assert stats["interval_cv"] is not None


def test_compute_interval_stats_returns_none_fields_for_empty_input() -> None:
    stats = compute_interval_stats([])

    assert stats == {
        "median_interval_ms": None,
        "interval_cv": None,
        "max_interval_ms": None,
        "max_interval_over_median": None,
    }
