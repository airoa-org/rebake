use std::collections::HashMap;
use std::error::Error;
use std::io;
use std::mem;

use pyo3::Bound;
use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;

use crate::common::PyImageFrame;
use crate::core::PyContext;
use rebake::core::stage::{Context, StageConfig};
use rebake::decode::video_decoder::{
    VideoDecoderConfig, decode_rgb_video_paths, decode_video_registry,
};
use rebake::encode::video_artifact::VideoArtifact;

#[pyclass(module = "rebake.decode", name = "VideoDecoderConfig")]
#[derive(Clone, Default)]
pub struct PyVideoDecoderConfig {
    pub inner: VideoDecoderConfig,
}

#[pymethods]
impl PyVideoDecoderConfig {
    #[new]
    pub fn new() -> Self {
        Self {
            inner: VideoDecoderConfig::new(),
        }
    }
}

#[pyclass(module = "rebake.decode", name = "VideoDecoder")]
pub struct PyVideoDecoder {
    config: VideoDecoderConfig,
}

#[pymethods]
impl PyVideoDecoder {
    #[new]
    pub fn new(config: &PyVideoDecoderConfig) -> Self {
        Self {
            config: config.inner.clone(),
        }
    }

    pub fn run(&self, context: Bound<'_, PyContext>) -> PyResult<()> {
        let mut guard = context.borrow_mut();
        let current = mem::take(&mut guard.inner);
        let updated = self
            .execute(current)
            .map_err(|err| PyRuntimeError::new_err(err.to_string()))?;
        guard.inner = updated;
        Ok(())
    }

    pub fn decode_rgb_paths(
        &self,
        video_paths: HashMap<String, String>,
    ) -> PyResult<HashMap<String, Vec<PyImageFrame>>> {
        self.decode_rgb_paths_inner(video_paths)
            .map_err(|err| PyRuntimeError::new_err(err.to_string()))
    }

    pub fn decode_video_registry_json(
        &self,
        video_registry_json: &str,
        bundle_root: Option<String>,
    ) -> PyResult<HashMap<String, Vec<PyImageFrame>>> {
        self.decode_video_registry_inner(video_registry_json, bundle_root)
            .map_err(|err| PyRuntimeError::new_err(err.to_string()))
    }
}

impl PyVideoDecoder {
    fn execute(&self, context: Context) -> Result<Context, Box<dyn Error>> {
        let mut stage = self.config.build();
        stage
            .run(context)
            .map_err(|err| Box::new(io::Error::other(err.reason().to_string())) as Box<dyn Error>)
    }

    fn decode_rgb_paths_inner(
        &self,
        video_paths: HashMap<String, String>,
    ) -> Result<HashMap<String, Vec<PyImageFrame>>, Box<dyn Error>> {
        let video_paths = video_paths
            .into_iter()
            .map(|(topic, path)| (topic, camino::Utf8PathBuf::from(path)))
            .collect();
        let image_data = decode_rgb_video_paths(&video_paths)
            .map_err(|err| io::Error::other(err.reason().to_string()))?;

        Ok(image_data
            .into_iter()
            .map(|(topic, frames)| {
                let frames = frames
                    .into_iter()
                    .map(|frame| PyImageFrame { inner: frame })
                    .collect();
                (topic, frames)
            })
            .collect())
    }

    fn decode_video_registry_inner(
        &self,
        video_registry_json: &str,
        bundle_root: Option<String>,
    ) -> Result<HashMap<String, Vec<PyImageFrame>>, Box<dyn Error>> {
        let video_registry: HashMap<String, VideoArtifact> =
            serde_json::from_str(video_registry_json).map_err(|err| {
                io::Error::other(format!("failed to parse video registry JSON: {err}"))
            })?;
        let bundle_root = bundle_root.as_deref().map(camino::Utf8Path::new);
        let image_data = decode_video_registry(&video_registry, bundle_root)
            .map_err(|err| io::Error::other(err.reason().to_string()))?;

        Ok(image_data
            .into_iter()
            .map(|(topic, frames)| {
                let frames = frames
                    .into_iter()
                    .map(|frame| PyImageFrame { inner: frame })
                    .collect();
                (topic, frames)
            })
            .collect())
    }
}
