use std::collections::HashMap;

use polars::prelude::*;

pub use crate::common::topic_name_to_relative_path;
use crate::common::{ImageFrame, ImageShape, infer_image_shape_from_bytes};

// CONTRACT: rsplit('/').next() always returns Some for non-empty iterator
#[allow(clippy::unwrap_used)]
pub fn is_compressed_image_topic(topic_name: &str) -> bool {
    topic_name.rsplit('/').next().unwrap() == "compressed"
}

// CONTRACT: rsplit('/').next() always returns Some for non-empty iterator
#[allow(clippy::unwrap_used)]
pub fn is_compressed_depth_topic(topic_name: &str) -> bool {
    topic_name.rsplit('/').next().unwrap() == "compressedDepth"
}

/// Returns a default extension for raw images (always "jpg" since they are JPEG-encoded).
pub fn get_raw_image_extension() -> &'static str {
    "jpg"
}

/// Normalizes ROS CompressedImage format string to a file extension.
///
/// ROS CompressedImage format field can contain values like:
/// - "jpeg", "jpg" -> "jpg"
/// - "png" -> "png"
/// - "webp" -> "webp"
/// - "rgb8; jpeg compressed" -> "jpg" (format with encoding suffix)
/// - "bgr8; png compressed" -> "png"
///
/// Unknown formats fall back to "bin".
pub fn normalize_ros_format(format: &str) -> String {
    let format_lower = format.to_lowercase();

    // Handle formats like "rgb8; jpeg compressed" or "bgr8; png compressed"
    if format_lower.contains("jpeg") || format_lower.contains("jpg") {
        return "jpg".to_string();
    }
    if format_lower.contains("png") {
        return "png".to_string();
    }
    if format_lower.contains("webp") {
        return "webp".to_string();
    }

    // Direct format match
    match format_lower.as_str() {
        "jpeg" | "jpg" => "jpg".to_string(),
        "png" => "png".to_string(),
        "webp" => "webp".to_string(),
        // Unknown format - treat as binary
        _ => "bin".to_string(),
    }
}

pub fn is_point_cloud2_type(type_name: &str) -> bool {
    type_name.ends_with("PointCloud2")
}

pub fn is_raw_image_type(type_name: &str) -> bool {
    type_name.ends_with("Image") && !type_name.ends_with("CompressedImage")
}

pub fn is_depth_image_topic(topic_name: &str) -> bool {
    topic_name.contains("depth")
}

/// Computes image shapes from camera_info topics for each image topic.
pub fn compute_image_shapes(
    dataset: &HashMap<String, LazyFrame>,
    image_data: &HashMap<String, Vec<ImageFrame>>,
) -> HashMap<String, ImageShape> {
    let mut shapes = HashMap::new();
    for topic in image_data.keys() {
        let camera_info_topic = camera_info_topic_for(topic);
        if let Some(camera_info_frame) = dataset.get(&camera_info_topic)
            && let Some(shape) = extract_shape(camera_info_frame, infer_channel_count(topic))
        {
            shapes.insert(topic.clone(), shape);
        }
    }
    shapes
}

/// Infer per-topic image shapes directly from encoded image payloads.
///
/// This is the preferred source for image dimensions because it does not depend
/// on optional `camera_info` topics being recorded alongside the images.
pub fn infer_image_topic_shapes_from_payload(
    image_data: &HashMap<String, Vec<ImageFrame>>,
) -> HashMap<String, ImageShape> {
    let mut shapes = HashMap::new();

    for (topic, frames) in image_data {
        let channels = infer_channel_count(topic) as usize;
        if let Some(shape) = frames
            .iter()
            .find_map(|frame| infer_image_shape_from_bytes(&frame.bytes, channels))
        {
            shapes.insert(topic.clone(), shape);
        }
    }

    shapes
}

/// Derives the camera_info topic name from an image topic name.
///
/// For example, `/hsrb/hand_camera/image_raw/compressed` becomes `/hsrb/hand_camera/camera_info`.
pub fn camera_info_topic_for(image_topic: &str) -> String {
    let mut parts: Vec<&str> = image_topic.trim_start_matches('/').split('/').collect();
    if parts.len() >= 2 {
        parts.pop();
        parts.pop();
    }
    format!("/{}/camera_info", parts.join("/"))
}

/// Extracts image shape (height, width, channels) from a camera_info LazyFrame.
pub fn extract_shape(camera_info_frame: &LazyFrame, channels: u8) -> Option<ImageShape> {
    let df = camera_info_frame
        .clone()
        .select([col("height"), col("width")])
        .limit(1)
        .collect()
        .ok()?;

    let height = df.column("height").ok()?.u32().ok()?.get(0)?;
    let width = df.column("width").ok()?.u32().ok()?.get(0)?;

    Some(ImageShape::new(
        height as usize,
        width as usize,
        channels as usize,
    ))
}

/// Infers the number of color channels from the topic name.
///
/// Returns 1 for depth images (compressedDepth), 3 for color images.
pub fn infer_channel_count(topic: &str) -> u8 {
    if topic.ends_with("compressedDepth") {
        1
    } else {
        3
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use image::{DynamicImage, ImageBuffer, ImageFormat, Rgb};

    #[test]
    fn infer_image_topic_shapes_from_payload_works_without_camera_info() {
        let image = ImageBuffer::from_pixel(8, 6, Rgb([10_u8, 20, 30]));
        let mut bytes = std::io::Cursor::new(Vec::new());
        DynamicImage::ImageRgb8(image)
            .write_to(&mut bytes, ImageFormat::Jpeg)
            .unwrap();

        let image_data = HashMap::from([(
            "/robot1/camera/world/compressed".to_string(),
            vec![ImageFrame::new(0_u32, "jpg", bytes.into_inner())],
        )]);

        let shapes = infer_image_topic_shapes_from_payload(&image_data);
        let shape = shapes.get("/robot1/camera/world/compressed").unwrap();
        assert_eq!(*shape, ImageShape::new(6, 8, 3));
    }
}
