use std::collections::HashMap;
use std::io;
use std::time::Duration;

use crate::common::{ImageFrame, ImageShape};
use crate::core::stage::{Context, Stage, StageConfig, StageError};
use crate::encode::depth_quantizer::Q10ClipParams;
use crate::encode::depth_video_encoder::{DepthCodecConfig, DepthVideoConfig};
use crate::encode::video_artifact::VideoArtifact;
use camino::{Utf8Path, Utf8PathBuf};
use ffmpeg_next as ffmpeg;
use ffmpeg_next::format::Pixel;
use ffmpeg_next::software::scaling::{context::Context as Scaler, flag::Flags};
use image::DynamicImage;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum VideoDecoderError {
    #[error("ffmpeg error: {0}")]
    Ffmpeg(#[from] ffmpeg::Error),
    #[error("io error: {0}")]
    Io(#[from] io::Error),
    #[error("video stream not found")]
    StreamNotFound,
    #[error("decoder not found")]
    DecoderNotFound,
    #[error("failed to create scaler")]
    ScalerCreationError,
    #[error("frame not found at index {0}")]
    FrameNotFound(usize),
    #[error("decoded frame is missing PTS (presentation timestamp)")]
    MissingPts,
    #[error("depth frame conversion failed: {0}")]
    DepthConversion(String),
}

/// Determines how decoded video frames are converted to output images.
///
/// - `Rgb`: Uses FFmpeg's swscale to convert to RGB24. Returns `DynamicImage::ImageRgb8`.
/// - `Depth`: Reads depth frames according to the underlying depth storage format.
pub enum OutputMode {
    /// Scale decoded frames to RGB24 using FFmpeg's swscale.
    Rgb(Scaler),
    /// Convert decoded depth frames to 16-bit millimeter values.
    Depth(DepthOutputMode),
}

/// Determines how decoded depth video frames are converted.
pub enum DepthOutputMode {
    /// Extract depth from Y-plane of 10-bit video and dequantize to millimeters.
    Q10(Q10ClipParams),
    /// Extract raw 16-bit depth values from a gray16 frame.
    RawGray16,
}

enum OutputConfig {
    Rgb,
    Depth(DepthOutputMode),
}

/// Extracts a required PTS value from an optional PTS.
///
/// # Errors
///
/// Returns `VideoDecoderError::MissingPts` if the PTS is `None`.
fn require_pts(pts: Option<i64>) -> Result<i64, VideoDecoderError> {
    pts.ok_or(VideoDecoderError::MissingPts)
}

/// A decoder that extracts frames from a video file.
///
/// This struct hides the complexity of FFmpeg's seeking and decoding mechanisms.
/// It provides a simple interface to access frames by index or timestamp.
///
/// # Sequential Access Optimization
///
/// When frames are accessed sequentially (0, 1, 2, ...), the decoder avoids
/// seeking and instead continues decoding from the current position. This provides
/// a significant performance improvement for bulk decoding operations.
pub struct SingleVideoDecoder {
    input: ffmpeg::format::context::Input,
    decoder: ffmpeg::decoder::Video,
    output_mode: OutputMode,
    stream_index: usize,
    time_base: ffmpeg::Rational,
    duration: i64,
    fps: f64,
    /// Cache of the last decoded frame and its index.
    /// Used to optimize repeated access to the same frame and sequential access.
    last_frame_cache: Option<(usize, DynamicImage)>,
    /// The PTS of the last decoded frame. Used to determine if we can continue
    /// sequential decoding without seeking.
    last_decoded_pts: Option<i64>,
}

impl SingleVideoDecoder {
    /// Opens a video file and initializes the decoder for RGB output.
    pub fn new(path: &Utf8Path) -> Result<Self, VideoDecoderError> {
        Self::open(path, OutputConfig::Rgb)
    }

    /// Opens a video file and initializes the decoder for depth output.
    ///
    /// Depth frames are returned as `DynamicImage::ImageLuma16` containing
    /// dequantized depth values in millimeters. The `depth_max_mm` parameter
    /// must match the value used during encoding (Q10Clip4 quantization).
    pub fn new_depth(path: &Utf8Path, depth_max_mm: u16) -> Result<Self, VideoDecoderError> {
        if depth_max_mm == 0 {
            return Err(VideoDecoderError::DepthConversion(
                "depth_max_mm must be greater than 0 for Q10 depth video decoding".to_string(),
            ));
        }
        Self::open(
            path,
            OutputConfig::Depth(DepthOutputMode::Q10(Q10ClipParams::new(depth_max_mm))),
        )
    }

    /// Opens a video file and initializes the decoder for raw gray16 depth output.
    ///
    /// This is used for FFV1 depth artifacts, which store millimeter values as
    /// raw `gray16le` frames without Q10 quantization.
    pub fn new_depth_raw_gray16(path: &Utf8Path) -> Result<Self, VideoDecoderError> {
        Self::open(path, OutputConfig::Depth(DepthOutputMode::RawGray16))
    }

    /// Common video file opening logic.
    ///
    /// RGB output creates a scaler; depth output reads the decoded frame's
    /// first plane directly.
    fn open(path: &Utf8Path, output_config: OutputConfig) -> Result<Self, VideoDecoderError> {
        ffmpeg::init().map_err(VideoDecoderError::Ffmpeg)?;

        let input = ffmpeg::format::input(&path).map_err(VideoDecoderError::Ffmpeg)?;

        let stream = input
            .streams()
            .best(ffmpeg::media::Type::Video)
            .ok_or(VideoDecoderError::StreamNotFound)?;

        let stream_index = stream.index();
        let time_base = stream.time_base();
        let duration = stream.duration();
        let fps = stream.avg_frame_rate();
        let fps_f64 = fps.numerator() as f64 / fps.denominator() as f64;

        let context = ffmpeg::codec::context::Context::from_parameters(stream.parameters())
            .map_err(VideoDecoderError::Ffmpeg)?;
        let decoder = if context.id() == ffmpeg::codec::Id::AV1 {
            // Explicitly use libdav1d for AV1 decoding
            // The default "av1" decoder might fail if hardware acceleration is not available
            let codec = ffmpeg::decoder::find_by_name("libdav1d")
                .ok_or(VideoDecoderError::DecoderNotFound)?;
            context
                .decoder()
                .open_as(codec)
                .map_err(VideoDecoderError::Ffmpeg)?
                .video()
                .map_err(VideoDecoderError::Ffmpeg)?
        } else {
            context
                .decoder()
                .video()
                .map_err(VideoDecoderError::Ffmpeg)?
        };

        let output_mode = match output_config {
            OutputConfig::Rgb => {
                let scaler = Scaler::get(
                    decoder.format(),
                    decoder.width(),
                    decoder.height(),
                    Pixel::RGB24,
                    decoder.width(),
                    decoder.height(),
                    Flags::BILINEAR,
                )
                .map_err(|_| VideoDecoderError::ScalerCreationError)?;
                OutputMode::Rgb(scaler)
            }
            OutputConfig::Depth(depth_mode) => OutputMode::Depth(depth_mode),
        };

        Ok(Self {
            input,
            decoder,
            output_mode,
            stream_index,
            time_base,
            duration,
            fps: fps_f64,

            last_frame_cache: None,
            last_decoded_pts: None,
        })
    }

    /// Returns the frame at the specified index.
    ///
    /// This method automatically handles seeking to the nearest keyframe
    /// and decoding up to the target frame.
    ///
    /// # Sequential Access Optimization
    ///
    /// When frames are accessed sequentially (e.g., 0, 1, 2, ...), this method
    /// avoids seeking and continues decoding from the current position.
    /// This provides significant performance improvement for bulk decoding.
    pub fn at_index(&mut self, index: usize) -> Result<Option<DynamicImage>, VideoDecoderError> {
        // Check cache first - exact match
        if let Some((cached_index, cached_image)) = &self.last_frame_cache
            && *cached_index == index
        {
            return Ok(Some(cached_image.clone()));
        }

        // Target time in seconds
        let target_seconds = index as f64 / self.fps;

        // Convert seconds to PTS using the stream's time base
        // PTS = seconds * time_base.denominator / time_base.numerator
        let target_pts = (target_seconds * self.time_base.denominator() as f64
            / self.time_base.numerator() as f64) as i64;

        // Optimization: If we're accessing a frame ahead of the current position,
        // continue decoding without seeking (forward access optimization).
        //
        // This optimization is beneficial when:
        // - Sequential access (0, 1, 2, ...)
        // - Sparse forward access after synchronization (0, 2, 4, ... or 0, 3, 6, ...)
        //
        // The threshold is set to allow skipping up to ~10 seconds of video before
        // falling back to seeking. This balances the cost of decoding unwanted frames
        // vs the cost of seeking (which requires flushing decoder state).
        let can_continue = if let Some(last_pts) = self.last_decoded_pts {
            let frame_duration_pts = (self.time_base.denominator() as f64
                / (self.fps * self.time_base.numerator() as f64))
                as i64;
            // Allow forward jumps of up to ~100 frames (10 seconds at 10fps)
            // before falling back to seeking
            let max_forward_frames = 100;
            target_pts > last_pts
                && target_pts <= last_pts + frame_duration_pts * max_forward_frames
        } else {
            false
        };

        let result = if can_continue {
            self.continue_decode(target_pts)?
        } else {
            self.seek_and_decode(target_pts)?
        };

        if let Some(image) = &result {
            self.last_frame_cache = Some((index, image.clone()));
        }

        Ok(result)
    }

    /// Returns the frame at the specified timestamp.
    pub fn at_timestamp(
        &mut self,
        timestamp: Duration,
    ) -> Result<Option<DynamicImage>, VideoDecoderError> {
        let target_seconds = timestamp.as_secs_f64();
        let target_pts = (target_seconds * self.time_base.denominator() as f64
            / self.time_base.numerator() as f64) as i64;

        self.seek_and_decode(target_pts)
    }

    /// Returns the total number of frames (estimated).
    pub fn frame_count(&self) -> usize {
        // Estimate based on duration and FPS if exact count is not available
        // duration is in time_base units.
        // duration_seconds = duration * num / den
        // frames = duration_seconds * fps

        let duration_seconds = self.duration as f64 * self.time_base.numerator() as f64
            / self.time_base.denominator() as f64;
        (duration_seconds * self.fps).round() as usize
    }

    /// Returns the frames per second.
    pub fn fps(&self) -> f64 {
        self.fps
    }

    /// Seeks to the target PTS and decodes frames until the target is reached.
    fn seek_and_decode(
        &mut self,
        target_pts: i64,
    ) -> Result<Option<DynamicImage>, VideoDecoderError> {
        // Seek to the nearest keyframe before the target
        // AVSEEK_FLAG_BACKWARD ensures we seek to a timestamp <= target_pts
        let seek_target = target_pts.max(0);
        self.input
            .seek(seek_target, ..seek_target)
            .map_err(VideoDecoderError::Ffmpeg)?;

        // Flush the decoder buffers after seeking
        self.decoder.flush();

        // Reset state after seek
        self.last_decoded_pts = None;

        self.decode_until(target_pts)
    }

    /// Continues decoding from the current position until the target PTS is reached.
    /// This is used for sequential frame access optimization.
    fn continue_decode(
        &mut self,
        target_pts: i64,
    ) -> Result<Option<DynamicImage>, VideoDecoderError> {
        self.decode_until(target_pts)
    }

    /// Decodes frames until the target PTS is reached.
    /// Used by both seek_and_decode and continue_decode.
    ///
    /// # Errors
    ///
    /// Returns `VideoDecoderError::Ffmpeg` if FFmpeg packet sending or frame scaling fails.
    /// Returns `VideoDecoderError::MissingPts` if a decoded frame lacks a presentation timestamp.
    /// Returns `VideoDecoderError::ScalerCreationError` if RGB image creation fails.
    /// Returns `VideoDecoderError::DepthConversion` if depth frame extraction fails.
    fn decode_until(&mut self, target_pts: i64) -> Result<Option<DynamicImage>, VideoDecoderError> {
        let mut decoded_frame = ffmpeg::util::frame::video::Video::empty();

        for (stream, packet) in self.input.packets() {
            if stream.index() != self.stream_index {
                continue;
            }

            self.decoder
                .send_packet(&packet)
                .map_err(VideoDecoderError::Ffmpeg)?;

            while self.decoder.receive_frame(&mut decoded_frame).is_ok() {
                let frame_pts = require_pts(decoded_frame.pts())?;

                // If we reached or passed the target
                // Note: exact match might not happen due to floating point or time base quirks,
                // but usually PTS are integers. We look for the first frame >= target.
                if frame_pts >= target_pts {
                    // Update last decoded PTS for sequential access optimization
                    self.last_decoded_pts = Some(frame_pts);

                    let image = match &mut self.output_mode {
                        OutputMode::Rgb(scaler) => {
                            let mut rgb_frame = ffmpeg::util::frame::video::Video::empty();
                            scaler
                                .run(&decoded_frame, &mut rgb_frame)
                                .map_err(VideoDecoderError::Ffmpeg)?;

                            let width = rgb_frame.width();
                            let height = rgb_frame.height();
                            let data = rgb_frame.data(0).to_vec();

                            let img = image::RgbImage::from_raw(width, height, data)
                                .ok_or(VideoDecoderError::ScalerCreationError)?;
                            DynamicImage::ImageRgb8(img)
                        }
                        OutputMode::Depth(depth_mode) => match depth_mode {
                            DepthOutputMode::Q10(params) => {
                                decode_depth_q10_frame(&decoded_frame, params)?
                            }
                            DepthOutputMode::RawGray16 => {
                                decode_depth_raw_gray16_frame(&decoded_frame)?
                            }
                        },
                    };

                    return Ok(Some(image));
                }
            }
        }

        Ok(None)
    }
}

/// Extracts depth values from a decoded 10-bit video frame.
///
/// Reads the Y-plane of the decoded frame, extracts quantized 10-bit depth values,
/// and dequantizes them to 16-bit millimeter values using the provided Q10ClipParams.
///
/// # Pixel Format Handling
///
/// The bit extraction method depends on the decoder's output pixel format:
/// - **P010LE** (semi-planar): 10-bit value stored in upper bits → `sample >> 6`
/// - **YUV420P10LE** and other planar 10-bit formats: value in lower bits → `sample & 0x3FF`
///
/// # Stride Handling
///
/// FFmpeg frames may have stride (row byte width) larger than `width * 2` due to
/// alignment padding. This function uses `frame.stride(0)` to correctly skip padding.
fn decode_depth_q10_frame(
    frame: &ffmpeg::util::frame::video::Video,
    params: &Q10ClipParams,
) -> Result<DynamicImage, VideoDecoderError> {
    let width = frame.width() as usize;
    let height = frame.height() as usize;
    let y_data = frame.data(0);
    let stride = frame.stride(0);
    let format = frame.format();

    // P010LE stores 10-bit values in the upper bits of a 16-bit word (value << 6).
    // Planar 10-bit formats (YUV420P10LE, etc.) store values in the lower 10 bits.
    let is_p010 = format == Pixel::P010LE || format == Pixel::P010BE;

    let mut depth_values = Vec::with_capacity(width * height);
    for row in 0..height {
        for col in 0..width {
            let offset = row * stride + col * 2;
            if offset + 1 >= y_data.len() {
                return Err(VideoDecoderError::DepthConversion(format!(
                    "Y-plane data too short: need offset {} but length is {}",
                    offset + 1,
                    y_data.len()
                )));
            }
            let sample = u16::from_le_bytes([y_data[offset], y_data[offset + 1]]);
            let q10 = if is_p010 { sample >> 6 } else { sample & 0x3FF };
            depth_values.push(params.dequantize(q10));
        }
    }

    let img = image::ImageBuffer::<image::Luma<u16>, Vec<u16>>::from_raw(
        width as u32,
        height as u32,
        depth_values,
    )
    .ok_or_else(|| {
        VideoDecoderError::DepthConversion(format!(
            "failed to create {}x{} depth image from decoded values",
            width, height
        ))
    })?;

    Ok(DynamicImage::ImageLuma16(img))
}

/// Extracts raw `gray16` depth values from a decoded lossless depth frame.
///
/// FFV1 depth artifacts store millimeter values directly as `gray16le`; they
/// must not be masked to 10 bits or passed through Q10 dequantization.
fn decode_depth_raw_gray16_frame(
    frame: &ffmpeg::util::frame::video::Video,
) -> Result<DynamicImage, VideoDecoderError> {
    let width = frame.width() as usize;
    let height = frame.height() as usize;
    let data = frame.data(0);
    let stride = frame.stride(0);
    let format = frame.format();

    let read_sample = match format {
        Pixel::GRAY16LE => u16::from_le_bytes,
        Pixel::GRAY16BE => u16::from_be_bytes,
        other => {
            return Err(VideoDecoderError::DepthConversion(format!(
                "raw gray16 depth decode requires gray16le/gray16be, got {other:?}"
            )));
        }
    };

    let mut depth_values = Vec::with_capacity(width * height);
    for row in 0..height {
        for col in 0..width {
            let offset = row * stride + col * 2;
            if offset + 1 >= data.len() {
                return Err(VideoDecoderError::DepthConversion(format!(
                    "gray16 data too short: need offset {} but length is {}",
                    offset + 1,
                    data.len()
                )));
            }
            depth_values.push(read_sample([data[offset], data[offset + 1]]));
        }
    }

    let img = image::ImageBuffer::<image::Luma<u16>, Vec<u16>>::from_raw(
        width as u32,
        height as u32,
        depth_values,
    )
    .ok_or_else(|| {
        VideoDecoderError::DepthConversion(format!(
            "failed to create {}x{} raw depth image from decoded values",
            width, height
        ))
    })?;

    Ok(DynamicImage::ImageLuma16(img))
}

/// Configuration for the VideoDecoder stage.
///
/// This decoder processes all video files found in `context.video_registry()`.
/// No configuration is needed - it automatically decodes all available videos.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct VideoDecoderConfig {}

impl VideoDecoderConfig {
    pub fn new() -> Self {
        Self {}
    }
}

#[typetag::serde(name = "VideoDecoderConfig")]
impl StageConfig for VideoDecoderConfig {
    fn build(&self) -> Box<dyn Stage> {
        Box::new(VideoDecoder {})
    }
}

/// A stage that decodes video files into memory.
///
/// This stage reads all video files from `context.video_registry()` and populates
/// `context.image_data` with PNG frames. RGB videos become RGB PNGs, and depth
/// videos become 16-bit grayscale PNGs.
///
/// **Warning**: This can consume a large amount of memory if videos are long or high resolution.
///
/// # Preconditions
///
/// - `video_registry`: **Required** (mapping of topic names to video artifacts)
///
/// # Postconditions
///
/// - `image_data`: **Guaranteed** (all frames from all videos decoded as PNG)
///
/// # Errors
///
/// - [`StageError::MissingData`]: `video_registry` not set in context
/// - [`StageError::Io`]: Video file not found at specified path
/// - [`StageError::External`]: FFmpeg decoder creation failure, frame decode failure, PNG encode failure
pub struct VideoDecoder {}

impl Stage for VideoDecoder {
    fn name(&self) -> &'static str {
        "video_decoder"
    }

    fn run(&mut self, mut context: Context) -> Result<Context, StageError> {
        let video_registry = context
            .video_registry()
            .ok_or_else(|| {
                StageError::missing("video_registry in context (did a video encoder run?)")
            })?
            .clone();

        let mut image_data = context.image_data.take().unwrap_or_default();
        let mut image_topic_shapes = context.image_topic_shapes.take().unwrap_or_default();
        let decoded =
            decode_video_registry(&video_registry, context.bundle_root().map(|p| p.as_ref()))?;

        for (topic, frames) in decoded {
            if let Some(shape) = frames.first().and_then(|frame| frame.shape) {
                image_topic_shapes.insert(topic.clone(), shape);
            }
            image_data.insert(topic, frames);
        }

        context.image_data = Some(image_data);
        if !image_topic_shapes.is_empty() {
            context.set_image_topic_shapes(image_topic_shapes);
        }
        Ok(context)
    }
}

/// Decode a video registry into rebake image frames.
///
/// The registry is the canonical source of truth because it carries media type
/// and encoding metadata needed for both RGB and depth decode.
pub fn decode_video_registry(
    video_registry: &HashMap<String, VideoArtifact>,
    bundle_root: Option<&Utf8Path>,
) -> Result<HashMap<String, Vec<ImageFrame>>, StageError> {
    let mut image_data = HashMap::with_capacity(video_registry.len());

    for (topic, artifact) in video_registry {
        let video_path = artifact.resolve_path(bundle_root)?;
        if !video_path.exists() {
            return Err(StageError::io(
                format!("video file not found: {}", video_path),
                std::io::Error::new(std::io::ErrorKind::NotFound, "file does not exist"),
            ));
        }

        let frames = decode_artifact_frames(artifact, &video_path)?;
        image_data.insert(topic.clone(), frames);
    }

    Ok(image_data)
}

/// Decode RGB video files into rebake image frames.
///
/// This is the pure-Rust entrypoint for path-only RGB decode convenience APIs.
pub fn decode_rgb_video_paths(
    video_paths: &HashMap<String, Utf8PathBuf>,
) -> Result<HashMap<String, Vec<ImageFrame>>, StageError> {
    let mut image_data = HashMap::with_capacity(video_paths.len());

    for (topic, video_path) in video_paths {
        if !video_path.exists() {
            return Err(StageError::io(
                format!("video file not found: {}", video_path),
                std::io::Error::new(std::io::ErrorKind::NotFound, "file does not exist"),
            ));
        }

        let frames = decode_rgb_video_path(video_path)?;
        image_data.insert(topic.clone(), frames);
    }

    Ok(image_data)
}

/// Decode a single RGB video file into rebake image frames.
pub fn decode_rgb_video_path(video_path: &Utf8Path) -> Result<Vec<ImageFrame>, StageError> {
    let mut decoder = SingleVideoDecoder::new(video_path)
        .map_err(|e| StageError::external(format!("failed to open RGB video: {video_path}"), e))?;
    decode_frames_from_decoder(&mut decoder, "rgb", 3, video_path)
}

fn build_decoder(
    artifact: &VideoArtifact,
    video_path: &Utf8Path,
) -> Result<SingleVideoDecoder, StageError> {
    match artifact.metadata.media_type.as_str() {
        "rgb" => SingleVideoDecoder::new(video_path).map_err(|e| {
            StageError::external(format!("failed to open RGB video: {video_path}"), e)
        }),
        "depth" => {
            let config: DepthVideoConfig =
                serde_json::from_str(&artifact.metadata.encoding_config_json).map_err(|e| {
                    StageError::invalid_with(
                        format!(
                            "failed to parse depth video config for topic artifact: {}",
                            artifact.video_path
                        ),
                        e,
                    )
                })?;
            match depth_decode_kind(artifact, &config)? {
                DepthDecodeKind::Q10 => {
                    SingleVideoDecoder::new_depth(video_path, config.depth_max_mm).map_err(|e| {
                        StageError::external(format!("failed to open depth video: {video_path}"), e)
                    })
                }
                DepthDecodeKind::RawGray16 => SingleVideoDecoder::new_depth_raw_gray16(video_path)
                    .map_err(|e| {
                        StageError::external(
                            format!("failed to open raw depth video: {video_path}"),
                            e,
                        )
                    }),
            }
        }
        other => Err(StageError::invalid(format!(
            "unsupported media_type for video decoding: {other}"
        ))),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DepthDecodeKind {
    Q10,
    RawGray16,
}

fn depth_decode_kind(
    artifact: &VideoArtifact,
    config: &DepthVideoConfig,
) -> Result<DepthDecodeKind, StageError> {
    if matches!(config.codec_config, DepthCodecConfig::Ffv1)
        || artifact.metadata.pix_fmt.eq_ignore_ascii_case("gray16le")
        || artifact.metadata.pix_fmt.eq_ignore_ascii_case("gray16be")
    {
        return Ok(DepthDecodeKind::RawGray16);
    }

    if config.depth_max_mm == 0 {
        return Err(StageError::invalid(
            "depth_max_mm must be greater than 0 for Q10 depth video decoding",
        ));
    }

    Ok(DepthDecodeKind::Q10)
}

fn artifact_channels(artifact: &VideoArtifact) -> Result<usize, StageError> {
    match artifact.metadata.media_type.as_str() {
        "rgb" => Ok(3),
        "depth" => Ok(1),
        other => Err(StageError::invalid(format!(
            "unsupported media_type for video decoding: {other}"
        ))),
    }
}

fn decode_artifact_frames(
    artifact: &VideoArtifact,
    video_path: &Utf8Path,
) -> Result<Vec<ImageFrame>, StageError> {
    let mut decoder = build_decoder(artifact, video_path)?;
    let channels = artifact_channels(artifact)?;
    decode_frames_from_decoder(
        &mut decoder,
        artifact.metadata.media_type.as_str(),
        channels,
        video_path,
    )
}

fn decode_frames_from_decoder(
    decoder: &mut SingleVideoDecoder,
    media_type: &str,
    channels: usize,
    video_path: &Utf8Path,
) -> Result<Vec<ImageFrame>, StageError> {
    let frame_count = decoder.frame_count();
    let mut frames = Vec::with_capacity(frame_count);

    for i in 0..frame_count {
        let Some(img) = decoder.at_index(i).map_err(|e| {
            StageError::external(
                format!("failed to decode frame {} from {}", i, video_path),
                e,
            )
        })?
        else {
            continue;
        };

        let mut buffer = std::io::Cursor::new(Vec::new());
        let (width, height) = match media_type {
            "rgb" => {
                let rgb_img = img.to_rgb8();
                let width = rgb_img.width() as usize;
                let height = rgb_img.height() as usize;
                rgb_img
                    .write_to(&mut buffer, image::ImageFormat::Png)
                    .map_err(|e| StageError::external("failed to encode RGB frame as PNG", e))?;
                (width, height)
            }
            "depth" => {
                let depth_img = img.to_luma16();
                let width = depth_img.width() as usize;
                let height = depth_img.height() as usize;
                depth_img
                    .write_to(&mut buffer, image::ImageFormat::Png)
                    .map_err(|e| StageError::external("failed to encode depth frame as PNG", e))?;
                (width, height)
            }
            other => {
                return Err(StageError::invalid(format!(
                    "unsupported media_type for video decoding: {other}"
                )));
            }
        };

        frames.push(ImageFrame {
            index: i as u32,
            extension: "png".to_string(),
            bytes: buffer.into_inner(),
            shape: Some(ImageShape {
                width,
                height,
                channels,
            }),
        });
    }

    Ok(frames)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::encode::video_encoder::{SoftwareVideoEncoder, VideoEncoderConfig};
    use tempfile::tempdir;

    #[test]
    fn raw_gray16_depth_decode_preserves_full_u16_values() {
        let values = [0_u16, 1, 1023, 1024, 4092, 65535];
        let mut frame = ffmpeg::util::frame::video::Video::new(Pixel::GRAY16LE, 3, 2);
        let stride = frame.stride(0);
        let data = frame.data_mut(0);

        for row in 0..2usize {
            for col in 0..3usize {
                let value = values[row * 3 + col];
                let offset = row * stride + col * 2;
                data[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
            }
        }

        let decoded = decode_depth_raw_gray16_frame(&frame).unwrap().to_luma16();
        assert_eq!(decoded.as_raw().as_slice(), values);
    }

    #[test]
    fn depth_decode_kind_uses_raw_gray16_for_ffv1_with_zero_depth_max() {
        let config = DepthVideoConfig {
            depth_max_mm: 0,
            codec_config: DepthCodecConfig::Ffv1,
            fps: 30,
        };
        let artifact = config
            .video_artifact("videos/depth.mkv", 3, 2)
            .expect("FFV1 artifact should allow depth_max_mm=0");

        assert_eq!(
            depth_decode_kind(&artifact, &config).unwrap(),
            DepthDecodeKind::RawGray16
        );
    }

    #[test]
    fn q10_depth_decode_rejects_zero_depth_max_without_panic() {
        let result = SingleVideoDecoder::new_depth(Utf8Path::new("/tmp/missing.mp4"), 0);
        assert!(matches!(result, Err(VideoDecoderError::DepthConversion(_))));
    }

    #[test]
    fn test_video_decoder() {
        let temp_dir = tempdir().unwrap();
        let temp_path = temp_dir.path().join("test.mp4");
        let output_path = Utf8Path::from_path(temp_path.as_path()).unwrap();

        // 1. Create a test video
        let config = VideoEncoderConfig::new(30);
        let mut encoder = SoftwareVideoEncoder::new(&output_path, config);

        // Create 60 frames (2 seconds) of dummy data
        // Frame i will have color (i, i, i) to be easily verifiable
        for i in 0..60 {
            let mut img = image::RgbImage::new(64, 64);
            for pixel in img.pixels_mut() {
                *pixel = image::Rgb([i as u8, i as u8, i as u8]);
            }
            // Encode expects a format that image::load_from_memory can handle.
            // So we need to encode the image to memory first (e.g. PNG).
            let mut buffer = std::io::Cursor::new(Vec::new());
            img.write_to(&mut buffer, image::ImageFormat::Png).unwrap();
            encoder.add_data(buffer.get_ref()).unwrap();
        }
        encoder.finish().unwrap();

        // 2. Test Decoder
        let mut decoder = SingleVideoDecoder::new(output_path).expect("failed to create decoder");

        assert_eq!(decoder.frame_count(), 60);
        assert!((decoder.fps() - 30.0).abs() < 0.1);

        // Test random access

        // Frame 0
        let frame0 = decoder.at_index(0).unwrap().unwrap();
        let pixel0 = frame0.to_rgb8().get_pixel(0, 0)[0];
        // Compression might introduce artifacts, but with constant color it should be close
        assert!(
            (pixel0 as i32).abs() < 5,
            "Frame 0 pixel value mismatch: {}",
            pixel0
        );

        // Frame 30 (1 second in)
        let frame30 = decoder.at_index(30).unwrap().unwrap();
        let pixel30 = frame30.to_rgb8().get_pixel(0, 0)[0];
        assert!(
            (pixel30 as i32 - 30).abs() < 5,
            "Frame 30 pixel value mismatch: {}",
            pixel30
        );

        // Frame 59 (last frame)
        let frame59 = decoder.at_index(59).unwrap().unwrap();
        let pixel59 = frame59.to_rgb8().get_pixel(0, 0)[0];
        assert!(
            (pixel59 as i32 - 59).abs() < 5,
            "Frame 59 pixel value mismatch: {}",
            pixel59
        );

        // Sequential access optimization check (internal logic, but we can check correctness)
        let frame31 = decoder.at_index(31).unwrap().unwrap();
        let pixel31 = frame31.to_rgb8().get_pixel(0, 0)[0];
        assert!((pixel31 as i32 - 31).abs() < 5);

        // Backwards seek
        let frame10 = decoder.at_index(10).unwrap().unwrap();
        let pixel10 = frame10.to_rgb8().get_pixel(0, 0)[0];
        assert!((pixel10 as i32 - 10).abs() < 5);
    }

    #[test]
    fn test_video_decoder_stage() {
        let temp_dir = tempdir().unwrap();
        let temp_path = temp_dir.path().join("test_stage.mp4");
        let output_path = Utf8Path::from_path(temp_path.as_path()).unwrap();

        // 1. Create a test video
        let config = VideoEncoderConfig::new(30);
        let mut encoder = SoftwareVideoEncoder::new(&output_path, config);

        // Create 10 frames
        for i in 0..10 {
            let mut img = image::RgbImage::new(64, 64);
            for pixel in img.pixels_mut() {
                *pixel = image::Rgb([i as u8, 0, 0]);
            }
            let mut buffer = std::io::Cursor::new(Vec::new());
            img.write_to(&mut buffer, image::ImageFormat::Png).unwrap();
            encoder.add_data(buffer.get_ref()).unwrap();
        }
        encoder.finish().unwrap();

        // 2. Run Stage
        let mut stage = VideoDecoderConfig::new().build();

        let mut context = Context::default();
        let artifact = VideoEncoderConfig::new(30)
            .video_artifact(output_path.as_str(), 64, 64)
            .unwrap();
        let mut video_registry = std::collections::HashMap::new();
        video_registry.insert("/test_topic".to_string(), artifact);
        context.set_video_registry(video_registry);

        let result_context = stage.run(context).expect("stage run failed");

        // 3. Verify
        assert!(result_context.image_data.is_some());
        let image_data = result_context.image_data.unwrap();
        assert!(image_data.contains_key("/test_topic"));
        let frames = image_data.get("/test_topic").unwrap();
        assert_eq!(frames.len(), 10);

        // Verify content of first frame
        let frame0 = &frames[0];
        let img0 = image::load_from_memory(&frame0.bytes).unwrap().to_rgb8();
        assert_eq!(img0.get_pixel(0, 0)[0], 0);
    }

    #[test]
    fn test_encode_decode_consistency() {
        let temp_dir = tempdir().unwrap();
        let temp_path = temp_dir.path().join("consistency_test.mp4");
        let output_path = Utf8Path::from_path(temp_path.as_path()).unwrap();

        // 1. Create a test video with a gradient pattern
        // Gradient helps detect color channel swaps (RGB vs BGR) and scaling artifacts.
        let width = 128;
        let height = 128;
        let config = VideoEncoderConfig::new(30);
        let mut encoder = SoftwareVideoEncoder::new(&output_path, config);

        let mut original_img = image::RgbImage::new(width, height);
        for (x, y, pixel) in original_img.enumerate_pixels_mut() {
            // R: Increases with X
            // G: Increases with Y
            // B: Constant (to differentiate from others)
            let r = (x as f32 / width as f32 * 255.0) as u8;
            let g = (y as f32 / height as f32 * 255.0) as u8;
            let b = 128;
            *pixel = image::Rgb([r, g, b]);
        }

        // Write 30 frames of the same image
        for _ in 0..30 {
            let mut buffer = std::io::Cursor::new(Vec::new());
            original_img
                .write_to(&mut buffer, image::ImageFormat::Png)
                .unwrap();
            encoder.add_data(buffer.get_ref()).unwrap();
        }
        encoder.finish().unwrap();

        // 2. Decode
        let mut decoder = SingleVideoDecoder::new(output_path).expect("failed to create decoder");

        // Check middle frame
        let decoded_frame = decoder.at_index(15).unwrap().unwrap();
        let decoded_img = decoded_frame.to_rgb8();

        assert_eq!(decoded_img.width(), width);
        assert_eq!(decoded_img.height(), height);

        // 3. Compare Pixels (Allowing for compression artifacts)
        // We calculate Mean Squared Error (MSE)
        let mut total_error: u64 = 0;
        for (x, y, original_pixel) in original_img.enumerate_pixels() {
            let decoded_pixel = decoded_img.get_pixel(x, y);

            let r_diff = (original_pixel[0] as i32 - decoded_pixel[0] as i32).abs();
            let g_diff = (original_pixel[1] as i32 - decoded_pixel[1] as i32).abs();
            let b_diff = (original_pixel[2] as i32 - decoded_pixel[2] as i32).abs();

            total_error += (r_diff * r_diff + g_diff * g_diff + b_diff * b_diff) as u64;
        }

        let mse = total_error as f64 / (width * height * 3) as f64;

        // Threshold for AV1 CRF 30 (default in our config)
        // Lossy compression will introduce some error, but it shouldn't be massive.
        // If channels are swapped (e.g. R becomes B), MSE will be huge.
        assert!(
            mse < 50.0,
            "MSE too high: {}. Likely color channel swap or severe compression artifacts.",
            mse
        );
    }

    #[test]
    fn test_require_pts_returns_value_when_present() {
        let result = require_pts(Some(12345));
        assert_eq!(result.unwrap(), 12345);
    }

    #[test]
    fn test_require_pts_returns_error_when_none() {
        let result = require_pts(None);
        assert!(matches!(result, Err(VideoDecoderError::MissingPts)));
    }
}
