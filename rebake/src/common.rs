use std::collections::HashMap;

use camino::{Utf8Path, Utf8PathBuf};
use polars::prelude::{LazyFrame, PlPath};
use walkdir::WalkDir;

use crate::core::stage::StageError;

#[derive(Clone, Debug)]
pub struct ImageFrame {
    pub index: u32,
    pub extension: String,
    pub bytes: Vec<u8>,
    pub shape: Option<ImageShape>,
}

impl ImageFrame {
    pub fn new<I, E>(index: I, extension: E, bytes: Vec<u8>) -> Self
    where
        I: Into<u32>,
        E: Into<String>,
    {
        Self {
            index: index.into(),
            extension: extension.into(),
            bytes,
            shape: None,
        }
    }

    /// Stores the decoded image shape on this frame.
    pub fn set_shape(&mut self, shape: ImageShape) {
        self.shape = Some(shape);
    }
}

#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct ImageShape {
    pub height: usize,
    pub width: usize,
    pub channels: usize,
}

impl ImageShape {
    pub fn new(height: usize, width: usize, channels: usize) -> Self {
        Self {
            height,
            width,
            channels,
        }
    }

    /// Returns the shape as a simple `[height, width, channels]` vector.
    pub fn to_vec(&self) -> Vec<usize> {
        vec![self.height, self.width, self.channels]
    }
}

/// Infer image dimensions from encoded image bytes.
///
/// The `channels` parameter reflects rebake's semantic representation of the
/// visual stream (typically RGB) rather than the source file's native channel
/// count. This keeps downstream metadata aligned with how video pipelines
/// consume image frames.
pub fn infer_image_shape_from_bytes(bytes: &[u8], channels: usize) -> Option<ImageShape> {
    let image = image::load_from_memory(bytes).ok()?;
    Some(ImageShape::new(
        image.height() as usize,
        image.width() as usize,
        channels,
    ))
}

/// Resolve a topic's image shape, preferring topic-level shapes over per-frame data.
pub fn resolve_image_shape(
    topic: &str,
    frames: Option<&[ImageFrame]>,
    image_topic_shapes: Option<&HashMap<String, ImageShape>>,
) -> Option<ImageShape> {
    image_topic_shapes
        .and_then(|shapes| shapes.get(topic).copied())
        .or_else(|| {
            frames.and_then(|topic_frames| topic_frames.iter().find_map(|frame| frame.shape))
        })
}

/// Resolve a topic's image shape, falling back to decoding image payloads if needed.
///
/// This helper centralizes rebake's RGB image shape policy:
/// 1. Prefer topic-level `image_topic_shapes`
/// 2. Fall back to per-frame cached `ImageFrame.shape`
/// 3. As a last resort, decode the first readable payload
pub fn resolve_or_infer_image_shape(
    topic: &str,
    frames: &[ImageFrame],
    image_topic_shapes: Option<&HashMap<String, ImageShape>>,
    channels: usize,
) -> Option<ImageShape> {
    resolve_image_shape(topic, Some(frames), image_topic_shapes).or_else(|| {
        frames
            .iter()
            .find_map(|frame| infer_image_shape_from_bytes(&frame.bytes, channels))
    })
}

#[derive(Clone, Debug)]
pub struct DepthFrame {
    pub index: u32,
    pub extension: String,
    pub bytes: Vec<u8>,
    pub ros_format: Option<String>,
}

impl DepthFrame {
    pub fn new<I, E>(index: I, extension: E, bytes: Vec<u8>) -> Self
    where
        I: Into<u32>,
        E: Into<String>,
    {
        Self {
            index: index.into(),
            extension: extension.into(),
            bytes,
            ros_format: None,
        }
    }

    /// Stores the original ROS `CompressedImage.format` string on this frame.
    pub fn set_ros_format(&mut self, ros_format: impl Into<String>) {
        self.ros_format = Some(ros_format.into());
    }
}

#[derive(Clone, Debug)]
pub struct PointCloudFrame {
    pub index: u32,
    pub extension: String,
    pub bytes: Vec<u8>,
}

impl PointCloudFrame {
    pub fn new<I, E>(index: I, extension: E, bytes: Vec<u8>) -> Self
    where
        I: Into<u32>,
        E: Into<String>,
    {
        Self {
            index: index.into(),
            extension: extension.into(),
            bytes,
        }
    }
}

/// Extracts the short message type name from a full type name.
///
/// # Example
/// `sensor_msgs/JointState` -> `JointState`
pub fn extract_short_type_name(full_type_name: &str) -> &str {
    full_type_name.rsplit('/').next().unwrap_or(full_type_name)
}

/// Converts a topic name to a relative file path from the output directory.
///
/// # Example
/// `/sensor_msgs/JointState` -> `sensor_msgs/JointState.parquet`
pub fn topic_name_to_relative_path(topic_name: &str) -> String {
    let trimmed_topic_name = topic_name.trim_start_matches('/');
    format!("{}.parquet", trimmed_topic_name)
}

/// Converts a topic name to rebake's flat export file stem.
///
/// This is the stable naming rule used by the Parquet/video intermediate format.
///
/// # Example
/// `/camera/rgb/image_raw` -> `camera__rgb__image_raw`
pub fn topic_name_to_flat_file_stem(topic_name: &str) -> String {
    topic_name.trim_start_matches('/').replace('/', "__")
}

/// Recursively collects file paths relative to dir_path with the specified extension.
/// Returns paths without the file extension.
pub fn get_file_paths_from_dir<P: AsRef<Utf8Path>>(
    dir_path: P,
    extension: &str,
) -> Vec<Utf8PathBuf> {
    let dir_path_ref = dir_path.as_ref();
    WalkDir::new(dir_path_ref.as_std_path())
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_type().is_file()
                && e.path().extension().and_then(|s| s.to_str()) == Some(extension)
        })
        .filter_map(|e| {
            e.path()
                .strip_prefix(dir_path_ref.as_std_path())
                .ok()
                .and_then(|p| Utf8PathBuf::try_from(p.to_path_buf()).ok())
                .map(|mut p| {
                    p.set_extension("");
                    p
                })
        })
        .collect()
}

/// Loads parquet frames from a directory.
///
/// Returns a HashMap where keys are relative paths from dir_path without extensions,
/// prefixed with `/` to match ROS topic naming conventions.
///
/// # Errors
///
/// Returns `StageError::External` if the parquet scan initialization fails
/// (e.g., invalid path format). Note that file content validation is lazy
/// (Polars `scan_parquet` behavior) and errors may occur when the returned
/// `LazyFrame` is collected. Use [`crate::synchronize::utils::scan_parquet_frames`]
/// for eager validation that filters out unreadable files.
pub fn load_parquet_frames<P: AsRef<Utf8Path>>(
    dir_path: P,
) -> Result<HashMap<String, LazyFrame>, StageError> {
    let dir_path_ref = dir_path.as_ref();
    let mut result = HashMap::new();

    for relative_path_without_ext in get_file_paths_from_dir(dir_path_ref, "parquet") {
        let mut full_path = dir_path_ref.join(&relative_path_without_ext);
        full_path.set_extension("parquet");
        let lazy_frame =
            LazyFrame::scan_parquet(PlPath::new(full_path.as_str()), Default::default()).map_err(
                |e| StageError::external(format!("failed to read parquet file: {}", full_path), e),
            )?;
        result.insert(format!("/{}", relative_path_without_ext), lazy_frame);
    }

    Ok(result)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use image::{DynamicImage, ImageBuffer, ImageFormat, Rgb};
    use tempfile::tempdir;

    // Tests that invalid parquet files fail only when the lazy frame is collected.
    #[test]
    fn load_parquet_frames_returns_lazyframes_that_fail_on_invalid_file() {
        let temp_dir = tempdir().unwrap();
        let temp_path = temp_dir.path();
        let invalid_parquet = temp_path.join("invalid.parquet");

        // Create an invalid parquet file (just some random bytes)
        std::fs::write(&invalid_parquet, b"not a valid parquet file").unwrap();

        let utf8_path = Utf8Path::from_path(temp_path).unwrap();
        // Note: scan_parquet is lazy, so load_parquet_frames succeeds
        // but collecting the LazyFrame should fail
        let result = load_parquet_frames(utf8_path).unwrap();

        assert_eq!(result.len(), 1);
        let (topic, frame) = result.into_iter().next().unwrap();
        assert_eq!(topic, "/invalid");

        // Collecting the LazyFrame should fail because the file is not valid parquet
        let collect_result = frame.collect();
        assert!(
            collect_result.is_err(),
            "Expected error when collecting invalid parquet, but got Ok"
        );
    }

    // Tests that loading from an empty directory returns no frames.
    #[test]
    fn load_parquet_frames_returns_empty_for_empty_dir() {
        let temp_dir = tempdir().unwrap();
        let utf8_path = Utf8Path::from_path(temp_dir.path()).unwrap();

        let result = load_parquet_frames(utf8_path).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn infer_image_shape_from_bytes_reads_dimensions() {
        let image = ImageBuffer::from_pixel(4, 3, Rgb([12_u8, 34, 56]));
        let mut bytes = std::io::Cursor::new(Vec::new());
        DynamicImage::ImageRgb8(image)
            .write_to(&mut bytes, ImageFormat::Png)
            .unwrap();

        let shape = infer_image_shape_from_bytes(&bytes.into_inner(), 3).unwrap();
        assert_eq!(shape, ImageShape::new(3, 4, 3));
    }

    #[test]
    fn resolve_image_shape_prefers_topic_shapes() {
        let frame_shape = ImageShape::new(10, 20, 3);
        let metadata_shape = ImageShape::new(30, 40, 3);
        let frames = vec![ImageFrame {
            index: 0,
            extension: "jpg".to_string(),
            bytes: vec![1, 2, 3],
            shape: Some(frame_shape),
        }];
        let shapes = HashMap::from([("/camera".to_string(), metadata_shape)]);

        let resolved = resolve_image_shape("/camera", Some(&frames), Some(&shapes));
        assert_eq!(resolved, Some(metadata_shape));
    }

    #[test]
    fn resolve_or_infer_image_shape_decodes_payload_when_metadata_missing() {
        let image = ImageBuffer::from_pixel(7, 5, Rgb([12_u8, 34, 56]));
        let mut bytes = std::io::Cursor::new(Vec::new());
        DynamicImage::ImageRgb8(image)
            .write_to(&mut bytes, ImageFormat::Jpeg)
            .unwrap();

        let frames = vec![ImageFrame::new(0_u32, "jpg", bytes.into_inner())];

        let resolved = resolve_or_infer_image_shape("/camera", &frames, None, 3);
        assert_eq!(resolved, Some(ImageShape::new(5, 7, 3)));
    }

    #[test]
    fn topic_name_to_flat_file_stem_flattens_slashes() {
        assert_eq!(
            topic_name_to_flat_file_stem("/camera/rgb/image_raw"),
            "camera__rgb__image_raw"
        );
        assert_eq!(
            topic_name_to_flat_file_stem("camera/rgb/image_raw"),
            "camera__rgb__image_raw"
        );
        assert_eq!(
            topic_name_to_flat_file_stem("/joint_states"),
            "joint_states"
        );
    }
}
