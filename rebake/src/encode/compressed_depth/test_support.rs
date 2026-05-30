use std::io::Cursor;

use image::{DynamicImage, ImageBuffer, ImageFormat, Luma};

use crate::common::DepthFrame;

// Encodes a flat `u16` depth image as a grayscale PNG fixture.
pub(crate) fn encode_png16_fixture(width: u32, height: u32, values: &[u16]) -> Vec<u8> {
    let image: ImageBuffer<Luma<u16>, Vec<u16>> =
        ImageBuffer::from_vec(width, height, values.to_vec()).expect("valid image size");
    let mut out = Cursor::new(Vec::new());
    DynamicImage::ImageLuma16(image)
        .write_to(&mut out, ImageFormat::Png)
        .expect("PNG encoding should succeed");
    out.into_inner()
}

// Wraps a PNG fixture in ROS `compressedDepth` framing.
//
// The PNG transport stores a 12-byte config header followed by the encoded PNG.
pub(crate) fn wrap_png_compressed_depth_payload(png: &[u8]) -> Vec<u8> {
    let mut payload = vec![0u8; 12];
    payload.extend_from_slice(png);
    payload
}

// Encodes one integer using RVL's variable-length nibble representation.
pub(crate) fn encode_rvl_vle(mut value: u32, nibbles: &mut Vec<u8>) {
    loop {
        let mut nibble = (value & 0x7) as u8;
        value >>= 3;
        if value != 0 {
            nibble |= 0x8;
        }
        nibbles.push(nibble);
        if value == 0 {
            break;
        }
    }
}

// Encodes a flat depth image into the RVL byte stream used by ROS.
//
// RVL alternates runs of zeros and non-zeros. Non-zero samples are delta-encoded
// from the previous depth and zigzag-encoded before VLE packing.
pub(crate) fn encode_rvl_stream_fixture(values: &[u16]) -> Vec<u8> {
    let mut nibbles = Vec::new();
    let mut index = 0usize;
    let mut previous = 0i32;

    while index < values.len() {
        let zero_start = index;
        while index < values.len() && values[index] == 0 {
            index += 1;
        }
        encode_rvl_vle((index - zero_start) as u32, &mut nibbles);

        let nonzero_start = index;
        while index < values.len() && values[index] != 0 {
            index += 1;
        }
        encode_rvl_vle((index - nonzero_start) as u32, &mut nibbles);

        for &value in &values[nonzero_start..index] {
            let delta = value as i32 - previous;
            let zigzag = ((delta << 1) ^ (delta >> 31)) as u32;
            encode_rvl_vle(zigzag, &mut nibbles);
            previous = value as i32;
        }
    }

    let mut words = Vec::new();
    for chunk in nibbles.chunks(8) {
        let mut word = 0u32;
        for &nibble in chunk {
            word = (word << 4) | u32::from(nibble);
        }
        let missing = 8usize.saturating_sub(chunk.len());
        word <<= missing * 4;
        words.extend_from_slice(&word.to_le_bytes());
    }

    words
}

// Builds a full ROS `compressedDepth rvl` payload for tests.
pub(crate) fn make_rvl_payload_fixture(width: u32, height: u32, values: &[u16]) -> Vec<u8> {
    let mut payload = vec![0u8; 12];
    payload.extend_from_slice(&width.to_le_bytes());
    payload.extend_from_slice(&height.to_le_bytes());
    payload.extend_from_slice(&encode_rvl_stream_fixture(values));
    payload
}

// Builds a `DepthFrame` carrying `16UC1; compressedDepth png` data.
pub(crate) fn make_png_depth_frame(
    index: u32,
    width: u32,
    height: u32,
    values: &[u16],
) -> DepthFrame {
    let png = encode_png16_fixture(width, height, values);
    let payload = wrap_png_compressed_depth_payload(&png);
    make_depth_frame(index, payload, "16UC1; compressedDepth png")
}

// Builds a `DepthFrame` carrying `16UC1; compressedDepth rvl` data.
pub(crate) fn make_rvl_depth_frame(
    index: u32,
    width: u32,
    height: u32,
    values: &[u16],
) -> DepthFrame {
    let payload = make_rvl_payload_fixture(width, height, values);
    make_depth_frame(index, payload, "16UC1; compressedDepth rvl")
}

// Wraps a payload and ROS format string in a `DepthFrame` fixture.
fn make_depth_frame(index: u32, payload: Vec<u8>, ros_format: &str) -> DepthFrame {
    let mut frame = DepthFrame::new(index, "bin", payload);
    frame.set_ros_format(ros_format);
    frame
}
