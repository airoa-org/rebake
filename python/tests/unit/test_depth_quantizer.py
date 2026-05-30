"""Cross-validation tests for Q10 depth quantization.

Verifies that the Python implementation of Q10Clip4 quantize/dequantize
(in experiment/depth_encoder_benchmark/metrics.py) produces identical results
to the Rust implementation (in rebake/src/encode/depth_quantizer.rs).

Test values are taken directly from the Rust unit tests.
If these tests fail, the implementations have diverged.
"""

from __future__ import annotations

import numpy as np
import pytest

# Cross-validates the Python Q10 implementation in experiment/ against the Rust one.
# experiment/ is not part of the published package, so skip cleanly when it is absent.
_metrics = pytest.importorskip("experiment.depth_encoder_benchmark.metrics")
quantize_q10 = _metrics.quantize_q10
dequantize_q10 = _metrics.dequantize_q10

DEPTH_MAX_MM = 4092


class TestQuantizeQ10:
    """Cross-validation of quantize_q10 against Rust depth_quantizer.rs."""

    def test_zero_returns_zero(self) -> None:
        """Rust: quantize_zero_returns_zero."""
        result = quantize_q10(np.array([0], dtype=np.uint16), DEPTH_MAX_MM)
        assert result[0] == 0

    def test_above_max_returns_zero(self) -> None:
        """Rust: quantize_above_max_returns_zero."""
        result = quantize_q10(
            np.array([4093, 65535], dtype=np.uint16), DEPTH_MAX_MM
        )
        assert result[0] == 0
        assert result[1] == 0

    def test_max_returns_1023(self) -> None:
        """Rust: quantize_max_returns_1023."""
        result = quantize_q10(np.array([4092], dtype=np.uint16), DEPTH_MAX_MM)
        assert result[0] == 1023

    def test_one_returns_at_least_one(self) -> None:
        """Rust: quantize_one_returns_at_least_one."""
        result = quantize_q10(np.array([1], dtype=np.uint16), DEPTH_MAX_MM)
        assert result[0] >= 1

    def test_frame_clipping(self) -> None:
        """Rust: quantize_frame_counts_clipped."""
        data = np.array([0, 100, 2000, 4092, 5000, 6000], dtype=np.uint16)
        result = quantize_q10(data, DEPTH_MAX_MM)
        assert result[0] == 0      # zero -> zero
        assert result[1] > 0       # valid -> non-zero
        assert result[2] > 0       # valid -> non-zero
        assert result[3] == 1023   # max -> 1023
        assert result[4] == 0      # above max -> clipped to 0
        assert result[5] == 0      # above max -> clipped to 0


class TestDequantizeQ10:
    """Cross-validation of dequantize_q10 against Rust depth_quantizer.rs."""

    def test_zero_returns_zero(self) -> None:
        """Rust: dequantize_zero_returns_zero."""
        result = dequantize_q10(np.array([0], dtype=np.uint16), DEPTH_MAX_MM)
        assert result[0] == 0


class TestRoundtrip:
    """Cross-validation of quantize -> dequantize roundtrip against Rust tests."""

    def test_roundtrip_error_within_step(self) -> None:
        """Rust: roundtrip_error_within_step.

        For all valid depths [1, depth_max_mm], the roundtrip error must
        not exceed the quantization step size: ceil(depth_max_mm / 1023).
        """
        # Step size = ceil(4092 / 1023) = 4mm
        step_size = (DEPTH_MAX_MM + 1023 - 1) // 1023  # = 4

        depths = np.arange(1, DEPTH_MAX_MM + 1, dtype=np.uint16)
        q10 = quantize_q10(depths, DEPTH_MAX_MM)
        recovered = dequantize_q10(q10, DEPTH_MAX_MM)

        errors = np.abs(depths.astype(np.int32) - recovered.astype(np.int32))
        max_error = int(errors.max())

        assert max_error <= step_size, (
            f"Max roundtrip error {max_error}mm exceeds step size {step_size}mm"
        )

    def test_roundtrip_metrics_within_hard_limits(self) -> None:
        """Rust integration test: depth_q10clip_roundtrip_metrics_within_hard_limits.

        Verifies MAE <= 10mm and p99 error <= 60mm across the full range.
        """
        # Build test data: [1..4092] + invalid values [0, 0, 5000, 6000]
        valid = np.arange(1, DEPTH_MAX_MM + 1, dtype=np.uint16)
        invalid = np.array([0, 0, 5000, 6000], dtype=np.uint16)
        depths = np.concatenate([valid, invalid])

        q10 = quantize_q10(depths, DEPTH_MAX_MM)
        recovered = dequantize_q10(q10, DEPTH_MAX_MM)

        # Only compare valid pixels (both original and recovered are non-zero)
        mask = (depths > 0) & (recovered > 0)
        errors = np.abs(depths[mask].astype(np.int32) - recovered[mask].astype(np.int32))

        mae = float(errors.mean())
        p99 = float(np.percentile(errors, 99))

        assert mae <= 10, f"MAE {mae:.1f}mm exceeds 10mm"
        assert p99 <= 60, f"p99 error {p99:.1f}mm exceeds 60mm"

    def test_exact_values_at_boundaries(self) -> None:
        """Verify exact quantize/dequantize values at critical boundaries.

        These values are computed using the Rust formula:
          quantize: q10 = (depth_mm * 1023 + depth_max_mm / 2) / depth_max_mm
          dequantize: depth = (q10 * depth_max_mm + 511) / 1023
        """
        # depth=1 -> q10 = (1*1023 + 2046) / 4092 = 3069/4092 = 0.75 -> rounds to 1
        assert quantize_q10(np.array([1], dtype=np.uint16), DEPTH_MAX_MM)[0] == 1
        # q10=1 -> depth = (1*4092 + 511) / 1023 = 4603/1023 = 4.5 -> rounds to 4
        assert dequantize_q10(np.array([1], dtype=np.uint16), DEPTH_MAX_MM)[0] == 4

        # depth=4092 -> q10 = (4092*1023 + 2046) / 4092 = 1023
        assert quantize_q10(np.array([4092], dtype=np.uint16), DEPTH_MAX_MM)[0] == 1023
        # q10=1023 -> depth = (1023*4092 + 511) / 1023 = 4092
        assert dequantize_q10(np.array([1023], dtype=np.uint16), DEPTH_MAX_MM)[0] == 4092
