//! Pipeline stage that saves decoded depth frames to disk as raw binary files.
//!
//! Each frame is decoded from ROS `compressedDepth` transport into raw
//! gray16le bytes (2 bytes per pixel, little-endian) and written as an
//! individual `.bin` file. A `meta.json` file records frame dimensions so that
//! downstream tools (e.g., benchmark scripts) can reshape the flat byte array.
//!
//! # Output Structure
//!
//! For a topic `/camera/depth/compressedDepth`, frames are saved as:
//! ```text
//! {output_dir}/camera/depth/compressedDepth/
//! ├── meta.json   # {"width": 848, "height": 480}
//! ├── 0.bin       # width × height × 2 bytes (gray16le)
//! ├── 1.bin
//! └── ...
//! ```
//!
//! Python usage:
//! ```python
//! import json, numpy as np
//! meta = json.load(open("meta.json"))
//! frame = np.fromfile("0.bin", dtype=np.uint16).reshape(meta["height"], meta["width"])
//! ```

use std::fs;

use camino::{Utf8Path, Utf8PathBuf};
use serde::{Deserialize, Serialize};

use crate::common::topic_name_to_relative_path;
use crate::core::error::OptionExt;
use crate::core::stage::{Context, Stage, StageConfig, StageError};
use crate::encode::compressed_depth::decode_depth_frame;

/// Configuration for [`DepthImageEncoder`].
///
/// No parameters needed — output directory comes from `Context`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct DepthImageEncoderConfig {}

#[typetag::serde(name = "DepthImageEncoderConfig")]
impl StageConfig for DepthImageEncoderConfig {
    /// Builds the depth image encoder stage.
    fn build(&self) -> Box<dyn Stage> {
        Box::new(DepthImageEncoder)
    }
}

/// A pipeline stage that decodes depth frames and saves them as raw binary files.
///
/// Each `DepthFrame` contains a ROS `compressedDepth` payload plus the original
/// ROS transport format string. This stage decodes each frame into raw gray16le
/// bytes and writes them to individual `.bin` files. A `meta.json` records the
/// frame width and height.
///
/// # Preconditions
///
/// - `output_dir`: **Required**
/// - `depth_data`: Conditional (if missing, stage returns early with no action)
///
/// # Postconditions
///
/// - `depth_data`: Conditional (preserved only if present)
/// - `output_dir`: **Guaranteed** (preserved)
///
/// # Errors
///
/// - [`StageError::MissingData`]: `output_dir` not set in context
/// - [`StageError::InvalidData`]: compressedDepth decode failure
/// - [`StageError::Io`]: Directory creation or file write failure
pub struct DepthImageEncoder;

impl Stage for DepthImageEncoder {
    /// Returns the stable stage name used in logs and configuration.
    fn name(&self) -> &'static str {
        "depth_image_encoder"
    }

    /// Decodes each depth frame and writes it to disk as a raw `.bin` file.
    fn run(&mut self, mut context: Context) -> Result<Context, StageError> {
        let output_dir = context
            .output_dir()
            .cloned()
            .or_missing("output_dir in context")?;

        let depth_data = match context.depth_data.take() {
            Some(data) => data,
            None => return Ok(context),
        };

        for (topic, frames) in depth_data.iter() {
            let base_dir = topic_to_base_dir(&output_dir, topic);

            let mut dimensions: Option<(u32, u32)> = None;
            for frame in frames {
                let decoded = decode_depth_frame(frame)?;
                let width = decoded.width;
                let height = decoded.height;
                let raw_gray16le = decoded.into_gray16le_bytes();
                fs::create_dir_all(base_dir.as_std_path())?;
                let output_path = base_dir.join(format!("{}.bin", frame.index));
                fs::write(output_path.as_std_path(), &raw_gray16le)?;
                dimensions = Some((width, height));
            }

            // Save frame dimensions for downstream tools (e.g., benchmark scripts).
            // Raw .bin files have no header, so width/height must be recorded separately.
            if let Some((width, height)) = dimensions {
                let meta = serde_json::json!({"width": width, "height": height});
                let meta_path = base_dir.join("meta.json");
                fs::write(meta_path.as_std_path(), meta.to_string().as_bytes())?;
            }
        }

        context.set_depth_data(depth_data);
        context.set_output_dir(output_dir);

        Ok(context)
    }
}

/// Convert topic name to output directory path.
///
/// Reuses [`topic_name_to_relative_path`] (which appends `.parquet`) and strips
/// the suffix to get a clean directory name.
fn topic_to_base_dir(output_dir: &Utf8Path, topic: &str) -> Utf8PathBuf {
    let relative_path = topic_name_to_relative_path(topic);
    let relative_base = relative_path
        .strip_suffix(".parquet")
        .unwrap_or(relative_path.as_str());
    output_dir.join(relative_base)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::core::stage::Context;
    use crate::encode::compressed_depth::test_support::{
        make_png_depth_frame, make_rvl_depth_frame,
    };
    use polars::prelude::{IntoLazy, df};
    use std::collections::HashMap;
    use tempfile::tempdir;

    // Creates a minimal context with an output directory for tests.
    fn make_context(outdir: Utf8PathBuf) -> Context {
        let mut datasets: HashMap<String, polars::prelude::LazyFrame> = HashMap::new();
        datasets.insert("/dummy".to_string(), df!("value" => &[1]).unwrap().lazy());
        let mut context = Context::new(datasets);
        context.set_output_dir(outdir);
        context
    }

    // Tests that PNG depth frames are written to disk as raw bytes.
    #[test]
    fn writes_depth_frames_to_disk() {
        let tmp_dir = tempdir().unwrap();
        let outdir = Utf8PathBuf::from_path_buf(tmp_dir.path().to_path_buf()).unwrap();

        let mut encoder = DepthImageEncoder;
        let mut context = make_context(outdir.clone());

        let values = [100u16, 200, 300, 400];
        let frame = make_png_depth_frame(0, 2, 2, &values);
        let mut depth_data = HashMap::new();
        depth_data.insert("/camera/depth/compressedDepth".to_string(), vec![frame]);
        context.set_depth_data(depth_data);

        encoder.run(context).unwrap();

        let output_path = outdir.join("camera/depth/compressedDepth/0.bin");
        assert!(output_path.exists());

        let raw_bytes = std::fs::read(output_path.as_std_path()).unwrap();
        assert_eq!(raw_bytes.len(), 4 * 2); // 4 pixels × 2 bytes
        let decoded: Vec<u16> = raw_bytes
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        assert_eq!(decoded, values);
    }

    // Tests that the encoder writes `meta.json` with frame dimensions.
    #[test]
    fn writes_meta_json() {
        let tmp_dir = tempdir().unwrap();
        let outdir = Utf8PathBuf::from_path_buf(tmp_dir.path().to_path_buf()).unwrap();

        let mut encoder = DepthImageEncoder;
        let mut context = make_context(outdir.clone());

        let frame = make_png_depth_frame(0, 4, 3, &[0u16; 12]);
        let mut depth_data = HashMap::new();
        depth_data.insert("/camera/depth/compressedDepth".to_string(), vec![frame]);
        context.set_depth_data(depth_data);

        encoder.run(context).unwrap();

        let meta_path = outdir.join("camera/depth/compressedDepth/meta.json");
        assert!(meta_path.exists());

        let meta_str = std::fs::read_to_string(meta_path.as_std_path()).unwrap();
        let meta: serde_json::Value = serde_json::from_str(&meta_str).unwrap();
        assert_eq!(meta["width"], 4);
        assert_eq!(meta["height"], 3);
    }

    // Tests that the stage preserves the expected context fields.
    #[test]
    fn preserves_context_data() {
        let tmp_dir = tempdir().unwrap();
        let outdir = Utf8PathBuf::from_path_buf(tmp_dir.path().to_path_buf()).unwrap();

        let mut encoder = DepthImageEncoder;
        let mut context = make_context(outdir.clone());

        let frame = make_png_depth_frame(0, 2, 2, &[10, 20, 30, 40]);
        let mut depth_data = HashMap::new();
        depth_data.insert("/camera/depth/compressedDepth".to_string(), vec![frame]);
        context.set_depth_data(depth_data);

        let result = encoder.run(context).unwrap();

        assert!(result.depth_data.is_some());
        assert_eq!(result.output_dir.unwrap(), outdir);
    }

    // Tests that the stage is a no-op when no depth frames are present.
    #[test]
    fn noop_without_depth_data() {
        let tmp_dir = tempdir().unwrap();
        let outdir = Utf8PathBuf::from_path_buf(tmp_dir.path().to_path_buf()).unwrap();

        let mut encoder = DepthImageEncoder;
        let context = make_context(outdir.clone());

        let result = encoder.run(context).unwrap();

        assert!(result.depth_data.is_none());
        // No files should be created
        assert!(std::fs::read_dir(tmp_dir.path()).unwrap().next().is_none());
    }

    // Tests that RVL depth frames are also written as raw `.bin` files.
    #[test]
    fn writes_rvl_depth_frames_to_disk() {
        let tmp_dir = tempdir().unwrap();
        let outdir = Utf8PathBuf::from_path_buf(tmp_dir.path().to_path_buf()).unwrap();

        let mut encoder = DepthImageEncoder;
        let mut context = make_context(outdir.clone());

        let values = [0u16, 42, 43, 0];
        let frame = make_rvl_depth_frame(0, 2, 2, &values);

        let mut depth_data = HashMap::new();
        depth_data.insert("/camera/depth/compressedDepth".to_string(), vec![frame]);
        context.set_depth_data(depth_data);

        encoder.run(context).unwrap();

        let output_path = outdir.join("camera/depth/compressedDepth/0.bin");
        let raw_bytes = std::fs::read(output_path.as_std_path()).unwrap();
        let decoded: Vec<u16> = raw_bytes
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        assert_eq!(decoded, values);
    }
}
