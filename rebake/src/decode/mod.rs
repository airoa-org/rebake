//! Media decoding stages.
//!
//! Provides decoders for reading video files back into image frames,
//! enabling re-processing of previously encoded data.
//!
//! # Responsibilities
//!
//! - Owns: Video decoding via FFmpeg
//! - Does not own: Encoding (see [`crate::encode`] module)

pub mod video_decoder;
