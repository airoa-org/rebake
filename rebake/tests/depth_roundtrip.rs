#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::io::Cursor;

use image::{DynamicImage, ImageBuffer, ImageFormat, Luma};

use rebake::encode::compressed_depth::decode_compressed_depth;
use rebake::encode::depth_quantizer::{Q10ClipParams, q10_to_p010le, quantize_frame};

// Creates a 16-bit grayscale PNG fixture for depth tests.
fn make_png(width: u32, height: u32, values: &[u16]) -> Vec<u8> {
    let image: ImageBuffer<Luma<u16>, Vec<u16>> =
        ImageBuffer::from_vec(width, height, values.to_vec()).expect("valid image size");
    let mut out = Cursor::new(Vec::new());
    DynamicImage::ImageLuma16(image)
        .write_to(&mut out, ImageFormat::Png)
        .expect("PNG encoding should succeed");
    out.into_inner()
}

// Returns the 99th percentile from a mutable slice of unsigned errors.
fn p99(values: &mut [u16]) -> u16 {
    values.sort_unstable();
    let idx = ((values.len() as f64 * 0.99).ceil() as usize).saturating_sub(1);
    values[idx]
}

// Tests that Q10Clip keeps roundtrip depth error within the hard limits.
#[test]
fn depth_q10clip_roundtrip_metrics_within_hard_limits() {
    let params = Q10ClipParams::new(4092);
    let mut raw = (1_u16..=4092_u16).collect::<Vec<_>>();
    raw.extend([0, 0, 5000, 6000]);

    let mut quantized = vec![0_u16; raw.len()];
    let _clipped = quantize_frame(&raw, &params, &mut quantized);

    let mut near_errors = Vec::new();
    for (&original, &q10) in raw.iter().zip(quantized.iter()) {
        if (1..=params.depth_max_mm).contains(&original) {
            let restored = params.dequantize(q10);
            near_errors.push((original as i32 - restored as i32).unsigned_abs() as u16);
        }
    }

    let mae = near_errors.iter().map(|&e| e as f64).sum::<f64>() / near_errors.len() as f64;
    let mut p99_input = near_errors.clone();
    let p99_err = p99(&mut p99_input);

    assert!(mae <= 10.0, "MAE should be <= 10mm, got {mae}");
    assert!(p99_err <= 60, "p99 should be <= 60mm, got {p99_err}");
}

// Tests that compressedDepth PNG data must start exactly at offset 12.
#[test]
fn compressed_depth_decode_requires_offset_12() {
    let png = make_png(2, 2, &[10, 20, 30, 40]);

    let mut expected_offset_payload = vec![0_u8; 12];
    expected_offset_payload.extend_from_slice(&png);
    let (_, width_a, height_a) =
        decode_compressed_depth(&expected_offset_payload, Some("16UC1; compressedDepth png"))
            .expect("decode with expected offset should succeed");
    assert_eq!((width_a, height_a), (2, 2));

    let mut scan_payload = vec![9_u8; 24];
    scan_payload.extend_from_slice(&png);
    let err = decode_compressed_depth(&scan_payload, Some("16UC1; compressedDepth png"))
        .expect_err("decode should fail when PNG is not at offset 12");
    assert!(err.reason().contains("offset 12"));
}

// Tests that the generated P010 frame uses a neutral UV plane.
#[test]
fn q10_to_p010le_has_neutral_uv_plane() {
    let q10 = vec![100_u16, 200_u16, 300_u16, 400_u16];
    let p010 = q10_to_p010le(&q10, 2, 2);
    let uv_values: Vec<u16> = p010[8..]
        .chunks_exact(2)
        .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
        .collect();
    assert_eq!(uv_values, vec![0x8000, 0x8000]);
}
