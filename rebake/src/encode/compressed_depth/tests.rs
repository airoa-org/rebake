use super::test_support::{
    encode_png16_fixture, make_rvl_payload_fixture, wrap_png_compressed_depth_payload,
};
use super::{CompressedDepthFormat, decode_compressed_depth, decode_compressed_depth_u16};

// Tests that the parser recognizes the supported transport variants.
#[test]
fn parse_recognizes_rvl_and_32fc1() {
    assert_eq!(
        CompressedDepthFormat::parse("16UC1; compressedDepth rvl").unwrap(),
        CompressedDepthFormat::Depth16Rvl
    );
    assert_eq!(
        CompressedDepthFormat::parse("32FC1; compressedDepth png").unwrap(),
        CompressedDepthFormat::Depth32FPng
    );
}

// Tests that PNG payloads decode to the expected dimensions and values.
#[test]
fn decode_png_u16_returns_correct_dimensions_and_values() {
    let values = [10u16, 20, 30, 40];
    let png = encode_png16_fixture(2, 2, &values);
    let payload = wrap_png_compressed_depth_payload(&png);

    let decoded =
        decode_compressed_depth_u16(&payload, Some("16UC1; compressedDepth png")).unwrap();

    assert_eq!(decoded.width, 2);
    assert_eq!(decoded.height, 2);
    assert_eq!(decoded.values_mm, values);
}

// Tests that the byte wrapper returns little-endian depth bytes.
#[test]
fn decode_wrapper_returns_little_endian_bytes() {
    let values = [10u16, 20, 30, 40];
    let png = encode_png16_fixture(2, 2, &values);
    let payload = wrap_png_compressed_depth_payload(&png);

    let (raw, width, height) =
        decode_compressed_depth(&payload, Some("16UC1; compressedDepth png")).unwrap();

    assert_eq!((width, height), (2, 2));
    let decoded: Vec<u16> = raw
        .chunks_exact(2)
        .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
        .collect();
    assert_eq!(decoded, values);
}

// Tests that very short payloads are rejected early.
#[test]
fn decode_rejects_short_payload() {
    let data = vec![0u8; 19];
    let result = decode_compressed_depth_u16(&data, Some("16UC1; compressedDepth png"));
    assert!(result.is_err());
    assert!(result.unwrap_err().reason().contains("too short"));
}

// Tests that a payload without a PNG header fails with a clear error.
#[test]
fn decode_rejects_invalid_png_signature() {
    let mut payload = vec![0u8; 12];
    payload.extend_from_slice(&[0x00; 8]);
    let result = decode_compressed_depth_u16(&payload, Some("16UC1; compressedDepth png"));
    assert!(result.is_err());
    assert!(result.unwrap_err().reason().contains("offset 12"));
}

// Tests that RVL payloads decode to the expected dimensions and values.
#[test]
fn decode_rvl_returns_correct_dimensions_and_values() {
    let values = [0u16, 100, 100, 0, 103, 120];
    let payload = make_rvl_payload_fixture(3, 2, &values);

    let decoded =
        decode_compressed_depth_u16(&payload, Some("16UC1; compressedDepth rvl")).unwrap();

    assert_eq!((decoded.width, decoded.height), (3, 2));
    assert_eq!(decoded.values_mm, values);
}

// Tests that missing format metadata falls back to PNG sniffing.
#[test]
fn decode_without_format_falls_back_to_png_signature() {
    let values = [7u16, 8, 9, 10];
    let png = encode_png16_fixture(2, 2, &values);
    let payload = wrap_png_compressed_depth_payload(&png);

    let decoded = decode_compressed_depth_u16(&payload, None).unwrap();
    assert_eq!(decoded.values_mm, values);
}

// Tests that recognized but unsupported 32FC1 payloads fail explicitly.
#[test]
fn decode_32fc1_returns_explicit_unsupported_error() {
    let png = encode_png16_fixture(2, 2, &[10, 20, 30, 40]);
    let payload = wrap_png_compressed_depth_payload(&png);

    let err = decode_compressed_depth_u16(&payload, Some("32FC1; compressedDepth png"))
        .expect_err("32FC1 should be recognized but unsupported");
    assert!(err.reason().contains("32FC1"));
}
