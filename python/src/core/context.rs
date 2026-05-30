use std::collections::HashMap;

use arrow::record_batch::RecordBatch;
use arrow_pyarrow::PyArrowType;
use camino::Utf8PathBuf;
use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyDictMethods};
use pyo3::{Bound, IntoPyObject};

use rebake::core::conversion::{lazy_to_record_batch_rechunk, record_batch_to_lazy};
use rebake::core::stage::Context;
use rebake::encode::video_artifact::VideoArtifact;
use rebake::schema::metadata::arrow::airoa_metadata_to_record_batch;
use rebake::schema::metadata::parse_metadata;

#[pyclass(module = "rebake.core")]
#[derive(Default)]
pub struct PyContext {
    pub(crate) inner: Context,
}

#[pymethods]
impl PyContext {
    #[new]
    pub fn new() -> Self {
        Self {
            inner: Context::default(),
        }
    }

    #[staticmethod]
    pub fn from_record_batches(batches: Bound<'_, PyDict>) -> PyResult<Self> {
        let mut dataset = HashMap::with_capacity(batches.len());
        for (key, value) in batches.iter() {
            let topic = key.extract::<String>()?;
            let batch: PyArrowType<RecordBatch> = value.extract()?;
            dataset.insert(topic, record_batch_to_lazy(&batch.0));
        }
        Ok(Self {
            inner: Context::new(dataset),
        })
    }

    pub fn dataset_topics(&self) -> Vec<String> {
        self.inner
            .dataset
            .as_ref()
            .map(|dataset| dataset.keys().cloned().collect())
            .unwrap_or_default()
    }

    pub fn topic_message_type(&self, topic: &str) -> Option<String> {
        self.inner
            .topic_message_type_map
            .as_ref()
            .and_then(|map| map.get(topic).cloned())
    }

    /// Get the full topic to message type mapping.
    ///
    /// Returns a dictionary mapping topic names to their ROS message types,
    /// or None if no mapping is available.
    pub fn get_topic_message_type_map(&self) -> Option<HashMap<String, String>> {
        self.inner.topic_message_type_map.clone()
    }

    /// Set the topic to message type mapping.
    ///
    /// Args:
    ///     map: A dictionary mapping topic names to their ROS message types,
    ///          or None to clear the mapping.
    pub fn set_topic_message_type_map(&mut self, map: Option<HashMap<String, String>>) {
        self.inner.topic_message_type_map = map;
    }

    pub fn get_record_batch(&self, topic: &str) -> PyResult<PyArrowType<RecordBatch>> {
        let dataset = self
            .inner
            .dataset
            .as_ref()
            .ok_or_else(|| PyRuntimeError::new_err("dataset is empty"))?;
        let frame = dataset
            .get(topic)
            .ok_or_else(|| PyRuntimeError::new_err(format!("topic not found: {topic}")))?;
        Ok(PyArrowType(lazy_to_record_batch_rechunk(frame)))
    }

    pub fn set_record_batch(
        &mut self,
        topic: &str,
        batch: PyArrowType<RecordBatch>,
    ) -> PyResult<()> {
        self.inner
            .dataset
            .get_or_insert_with(HashMap::new)
            .insert(topic.to_string(), record_batch_to_lazy(&batch.0));
        Ok(())
    }

    pub fn to_record_batches(&self, py: Python<'_>) -> PyResult<Py<PyDict>> {
        let dict = PyDict::new(py);
        if let Some(dataset) = self.inner.dataset.as_ref() {
            for (topic, frame) in dataset {
                let batch = PyArrowType(lazy_to_record_batch_rechunk(frame));
                dict.set_item(topic, batch.into_pyobject(py)?)?;
            }
        }
        Ok(dict.unbind())
    }

    #[getter]
    pub fn fps(&self) -> Option<usize> {
        self.inner.fps
    }

    #[setter]
    pub fn set_fps(&mut self, value: Option<usize>) {
        self.inner.fps = value;
    }

    #[getter]
    pub fn output_dir(&self) -> Option<String> {
        self.inner.output_dir.as_ref().map(|path| path.to_string())
    }

    #[setter]
    pub fn set_output_dir(&mut self, value: Option<&str>) {
        self.inner.output_dir = value.map(Utf8PathBuf::from);
    }

    #[getter]
    pub fn video_cache_dir(&self) -> Option<String> {
        self.inner
            .video_cache_dir
            .as_ref()
            .map(|path| path.to_string())
    }

    #[setter]
    pub fn set_video_cache_dir(&mut self, value: Option<&str>) {
        self.inner.video_cache_dir = value.map(Utf8PathBuf::from);
    }

    #[getter]
    pub fn rosbag_path(&self) -> Option<String> {
        self.inner.rosbag_path.as_ref().map(|path| path.to_string())
    }

    #[setter]
    pub fn set_rosbag_path(&mut self, value: Option<&str>) {
        self.inner.rosbag_path = value.map(Utf8PathBuf::from);
    }

    #[getter]
    pub fn bundle_root(&self) -> Option<String> {
        self.inner.bundle_root.as_ref().map(|path| path.to_string())
    }

    #[setter]
    pub fn set_bundle_root(&mut self, value: Option<&str>) {
        self.inner.bundle_root = value.map(Utf8PathBuf::from);
    }

    pub fn get_image_data(&self) -> Option<HashMap<String, Vec<crate::common::PyImageFrame>>> {
        self.inner.image_data.as_ref().map(|data| {
            data.iter()
                .map(|(topic, frames)| {
                    (
                        topic.clone(),
                        frames
                            .iter()
                            .map(|f| crate::common::PyImageFrame { inner: f.clone() })
                            .collect(),
                    )
                })
                .collect()
        })
    }

    pub fn set_image_data(
        &mut self,
        data: Option<HashMap<String, Vec<crate::common::PyImageFrame>>>,
    ) {
        self.inner.image_data = data.map(|d| {
            d.into_iter()
                .map(|(topic, frames)| (topic, frames.into_iter().map(|f| f.inner).collect()))
                .collect()
        });
    }

    pub fn get_depth_data(&self) -> Option<HashMap<String, Vec<crate::common::PyDepthFrame>>> {
        self.inner.depth_data.as_ref().map(|data| {
            data.iter()
                .map(|(topic, frames)| {
                    (
                        topic.clone(),
                        frames
                            .iter()
                            .map(|f| crate::common::PyDepthFrame { inner: f.clone() })
                            .collect(),
                    )
                })
                .collect()
        })
    }

    pub fn set_depth_data(
        &mut self,
        data: Option<HashMap<String, Vec<crate::common::PyDepthFrame>>>,
    ) {
        self.inner.depth_data = data.map(|d| {
            d.into_iter()
                .map(|(topic, frames)| (topic, frames.into_iter().map(|f| f.inner).collect()))
                .collect()
        });
    }

    /// Get the airoa metadata as a JSON string.
    pub fn get_airoa_metadata_json(&self) -> PyResult<Option<String>> {
        match &self.inner.airoa_metadata {
            Some(metadata) => {
                let json = serde_json::to_string(metadata).map_err(|e| {
                    PyRuntimeError::new_err(format!("Failed to serialize metadata: {e}"))
                })?;
                Ok(Some(json))
            }
            None => Ok(None),
        }
    }

    /// Set the airoa metadata from a JSON string.
    ///
    /// Supports both V1.3 and V2.0 metadata formats. The metadata is stored
    /// in its original format and converted to V2.0 when needed.
    /// V2.0 inputs are validated against schema constraints at this boundary.
    pub fn set_airoa_metadata_json(&mut self, json: &str) -> PyResult<()> {
        let metadata = parse_metadata(json)
            .map_err(|e| PyRuntimeError::new_err(format!("Failed to parse metadata JSON: {e}")))?;
        if let rebake::schema::metadata::AiroaMetadata::V2_0(ref v2) = metadata {
            crate::core::metadata::validate_metadata(v2)?;
        }
        self.inner.set_airoa_metadata(metadata);
        Ok(())
    }

    /// Set the airoa metadata directly from a typed MetadataV2_0 object.
    ///
    /// Boundary validation runs here: if the metadata violates schema
    /// constraints (e.g. empty files, missing episode label), a ValueError
    /// is raised before the metadata reaches downstream stages.
    pub fn set_airoa_metadata(
        &mut self,
        metadata: crate::core::metadata::PyMetadataV2_0,
    ) -> PyResult<()> {
        let inner: rebake::schema::metadata::v2_0::MetadataV2_0 = metadata.into();
        crate::core::metadata::validate_metadata(&inner)?;
        self.inner
            .set_airoa_metadata(rebake::schema::metadata::AiroaMetadata::V2_0(inner));
        Ok(())
    }

    /// Get the airoa metadata as an Arrow RecordBatch.
    ///
    /// This preserves the metadata in its original format (V1.3 or V2.0).
    /// The Arrow schema will match the stored version.
    ///
    /// For V2.0 metadata, the structure includes:
    /// - robot as `Struct<uri, robot_type, id, checksum>`
    /// - environment as `Struct<env_type, site, location>`
    /// - runner as `Struct<runner_type, organization, name>`
    /// - devices as `List<Struct<role, device_type, id>>`
    /// - programs as `List<Struct<role, name, source>>`
    /// - episode as `Struct<start_time, end_time, success, label>`
    /// - labels as `List<String>`
    /// - segments as `List<Struct<start_time, end_time, label_idx, success>>`
    pub fn get_metadata_record_batch(&self) -> PyResult<PyArrowType<RecordBatch>> {
        let metadata = self
            .inner
            .airoa_metadata
            .as_ref()
            .ok_or_else(|| PyRuntimeError::new_err("no metadata available"))?;
        let batch = airoa_metadata_to_record_batch(metadata)
            .map_err(|e| PyRuntimeError::new_err(format!("Failed to convert metadata: {e}")))?;
        Ok(PyArrowType(batch))
    }

    /// Get the video paths stored in the context.
    ///
    /// Returns a dictionary mapping topic names to video file paths,
    /// or None if no video paths are set.
    pub fn get_video_paths(&self) -> Option<HashMap<String, String>> {
        self.inner.resolve_video_paths().ok().map(|paths| {
            paths
                .into_iter()
                .map(|(topic, path)| (topic, path.to_string()))
                .collect()
        })
    }

    /// Set the canonical video registry from a JSON string.
    ///
    /// The JSON must decode to ``dict[str, VideoArtifact]`` and becomes the
    /// source of truth for lazy video access inside rebake.
    pub fn set_video_registry_json(&mut self, json: &str) -> PyResult<()> {
        let video_registry: HashMap<String, VideoArtifact> =
            serde_json::from_str(json).map_err(|err| {
                PyRuntimeError::new_err(format!("Failed to parse video registry JSON: {err}"))
            })?;
        self.inner.set_video_registry(video_registry);
        Ok(())
    }

    /// Save the context dataset to Parquet files.
    ///
    /// This mirrors Orchestrator's save_context(), saving each topic's
    /// data as a separate Parquet file.
    ///
    /// Args:
    ///     output_dir: Directory to save Parquet files to.
    ///
    /// Output structure:
    ///     {output_dir}/{topic}.parquet
    ///
    /// Example:
    ///     >>> context.save_to_parquet("./parquet_output")
    ///     # Creates: ./parquet_output/joint_states.parquet
    ///     #          ./parquet_output/camera/image.parquet
    pub fn save_to_parquet(&self, output_dir: &str) -> PyResult<()> {
        let path = camino::Utf8Path::new(output_dir);
        self.inner
            .save_to_parquet(path)
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))
    }
}
