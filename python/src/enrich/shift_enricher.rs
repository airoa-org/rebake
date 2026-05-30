use std::mem;

use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;

use rebake::core::Stage;
use rebake::core::stage::{Context, StageError};
use rebake::enrich::shift_enricher::{FillStrategy, ShiftEnricher, ShiftEnricherConfig};

use crate::core::PyContext;

fn parse_fill_strategy(s: &str) -> PyResult<FillStrategy> {
    match s {
        "edge" => Ok(FillStrategy::Edge),
        "zero" => Ok(FillStrategy::Zero),
        other => Err(PyRuntimeError::new_err(format!(
            "Invalid fill_strategy: '{other}'. Expected 'edge' or 'zero'."
        ))),
    }
}

fn fill_strategy_to_str(strategy: &FillStrategy) -> &'static str {
    match strategy {
        FillStrategy::Edge => "edge",
        FillStrategy::Zero => "zero",
    }
}

#[pyclass]
#[derive(Clone)]
pub struct PyShiftEnricherConfig {
    inner: ShiftEnricherConfig,
}

impl PyShiftEnricherConfig {
    pub(crate) fn clone_inner(&self) -> ShiftEnricherConfig {
        self.inner.clone()
    }
}

#[pymethods]
impl PyShiftEnricherConfig {
    #[new]
    #[pyo3(signature = (source_topic, output_topic, shift_steps, fill_strategy = "edge"))]
    pub fn new(
        source_topic: String,
        output_topic: String,
        shift_steps: i64,
        fill_strategy: &str,
    ) -> PyResult<Self> {
        let strategy = parse_fill_strategy(fill_strategy)?;
        Ok(Self {
            inner: ShiftEnricherConfig {
                source_topic,
                output_topic,
                shift_steps,
                fill_strategy: strategy,
            },
        })
    }

    #[getter]
    pub fn source_topic(&self) -> &str {
        &self.inner.source_topic
    }

    #[setter]
    pub fn set_source_topic(&mut self, source_topic: String) {
        self.inner.source_topic = source_topic;
    }

    #[getter]
    pub fn output_topic(&self) -> &str {
        &self.inner.output_topic
    }

    #[setter]
    pub fn set_output_topic(&mut self, output_topic: String) {
        self.inner.output_topic = output_topic;
    }

    #[getter]
    pub fn shift_steps(&self) -> i64 {
        self.inner.shift_steps
    }

    #[setter]
    pub fn set_shift_steps(&mut self, shift_steps: i64) {
        self.inner.shift_steps = shift_steps;
    }

    #[getter]
    pub fn fill_strategy(&self) -> &'static str {
        fill_strategy_to_str(&self.inner.fill_strategy)
    }

    #[setter]
    pub fn set_fill_strategy(&mut self, fill_strategy: &str) -> PyResult<()> {
        self.inner.fill_strategy = parse_fill_strategy(fill_strategy)?;
        Ok(())
    }

    pub fn build(&self) -> PyShiftEnricher {
        PyShiftEnricher {
            config: self.inner.clone(),
        }
    }
}

#[pyclass]
pub struct PyShiftEnricher {
    config: ShiftEnricherConfig,
}

#[pymethods]
impl PyShiftEnricher {
    #[new]
    pub fn new(config: &PyShiftEnricherConfig) -> Self {
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

impl PyShiftEnricher {
    fn execute(&self, context: Context) -> Result<Context, StageError> {
        let mut enricher = ShiftEnricher::new(self.config.clone());
        enricher.run(context)
    }
}
