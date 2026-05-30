use std::collections::HashMap;
use std::error::Error;

use camino::Utf8PathBuf;
use image::DynamicImage;

use crate::common::ImageFrame;
use crate::decode::video_decoder::SingleVideoDecoder;
use crate::encode::video_artifact::VideoArtifact;

/// A trait for providing frames for video encoding.
///
/// This abstraction allows the `VideoEncoderPipeline` to retrieve frames either from
/// memory (when available in `Context`) or by decoding them from source video files
/// on-demand (lazy loading).
pub trait FrameProvider {
    /// Retrieves a frame for the given topic and index.
    fn get_frame(
        &mut self,
        topic: &str,
        index: usize,
    ) -> Result<DynamicImage, Box<dyn Error + Send + Sync>>;
}

/// A frame provider that retrieves frames from in-memory `ImageFrame` data.
pub struct InMemoryFrameProvider<'a> {
    image_data: &'a HashMap<String, Vec<ImageFrame>>,
}

impl<'a> InMemoryFrameProvider<'a> {
    pub fn new(image_data: &'a HashMap<String, Vec<ImageFrame>>) -> Self {
        Self { image_data }
    }
}

impl<'a> FrameProvider for InMemoryFrameProvider<'a> {
    fn get_frame(
        &mut self,
        topic: &str,
        index: usize,
    ) -> Result<DynamicImage, Box<dyn Error + Send + Sync>> {
        let frames = self
            .image_data
            .get(topic)
            .ok_or_else(|| format!("image frames not found for topic: {}", topic))?;

        let frame = frames
            .get(index)
            .ok_or_else(|| format!("frame index {} out of bounds for topic: {}", index, topic))?;

        image::load_from_memory(&frame.bytes)
            .map_err(|e| Box::new(e) as Box<dyn Error + Send + Sync>)
    }
}

/// A frame provider that decodes frames from video files on-demand.
pub struct VideoFileFrameProvider {
    bundle_root: Option<Utf8PathBuf>,
    video_registry: HashMap<String, VideoArtifact>,
    decoders: HashMap<String, SingleVideoDecoder>,
}

impl VideoFileFrameProvider {
    pub fn new(
        bundle_root: Option<Utf8PathBuf>,
        video_registry: HashMap<String, VideoArtifact>,
    ) -> Self {
        Self {
            bundle_root,
            video_registry,
            decoders: HashMap::new(),
        }
    }

    fn get_decoder(
        &mut self,
        topic: &str,
    ) -> Result<&mut SingleVideoDecoder, Box<dyn Error + Send + Sync>> {
        if !self.decoders.contains_key(topic) {
            let artifact = self
                .video_registry
                .get(topic)
                .ok_or_else(|| format!("video artifact not found for topic: {}", topic))?;
            if artifact.metadata.media_type != "rgb" {
                return Err(format!(
                    "LeRobot video frame provider only supports RGB videos, got {} for topic: {}",
                    artifact.metadata.media_type, topic
                )
                .into());
            }

            let video_path = artifact
                .resolve_path(self.bundle_root.as_deref())
                .map_err(|e| format!("failed to resolve video path for topic {topic}: {e}"))?;

            if !video_path.exists() {
                return Err(format!("video file not found: {}", video_path).into());
            }

            let decoder = SingleVideoDecoder::new(&video_path)
                .map_err(|e| Box::new(e) as Box<dyn Error + Send + Sync>)?;

            self.decoders.insert(topic.to_string(), decoder);
        }

        // INVARIANT: We just inserted the decoder in the line above, so get_mut is guaranteed to succeed
        #[allow(clippy::expect_used)]
        Ok(self
            .decoders
            .get_mut(topic)
            .expect("decoder was just inserted"))
    }
}

impl FrameProvider for VideoFileFrameProvider {
    fn get_frame(
        &mut self,
        topic: &str,
        index: usize,
    ) -> Result<DynamicImage, Box<dyn Error + Send + Sync>> {
        let decoder = self.get_decoder(topic)?;

        // We use `at_index` which handles seeking and caching
        match decoder.at_index(index) {
            Ok(Some(image)) => Ok(image),
            Ok(None) => {
                Err(format!("frame index {} out of bounds for video: {}", index, topic).into())
            }
            Err(e) => Err(Box::new(e)),
        }
    }
}
