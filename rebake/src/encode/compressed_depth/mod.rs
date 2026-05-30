//! Decoder for ROS `compressedDepth` payloads.
//!
//! ROS `compressed_depth_image_transport` stores a 12-byte configuration header
//! followed by a codec-specific payload. The transport used in practice is
//! encoded in the ROS message's `format` field, for example:
//!
//! - `16UC1; compressedDepth png`
//! - `16UC1; compressedDepth rvl`
//! - `32FC1; compressedDepth png`
//!
//! rebake currently supports:
//!
//! - `16UC1 + PNG`
//! - `16UC1 + RVL`
//!
//! It explicitly recognizes, but does not yet decode:
//!
//! - `32FC1 + PNG`
//!
//! # Recommended Reading
//!
//! If you are new to ROS depth compression, these are the most useful upstream
//! references to read before diving into the implementation:
//!
//! 1. REP 118 for the meaning of `16UC1` and `32FC1` depth images:
//!    <https://ros.org/reps/rep-0118.html>
//! 2. The `compressed_depth_image_transport` package docs for the transport's
//!    purpose and user-facing behavior:
//!    <https://docs.ros.org/en/humble/p/compressed_depth_image_transport/>
//! 3. ROS's `codec.cpp` for the PNG payload layout and format handling:
//!    <https://raw.githubusercontent.com/ros-perception/image_transport_plugins/rolling/compressed_depth_image_transport/src/codec.cpp>
//! 4. ROS's `rvl_codec.cpp` for the RVL nibble/VLE reference implementation:
//!    <https://raw.githubusercontent.com/ros-perception/image_transport_plugins/rolling/compressed_depth_image_transport/src/rvl_codec.cpp>

mod format;
mod png;
mod rvl;
#[cfg(test)]
pub(crate) mod test_support;

pub use format::CompressedDepthFormat;

use crate::common::DepthFrame;
use crate::core::stage::StageError;

/// A decoded depth frame in millimeters.
///
/// This is the typed core representation used by depth encoders. It keeps the
/// semantic payload (`u16` depth values) separate from any byte-level transport
/// concerns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedDepthFrame {
    pub width: u32,
    pub height: u32,
    pub values_mm: Vec<u16>,
}

impl DecodedDepthFrame {
    fn new(width: u32, height: u32, values_mm: Vec<u16>) -> Result<Self, StageError> {
        let expected = expected_pixel_count(width, height)?;
        if values_mm.len() != expected {
            return Err(StageError::invalid(format!(
                "decoded depth frame has {} values, expected {} for {width}x{height}",
                values_mm.len(),
                expected
            )));
        }

        Ok(Self {
            width,
            height,
            values_mm,
        })
    }

    /// Serializes the depth values to raw `gray16le` bytes.
    pub fn gray16le_bytes(&self) -> Vec<u8> {
        values_mm_to_gray16le_bytes(&self.values_mm)
    }

    /// Consumes the frame and serializes it to raw `gray16le` bytes.
    pub fn into_gray16le_bytes(self) -> Vec<u8> {
        values_mm_to_gray16le_bytes(&self.values_mm)
    }
}

/// Decodes a [`DepthFrame`] carrying ROS `compressedDepth` bytes.
///
/// This is the domain-level entry point used by depth encoders. It keeps the
/// caller from having to manually thread `bytes` and `ros_format` together.
pub fn decode_depth_frame(frame: &DepthFrame) -> Result<DecodedDepthFrame, StageError> {
    decode_compressed_depth_u16(&frame.bytes, frame.ros_format.as_deref())
}

/// Decodes a ROS `compressedDepth` payload into a typed depth frame.
///
/// The `ros_format` value should be taken from the message's `format` field.
/// If it is missing, rebake falls back to PNG signature sniffing for backwards
/// compatibility with older tests and fixtures.
///
/// Prefer [`decode_depth_frame`] at call sites that already have a [`DepthFrame`].
pub fn decode_compressed_depth_u16(
    data: &[u8],
    ros_format: Option<&str>,
) -> Result<DecodedDepthFrame, StageError> {
    match resolve_format_or_sniff_png(ros_format, data)? {
        CompressedDepthFormat::Depth16Png => png::decode_png_depth(data),
        CompressedDepthFormat::Depth16Rvl => rvl::decode_rvl_depth(data),
        CompressedDepthFormat::Depth32FPng => Err(StageError::invalid(
            "compressedDepth 32FC1 PNG is recognized but not supported yet",
        )),
    }
}

/// Decodes a ROS `compressedDepth` payload into little-endian `gray16le` bytes.
///
/// This is a compatibility wrapper around [`decode_compressed_depth_u16`].
pub fn decode_compressed_depth(
    data: &[u8],
    ros_format: Option<&str>,
) -> Result<(Vec<u8>, u32, u32), StageError> {
    let decoded = decode_compressed_depth_u16(data, ros_format)?;
    let width = decoded.width;
    let height = decoded.height;
    let raw_bytes = decoded.into_gray16le_bytes();
    Ok((raw_bytes, width, height))
}

/// Resolves the declared ROS format, with a PNG-sniffing fallback kept only for
/// legacy callers that do not pass the `CompressedImage.format` string through.
fn resolve_format_or_sniff_png(
    ros_format: Option<&str>,
    data: &[u8],
) -> Result<CompressedDepthFormat, StageError> {
    if let Some(ros_format) = ros_format {
        return CompressedDepthFormat::parse(ros_format);
    }

    if png::has_png_payload(data) {
        return Ok(CompressedDepthFormat::Depth16Png);
    }

    Err(StageError::invalid(
        "compressedDepth format is missing; cannot decode non-PNG payload",
    ))
}

/// Returns the expected number of pixels for the given frame size.
fn expected_pixel_count(width: u32, height: u32) -> Result<usize, StageError> {
    (width as usize)
        .checked_mul(height as usize)
        .ok_or_else(|| StageError::invalid("compressedDepth dimensions overflow"))
}

/// Converts decoded depth values to little-endian `gray16le` bytes.
fn values_mm_to_gray16le_bytes(values_mm: &[u16]) -> Vec<u8> {
    let mut raw_bytes = Vec::with_capacity(values_mm.len() * 2);
    for &value in values_mm {
        raw_bytes.extend_from_slice(&value.to_le_bytes());
    }
    raw_bytes
}

#[cfg(test)]
mod tests;
