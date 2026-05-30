//! Media encoding stages.
//!
//! Provides encoders for converting image and depth data to various output formats,
//! including video files (AV1, H.264, H.265, FFV1) and image sequences.
//!
//! # Responsibilities
//!
//! - Owns: Video encoding via FFmpeg (RGB and depth); image frame output to files
//! - Does not own: Decoding (see [`crate::decode`] module)

pub mod compressed_depth;
pub mod depth_image_encoder;
pub mod depth_quantizer;
pub mod depth_video_encoder;
pub mod ffmpeg_cli;
pub mod ffmpeg_subprocess;
pub mod image_encoder;
pub mod nvenc;
pub mod video_artifact;
pub mod video_encoder;
