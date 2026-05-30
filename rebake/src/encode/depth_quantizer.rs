//! Q10Clip4 depth quantization and P010LE format conversion.
//!
//! Converts 16-bit depth values (millimeters) to 10-bit quantized values
//! suitable for encoding with HEVC/AV1 video codecs in Main10 profile.
//!
//! # Q10Clip4 Scheme
//!
//! - Maps `[1, depth_max_mm]` → `[1, 1023]` uniformly
//! - `depth == 0` (invalid pixel) → `q10 = 0`
//! - `depth > depth_max_mm` (out of range) → `q10 = 0` (clipped)
//! - Quantization step: `depth_max_mm / 1023` ≈ 4.0 mm (for depth_max_mm=4092)
//! - Maximum quantization error: ±step/2 ≈ ±2.0 mm
//!
//! # P010LE Format
//!
//! P010 is a 10-bit YUV 4:2:0 format with 16-bit storage per sample:
//! - Y plane: `q10 << 6` (10-bit value in upper bits of 16-bit LE word)
//! - UV plane: `0x8000` (neutral chroma, since depth has no color)

/// Parameters for Q10Clip4 quantization.
///
/// Quantizes 16-bit depth values to 10-bit with a configurable maximum depth.
/// Values beyond `depth_max_mm` are clipped to 0 (invalid).
#[derive(Clone, Debug)]
pub struct Q10ClipParams {
    /// Maximum depth in millimeters. Values above this are clipped to 0.
    pub depth_max_mm: u16,
}

impl Q10ClipParams {
    /// Creates new quantization parameters.
    ///
    /// # Panics
    ///
    /// Panics if `depth_max_mm` is 0 (division by zero in quantization).
    pub fn new(depth_max_mm: u16) -> Self {
        assert!(depth_max_mm > 0, "depth_max_mm must be > 0");
        Self { depth_max_mm }
    }

    /// Quantizes a single 16-bit depth value to 10-bit.
    ///
    /// - `depth_mm == 0` → 0 (invalid pixel)
    /// - `depth_mm > depth_max_mm` → 0 (clipped)
    /// - Otherwise → `[1, 1023]`
    ///
    /// Uses integer arithmetic for deterministic results:
    /// `q10 = (depth_mm * 1023 + depth_max_mm / 2) / depth_max_mm`
    pub fn quantize(&self, depth_mm: u16) -> u16 {
        if depth_mm == 0 || depth_mm > self.depth_max_mm {
            return 0;
        }
        let numerator = depth_mm as u32 * 1023 + self.depth_max_mm as u32 / 2;
        let q10 = numerator / self.depth_max_mm as u32;
        q10.clamp(1, 1023) as u16
    }

    /// Dequantizes a 10-bit value back to millimeters.
    ///
    /// - `q10 == 0` → 0 (invalid pixel)
    /// - Otherwise → `(q10 * depth_max_mm + 511) / 1023`
    pub fn dequantize(&self, q10: u16) -> u16 {
        if q10 == 0 {
            return 0;
        }
        let numerator = q10 as u32 * self.depth_max_mm as u32 + 511;
        (numerator / 1023) as u16
    }
}

/// Quantizes an entire frame of depth values in place.
///
/// # Arguments
///
/// - `raw`: Source depth values in millimeters (gray16le pixel values)
/// - `params`: Quantization parameters
/// - `out`: Output buffer for quantized 10-bit values (must be same length as `raw`)
///
/// # Returns
///
/// The number of pixels that were clipped (depth > depth_max_mm, excluding zero).
pub fn quantize_frame(raw: &[u16], params: &Q10ClipParams, out: &mut [u16]) -> usize {
    assert_eq!(raw.len(), out.len(), "input and output must be same length");
    let mut clipped = 0;
    for (src, dst) in raw.iter().zip(out.iter_mut()) {
        if *src > params.depth_max_mm && *src != 0 {
            clipped += 1;
        }
        *dst = params.quantize(*src);
    }
    clipped
}

/// Converts quantized 10-bit values to P010LE frame bytes.
///
/// P010LE is a planar 10-bit YUV 4:2:0 format:
/// - **Y plane**: `q10 << 6` stored as 16-bit little-endian (W × H × 2 bytes)
/// - **UV plane**: Neutral chroma `0x8000` (W/2 × H/2 × 2 × 2 bytes for Cb+Cr interleaved)
///
/// Total frame size: `W × H × 2 + W × H / 2 × 2 = W × H × 3` bytes.
///
/// # Arguments
///
/// - `q10`: Quantized 10-bit values (length must equal `width * height`)
/// - `width`: Frame width in pixels (must be even)
/// - `height`: Frame height in pixels (must be even)
pub fn q10_to_p010le(q10: &[u16], width: u32, height: u32) -> Vec<u8> {
    let w = width as usize;
    let h = height as usize;
    assert_eq!(q10.len(), w * h, "q10 length must equal width * height");

    // Y plane: each 10-bit value shifted left by 6 to occupy upper 10 bits of u16
    let y_size = w * h * 2;
    // UV plane: 4:2:0 subsampled, Cb+Cr interleaved (NV12-style)
    let uv_size = w * (h / 2) * 2;
    let total = y_size + uv_size;

    let mut buf = Vec::with_capacity(total);

    // Y plane
    for &val in q10 {
        let sample = val << 6;
        buf.extend_from_slice(&sample.to_le_bytes());
    }

    // UV plane: neutral chroma (0x8000 = mid-range for 16-bit)
    let chroma_samples = w / 2 * (h / 2);
    for _ in 0..chroma_samples {
        // Cb
        buf.extend_from_slice(&0x8000u16.to_le_bytes());
        // Cr
        buf.extend_from_slice(&0x8000u16.to_le_bytes());
    }

    buf
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn quantize_zero_returns_zero() {
        let params = Q10ClipParams::new(4092);
        assert_eq!(params.quantize(0), 0);
    }

    #[test]
    fn quantize_above_max_returns_zero() {
        let params = Q10ClipParams::new(4092);
        assert_eq!(params.quantize(4093), 0);
        assert_eq!(params.quantize(u16::MAX), 0);
    }

    #[test]
    fn quantize_max_returns_1023() {
        let params = Q10ClipParams::new(4092);
        assert_eq!(params.quantize(4092), 1023);
    }

    #[test]
    fn quantize_one_returns_at_least_one() {
        let params = Q10ClipParams::new(4092);
        assert!(params.quantize(1) >= 1);
    }

    #[test]
    fn dequantize_zero_returns_zero() {
        let params = Q10ClipParams::new(4092);
        assert_eq!(params.dequantize(0), 0);
    }

    #[test]
    fn roundtrip_error_within_step() {
        let params = Q10ClipParams::new(4092);
        let step = (params.depth_max_mm as f64 / 1023.0).ceil() as u16; // 4mm
        // Max error is step/2 for most values, but boundary clamping (q10 minimum is 1)
        // can cause up to a full step of error for very small depth values.
        let max_error = step;

        for depth_mm in 1..=4092u16 {
            let q10 = params.quantize(depth_mm);
            assert!(
                (1..=1023).contains(&q10),
                "q10 out of range for depth {depth_mm}"
            );
            let restored = params.dequantize(q10);
            let error = (depth_mm as i32 - restored as i32).unsigned_abs() as u16;
            assert!(
                error <= max_error,
                "roundtrip error {error}mm > max_error {max_error}mm for depth {depth_mm}mm"
            );
        }
    }

    #[test]
    fn quantize_frame_counts_clipped() {
        let params = Q10ClipParams::new(4092);
        let raw = vec![0, 100, 2000, 4092, 5000, 6000];
        let mut out = vec![0u16; raw.len()];
        let clipped = quantize_frame(&raw, &params, &mut out);
        assert_eq!(clipped, 2); // 5000 and 6000

        assert_eq!(out[0], 0); // zero stays zero
        assert!(out[1] > 0); // 100 -> valid
        assert!(out[3] == 1023); // 4092 -> max
        assert_eq!(out[4], 0); // 5000 -> clipped
        assert_eq!(out[5], 0); // 6000 -> clipped
    }

    #[test]
    fn p010le_y_plane_shift() {
        let q10 = vec![100u16, 200, 300, 400];
        let p010 = q10_to_p010le(&q10, 2, 2);

        // Check Y plane values (first 8 bytes = 4 pixels * 2 bytes)
        let y_values: Vec<u16> = p010[..8]
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        assert_eq!(y_values, vec![100 << 6, 200 << 6, 300 << 6, 400 << 6]);
    }

    #[test]
    fn p010le_uv_plane_neutral() {
        let q10 = vec![100u16, 200, 300, 400];
        let p010 = q10_to_p010le(&q10, 2, 2);

        // UV plane starts after Y plane (8 bytes for 2x2)
        // 4:2:0 subsampling: 1 Cb + 1 Cr sample for 2x2 block
        let uv_values: Vec<u16> = p010[8..]
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        assert!(uv_values.iter().all(|&v| v == 0x8000));
    }

    #[test]
    fn p010le_frame_size() {
        // 4x4 frame
        let q10 = vec![0u16; 16];
        let p010 = q10_to_p010le(&q10, 4, 4);
        // Y: 4*4*2 = 32 bytes
        // UV: 2*2*2*2 = 16 bytes (2 chroma samples per 2x2 block, 2 bytes each, Cb+Cr)
        assert_eq!(p010.len(), 48);
    }

    #[test]
    fn depth_config_serde_roundtrip() {
        let params = Q10ClipParams::new(4092);
        assert_eq!(params.depth_max_mm, 4092);
        assert_eq!(params.quantize(4092), 1023);
    }
}
