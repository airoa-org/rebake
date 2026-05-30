use camino::Utf8PathBuf;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io;

use crate::common::{ImageFrame, topic_name_to_relative_path};
use crate::core::error::OptionExt;
use crate::core::stage::{Context, Stage, StageConfig, StageError};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct ImageEncoderConfig {}

#[typetag::serde(name = "ImageEncoderConfig")]
impl StageConfig for ImageEncoderConfig {
    fn build(&self) -> Box<dyn Stage> {
        Box::new(ImageEncoder::new(self.clone()))
    }
}

pub struct SingleTopicImageEncoder {
    base_dir: Utf8PathBuf,
}

impl SingleTopicImageEncoder {
    pub fn new(base_dir: Utf8PathBuf) -> Self {
        Self { base_dir }
    }

    pub fn add_frame(&self, frame: &ImageFrame) -> io::Result<()> {
        let file_name = format!("{}.{}", frame.index, frame.extension);
        let output_path = self.base_dir.join(file_name);
        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(output_path.as_std_path(), &frame.bytes)?;
        Ok(())
    }
}

/// A pipeline stage that saves image frames to disk as individual files.
///
/// This stage iterates through the image data provided in the `Context`, and for each topic,
/// it saves all frames as individual image files. The output directory structure mirrors
/// the topic name hierarchy.
///
/// # Output Structure
///
/// For a topic `/camera/image_raw/compressed`, frames are saved as:
/// ```text
/// {output_dir}/camera/image_raw/compressed/0.jpg
/// {output_dir}/camera/image_raw/compressed/1.jpg
/// ...
/// ```
///
/// # Preconditions
///
/// - `output_dir`: **Required**
/// - `image_data`: Conditional (if missing, stage returns early with no action)
///
/// # Postconditions
///
/// - `image_data`: Conditional (preserved only if present)
/// - `output_dir`: **Guaranteed** (preserved)
///
/// # Errors
///
/// - [`StageError::MissingData`]: `output_dir` not set in context
/// - [`StageError::Io`]: Directory creation or file write failure
pub struct ImageEncoder;

impl ImageEncoder {
    pub fn new(_config: ImageEncoderConfig) -> Self {
        Self
    }
}

impl Stage for ImageEncoder {
    fn name(&self) -> &'static str {
        "image_encoder"
    }

    fn run(&mut self, mut context: Context) -> Result<Context, StageError> {
        let output_dir = context
            .output_dir()
            .cloned()
            .or_missing("output_dir in context")?;

        let image_data = match context.image_data.take() {
            Some(data) => data,
            None => return Ok(context),
        };

        for (topic, frames) in image_data.iter() {
            let relative_path = topic_name_to_relative_path(topic);
            let relative_base = relative_path
                .strip_suffix(".parquet")
                .unwrap_or(relative_path.as_str());
            let base_dir = output_dir.join(relative_base);
            let encoder = SingleTopicImageEncoder::new(base_dir);
            for frame in frames {
                encoder.add_frame(frame)?;
            }
        }

        context.set_image_data(image_data);
        context.set_output_dir(output_dir);

        Ok(context)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::common::ImageFrame;
    use crate::core::stage::Context;
    use polars::prelude::{IntoLazy, df};
    use std::collections::HashMap;
    use tempfile::tempdir;

    #[test]
    fn writes_images_to_disk() {
        let tmp_dir = tempdir().unwrap();
        let outdir = Utf8PathBuf::from_path_buf(tmp_dir.path().to_path_buf()).unwrap();

        let mut encoder = ImageEncoder::new(ImageEncoderConfig::default());

        let mut datasets: HashMap<String, polars::prelude::LazyFrame> = HashMap::new();
        datasets.insert("/dummy".to_string(), df!("value" => &[1]).unwrap().lazy());
        let mut context = Context::new(datasets);
        context.set_output_dir(outdir.clone());

        let frame = ImageFrame::new(0_u32, "jpg", vec![0_u8; 10]);
        let mut images = std::collections::HashMap::new();
        images.insert("/camera/topic/compressed".to_string(), vec![frame]);
        context.set_image_data(images);

        let result = encoder.run(context).unwrap();
        let output_path = outdir.join("camera/topic/compressed/0.jpg");
        assert!(output_path.exists());

        // image data should remain available for subsequent stages
        assert!(result.image_data.is_some());
        assert_eq!(result.output_dir.unwrap(), outdir);
    }
}
