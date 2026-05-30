use std::collections::HashMap;
use std::fs;

use camino::{Utf8Path, Utf8PathBuf};
use polars::prelude::LazyFrame;

use crate::common::{DepthFrame, ImageFrame, ImageShape, PointCloudFrame};
use crate::encode::video_artifact::VideoArtifact;
use crate::schema::metadata::AiroaMetadata;

// Re-export StageError for backward compatibility
pub use crate::core::error::StageError;
use crate::core::error::{OptionExt, StageResult};

/// A trait for stage configuration objects that can be deserialized and built into a `Stage`.
///
/// This trait is the foundation of `rebake`'s configurable pipeline. Each implementation
/// of `StageConfig` represents the serializable parameters for a corresponding `Stage`.
/// The `#[typetag::serde]` attribute allows these trait objects to be deserialized from
/// configuration files (e.g., YAML), enabling the dynamic construction of a pipeline.
#[typetag::serde]
pub trait StageConfig: Send + Sync {
    fn build(&self) -> Box<dyn Stage>;

    /// Declares which kind of CLI/orchestrator input this stage expects when it
    /// is used as the first stage of a pipeline.
    fn pipeline_input_kind(&self) -> Option<PipelineInputKind> {
        None
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PipelineInputKind {
    Rosbag,
    ParquetVideoBundle,
}

/// A trait representing a single, executable step in a data processing pipeline.
///
/// A `Stage` takes the shared `Context` object, performs a specific task (such as ingesting,
/// enriching, or transforming data), and returns the modified `Context`. Pipelines are
/// constructed by chaining multiple `Stage` implementations together in an `Orchestrator`.
pub trait Stage: Send + Sync {
    /// Returns the name of the stage.
    ///
    /// This name is used for identification, logging, and creating output directories.
    fn name(&self) -> &'static str;

    /// Executes the logic for this stage.
    ///
    /// This method consumes the `Context` from the previous stage, performs its processing,
    /// and returns a new or modified `Context` to be passed to the next stage.
    fn run(&mut self, context: Context) -> Result<Context, StageError>;
}

/// A container for data passed between pipeline stages.
///
/// The `Context` acts as a shared data bus for the entire pipeline. Each `Stage` receives a
/// `Context` as input, performs its processing, and returns an updated `Context` as output.
/// This allows stages to communicate and pass data (like datasets, image data, and metadata)
/// to subsequent stages in a decoupled manner.
#[derive(Default)]
pub struct Context {
    pub dataset: Option<HashMap<String, LazyFrame>>,
    pub image_data: Option<HashMap<String, Vec<ImageFrame>>>,
    pub image_topic_shapes: Option<HashMap<String, ImageShape>>,
    pub depth_data: Option<HashMap<String, Vec<DepthFrame>>>,
    pub pointcloud_data: Option<HashMap<String, Vec<PointCloudFrame>>>,
    pub fps: Option<usize>,
    pub output_dir: Option<Utf8PathBuf>,
    pub video_cache_dir: Option<Utf8PathBuf>,
    pub rosbag_path: Option<Utf8PathBuf>,
    pub bundle_root: Option<Utf8PathBuf>,
    pub video_registry: Option<HashMap<String, VideoArtifact>>,
    pub topic_message_type_map: Option<HashMap<String, String>>,
    pub airoa_metadata: Option<AiroaMetadata>,
}

impl Context {
    pub fn new(dataset: HashMap<String, LazyFrame>) -> Self {
        Self {
            dataset: Some(dataset),
            image_data: None,
            image_topic_shapes: None,
            depth_data: None,
            pointcloud_data: None,
            fps: None,
            output_dir: None,
            video_cache_dir: None,
            rosbag_path: None,
            bundle_root: None,
            video_registry: None,
            topic_message_type_map: None,
            airoa_metadata: None,
        }
    }

    pub fn set_dataset(&mut self, dataset: HashMap<String, LazyFrame>) {
        self.dataset = Some(dataset);
    }

    /// Returns a reference to the dataset.
    pub fn dataset(&self) -> Option<&HashMap<String, LazyFrame>> {
        self.dataset.as_ref()
    }

    /// Returns a mutable reference to the dataset.
    pub fn dataset_mut(&mut self) -> Option<&mut HashMap<String, LazyFrame>> {
        self.dataset.as_mut()
    }

    /// Inserts data into the dataset.
    ///
    /// # Errors
    ///
    /// Returns `StageError::MissingData` if the dataset has not been initialized.
    /// Call `set_dataset()` first to initialize the dataset before inserting data.
    pub fn insert_data(&mut self, key: String, value: LazyFrame) -> StageResult<()> {
        self.dataset
            .as_mut()
            .or_missing("dataset (call set_dataset first)")?
            .insert(key, value);
        Ok(())
    }

    pub fn set_image_data(&mut self, image_data: HashMap<String, Vec<ImageFrame>>) {
        self.image_data = Some(image_data);
    }

    pub fn image_data(&self) -> Option<&HashMap<String, Vec<ImageFrame>>> {
        self.image_data.as_ref()
    }

    pub fn set_image_topic_shapes(&mut self, image_topic_shapes: HashMap<String, ImageShape>) {
        self.image_topic_shapes = Some(image_topic_shapes);
    }

    pub fn image_topic_shapes(&self) -> Option<&HashMap<String, ImageShape>> {
        self.image_topic_shapes.as_ref()
    }

    pub fn set_depth_data(&mut self, depth_data: HashMap<String, Vec<DepthFrame>>) {
        self.depth_data = Some(depth_data);
    }

    pub fn depth_data(&self) -> Option<&HashMap<String, Vec<DepthFrame>>> {
        self.depth_data.as_ref()
    }

    pub fn set_pointcloud_data(&mut self, pointcloud_data: HashMap<String, Vec<PointCloudFrame>>) {
        self.pointcloud_data = Some(pointcloud_data);
    }

    pub fn pointcloud_data(&self) -> Option<&HashMap<String, Vec<PointCloudFrame>>> {
        self.pointcloud_data.as_ref()
    }

    pub fn set_fps(&mut self, fps: usize) {
        self.fps = Some(fps);
    }

    pub fn fps(&self) -> Option<usize> {
        self.fps
    }

    pub fn set_output_dir(&mut self, output_dir: Utf8PathBuf) {
        self.output_dir = Some(output_dir);
    }

    /// Returns a reference to the output directory path.
    pub fn output_dir(&self) -> Option<&Utf8PathBuf> {
        self.output_dir.as_ref()
    }

    pub fn set_video_cache_dir(&mut self, video_cache_dir: Utf8PathBuf) {
        self.video_cache_dir = Some(video_cache_dir);
    }

    /// Returns a reference to the video cache directory path.
    pub fn video_cache_dir(&self) -> Option<&Utf8PathBuf> {
        self.video_cache_dir.as_ref()
    }

    pub fn set_rosbag_path(&mut self, rosbag_path: Utf8PathBuf) {
        self.rosbag_path = Some(rosbag_path);
    }

    /// Returns a reference to the rosbag path.
    pub fn rosbag_path(&self) -> Option<&Utf8PathBuf> {
        self.rosbag_path.as_ref()
    }

    pub fn set_bundle_root(&mut self, bundle_root: Utf8PathBuf) {
        self.bundle_root = Some(bundle_root);
    }

    pub fn bundle_root(&self) -> Option<&Utf8PathBuf> {
        self.bundle_root.as_ref()
    }

    pub fn set_video_registry(&mut self, video_registry: HashMap<String, VideoArtifact>) {
        self.video_registry = Some(video_registry);
    }

    pub fn video_registry(&self) -> Option<&HashMap<String, VideoArtifact>> {
        self.video_registry.as_ref()
    }

    /// Populate missing image topic shapes from the canonical video registry.
    ///
    /// This keeps metadata composition independent from eager image decode:
    /// callers that provide encoded video artifacts can still derive stable
    /// `[height, width, channels]` shapes for each visual topic.
    pub fn populate_image_topic_shapes_from_video_registry(&mut self) -> StageResult<()> {
        let Some(video_registry) = self.video_registry.as_ref() else {
            return Ok(());
        };

        let image_topic_shapes = self.image_topic_shapes.get_or_insert_with(HashMap::new);
        for (topic, artifact) in video_registry {
            image_topic_shapes
                .entry(topic.clone())
                .or_insert(artifact.image_shape()?);
        }

        Ok(())
    }

    pub fn resolve_video_path(&self, topic: &str) -> StageResult<Utf8PathBuf> {
        let video_registry = self
            .video_registry
            .as_ref()
            .or_missing("video_registry in context")?;
        let artifact = video_registry
            .get(topic)
            .ok_or_else(|| StageError::missing(format!("video artifact for topic: {topic}")))?;
        artifact.resolve_path(self.bundle_root.as_deref())
    }

    pub fn resolve_video_paths(&self) -> StageResult<HashMap<String, Utf8PathBuf>> {
        let video_registry = self
            .video_registry
            .as_ref()
            .or_missing("video_registry in context")?;
        video_registry
            .iter()
            .map(|(topic, artifact)| {
                let path = artifact.resolve_path(self.bundle_root.as_deref())?;
                Ok((topic.clone(), path))
            })
            .collect()
    }

    pub fn set_topic_message_type_map(&mut self, map: HashMap<String, String>) {
        self.topic_message_type_map = Some(map);
    }

    pub fn topic_message_type_map(&self) -> Option<&HashMap<String, String>> {
        self.topic_message_type_map.as_ref()
    }

    pub fn set_airoa_metadata(&mut self, metadata: AiroaMetadata) {
        self.airoa_metadata = Some(metadata);
    }

    pub fn airoa_metadata(&self) -> Option<&AiroaMetadata> {
        self.airoa_metadata.as_ref()
    }

    /// Take ownership of the airoa metadata, leaving None in its place.
    pub fn take_airoa_metadata(&mut self) -> Option<AiroaMetadata> {
        self.airoa_metadata.take()
    }

    /// Save the context dataset to Parquet files.
    ///
    /// This saves each topic's LazyFrame as a separate Parquet file.
    /// The logic mirrors `Orchestrator::save_context()`.
    ///
    /// # Arguments
    ///
    /// * `output_dir` - Directory to save Parquet files to.
    ///
    /// # Output structure
    ///
    /// ```text
    /// {output_dir}/{topic}.parquet
    /// ```
    ///
    /// # Example
    ///
    /// ```ignore
    /// context.save_to_parquet(Utf8Path::new("./parquet_output"))?;
    /// // Creates: ./parquet_output/joint_states.parquet
    /// //          ./parquet_output/camera/image.parquet
    /// ```
    pub fn save_to_parquet(&self, output_dir: &Utf8Path) -> Result<(), StageError> {
        use polars::prelude::ParquetWriter;

        let Some(dataset) = self.dataset.as_ref() else {
            return Ok(());
        };

        for (topic, frame) in dataset {
            let sanitized_topic = topic.trim_start_matches('/');
            let file_path = output_dir.join(format!("{}.parquet", sanitized_topic));

            if let Some(parent) = file_path.parent() {
                fs::create_dir_all(parent.as_std_path())
                    .map_err(|e| StageError::io("failed to create parquet directory", e))?;
            }

            let mut df = frame.clone().collect()?;

            let mut file = fs::File::create(file_path.as_std_path())
                .map_err(|e| StageError::io("failed to create parquet file", e))?;

            ParquetWriter::new(&mut file).finish(&mut df)?;
        }

        Ok(())
    }
}

/// A boxed error type for backward compatibility.
///
/// This type alias is kept for backward compatibility with existing code.
/// New code should use `StageError` directly.
pub type DynError = Box<dyn std::error::Error + Send + Sync + 'static>;

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::common::ImageShape;
    use crate::encode::video_artifact::{VideoArtifact, VideoMetadata};

    fn sample_artifact() -> VideoArtifact {
        VideoArtifact {
            video_path: "/tmp/camera.mp4".to_string(),
            metadata: VideoMetadata {
                media_type: "rgb".to_string(),
                codec_family: "av1".to_string(),
                encoder_name: "libsvtav1".to_string(),
                pix_fmt: "yuv420p".to_string(),
                width: 640,
                height: 480,
                fps: 30,
                encoding_config_json: "{}".to_string(),
            },
        }
    }

    #[test]
    fn populate_image_topic_shapes_from_video_registry_adds_missing_topics() {
        let mut context = Context::default();
        context.set_video_registry(HashMap::from([(
            "/camera/image".to_string(),
            sample_artifact(),
        )]));

        context
            .populate_image_topic_shapes_from_video_registry()
            .unwrap();

        assert_eq!(
            context
                .image_topic_shapes()
                .and_then(|shapes| shapes.get("/camera/image").copied()),
            Some(ImageShape::new(480, 640, 3))
        );
    }

    #[test]
    fn populate_image_topic_shapes_from_video_registry_preserves_existing_shapes() {
        let mut context = Context::default();
        context.set_video_registry(HashMap::from([(
            "/camera/image".to_string(),
            sample_artifact(),
        )]));
        context.set_image_topic_shapes(HashMap::from([(
            "/camera/image".to_string(),
            ImageShape::new(10, 20, 3),
        )]));

        context
            .populate_image_topic_shapes_from_video_registry()
            .unwrap();

        assert_eq!(
            context
                .image_topic_shapes()
                .and_then(|shapes| shapes.get("/camera/image").copied()),
            Some(ImageShape::new(10, 20, 3))
        );
    }
}
