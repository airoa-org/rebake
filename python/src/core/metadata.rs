use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::PyType;

use rebake::schema::metadata::parse_metadata_as_v2_0 as schema_parse_metadata_as_v2_0;
use rebake::schema::metadata::v2_0::{
    Device as CoreDevice, EnvType as CoreEnvType, Environment as CoreEnvironment,
    Episode as CoreEpisode, File as CoreFile, GitSource as CoreGitSource,
    MetadataV2_0 as CoreMetadataV2_0, Program as CoreProgram, Robot as CoreRobot,
    Runner as CoreRunner, RunnerType as CoreRunnerType, Segment as CoreSegment,
    Source as CoreSource,
};

// =============================================================================
// Validators
// =============================================================================

pub(crate) fn validate_metadata(m: &CoreMetadataV2_0) -> PyResult<()> {
    if m.files.is_empty() {
        return Err(PyValueError::new_err(
            "MetadataV2_0.files must have at least one entry",
        ));
    }
    if m.programs.is_empty() {
        return Err(PyValueError::new_err(
            "MetadataV2_0.programs must have at least one entry",
        ));
    }
    if m.uuid.is_empty() {
        return Err(PyValueError::new_err("MetadataV2_0.uuid must be non-empty"));
    }
    if m.episode.label.is_empty() {
        return Err(PyValueError::new_err(
            "MetadataV2_0.episode.label must be non-empty",
        ));
    }
    for (i, f) in m.files.iter().enumerate() {
        if f.name.is_empty() {
            return Err(PyValueError::new_err(format!(
                "MetadataV2_0.files[{i}].name must be non-empty"
            )));
        }
    }
    for (i, p) in m.programs.iter().enumerate() {
        if p.role.is_empty() {
            return Err(PyValueError::new_err(format!(
                "MetadataV2_0.programs[{i}].role must be non-empty"
            )));
        }
        if p.name.is_empty() {
            return Err(PyValueError::new_err(format!(
                "MetadataV2_0.programs[{i}].name must be non-empty"
            )));
        }
    }
    Ok(())
}

const V2_0_SCHEMA_URL: &str =
    "https://raw.githubusercontent.com/airoa-org/airoa-metadata/main/airoa_metadata/schemas/v2_0.json";

// =============================================================================
// parse_metadata_as_v2_0 (back-compat helper)
// =============================================================================

#[pyfunction]
pub fn parse_metadata_as_v2_0(metadata_json: &str) -> PyResult<PyMetadataV2_0> {
    let metadata = schema_parse_metadata_as_v2_0(metadata_json)
        .map_err(|e| PyRuntimeError::new_err(format!("Failed to parse metadata as V2.0: {e}")))?;
    Ok(metadata.into())
}

// =============================================================================
// MetadataV2_0
// =============================================================================

#[pyclass(module = "rebake.core", name = "MetadataV2_0", eq)]
#[derive(Clone, PartialEq)]
pub struct PyMetadataV2_0 {
    pub(crate) inner: CoreMetadataV2_0,
}

impl From<CoreMetadataV2_0> for PyMetadataV2_0 {
    fn from(inner: CoreMetadataV2_0) -> Self {
        Self { inner }
    }
}

impl From<PyMetadataV2_0> for CoreMetadataV2_0 {
    fn from(value: PyMetadataV2_0) -> Self {
        value.inner
    }
}

#[pymethods]
impl PyMetadataV2_0 {
    #[new]
    #[pyo3(signature = (
        episode,
        files,
        programs,
        *,
        uuid = None,
        robot = None,
        environment = None,
        runner = None,
        devices = None,
        labels = None,
        segments = None,
    ))]
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        episode: PyEpisode,
        files: Vec<PyFile>,
        programs: Vec<PyProgram>,
        uuid: Option<String>,
        robot: Option<PyRobot>,
        environment: Option<PyEnvironment>,
        runner: Option<PyRunner>,
        devices: Option<Vec<PyDevice>>,
        labels: Option<Vec<String>>,
        segments: Option<Vec<PySegment>>,
    ) -> PyResult<Self> {
        if files.is_empty() {
            return Err(PyValueError::new_err(
                "MetadataV2_0.files must have at least one entry",
            ));
        }
        if programs.is_empty() {
            return Err(PyValueError::new_err(
                "MetadataV2_0.programs must have at least one entry",
            ));
        }
        let inner = CoreMetadataV2_0 {
            schema: V2_0_SCHEMA_URL.to_string(),
            schema_version: "2.0".to_string(),
            uuid: uuid.unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
            robot: robot.map(Into::into).unwrap_or_default(),
            files: files.into_iter().map(Into::into).collect(),
            environment: environment.map(Into::into).unwrap_or_default(),
            runner: runner.map(Into::into).unwrap_or_default(),
            devices: devices
                .unwrap_or_default()
                .into_iter()
                .map(Into::into)
                .collect(),
            programs: programs.into_iter().map(Into::into).collect(),
            episode: episode.into(),
            labels: labels.unwrap_or_default(),
            segments: segments
                .unwrap_or_default()
                .into_iter()
                .map(Into::into)
                .collect(),
        };
        Ok(Self { inner })
    }

    // === Getters ===

    #[getter]
    pub fn schema(&self) -> String {
        self.inner.schema.clone()
    }

    #[getter]
    pub fn schema_version(&self) -> String {
        self.inner.schema_version.clone()
    }

    #[getter]
    pub fn uuid(&self) -> String {
        self.inner.uuid.clone()
    }

    #[getter]
    pub fn robot(&self) -> PyRobot {
        self.inner.robot.clone().into()
    }

    #[getter]
    pub fn files(&self) -> Vec<PyFile> {
        self.inner.files.iter().cloned().map(PyFile::from).collect()
    }

    #[getter]
    pub fn environment(&self) -> PyEnvironment {
        self.inner.environment.clone().into()
    }

    #[getter]
    pub fn runner(&self) -> PyRunner {
        self.inner.runner.clone().into()
    }

    #[getter]
    pub fn devices(&self) -> Vec<PyDevice> {
        self.inner
            .devices
            .iter()
            .cloned()
            .map(PyDevice::from)
            .collect()
    }

    #[getter]
    pub fn programs(&self) -> Vec<PyProgram> {
        self.inner
            .programs
            .iter()
            .cloned()
            .map(PyProgram::from)
            .collect()
    }

    #[getter]
    pub fn episode(&self) -> PyEpisode {
        self.inner.episode.clone().into()
    }

    #[getter]
    pub fn labels(&self) -> Vec<String> {
        self.inner.labels.clone()
    }

    #[getter]
    pub fn segments(&self) -> Vec<PySegment> {
        self.inner
            .segments
            .iter()
            .cloned()
            .map(PySegment::from)
            .collect()
    }

    // === Setters ===

    #[setter]
    pub fn set_uuid(&mut self, value: String) {
        self.inner.uuid = value;
    }

    #[setter]
    pub fn set_robot(&mut self, value: PyRobot) {
        self.inner.robot = value.into();
    }

    #[setter]
    pub fn set_files(&mut self, value: Vec<PyFile>) {
        self.inner.files = value.into_iter().map(Into::into).collect();
    }

    #[setter]
    pub fn set_environment(&mut self, value: PyEnvironment) {
        self.inner.environment = value.into();
    }

    #[setter]
    pub fn set_runner(&mut self, value: PyRunner) {
        self.inner.runner = value.into();
    }

    #[setter]
    pub fn set_devices(&mut self, value: Vec<PyDevice>) {
        self.inner.devices = value.into_iter().map(Into::into).collect();
    }

    #[setter]
    pub fn set_programs(&mut self, value: Vec<PyProgram>) {
        self.inner.programs = value.into_iter().map(Into::into).collect();
    }

    #[setter]
    pub fn set_episode(&mut self, value: PyEpisode) {
        self.inner.episode = value.into();
    }

    #[setter]
    pub fn set_labels(&mut self, value: Vec<String>) {
        self.inner.labels = value;
    }

    #[setter]
    pub fn set_segments(&mut self, value: Vec<PySegment>) {
        self.inner.segments = value.into_iter().map(Into::into).collect();
    }

    // === Serialization ===

    pub fn to_json(&self) -> PyResult<String> {
        validate_metadata(&self.inner)?;
        serde_json::to_string(&self.inner)
            .map_err(|e| PyRuntimeError::new_err(format!("Failed to serialize metadata: {e}")))
    }

    pub fn to_dict<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let json_str = self.to_json()?;
        py.import("json")?.call_method1("loads", (json_str,))
    }

    #[classmethod]
    pub fn from_json(_cls: &Bound<'_, PyType>, json_str: &str) -> PyResult<Self> {
        let inner = schema_parse_metadata_as_v2_0(json_str)
            .map_err(|e| PyRuntimeError::new_err(format!("Failed to parse JSON: {e}")))?;
        validate_metadata(&inner)?;
        Ok(Self { inner })
    }

    #[classmethod]
    pub fn from_dict(
        cls: &Bound<'_, PyType>,
        py: Python<'_>,
        data: &Bound<'_, PyAny>,
    ) -> PyResult<Self> {
        let json_str: String = py
            .import("json")?
            .call_method1("dumps", (data,))?
            .extract()?;
        Self::from_json(cls, &json_str)
    }

    pub fn __repr__(&self) -> String {
        format!(
            "MetadataV2_0(uuid={:?}, schema_version={:?})",
            self.inner.uuid, self.inner.schema_version
        )
    }
}

// =============================================================================
// EnvType (enum)
// =============================================================================

#[pyclass(frozen, module = "rebake.core", name = "EnvType", eq)]
#[derive(Clone, PartialEq)]
pub enum PyEnvType {
    RealWorld,
    Simulation,
}

impl From<CoreEnvType> for PyEnvType {
    fn from(value: CoreEnvType) -> Self {
        match value {
            CoreEnvType::RealWorld => Self::RealWorld,
            CoreEnvType::Simulation => Self::Simulation,
        }
    }
}

impl From<PyEnvType> for CoreEnvType {
    fn from(value: PyEnvType) -> Self {
        match value {
            PyEnvType::RealWorld => CoreEnvType::RealWorld,
            PyEnvType::Simulation => CoreEnvType::Simulation,
        }
    }
}

#[pymethods]
impl PyEnvType {
    pub fn __str__(&self) -> &'static str {
        match self {
            Self::RealWorld => "real_world",
            Self::Simulation => "simulation",
        }
    }

    pub fn __repr__(&self) -> &'static str {
        match self {
            Self::RealWorld => "EnvType.RealWorld",
            Self::Simulation => "EnvType.Simulation",
        }
    }
}

// =============================================================================
// RunnerType (enum)
// =============================================================================

#[pyclass(frozen, module = "rebake.core", name = "RunnerType", eq)]
#[derive(Clone, PartialEq)]
pub enum PyRunnerType {
    Operator,
    Model,
}

impl From<CoreRunnerType> for PyRunnerType {
    fn from(value: CoreRunnerType) -> Self {
        match value {
            CoreRunnerType::Operator => Self::Operator,
            CoreRunnerType::Model => Self::Model,
        }
    }
}

impl From<PyRunnerType> for CoreRunnerType {
    fn from(value: PyRunnerType) -> Self {
        match value {
            PyRunnerType::Operator => CoreRunnerType::Operator,
            PyRunnerType::Model => CoreRunnerType::Model,
        }
    }
}

#[pymethods]
impl PyRunnerType {
    pub fn __str__(&self) -> &'static str {
        match self {
            Self::Operator => "operator",
            Self::Model => "model",
        }
    }

    pub fn __repr__(&self) -> &'static str {
        match self {
            Self::Operator => "RunnerType.Operator",
            Self::Model => "RunnerType.Model",
        }
    }
}

// =============================================================================
// GitSource
// =============================================================================

#[pyclass(module = "rebake.core", name = "GitSource", eq)]
#[derive(Clone, PartialEq)]
pub struct PyGitSource {
    pub(crate) inner: CoreGitSource,
}

impl From<CoreGitSource> for PyGitSource {
    fn from(inner: CoreGitSource) -> Self {
        Self { inner }
    }
}

impl From<PyGitSource> for CoreGitSource {
    fn from(value: PyGitSource) -> Self {
        value.inner
    }
}

#[pymethods]
impl PyGitSource {
    #[new]
    #[pyo3(signature = (uri, hash, branch, *, tag=None))]
    pub fn new(uri: String, hash: String, branch: String, tag: Option<String>) -> PyResult<Self> {
        if uri.is_empty() {
            return Err(PyValueError::new_err("GitSource.uri must be non-empty"));
        }
        if hash.is_empty() {
            return Err(PyValueError::new_err("GitSource.hash must be non-empty"));
        }
        if branch.is_empty() {
            return Err(PyValueError::new_err("GitSource.branch must be non-empty"));
        }
        Ok(Self {
            inner: CoreGitSource {
                uri,
                hash,
                branch,
                tag,
            },
        })
    }

    #[getter]
    pub fn uri(&self) -> String {
        self.inner.uri.clone()
    }
    #[getter]
    pub fn hash(&self) -> String {
        self.inner.hash.clone()
    }
    #[getter]
    pub fn branch(&self) -> String {
        self.inner.branch.clone()
    }
    #[getter]
    pub fn tag(&self) -> Option<String> {
        self.inner.tag.clone()
    }

    #[setter]
    pub fn set_uri(&mut self, value: String) {
        self.inner.uri = value;
    }
    #[setter]
    pub fn set_hash(&mut self, value: String) {
        self.inner.hash = value;
    }
    #[setter]
    pub fn set_branch(&mut self, value: String) {
        self.inner.branch = value;
    }
    #[setter]
    pub fn set_tag(&mut self, value: Option<String>) {
        self.inner.tag = value;
    }

    pub fn __repr__(&self) -> String {
        format!(
            "GitSource(uri={:?}, hash={:?}, branch={:?})",
            self.inner.uri, self.inner.hash, self.inner.branch
        )
    }
}

// =============================================================================
// Source
// =============================================================================

#[pyclass(module = "rebake.core", name = "Source", eq)]
#[derive(Clone, PartialEq)]
pub struct PySource {
    pub(crate) inner: CoreSource,
}

impl From<CoreSource> for PySource {
    fn from(inner: CoreSource) -> Self {
        Self { inner }
    }
}

impl From<PySource> for CoreSource {
    fn from(value: PySource) -> Self {
        value.inner
    }
}

#[pymethods]
impl PySource {
    #[new]
    #[pyo3(signature = (*, git=None))]
    pub fn new(git: Option<PyGitSource>) -> Self {
        Self {
            inner: CoreSource {
                git: git.map(Into::into),
            },
        }
    }

    #[getter]
    pub fn git(&self) -> Option<PyGitSource> {
        self.inner.git.clone().map(PyGitSource::from)
    }

    #[setter]
    pub fn set_git(&mut self, value: Option<PyGitSource>) {
        self.inner.git = value.map(Into::into);
    }

    pub fn __repr__(&self) -> String {
        format!("Source(git={:?})", self.inner.git.is_some())
    }
}

// =============================================================================
// Robot
// =============================================================================

#[pyclass(module = "rebake.core", name = "Robot", eq)]
#[derive(Clone, PartialEq)]
pub struct PyRobot {
    pub(crate) inner: CoreRobot,
}

impl From<CoreRobot> for PyRobot {
    fn from(inner: CoreRobot) -> Self {
        Self { inner }
    }
}

impl From<PyRobot> for CoreRobot {
    fn from(value: PyRobot) -> Self {
        value.inner
    }
}

#[pymethods]
impl PyRobot {
    #[new]
    #[pyo3(signature = (*, robot_type=None, id=None, uri=None, checksum=None))]
    pub fn new(
        robot_type: Option<String>,
        id: Option<String>,
        uri: Option<String>,
        checksum: Option<String>,
    ) -> Self {
        Self {
            inner: CoreRobot {
                uri,
                robot_type: robot_type.unwrap_or_else(|| "unknown".to_string()),
                // Empty default: "no robot ID known". A fresh uuid here would
                // falsely claim a specific robot identity.
                id: id.unwrap_or_default(),
                checksum,
            },
        }
    }

    #[getter]
    pub fn uri(&self) -> Option<String> {
        self.inner.uri.clone()
    }
    #[getter]
    pub fn robot_type(&self) -> String {
        self.inner.robot_type.clone()
    }
    #[getter]
    pub fn id(&self) -> String {
        self.inner.id.clone()
    }
    #[getter]
    pub fn checksum(&self) -> Option<String> {
        self.inner.checksum.clone()
    }

    #[setter]
    pub fn set_uri(&mut self, value: Option<String>) {
        self.inner.uri = value;
    }
    #[setter]
    pub fn set_robot_type(&mut self, value: String) {
        self.inner.robot_type = value;
    }
    #[setter]
    pub fn set_id(&mut self, value: String) {
        self.inner.id = value;
    }
    #[setter]
    pub fn set_checksum(&mut self, value: Option<String>) {
        self.inner.checksum = value;
    }

    pub fn __repr__(&self) -> String {
        format!(
            "Robot(type={:?}, id={:?})",
            self.inner.robot_type, self.inner.id
        )
    }
}

// =============================================================================
// File
// =============================================================================

#[pyclass(module = "rebake.core", name = "File", eq)]
#[derive(Clone, PartialEq)]
pub struct PyFile {
    pub(crate) inner: CoreFile,
}

impl From<CoreFile> for PyFile {
    fn from(inner: CoreFile) -> Self {
        Self { inner }
    }
}

impl From<PyFile> for CoreFile {
    fn from(value: PyFile) -> Self {
        value.inner
    }
}

#[pymethods]
impl PyFile {
    #[new]
    #[pyo3(signature = (name, *, file_type=None, checksum=None))]
    pub fn new(
        name: String,
        file_type: Option<String>,
        checksum: Option<String>,
    ) -> PyResult<Self> {
        if name.is_empty() {
            return Err(PyValueError::new_err("File.name must be non-empty"));
        }
        Ok(Self {
            inner: CoreFile {
                file_type: file_type.unwrap_or_else(|| "mcap".to_string()),
                name,
                checksum,
            },
        })
    }

    #[getter]
    pub fn file_type(&self) -> String {
        self.inner.file_type.clone()
    }
    #[getter]
    pub fn name(&self) -> String {
        self.inner.name.clone()
    }
    #[getter]
    pub fn checksum(&self) -> Option<String> {
        self.inner.checksum.clone()
    }

    #[setter]
    pub fn set_file_type(&mut self, value: String) {
        self.inner.file_type = value;
    }
    #[setter]
    pub fn set_name(&mut self, value: String) {
        self.inner.name = value;
    }
    #[setter]
    pub fn set_checksum(&mut self, value: Option<String>) {
        self.inner.checksum = value;
    }

    pub fn __repr__(&self) -> String {
        format!(
            "File(name={:?}, type={:?})",
            self.inner.name, self.inner.file_type
        )
    }
}

// =============================================================================
// Environment
// =============================================================================

#[pyclass(module = "rebake.core", name = "Environment", eq)]
#[derive(Clone, PartialEq)]
pub struct PyEnvironment {
    pub(crate) inner: CoreEnvironment,
}

impl From<CoreEnvironment> for PyEnvironment {
    fn from(inner: CoreEnvironment) -> Self {
        Self { inner }
    }
}

impl From<PyEnvironment> for CoreEnvironment {
    fn from(value: PyEnvironment) -> Self {
        value.inner
    }
}

#[pymethods]
impl PyEnvironment {
    #[new]
    #[pyo3(signature = (*, env_type=None, site=None, location=None))]
    pub fn new(
        env_type: Option<PyEnvType>,
        site: Option<String>,
        location: Option<String>,
    ) -> Self {
        Self {
            inner: CoreEnvironment {
                env_type: env_type.map(Into::into).unwrap_or(CoreEnvType::RealWorld),
                site: site.unwrap_or_else(|| "unknown".to_string()),
                location,
            },
        }
    }

    #[getter]
    pub fn env_type(&self) -> PyEnvType {
        self.inner.env_type.clone().into()
    }
    #[getter]
    pub fn site(&self) -> String {
        self.inner.site.clone()
    }
    #[getter]
    pub fn location(&self) -> Option<String> {
        self.inner.location.clone()
    }

    #[setter]
    pub fn set_env_type(&mut self, value: PyEnvType) {
        self.inner.env_type = value.into();
    }
    #[setter]
    pub fn set_site(&mut self, value: String) {
        self.inner.site = value;
    }
    #[setter]
    pub fn set_location(&mut self, value: Option<String>) {
        self.inner.location = value;
    }

    pub fn __repr__(&self) -> String {
        format!(
            "Environment(type={:?}, site={:?})",
            self.inner.env_type, self.inner.site
        )
    }
}

// =============================================================================
// Runner
// =============================================================================

#[pyclass(module = "rebake.core", name = "Runner", eq)]
#[derive(Clone, PartialEq)]
pub struct PyRunner {
    pub(crate) inner: CoreRunner,
}

impl From<CoreRunner> for PyRunner {
    fn from(inner: CoreRunner) -> Self {
        Self { inner }
    }
}

impl From<PyRunner> for CoreRunner {
    fn from(value: PyRunner) -> Self {
        value.inner
    }
}

#[pymethods]
impl PyRunner {
    #[new]
    #[pyo3(signature = (*, runner_type=None, organization=None, name=None))]
    pub fn new(
        runner_type: Option<PyRunnerType>,
        organization: Option<String>,
        name: Option<String>,
    ) -> Self {
        Self {
            inner: CoreRunner {
                runner_type: runner_type
                    .map(Into::into)
                    .unwrap_or(CoreRunnerType::Operator),
                organization: organization.unwrap_or_default(),
                name: name.unwrap_or_default(),
            },
        }
    }

    #[getter]
    pub fn runner_type(&self) -> PyRunnerType {
        self.inner.runner_type.clone().into()
    }
    #[getter]
    pub fn organization(&self) -> String {
        self.inner.organization.clone()
    }
    #[getter]
    pub fn name(&self) -> String {
        self.inner.name.clone()
    }

    #[setter]
    pub fn set_runner_type(&mut self, value: PyRunnerType) {
        self.inner.runner_type = value.into();
    }
    #[setter]
    pub fn set_organization(&mut self, value: String) {
        self.inner.organization = value;
    }
    #[setter]
    pub fn set_name(&mut self, value: String) {
        self.inner.name = value;
    }

    pub fn __repr__(&self) -> String {
        format!(
            "Runner(type={:?}, name={:?})",
            self.inner.runner_type, self.inner.name
        )
    }
}

// =============================================================================
// Program
// =============================================================================

#[pyclass(module = "rebake.core", name = "Program", eq)]
#[derive(Clone, PartialEq)]
pub struct PyProgram {
    pub(crate) inner: CoreProgram,
}

impl From<CoreProgram> for PyProgram {
    fn from(inner: CoreProgram) -> Self {
        Self { inner }
    }
}

impl From<PyProgram> for CoreProgram {
    fn from(value: PyProgram) -> Self {
        value.inner
    }
}

#[pymethods]
impl PyProgram {
    #[new]
    #[pyo3(signature = (role, name, *, source=None))]
    pub fn new(role: String, name: String, source: Option<PySource>) -> PyResult<Self> {
        if role.is_empty() {
            return Err(PyValueError::new_err("Program.role must be non-empty"));
        }
        if name.is_empty() {
            return Err(PyValueError::new_err("Program.name must be non-empty"));
        }
        Ok(Self {
            inner: CoreProgram {
                role,
                name,
                source: source.map(Into::into).unwrap_or_default(),
            },
        })
    }

    #[getter]
    pub fn role(&self) -> String {
        self.inner.role.clone()
    }
    #[getter]
    pub fn name(&self) -> String {
        self.inner.name.clone()
    }
    #[getter]
    pub fn source(&self) -> PySource {
        self.inner.source.clone().into()
    }

    #[setter]
    pub fn set_role(&mut self, value: String) {
        self.inner.role = value;
    }
    #[setter]
    pub fn set_name(&mut self, value: String) {
        self.inner.name = value;
    }
    #[setter]
    pub fn set_source(&mut self, value: PySource) {
        self.inner.source = value.into();
    }

    pub fn __repr__(&self) -> String {
        format!(
            "Program(role={:?}, name={:?})",
            self.inner.role, self.inner.name
        )
    }
}

// =============================================================================
// Device
// =============================================================================

#[pyclass(module = "rebake.core", name = "Device", eq)]
#[derive(Clone, PartialEq)]
pub struct PyDevice {
    pub(crate) inner: CoreDevice,
}

impl From<CoreDevice> for PyDevice {
    fn from(inner: CoreDevice) -> Self {
        Self { inner }
    }
}

impl From<PyDevice> for CoreDevice {
    fn from(value: PyDevice) -> Self {
        value.inner
    }
}

#[pymethods]
impl PyDevice {
    #[new]
    #[pyo3(signature = (role, device_type, *, id=None))]
    pub fn new(role: String, device_type: String, id: Option<String>) -> PyResult<Self> {
        if role.is_empty() {
            return Err(PyValueError::new_err("Device.role must be non-empty"));
        }
        if device_type.is_empty() {
            return Err(PyValueError::new_err("Device.device_type must be non-empty"));
        }
        Ok(Self {
            inner: CoreDevice {
                role,
                device_type,
                // Empty default: "no device ID known". See Robot.id rationale.
                id: id.unwrap_or_default(),
            },
        })
    }

    #[getter]
    pub fn role(&self) -> String {
        self.inner.role.clone()
    }
    #[getter]
    pub fn device_type(&self) -> String {
        self.inner.device_type.clone()
    }
    #[getter]
    pub fn id(&self) -> String {
        self.inner.id.clone()
    }

    #[setter]
    pub fn set_role(&mut self, value: String) {
        self.inner.role = value;
    }
    #[setter]
    pub fn set_device_type(&mut self, value: String) {
        self.inner.device_type = value;
    }
    #[setter]
    pub fn set_id(&mut self, value: String) {
        self.inner.id = value;
    }

    pub fn __repr__(&self) -> String {
        format!(
            "Device(role={:?}, type={:?})",
            self.inner.role, self.inner.device_type
        )
    }
}

// =============================================================================
// Episode
// =============================================================================

#[pyclass(module = "rebake.core", name = "Episode", eq)]
#[derive(Clone, PartialEq)]
pub struct PyEpisode {
    pub(crate) inner: CoreEpisode,
}

impl From<CoreEpisode> for PyEpisode {
    fn from(inner: CoreEpisode) -> Self {
        Self { inner }
    }
}

impl From<PyEpisode> for CoreEpisode {
    fn from(value: PyEpisode) -> Self {
        value.inner
    }
}

#[pymethods]
impl PyEpisode {
    #[new]
    #[pyo3(signature = (label, start_time, end_time, *, success=true))]
    pub fn new(label: String, start_time: f64, end_time: f64, success: bool) -> PyResult<Self> {
        if label.is_empty() {
            return Err(PyValueError::new_err("Episode.label must be non-empty"));
        }
        if start_time < 0.0 {
            return Err(PyValueError::new_err(
                "Episode.start_time must be non-negative (Unix epoch seconds)",
            ));
        }
        if end_time < 0.0 {
            return Err(PyValueError::new_err(
                "Episode.end_time must be non-negative (Unix epoch seconds)",
            ));
        }
        Ok(Self {
            inner: CoreEpisode {
                start_time,
                end_time,
                success,
                label,
            },
        })
    }

    #[getter]
    pub fn start_time(&self) -> f64 {
        self.inner.start_time
    }
    #[getter]
    pub fn end_time(&self) -> f64 {
        self.inner.end_time
    }
    #[getter]
    pub fn success(&self) -> bool {
        self.inner.success
    }
    #[getter]
    pub fn label(&self) -> String {
        self.inner.label.clone()
    }

    #[setter]
    pub fn set_start_time(&mut self, value: f64) {
        self.inner.start_time = value;
    }
    #[setter]
    pub fn set_end_time(&mut self, value: f64) {
        self.inner.end_time = value;
    }
    #[setter]
    pub fn set_success(&mut self, value: bool) {
        self.inner.success = value;
    }
    #[setter]
    pub fn set_label(&mut self, value: String) {
        self.inner.label = value;
    }

    pub fn __repr__(&self) -> String {
        format!(
            "Episode(label={:?}, success={})",
            self.inner.label, self.inner.success
        )
    }
}

// =============================================================================
// Segment
// =============================================================================

#[pyclass(module = "rebake.core", name = "Segment", eq)]
#[derive(Clone, PartialEq)]
pub struct PySegment {
    pub(crate) inner: CoreSegment,
}

impl From<CoreSegment> for PySegment {
    fn from(inner: CoreSegment) -> Self {
        Self { inner }
    }
}

impl From<PySegment> for CoreSegment {
    fn from(value: PySegment) -> Self {
        value.inner
    }
}

#[pymethods]
impl PySegment {
    #[new]
    #[pyo3(signature = (start_time, end_time, label_idx, *, success=true))]
    pub fn new(start_time: f64, end_time: f64, label_idx: usize, success: bool) -> Self {
        Self {
            inner: CoreSegment {
                start_time,
                end_time,
                label_idx,
                success,
            },
        }
    }

    #[getter]
    pub fn start_time(&self) -> f64 {
        self.inner.start_time
    }
    #[getter]
    pub fn end_time(&self) -> f64 {
        self.inner.end_time
    }
    #[getter]
    pub fn label_idx(&self) -> usize {
        self.inner.label_idx
    }
    #[getter]
    pub fn success(&self) -> bool {
        self.inner.success
    }

    #[setter]
    pub fn set_start_time(&mut self, value: f64) {
        self.inner.start_time = value;
    }
    #[setter]
    pub fn set_end_time(&mut self, value: f64) {
        self.inner.end_time = value;
    }
    #[setter]
    pub fn set_label_idx(&mut self, value: usize) {
        self.inner.label_idx = value;
    }
    #[setter]
    pub fn set_success(&mut self, value: bool) {
        self.inner.success = value;
    }

    pub fn __repr__(&self) -> String {
        format!(
            "Segment(start_time={}, end_time={}, label_idx={})",
            self.inner.start_time, self.inner.end_time, self.inner.label_idx
        )
    }
}
