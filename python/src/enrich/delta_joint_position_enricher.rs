use std::mem;

use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;

use rebake::core::Stage;
use rebake::core::stage::{Context, StageError};
use rebake::enrich::delta_joint_position_enricher::{
    DeltaJointPositionEnricher, DeltaJointPositionEnricherConfig,
};

use crate::core::PyContext;

#[pyclass]
#[derive(Clone)]
pub struct PyDeltaJointPositionEnricherConfig {
    inner: DeltaJointPositionEnricherConfig,
}

impl PyDeltaJointPositionEnricherConfig {
    pub(crate) fn clone_inner(&self) -> DeltaJointPositionEnricherConfig {
        self.inner.clone()
    }
}

#[pymethods]
impl PyDeltaJointPositionEnricherConfig {
    #[new]
    pub fn new(topic_names: Vec<String>) -> Self {
        Self {
            inner: DeltaJointPositionEnricherConfig::new(topic_names),
        }
    }

    #[getter]
    pub fn topic_names(&self) -> Vec<String> {
        self.inner.topic_names.clone()
    }

    #[setter]
    pub fn set_topic_names(&mut self, topic_names: Vec<String>) {
        self.inner.topic_names = topic_names;
    }

    pub fn build(&self) -> PyDeltaJointPositionEnricher {
        PyDeltaJointPositionEnricher {
            config: self.inner.clone(),
        }
    }
}

#[pyclass]
pub struct PyDeltaJointPositionEnricher {
    config: DeltaJointPositionEnricherConfig,
}

#[pymethods]
impl PyDeltaJointPositionEnricher {
    #[new]
    pub fn new(config: &PyDeltaJointPositionEnricherConfig) -> Self {
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

impl PyDeltaJointPositionEnricher {
    fn execute(&self, context: Context) -> Result<Context, StageError> {
        let mut enricher = DeltaJointPositionEnricher::new(self.config.clone());
        enricher.run(context)
    }
}
