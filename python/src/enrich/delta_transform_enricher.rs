use std::mem;

use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;

use rebake::core::Stage;
use rebake::core::stage::{Context, StageError};
use rebake::enrich::delta_transform_enricher::{
    DeltaReferenceFrame, DeltaTransformEnricher, DeltaTransformEnricherConfig,
};

use crate::core::PyContext;

#[pyclass]
#[derive(Clone)]
pub struct PyDeltaTransformEnricherConfig {
    inner: DeltaTransformEnricherConfig,
}

impl PyDeltaTransformEnricherConfig {
    pub(crate) fn clone_inner(&self) -> DeltaTransformEnricherConfig {
        self.inner.clone()
    }
}

fn parse_delta_reference_frame(value: &str) -> PyResult<DeltaReferenceFrame> {
    match value {
        "source_frame" => Ok(DeltaReferenceFrame::SourceFrame),
        "previous_target_frame" => Ok(DeltaReferenceFrame::PreviousTargetFrame),
        other => Err(PyValueError::new_err(format!(
            "invalid delta_reference_frame: {other}; expected 'source_frame' or 'previous_target_frame'"
        ))),
    }
}

fn delta_reference_frame_to_str(value: DeltaReferenceFrame) -> &'static str {
    match value {
        DeltaReferenceFrame::SourceFrame => "source_frame",
        DeltaReferenceFrame::PreviousTargetFrame => "previous_target_frame",
    }
}

#[pymethods]
impl PyDeltaTransformEnricherConfig {
    #[new]
    pub fn new(topic_names: Vec<String>, delta_reference_frame: String) -> PyResult<Self> {
        let delta_reference_frame = parse_delta_reference_frame(&delta_reference_frame)?;
        Ok(Self {
            inner: DeltaTransformEnricherConfig::new(topic_names, delta_reference_frame),
        })
    }

    #[getter]
    pub fn topic_names(&self) -> Vec<String> {
        self.inner.topic_names.clone()
    }

    #[setter]
    pub fn set_topic_names(&mut self, topic_names: Vec<String>) {
        self.inner.topic_names = topic_names;
    }

    #[getter]
    pub fn delta_reference_frame(&self) -> String {
        delta_reference_frame_to_str(self.inner.delta_reference_frame).to_string()
    }

    #[setter]
    pub fn set_delta_reference_frame(&mut self, delta_reference_frame: String) -> PyResult<()> {
        self.inner.delta_reference_frame = parse_delta_reference_frame(&delta_reference_frame)?;
        Ok(())
    }

    pub fn build(&self) -> PyDeltaTransformEnricher {
        PyDeltaTransformEnricher {
            config: self.inner.clone(),
        }
    }
}

#[pyclass]
pub struct PyDeltaTransformEnricher {
    config: DeltaTransformEnricherConfig,
}

#[pymethods]
impl PyDeltaTransformEnricher {
    #[new]
    pub fn new(config: &PyDeltaTransformEnricherConfig) -> Self {
        Self {
            config: config.clone_inner(),
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
}

impl PyDeltaTransformEnricher {
    fn execute(&self, context: Context) -> Result<Context, StageError> {
        let mut enricher = DeltaTransformEnricher::new(self.config.clone());
        enricher.run(context)
    }
}
