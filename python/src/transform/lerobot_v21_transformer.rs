use std::{error::Error, io, mem};

use pyo3::Bound;
use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;

use rebake::core::stage::{Context, Stage};
use rebake::transform::lerobot_v21::lerobot_v21_transformer::{
    LeRobotV21Transformer, LeRobotV21TransformerConfig,
};

use crate::core::PyContext;

#[pyclass(module = "rebake.transform")]
pub struct PyLeRobotV21TransformerConfig {
    inner: LeRobotV21TransformerConfig,
}

#[pymethods]
impl PyLeRobotV21TransformerConfig {
    #[new]
    pub fn new(config_json: String) -> PyResult<Self> {
        let config: LeRobotV21TransformerConfig = serde_json::from_str(&config_json)
            .map_err(|e| PyValueError::new_err(format!("{e}")))?;
        Ok(Self { inner: config })
    }

    pub fn build(&self) -> PyLeRobotV21Transformer {
        PyLeRobotV21Transformer {
            config: self.inner.clone(),
        }
    }
}

#[pyclass(module = "rebake.transform")]
pub struct PyLeRobotV21Transformer {
    config: LeRobotV21TransformerConfig,
}

#[pymethods]
impl PyLeRobotV21Transformer {
    #[new]
    pub fn new(config: &PyLeRobotV21TransformerConfig) -> Self {
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
}

impl PyLeRobotV21Transformer {
    fn execute(&self, context: Context) -> Result<Context, Box<dyn Error>> {
        let mut stage = LeRobotV21Transformer::new(self.config.clone());
        stage
            .run(context)
            .map_err(|err| Box::new(io::Error::other(err.reason().to_string())) as Box<dyn Error>)
    }
}
