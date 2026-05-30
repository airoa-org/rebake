use std::collections::HashMap;
use std::error::Error;

use camino::{Utf8Path, Utf8PathBuf};

use crate::core::error::StageResult;
use crate::core::stage::StageError;
use crate::encode::video_encoder::{VideoEncoderConfig, VideoEncoderVariant};
use crate::schema::TopicFeatureMapEntry;
use crate::transform::lerobot_v21::video::frame_provider::FrameProvider;

#[derive(Clone, Debug)]
struct ChannelAccumulator {
    min: f64,
    max: f64,
    sum: f64,
    sum_sq: f64,
}

impl ChannelAccumulator {
    fn new() -> Self {
        Self {
            min: f64::INFINITY,
            max: f64::NEG_INFINITY,
            sum: 0.0,
            sum_sq: 0.0,
        }
    }

    fn update(&mut self, value: f64) {
        if value < self.min {
            self.min = value;
        }
        if value > self.max {
            self.max = value;
        }
        self.sum += value;
        self.sum_sq += value * value;
    }
}

#[derive(Clone, Debug)]
struct VideoStatsAccumulator {
    channels: usize,
    channels_acc: Vec<ChannelAccumulator>,
    pixel_count: u64,
    frame_count: u64,
}

impl VideoStatsAccumulator {
    fn new(channels: usize) -> Self {
        Self {
            channels,
            channels_acc: (0..channels).map(|_| ChannelAccumulator::new()).collect(),
            pixel_count: 0,
            frame_count: 0,
        }
    }

    fn update_frame(&mut self, rgb_bytes: &[u8]) {
        debug_assert_eq!(rgb_bytes.len() % self.channels, 0);
        let mut channel_index = 0;
        for &byte in rgb_bytes {
            let value = (byte as f64) / 255.0;
            self.channels_acc[channel_index].update(value);
            channel_index += 1;
            if channel_index == self.channels {
                channel_index = 0;
                self.pixel_count += 1;
            }
        }
        self.frame_count += 1;
    }

    fn finalize(self) -> Option<VideoStats> {
        if self.pixel_count == 0 {
            return None;
        }

        let pixel_count = self.pixel_count as f64;
        let mut min = Vec::with_capacity(self.channels);
        let mut max = Vec::with_capacity(self.channels);
        let mut mean = Vec::with_capacity(self.channels);
        let mut std = Vec::with_capacity(self.channels);

        for acc in self.channels_acc {
            min.push(acc.min);
            max.push(acc.max);
            let mean_value = acc.sum / pixel_count;
            mean.push(mean_value);
            let variance = (acc.sum_sq / pixel_count) - mean_value.powi(2);
            std.push(variance.max(0.0).sqrt());
        }

        Some(VideoStats {
            min,
            max,
            mean,
            std,
            frame_count: self.frame_count,
        })
    }
}

#[derive(Clone, Debug)]
pub struct VideoStats {
    pub min: Vec<f64>,
    pub max: Vec<f64>,
    pub mean: Vec<f64>,
    pub std: Vec<f64>,
    pub frame_count: u64,
}

struct VideoTopicContext {
    encoder: VideoEncoderVariant,
    stats: Option<VideoStatsAccumulator>,
    feature: String,
}

impl VideoTopicContext {
    fn new(base_dir: Utf8PathBuf, feature: String, config: VideoEncoderConfig) -> Self {
        Self {
            encoder: VideoEncoderVariant::from_config(&base_dir, config),
            stats: None,
            feature,
        }
    }
}

pub struct VideoEncoderPipeline {
    contexts: HashMap<String, VideoTopicContext>,
}

impl VideoEncoderPipeline {
    /// Create a new VideoEncoderPipeline for episode 0 (SHT mode).
    pub fn new(
        outdir: &Utf8Path,
        video_config: VideoEncoderConfig,
        entries: &[TopicFeatureMapEntry],
    ) -> Self {
        Self::new_with_episode_id(outdir, video_config, entries, 0)
    }

    /// Create a new VideoEncoderPipeline for a specific episode (PA mode).
    /// Each episode gets its own video file: episode_{episode_id:06d}.mp4
    pub fn new_with_episode_id(
        outdir: &Utf8Path,
        video_config: VideoEncoderConfig,
        entries: &[TopicFeatureMapEntry],
        episode_id: usize,
    ) -> Self {
        let mut contexts = HashMap::new();
        for entry in entries {
            if let TopicFeatureMapEntry::Video { topic, feature, .. } = entry {
                let base_dir = Self::video_output_path_with_episode_id(outdir, feature, episode_id);
                contexts.insert(
                    topic.clone(),
                    VideoTopicContext::new(base_dir, feature.clone(), video_config.clone()),
                );
            }
        }

        Self { contexts }
    }

    /// Encode video frames for a topic.
    ///
    /// # Errors
    ///
    /// Returns `StageError::MissingData` if no video context exists for the topic.
    /// Returns an error if frame retrieval or encoding fails.
    pub fn encode(
        &mut self,
        topic: &str,
        indices: &[usize],
        frame_provider: &mut dyn FrameProvider,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        let topic_context = self
            .contexts
            .get_mut(topic)
            .ok_or_else(|| StageError::missing(format!("video context for topic '{}'", topic)))?;

        for &index in indices {
            let rgb_img = frame_provider.get_frame(topic, index)?;
            // Note: get_frame returns DynamicImage, which we can convert to RGB8
            let rgb_img = rgb_img.to_rgb8();

            if topic_context.stats.is_none() {
                // Initialize stats. We use 3 channels because we convert all frames to RGB8.
                let channels = 3;
                topic_context.stats = Some(VideoStatsAccumulator::new(channels));
            }

            // Stats is guaranteed to be Some here due to the initialization above
            if let Some(stats) = topic_context.stats.as_mut() {
                stats.update_frame(rgb_img.as_raw());
            }

            topic_context
                .encoder
                .add_frame(&image::DynamicImage::ImageRgb8(rgb_img))?;
        }

        Ok(())
    }

    pub fn contains_topic(&self, topic: &str) -> bool {
        self.contexts.contains_key(topic)
    }

    /// Finalize all video encoders and return statistics for each feature.
    ///
    /// # Errors
    ///
    /// Returns `StageError::External` if video encoding finalization fails.
    pub fn finalize(mut self) -> StageResult<HashMap<String, VideoStats>> {
        for (topic, context) in self.contexts.iter_mut() {
            context.encoder.finish().map_err(|e| {
                StageError::external(
                    format!("video encoding finalization failed for '{}'", topic),
                    e,
                )
            })?;
        }

        let mut stats = HashMap::new();
        for (_, context) in self.contexts.into_iter() {
            if let Some(stats_acc) = context.stats
                && let Some(entry) = stats_acc.finalize()
            {
                stats.insert(context.feature.clone(), entry);
            }
        }

        Ok(stats)
    }

    fn video_output_path_with_episode_id(
        outdir: &Utf8Path,
        feature: &str,
        episode_id: usize,
    ) -> Utf8PathBuf {
        outdir
            .join("videos")
            .join("chunk-000")
            .join(feature)
            .join(format!("episode_{:06}.mp4", episode_id))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::encode::video_encoder::SoftwareVideoEncoder;
    use crate::transform::lerobot_v21::video::frame_provider::VideoFileFrameProvider;
    use serial_test::serial;

    #[test]
    #[serial(ffmpeg)]
    fn test_lazy_video_reencoding() {
        let temp_dir = tempfile::tempdir().unwrap();
        let source_dir = temp_dir.path().join("source");
        let output_dir = temp_dir.path().join("output");
        std::fs::create_dir_all(&source_dir).unwrap();
        std::fs::create_dir_all(&output_dir).unwrap();

        let source_dir_utf8 = Utf8PathBuf::from_path_buf(source_dir.clone()).unwrap();
        let output_dir_utf8 = Utf8PathBuf::from_path_buf(output_dir.clone()).unwrap();

        // 1. Create a source video
        let topic = "/camera/image_raw";
        let feature = "observation.image";
        // VideoFileFrameProvider expects video at base_dir + relative_topic + .mp4
        // e.g. source_dir/camera/image_raw.mp4
        let relative_topic = "camera/image_raw";
        let source_video_path = source_dir_utf8.join(format!("{}.mp4", relative_topic));

        let config = VideoEncoderConfig::new(30);
        let mut encoder = SoftwareVideoEncoder::new(&source_video_path, config.clone());

        // Create 10 frames of red color
        for _ in 0..10 {
            let img = image::RgbImage::from_pixel(64, 64, image::Rgb([255, 0, 0]));
            let dynamic_img = image::DynamicImage::ImageRgb8(img);
            encoder.add_frame(&dynamic_img).unwrap();
        }
        encoder.finish().unwrap();

        // 2. Setup Pipeline with VideoFileFrameProvider
        let entries = vec![TopicFeatureMapEntry::Video {
            topic: topic.to_string(),
            feature: feature.to_string(),
            names: None,
            description: None,
        }];

        let mut pipeline = VideoEncoderPipeline::new(&output_dir_utf8, config, &entries);

        let mut video_registry = HashMap::new();
        video_registry.insert(
            topic.to_string(),
            VideoEncoderConfig::new(30)
                .video_artifact(source_video_path.as_str(), 64, 64)
                .unwrap(),
        );
        let mut provider = VideoFileFrameProvider::new(None, video_registry);

        // 3. Encode (Re-encode)
        // We want to encode indices 0, 2, 4, 6, 8
        let indices = vec![0, 2, 4, 6, 8];
        pipeline
            .encode(topic, &indices, &mut provider)
            .expect("encoding failed");

        let stats = pipeline.finalize().unwrap();

        // 4. Verify
        assert!(stats.contains_key(feature));
        let stat = stats.get(feature).unwrap();
        assert_eq!(stat.frame_count, 5);

        // Check if output video exists
        // VideoEncoderPipeline output path: output_dir/videos/chunk-000/{feature}/episode_000000.mp4
        let output_video_path = output_dir_utf8
            .join("videos")
            .join("chunk-000")
            .join(feature)
            .join("episode_000000.mp4");

        assert!(output_video_path.exists());
    }
}
