use crate::core::stage::StageError;

/// Structured variants of ROS `compressedDepth` transport.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompressedDepthFormat {
    Depth16Png,
    Depth16Rvl,
    Depth32FPng,
}

impl CompressedDepthFormat {
    /// Parses the ROS `CompressedImage.format` string for a compressedDepth topic.
    ///
    /// Examples:
    ///
    /// - `16UC1; compressedDepth png`
    /// - `16UC1; compressedDepth rvl`
    /// - `32FC1; compressedDepth png`
    ///
    /// Older payloads may omit the compression suffix; those default to `png`.
    pub fn parse(ros_format: &str) -> Result<Self, StageError> {
        let normalized = ros_format.trim().to_ascii_lowercase();
        let (encoding, transport_suffix) = split_format_components(&normalized);
        let transport_suffix = normalize_transport_suffix(transport_suffix);

        match (encoding, transport_suffix) {
            ("16uc1", "compresseddepth png") => Ok(Self::Depth16Png),
            ("16uc1", "compresseddepth rvl") => Ok(Self::Depth16Rvl),
            ("32fc1", "compresseddepth png") => Ok(Self::Depth32FPng),
            ("16uc1", other) => Err(StageError::invalid(format!(
                "unsupported compressedDepth codec for 16UC1: {other} (from {ros_format})"
            ))),
            ("32fc1", other) => Err(StageError::invalid(format!(
                "unsupported compressedDepth codec for 32FC1: {other} (from {ros_format})"
            ))),
            (other, _) => Err(StageError::invalid(format!(
                "unsupported compressedDepth encoding '{other}' in format: {ros_format}"
            ))),
        }
    }
}

/// Splits a normalized ROS format string into encoding and transport parts.
fn split_format_components(normalized_format: &str) -> (&str, &str) {
    match normalized_format.split_once(';') {
        Some((encoding, compression)) => (encoding.trim(), compression.trim()),
        None => (normalized_format, "compresseddepth png"),
    }
}

/// Normalizes historical `CompressedImage.format` variations to one of the
/// transport strings that rebake actually supports.
fn normalize_transport_suffix(transport_suffix: &str) -> &str {
    match transport_suffix {
        "" | "compresseddepth" => "compresseddepth png",
        "compresseddepth png" => "compresseddepth png",
        "compresseddepth rvl" => "compresseddepth rvl",
        other if !other.contains("compresseddepth") => "compresseddepth png",
        other => other,
    }
}
