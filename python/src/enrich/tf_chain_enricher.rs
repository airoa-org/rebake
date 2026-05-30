use std::mem;

use pyo3::Bound;
use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;

use rebake::core::stage::Stage;
use rebake::enrich::tf_chain_enricher::{FramePair, TfChainEnricher, TfChainEnricherConfig};

use crate::core::PyContext;

#[pyclass(module = "rebake.enrich")]
pub struct PyFramePair {
    inner: FramePair,
}

#[pymethods]
impl PyFramePair {
    #[new]
    pub fn new(source: String, target: String) -> Self {
        Self {
            inner: FramePair { source, target },
        }
    }

    #[getter]
    pub fn source(&self) -> String {
        self.inner.source.clone()
    }

    #[getter]
    pub fn target(&self) -> String {
        self.inner.target.clone()
    }
}

impl PyFramePair {
    pub(crate) fn clone_inner(&self) -> FramePair {
        self.inner.clone()
    }
}

#[pyclass(module = "rebake.enrich")]
pub struct PyTfChainEnricherConfig {
    inner: TfChainEnricherConfig,
}

impl PyTfChainEnricherConfig {
    pub(crate) fn clone_inner(&self) -> TfChainEnricherConfig {
        self.inner.clone()
    }
}

#[pymethods]
impl PyTfChainEnricherConfig {
    #[new]
    pub fn new(py: Python<'_>, frame_pairs: Vec<Py<PyFramePair>>) -> PyResult<Self> {
        let inner_pairs = frame_pairs
            .into_iter()
            .map(|pair| Ok(pair.borrow(py).clone_inner()))
            .collect::<PyResult<Vec<_>>>()?;

        Ok(Self {
            inner: TfChainEnricherConfig::new(inner_pairs),
        })
    }

    pub fn build(&self) -> PyTfChainEnricher {
        PyTfChainEnricher {
            config: self.inner.clone(),
        }
    }
}

#[pyclass(module = "rebake.enrich")]
pub struct PyTfChainEnricher {
    config: TfChainEnricherConfig,
}

#[pymethods]
impl PyTfChainEnricher {
    #[new]
    pub fn new(config: &PyTfChainEnricherConfig) -> Self {
        Self {
            config: config.clone_inner(),
        }
    }

    pub fn run(&self, context: Bound<'_, PyContext>) -> PyResult<()> {
        let mut guard = context.borrow_mut();
        let current = mem::take(&mut guard.inner);
        let mut enricher = TfChainEnricher::new(self.config.clone());
        let updated = enricher
            .run(current)
            .map_err(|err| PyRuntimeError::new_err(err.to_string()))?;
        guard.inner = updated;
        Ok(())
    }
}
