use crate::core::stage::StageError;

use super::DecodedDepthFrame;

/// 12-byte `ConfigHeader` at the start of every compressedDepth payload.
const CONFIG_HEADER_LEN: usize = 12;

/// RVL payload stores width/height immediately after the 12-byte config header.
const RVL_DIMENSIONS_LEN: usize = 8;

/// Offset of the RVL width/height fields within the compressedDepth payload.
const RVL_DIMENSIONS_OFFSET: usize = CONFIG_HEADER_LEN;

/// Parsed view of a ROS `compressedDepth rvl` payload.
struct RvlPayload<'a> {
    width: u32,
    height: u32,
    stream: &'a [u8],
}

impl RvlPayload<'_> {
    /// Returns the number of pixels described by the payload dimensions.
    fn pixel_count(&self) -> Result<usize, StageError> {
        (self.width as usize)
            .checked_mul(self.height as usize)
            .ok_or_else(|| StageError::invalid("compressedDepth RVL dimensions overflow"))
    }
}

/// Decodes a `16UC1; compressedDepth rvl` payload into millimeter depth values.
///
/// The payload layout matches ROS `compressed_depth_image_transport`:
/// a 12-byte config header, followed by little-endian `width`, `height`, and the
/// RVL bitstream itself.
pub(super) fn decode_rvl_depth(data: &[u8]) -> Result<DecodedDepthFrame, StageError> {
    let payload = parse_rvl_payload(data)?;
    let values_mm = decode_rvl_stream(payload.stream, payload.pixel_count()?)?;
    DecodedDepthFrame::new(payload.width, payload.height, values_mm)
}

/// Parses the fixed-size header fields of an RVL payload.
fn parse_rvl_payload(data: &[u8]) -> Result<RvlPayload<'_>, StageError> {
    if data.len() < RVL_DIMENSIONS_OFFSET + RVL_DIMENSIONS_LEN {
        return Err(StageError::invalid(format!(
            "compressedDepth RVL payload too short: {} bytes (minimum {})",
            data.len(),
            RVL_DIMENSIONS_OFFSET + RVL_DIMENSIONS_LEN
        )));
    }

    let width = u32::from_le_bytes(
        data[RVL_DIMENSIONS_OFFSET..RVL_DIMENSIONS_OFFSET + 4]
            .try_into()
            .expect("slice length checked above"),
    );
    let height = u32::from_le_bytes(
        data[RVL_DIMENSIONS_OFFSET + 4..RVL_DIMENSIONS_OFFSET + 8]
            .try_into()
            .expect("slice length checked above"),
    );

    if width == 0 || height == 0 {
        return Err(StageError::invalid(format!(
            "compressedDepth RVL has non-positive dimensions: {width}x{height}"
        )));
    }

    Ok(RvlPayload {
        width,
        height,
        stream: &data[RVL_DIMENSIONS_OFFSET + RVL_DIMENSIONS_LEN..],
    })
}

/// Expands the RVL bitstream into a flat `u16` depth buffer.
///
/// RVL stores alternating runs of zeros and non-zeros. The non-zero values are
/// delta-encoded from the previous depth sample and zigzag-encoded before being
/// written as variable-length integers.
fn decode_rvl_stream(data: &[u8], pixel_count: usize) -> Result<Vec<u16>, StageError> {
    let mut decoder = RvlDecoder::new(data);
    let mut output = Vec::with_capacity(pixel_count);
    let mut previous = 0i32;

    while output.len() < pixel_count {
        extend_zero_run(&mut decoder, &mut output, pixel_count)?;
        extend_nonzero_run(&mut decoder, &mut output, &mut previous, pixel_count)?;
    }

    Ok(output)
}

/// Expands one run of zeros from the RVL stream into the output buffer.
fn extend_zero_run(
    decoder: &mut RvlDecoder<'_>,
    output: &mut Vec<u16>,
    pixel_count: usize,
) -> Result<(), StageError> {
    let zero_count = decode_run_length(decoder)?;
    ensure_run_fits(output.len(), zero_count, pixel_count)?;
    output.extend(std::iter::repeat_n(0u16, zero_count));
    Ok(())
}

/// Expands one run of non-zero values from the RVL stream into the output buffer.
fn extend_nonzero_run(
    decoder: &mut RvlDecoder<'_>,
    output: &mut Vec<u16>,
    previous: &mut i32,
    pixel_count: usize,
) -> Result<(), StageError> {
    let nonzero_count = decode_run_length(decoder)?;
    ensure_run_fits(output.len(), nonzero_count, pixel_count)?;

    for _ in 0..nonzero_count {
        output.push(decode_next_depth_value(decoder, previous)?);
    }

    Ok(())
}

/// Reads a single run length from the RVL stream.
fn decode_run_length(decoder: &mut RvlDecoder<'_>) -> Result<usize, StageError> {
    usize::try_from(decoder.decode_vle()?)
        .map_err(|_| StageError::invalid("compressedDepth RVL run length overflow"))
}

/// Checks that the next decoded run fits in the declared frame size.
fn ensure_run_fits(
    output_len: usize,
    run_length: usize,
    pixel_count: usize,
) -> Result<(), StageError> {
    if output_len + run_length > pixel_count {
        return Err(StageError::invalid(
            "compressedDepth RVL expands past the declared image size",
        ));
    }

    Ok(())
}

/// Decodes one non-zero depth sample from the RVL stream.
fn decode_next_depth_value(
    decoder: &mut RvlDecoder<'_>,
    previous: &mut i32,
) -> Result<u16, StageError> {
    let delta = decode_zigzag_delta(decoder.decode_vle()?);
    let current = previous
        .checked_add(delta)
        .ok_or_else(|| StageError::invalid("compressedDepth RVL delta overflow"))?;
    let depth = u16::try_from(current).map_err(|_| {
        StageError::invalid("compressedDepth RVL produced an out-of-range depth value")
    })?;
    *previous = current;
    Ok(depth)
}

/// Converts a zigzag-encoded unsigned integer back to a signed delta.
fn decode_zigzag_delta(encoded: u32) -> i32 {
    ((encoded >> 1) as i32) ^ -((encoded & 1) as i32)
}

/// Streaming decoder for the nibble-packed RVL integer stream.
///
/// ROS writes RVL data as little-endian 32-bit words. Each word contains eight
/// 4-bit nibbles, and each variable-length integer uses the low three bits for
/// payload and the high bit as the continuation flag.
struct RvlDecoder<'a> {
    data: &'a [u8],
    offset: usize,
    word: u32,
    nibbles_left: u8,
}

impl<'a> RvlDecoder<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            offset: 0,
            word: 0,
            nibbles_left: 0,
        }
    }

    /// Decodes one RVL variable-length integer.
    ///
    /// Each nibble contributes three payload bits. The top bit indicates whether
    /// another nibble follows.
    fn decode_vle(&mut self) -> Result<u32, StageError> {
        let mut value = 0u32;
        let mut shift = 0u32;

        loop {
            let nibble = self.next_nibble()?;
            let bits = u32::from(nibble & 0x7)
                .checked_shl(shift)
                .ok_or_else(|| StageError::invalid("compressedDepth RVL integer overflow"))?;
            value = value
                .checked_add(bits)
                .ok_or_else(|| StageError::invalid("compressedDepth RVL integer overflow"))?;

            if nibble & 0x8 == 0 {
                return Ok(value);
            }

            shift += 3;
            if shift >= 32 {
                return Err(StageError::invalid("compressedDepth RVL integer overflow"));
            }
        }
    }

    /// Returns the next 4-bit nibble from the current RVL stream.
    ///
    /// Nibbles are read from the most significant side of each 32-bit word to
    /// match the packing used by ROS's reference encoder.
    fn next_nibble(&mut self) -> Result<u8, StageError> {
        if self.nibbles_left == 0 {
            let word_bytes = self
                .data
                .get(self.offset..self.offset + 4)
                .ok_or_else(|| StageError::invalid("compressedDepth RVL payload truncated"))?;
            self.word =
                u32::from_le_bytes(word_bytes.try_into().expect("slice length checked above"));
            self.offset += 4;
            self.nibbles_left = 8;
        }

        let nibble = ((self.word & 0xF000_0000) >> 28) as u8;
        self.word <<= 4;
        self.nibbles_left -= 1;
        Ok(nibble)
    }
}
