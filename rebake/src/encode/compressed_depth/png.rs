use image::DynamicImage;

use crate::core::stage::StageError;

use super::DecodedDepthFrame;

/// 12-byte `ConfigHeader` at the start of every compressedDepth payload.
const CONFIG_HEADER_LEN: usize = 12;

/// Minimum payload length for a PNG depth payload: 12-byte header + 8-byte PNG signature.
const MIN_PNG_PAYLOAD_LEN: usize = CONFIG_HEADER_LEN + 8;

/// Offset of the PNG data within the compressedDepth payload.
const PNG_OFFSET: usize = CONFIG_HEADER_LEN;

/// PNG file signature (first 8 bytes).
const PNG_SIGNATURE: [u8; 8] = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];

/// Returns true when the payload contains a PNG stream at byte offset 12.
pub(super) fn has_png_payload(data: &[u8]) -> bool {
    let png_data = match data.get(PNG_OFFSET..) {
        Some(png_data) => png_data,
        None => return false,
    };
    png_data.len() >= 8 && png_data[..8] == PNG_SIGNATURE
}

/// Decodes a `compressedDepth png` payload into millimeter depth values.
pub(super) fn decode_png_depth(data: &[u8]) -> Result<DecodedDepthFrame, StageError> {
    if data.len() < MIN_PNG_PAYLOAD_LEN {
        return Err(StageError::invalid(format!(
            "compressedDepth payload too short: {} bytes (minimum {})",
            data.len(),
            MIN_PNG_PAYLOAD_LEN
        )));
    }

    let png_data = &data[PNG_OFFSET..];

    if png_data.len() < 8 || png_data[..8] != PNG_SIGNATURE {
        return Err(StageError::invalid(
            "compressedDepth: PNG signature not found at offset 12. \
             Expected PNG header (89 50 4E 47) at byte 12 of the payload.",
        ));
    }

    let image = image::load_from_memory_with_format(png_data, image::ImageFormat::Png)
        .map_err(|e| StageError::invalid(format!("compressedDepth: PNG decode failed: {e}")))?;

    let luma16 = match image {
        DynamicImage::ImageLuma16(buf) => buf,
        other => {
            return Err(StageError::invalid(format!(
                "compressedDepth: expected gray16 PNG, got {:?}",
                other.color()
            )));
        }
    };

    let width = luma16.width();
    let height = luma16.height();
    let values_mm = luma16.into_raw();

    DecodedDepthFrame::new(width, height, values_mm)
}
