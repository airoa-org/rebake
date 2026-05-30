use camino::{Utf8Path, Utf8PathBuf};

use crate::common::ImageShape;
use crate::core::stage::StageError;

use serde::{Deserialize, Serialize};

/// Canonical metadata for a single encoded video.
///
/// This captures the stable semantic summary of an encoded video independently
/// from where the file currently lives. The path is intentionally excluded so
/// callers can move artifacts between local cache, export directories, and
/// object storage without rebuilding the metadata.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct VideoMetadata {
    pub media_type: String,
    pub codec_family: String,
    pub encoder_name: String,
    pub pix_fmt: String,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub encoding_config_json: String,
}

impl VideoMetadata {
    /// Convert canonical video metadata into rebake's image shape representation.
    pub fn image_shape(&self) -> Result<ImageShape, StageError> {
        if self.width == 0 || self.height == 0 {
            return Err(StageError::invalid(
                "video metadata requires positive width and height",
            ));
        }

        let channels = match self.media_type.as_str() {
            "rgb" => 3,
            "depth" => 1,
            other => {
                return Err(StageError::invalid(format!(
                    "unsupported media_type in video metadata: {other}"
                )));
            }
        };

        Ok(ImageShape::new(
            self.height as usize,
            self.width as usize,
            channels,
        ))
    }
}

/// Encoded video file plus its canonical metadata.
///
/// This is the natural boundary object for callers that need both the file path
/// and the semantic summary of the encoded video.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct VideoArtifact {
    pub video_path: String,
    pub metadata: VideoMetadata,
}

impl VideoArtifact {
    /// Convert the artifact's canonical metadata into rebake's image shape representation.
    pub fn image_shape(&self) -> Result<ImageShape, StageError> {
        self.metadata.image_shape()
    }

    /// Resolve this artifact's current local path.
    ///
    /// Relative artifact paths are interpreted against the provided bundle root.
    pub fn resolve_path(&self, bundle_root: Option<&Utf8Path>) -> Result<Utf8PathBuf, StageError> {
        let artifact_path = Utf8Path::new(&self.video_path);
        if artifact_path.is_absolute() {
            return Ok(artifact_path.to_path_buf());
        }

        let bundle_root = bundle_root.ok_or_else(|| {
            StageError::missing("bundle_root in context for relative video artifact path")
        })?;
        Ok(bundle_root.join(artifact_path))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn video_metadata_image_shape_supports_rgb_and_depth() {
        let rgb = VideoMetadata {
            media_type: "rgb".to_string(),
            codec_family: "av1".to_string(),
            encoder_name: "libsvtav1".to_string(),
            pix_fmt: "yuv420p".to_string(),
            width: 640,
            height: 480,
            fps: 30,
            encoding_config_json: "{}".to_string(),
        };
        let depth = VideoMetadata {
            media_type: "depth".to_string(),
            codec_family: "ffv1".to_string(),
            encoder_name: "ffv1".to_string(),
            pix_fmt: "gray16le".to_string(),
            width: 848,
            height: 480,
            fps: 30,
            encoding_config_json: "{}".to_string(),
        };

        assert_eq!(rgb.image_shape().unwrap(), ImageShape::new(480, 640, 3));
        assert_eq!(depth.image_shape().unwrap(), ImageShape::new(480, 848, 1));
    }

    #[test]
    fn video_metadata_image_shape_rejects_invalid_media_type() {
        let metadata = VideoMetadata {
            media_type: "segmentation".to_string(),
            codec_family: "av1".to_string(),
            encoder_name: "encoder".to_string(),
            pix_fmt: "yuv420p".to_string(),
            width: 640,
            height: 480,
            fps: 30,
            encoding_config_json: "{}".to_string(),
        };

        let error = metadata.image_shape().unwrap_err();
        assert!(error.reason().contains("unsupported media_type"));
    }
}
