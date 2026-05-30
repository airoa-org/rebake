"""Timestamp primitives for recording analysis."""

from __future__ import annotations

import numpy as np
import numpy.typing as npt

from .. import _internal

IntArrayLike = npt.ArrayLike
FloatArrayLike = npt.ArrayLike


def slice_timestamps_to_window(
    timestamps_ns: IntArrayLike,
    start_ns: int,
    end_ns: int,
) -> np.ndarray:
    """Return sorted timestamps that fall inside an inclusive time window.

    Args:
        timestamps_ns: One-dimensional sequence of nanosecond timestamps.
        start_ns: Inclusive window start in nanoseconds.
        end_ns: Inclusive window end in nanoseconds.

    Returns:
        A sorted ``numpy.ndarray`` of ``int64`` timestamps inside the window.
    """

    return np.asarray(
        _internal.analysis.slice_timestamps_to_window(timestamps_ns, start_ns, end_ns),
        dtype=np.int64,
    )


def compute_message_intervals_ms(timestamps_ns: IntArrayLike) -> np.ndarray:
    """Compute adjacent message intervals in milliseconds.

    Args:
        timestamps_ns: One-dimensional sequence of nanosecond timestamps.

    Returns:
        A ``float64`` NumPy array containing the interval between adjacent
        timestamps in milliseconds. Fewer than two timestamps yields an empty
        array.
    """

    return np.asarray(
        _internal.analysis.compute_message_intervals_ms(timestamps_ns),
        dtype=np.float64,
    )


def compute_coverage_ratio(
    timestamps_ns: IntArrayLike,
    start_ns: int,
    end_ns: int,
) -> float:
    """Compute how much of a window is covered by the observed timestamp span.

    Coverage is defined as the observed timestamp span divided by the episode
    window duration. When no timestamps fall inside the window, the result is
    ``0.0``.
    """

    return _internal.analysis.compute_coverage_ratio(timestamps_ns, start_ns, end_ns)


def compute_observed_hz(timestamps_ns: IntArrayLike) -> float | None:
    """Estimate the observed message frequency from timestamp density.

    Returns:
        Observed frequency in Hz, or ``None`` when fewer than two timestamps
        are available or the observed span is zero.
    """

    return _internal.analysis.compute_observed_hz(timestamps_ns)


def compute_interval_stats(intervals_ms: FloatArrayLike) -> dict[str, float | None]:
    """Summarize interval stability statistics from a sequence of intervals.

    The returned dictionary contains:

    - ``median_interval_ms``
    - ``interval_cv``
    - ``max_interval_ms``
    - ``max_interval_over_median``
    """

    return _internal.analysis.compute_interval_stats(intervals_ms)
