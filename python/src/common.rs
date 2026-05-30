use pyo3::prelude::*;
use rebake::common::{DepthFrame, ImageFrame, ImageShape};

#[pyclass(module = "rebake.common")]
#[derive(Clone)]
pub struct PyImageShape {
    pub inner: ImageShape,
}

#[pymethods]
impl PyImageShape {
    #[new]
    pub fn new(height: usize, width: usize, channels: usize) -> Self {
        Self {
            inner: ImageShape {
                height,
                width,
                channels,
            },
        }
    }

    #[getter]
    pub fn height(&self) -> usize {
        self.inner.height
    }

    #[getter]
    pub fn width(&self) -> usize {
        self.inner.width
    }

    #[getter]
    pub fn channels(&self) -> usize {
        self.inner.channels
    }

    /// Pickle support: return constructor arguments for unpickling.
    pub fn __getnewargs__(&self) -> (usize, usize, usize) {
        (self.inner.height, self.inner.width, self.inner.channels)
    }
}

#[pyclass(module = "rebake.common")]
#[derive(Clone)]
pub struct PyImageFrame {
    pub inner: ImageFrame,
}

#[pymethods]
impl PyImageFrame {
    #[new]
    pub fn new(index: u32, extension: String, bytes: Vec<u8>, shape: Option<PyImageShape>) -> Self {
        let mut frame = ImageFrame::new(index, extension, bytes);
        if let Some(s) = shape {
            frame.set_shape(s.inner);
        }
        Self { inner: frame }
    }

    #[getter]
    pub fn index(&self) -> u32 {
        self.inner.index
    }

    #[getter]
    pub fn extension(&self) -> String {
        self.inner.extension.clone()
    }

    #[getter]
    pub fn bytes(&self) -> Vec<u8> {
        self.inner.bytes.clone()
    }

    #[getter]
    pub fn shape(&self) -> Option<PyImageShape> {
        self.inner.shape.map(|s| PyImageShape { inner: s })
    }

    /// Pickle support: return constructor arguments for unpickling.
    pub fn __getnewargs__(&self) -> (u32, String, Vec<u8>, Option<PyImageShape>) {
        (
            self.inner.index,
            self.inner.extension.clone(),
            self.inner.bytes.clone(),
            self.shape(),
        )
    }
}

#[pyclass(module = "rebake.common")]
#[derive(Clone)]
pub struct PyDepthFrame {
    pub inner: DepthFrame,
}

#[pymethods]
impl PyDepthFrame {
    #[new]
    #[pyo3(signature = (index, extension, bytes, ros_format=None))]
    pub fn new(index: u32, extension: String, bytes: Vec<u8>, ros_format: Option<String>) -> Self {
        let mut frame = DepthFrame::new(index, extension, bytes);
        if let Some(fmt) = ros_format {
            frame.set_ros_format(fmt);
        }
        Self { inner: frame }
    }

    #[getter]
    pub fn index(&self) -> u32 {
        self.inner.index
    }

    #[getter]
    pub fn extension(&self) -> String {
        self.inner.extension.clone()
    }

    #[getter]
    pub fn bytes(&self) -> Vec<u8> {
        self.inner.bytes.clone()
    }

    #[getter]
    pub fn ros_format(&self) -> Option<String> {
        self.inner.ros_format.clone()
    }

    /// Pickle support: return constructor arguments for unpickling.
    pub fn __getnewargs__(&self) -> (u32, String, Vec<u8>, Option<String>) {
        (
            self.inner.index,
            self.inner.extension.clone(),
            self.inner.bytes.clone(),
            self.inner.ros_format.clone(),
        )
    }
}

use pyo3::types::PyModuleMethods;

#[pyo3::pymodule(name = "common")]
pub fn common(
    _py: pyo3::Python<'_>,
    m: &pyo3::Bound<'_, pyo3::types::PyModule>,
) -> pyo3::PyResult<()> {
    m.add_class::<PyImageShape>()?;
    m.add_class::<PyImageFrame>()?;
    m.add_class::<PyDepthFrame>()?;
    Ok(())
}
