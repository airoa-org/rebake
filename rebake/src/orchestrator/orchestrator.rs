use std::error::Error;
use std::fs;
use std::process::Command;
use std::time::Instant;

use camino::{Utf8Path, Utf8PathBuf};
use polars::prelude::ParquetWriter;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::core::stage::{Context, PipelineInputKind, StageConfig};

/// Defines the blueprint for an entire data processing pipeline.
///
/// This struct is typically deserialized from a YAML file and specifies the sequence of
/// stages to be executed, a working directory for outputs, and whether to save the
/// results from intermediate stages.
#[derive(Serialize, Deserialize)]
pub struct OrchestratorConfig {
    /// The root directory where all pipeline outputs, including intermediate results, will be stored.
    pub work_dir: String,
    /// If `true`, the state of the dataset after each stage is saved to a subdirectory
    /// within `work_dir`. This is useful for debugging and inspecting intermediate data.
    /// Defaults to `false` when omitted from the config.
    #[serde(default)]
    pub save_contexts: bool,
    /// The sequence of stage configurations that make up the pipeline. Each element in this
    /// vector defines a processing step (e.g., ingesting, enriching, synchronizing) and its
    /// specific parameters.
    pub stage_configs: Vec<Box<dyn StageConfig>>,
    /// Optional root directory for video cache files.
    ///
    /// When set, video files are saved to `{video_cache_root}/{uuid}/{topic}.mp4`.
    /// This directory can be separate from `work_dir` to allow for independent
    /// management (e.g., migration to object storage).
    ///
    /// If not set, `VideoEncoder` will fall back to `./video_cache/{uuid}/`
    /// (relative to current working directory).
    #[serde(default)]
    pub video_cache_root: Option<String>,
    /// If `true` (default), stop all processing immediately when any rosbag fails.
    ///
    /// This is useful when a failure likely indicates a configuration error that
    /// would affect all rosbags (e.g., missing stage config, invalid paths).
    /// When `false`, processing continues for remaining rosbags and failed ones
    /// are reported at the end.
    #[serde(default = "default_stop_on_error")]
    pub stop_on_error: bool,
}

fn default_stop_on_error() -> bool {
    true
}

impl OrchestratorConfig {
    pub fn new(
        work_dir: String,
        save_contexts: bool,
        stage_configs: Vec<Box<dyn StageConfig>>,
    ) -> Self {
        Self {
            work_dir,
            save_contexts,
            stage_configs,
            video_cache_root: None,
            stop_on_error: true,
        }
    }

    pub fn build(self) -> Orchestrator {
        Orchestrator::new(self)
    }

    pub fn pipeline_input_kind(&self) -> PipelineInputKind {
        self.stage_configs
            .first()
            .and_then(|config| config.pipeline_input_kind())
            .unwrap_or(PipelineInputKind::Rosbag)
    }
}

/// The pipeline execution engine.
///
/// The `Orchestrator` takes a sequence of stages (built from an `OrchestratorConfig`) and
/// executes them in order. It manages the flow of data between stages by passing a `Context`
/// object, which holds the dataset and other shared information.
pub struct Orchestrator {
    config: OrchestratorConfig,
}

#[derive(Debug)]
struct RosbagFailure {
    path: Utf8PathBuf,
    reason: String,
    source: Option<Box<dyn Error + Send + Sync + 'static>>,
}

impl RosbagFailure {
    fn new(path: Utf8PathBuf, reason: impl Into<String>) -> Self {
        Self {
            path,
            reason: reason.into(),
            source: None,
        }
    }

    fn from_error(path: Utf8PathBuf, err: Box<dyn Error + Send + Sync + 'static>) -> Self {
        let reason = format_error_chain(err.as_ref());
        Self {
            path,
            reason,
            source: Some(err),
        }
    }

    fn into_error(self) -> Box<dyn Error + Send + Sync + 'static> {
        self.source.unwrap_or_else(|| {
            Box::new(PipelineError::new(format!(
                "input {} failed: {}",
                self.path, self.reason
            )))
        })
    }
}

impl Orchestrator {
    pub fn new(config: OrchestratorConfig) -> Self {
        Self { config }
    }

    fn work_dir(&self) -> Utf8PathBuf {
        Utf8PathBuf::from(&self.config.work_dir)
    }

    fn video_cache_root(&self) -> Option<Utf8PathBuf> {
        self.config.video_cache_root.as_ref().map(Utf8PathBuf::from)
    }

    /// Serializes the configuration to YAML for passing to child processes.
    fn serialize_config(&self) -> Result<String, Box<dyn Error + Send + Sync + 'static>> {
        serde_yaml::to_string(&self.config)
            .map_err(|e| Box::new(e) as Box<dyn Error + Send + Sync + 'static>)
    }

    /// Runs the pipeline for the provided inputs.
    ///
    /// # Behavior
    /// - Effective parallelism of 1: Runs in the current process
    /// - Effective parallelism > 1: Runs in parallel using process isolation
    ///
    /// Process isolation is used for parallel execution to avoid SVT-AV1's global state
    /// race conditions. See `docs/SVT_AV1_ROOT_CAUSE_ANALYSIS.md` for details.
    ///
    /// When `quiet` is true, in-process progress messages are suppressed. This
    /// is used when the process is a child spawned by another orchestrator,
    /// which handles batch-level progress reporting itself.
    pub fn run(
        &self,
        inputs: Vec<Utf8PathBuf>,
        max_parallel: usize,
        quiet: bool,
    ) -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
        if inputs.is_empty() {
            return Ok(());
        }

        let parallelism = max_parallel.max(1).min(inputs.len());
        if parallelism <= 1 {
            return self.run_in_process(inputs, quiet);
        }

        // Multiple concurrent workers require process isolation to avoid SVT-AV1
        // global state conflicts.
        self.run_with_process_isolation(inputs, parallelism)
    }

    /// Runs one or more inputs in the current process.
    ///
    /// This path preserves the debuggability of direct execution while still
    /// honoring `stop_on_error` across a batch of rosbags.
    fn run_in_process(
        &self,
        inputs: Vec<Utf8PathBuf>,
        quiet: bool,
    ) -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
        let total = inputs.len();
        let mut failed = Vec::new();
        let input_label = self.config.pipeline_input_kind().display_name();

        for (index, input_path) in inputs.into_iter().enumerate() {
            // Child processes spawned by the parallel scheduler also execute in
            // this path. Suppress these per-input progress lines there so the
            // parent remains the only batch-level progress reporter.
            if !quiet {
                println!(
                    "[{}/{}] Processing {}: {}",
                    index + 1,
                    total,
                    input_label,
                    input_path
                );
            }

            match self.run_single_input(input_path.clone()) {
                Ok(()) => {
                    if !quiet {
                        println!("[{}/{}] Finished", index + 1, total);
                    }
                }
                Err(err) => {
                    let failure = RosbagFailure::from_error(input_path.clone(), err);
                    if total > 1 {
                        println!(
                            "Failed {} {}: {} ({})",
                            input_label,
                            index + 1,
                            input_path,
                            failure.reason
                        );
                    }
                    failed.push(failure);

                    if self.config.stop_on_error {
                        break;
                    }
                }
            }
        }

        Self::finish_input_batch(total, failed, input_label)
    }

    /// Runs the pipeline for multiple inputs using process isolation.
    ///
    /// Each rosbag runs in a separate child process to isolate SVT-AV1's global state.
    fn run_with_process_isolation(
        &self,
        inputs: Vec<Utf8PathBuf>,
        parallelism: usize,
    ) -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
        let total = inputs.len();
        let current_exe = std::env::current_exe()
            .map_err(|e| PipelineError::new(format!("failed to get current executable: {}", e)))?;
        let input_label = self.config.pipeline_input_kind().display_name();

        // Serialize config to pass to child processes
        let config_data = self.serialize_config()?;

        let mut pending: Vec<(usize, Utf8PathBuf)> = inputs.into_iter().enumerate().collect();
        let mut running: Vec<(usize, Utf8PathBuf, std::process::Child)> = Vec::new();
        let mut failed: Vec<RosbagFailure> = Vec::new();
        let mut should_stop = false;

        while !pending.is_empty() || !running.is_empty() {
            // Stop early if configured and an error occurred
            if should_stop && self.config.stop_on_error {
                for (index, path, mut child) in running {
                    let _ = child.kill();
                    println!(
                        "Killed {} {}: {} (stop_on_error)",
                        input_label,
                        index + 1,
                        path
                    );
                }
                break;
            }

            // Spawn new processes up to parallelism limit.
            // When stop_on_error is false, keep spawning despite failures.
            while running.len() < parallelism
                && !pending.is_empty()
                && !(should_stop && self.config.stop_on_error)
            {
                let (index, path) = pending.remove(0);
                println!("Starting {} {}: {}", input_label, index + 1, path);

                match Command::new(&current_exe)
                    .arg("run")
                    .args(["--config-data", &config_data])
                    .args(["--jobs", "1"])
                    .arg(path.as_str())
                    .spawn()
                {
                    Ok(child) => running.push((index, path, child)),
                    Err(e) => {
                        let msg = format!("spawn failed: {}", e);
                        println!("Failed {} {}: {} ({})", input_label, index + 1, path, msg);
                        failed.push(RosbagFailure::new(path, msg));
                        should_stop = true;
                    }
                }
            }

            // Poll running processes
            let mut still_running = Vec::new();
            for (index, path, mut child) in running {
                match child.try_wait() {
                    Ok(Some(status)) => {
                        if status.success() {
                            println!("Finished {} {}: {}", input_label, index + 1, path);
                        } else {
                            let msg = status
                                .code()
                                .map(|c| format!("exit code {}", c))
                                .unwrap_or_else(|| "terminated by signal".into());
                            println!("Failed {} {}: {} ({})", input_label, index + 1, path, msg);
                            failed.push(RosbagFailure::new(path, msg));
                            should_stop = true;
                        }
                    }
                    Ok(None) => still_running.push((index, path, child)),
                    Err(e) => {
                        let msg = format!("status check failed: {}", e);
                        println!("Failed {} {}: {} ({})", input_label, index + 1, path, msg);
                        failed.push(RosbagFailure::new(path, msg));
                        should_stop = true;
                    }
                }
            }
            running = still_running;

            // Avoid busy-waiting
            if !running.is_empty() {
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        }

        Self::finish_input_batch(total, failed, input_label)
    }

    /// Runs the pipeline for a single input within the current process.
    fn run_single_input(
        &self,
        input_path: Utf8PathBuf,
    ) -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
        let work_dir = self.work_dir();
        let video_cache_root = self.video_cache_root();
        let pipeline = Pipeline::new(
            input_path,
            self.config.pipeline_input_kind(),
            &work_dir,
            self.config.save_contexts,
            &self.config.stage_configs,
            video_cache_root.as_ref(),
        );
        pipeline.run()?;
        Ok(())
    }

    fn save_context(
        context: &Context,
        output_dir: &Utf8PathBuf,
    ) -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
        let Some(dataset) = context.dataset.as_ref() else {
            return Ok(());
        };

        for (topic, frame) in dataset {
            let sanitized_topic = topic.trim_start_matches('/');
            let output_path = output_dir.join(format!("{}.parquet", sanitized_topic));
            if let Some(parent) = output_path.parent() {
                fs::create_dir_all(parent.as_std_path())?;
            }

            let mut df = frame.clone().collect()?;
            let mut file = fs::File::create(output_path.as_std_path())?;
            ParquetWriter::new(&mut file).finish(&mut df)?;
        }

        Ok(())
    }

    fn finish_input_batch(
        total: usize,
        failed: Vec<RosbagFailure>,
        input_label: &str,
    ) -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
        if failed.is_empty() {
            return Ok(());
        }

        if total == 1 && failed.len() == 1 {
            let failure = failed
                .into_iter()
                .next()
                .ok_or_else(|| PipelineError::new("single input failure missing"))?;
            return Err(failure.into_error());
        }

        eprintln!("\nFailed {}s:", input_label);
        for failure in &failed {
            eprintln!("  - {} ({})", failure.path, failure.reason);
        }

        Err(Box::new(PipelineError::new(format!(
            "{} {}(s) failed",
            failed.len(),
            input_label
        ))))
    }
}

/// Formats a full error chain into a single string.
///
/// Walks the [`std::error::Error::source()`] chain and joins all messages
/// with `": "`, producing output like:
///
/// ```text
/// failed to encode video frame: VA-API codecs must use VaapiVideoEncoder...
/// ```
///
/// This ensures that nested error context is not lost when logging or
/// converting errors to strings at the pipeline boundary.
fn format_error_chain(err: &dyn std::error::Error) -> String {
    let mut chain = vec![err.to_string()];
    let mut current = err.source();
    while let Some(cause) = current {
        chain.push(cause.to_string());
        current = cause.source();
    }
    chain.join(": ")
}

struct Pipeline<'a> {
    input_path: Utf8PathBuf,
    input_kind: PipelineInputKind,
    work_dir: &'a Utf8Path,
    save_contexts: bool,
    stage_configs: &'a [Box<dyn StageConfig>],
    video_cache_root: Option<&'a Utf8PathBuf>,
}

impl<'a> Pipeline<'a> {
    fn new(
        input_path: Utf8PathBuf,
        input_kind: PipelineInputKind,
        work_dir: &'a Utf8Path,
        save_contexts: bool,
        stage_configs: &'a [Box<dyn StageConfig>],
        video_cache_root: Option<&'a Utf8PathBuf>,
    ) -> Self {
        Self {
            input_path,
            input_kind,
            work_dir,
            save_contexts,
            stage_configs,
            video_cache_root,
        }
    }

    fn run(&self) -> Result<(), PipelineError> {
        let dir_name = match self.input_kind {
            PipelineInputKind::Rosbag => self
                .input_path
                .parent()
                .ok_or_else(|| PipelineError::new("rosbag path must have parent"))?
                .file_name()
                .ok_or_else(|| PipelineError::new("parent directory must have name"))?,
            PipelineInputKind::ParquetVideoBundle => self
                .input_path
                .file_name()
                .ok_or_else(|| PipelineError::new("bundle root must have directory name"))?,
        };
        let run_root = self.work_dir.join(dir_name);

        if let Some(parent_dir) = run_root.parent() {
            fs::create_dir_all(parent_dir.as_std_path())?;
        }
        fs::create_dir_all(run_root.as_std_path())?;

        let mut context = Context::default();
        match self.input_kind {
            PipelineInputKind::Rosbag => context.set_rosbag_path(self.input_path.clone()),
            PipelineInputKind::ParquetVideoBundle => {
                context.set_bundle_root(self.input_path.clone())
            }
        }

        // Set video_cache_dir if video_cache_root is configured
        if let Some(video_cache_root) = self.video_cache_root {
            context.set_video_cache_dir(video_cache_root.clone());
        }
        let input_name = self.input_path.as_str().to_string();
        let pipeline_span = tracing::info_span!(
            "pipeline_input",
            input = input_name.as_str(),
            duration_ms = tracing::field::Empty
        );
        let _pipeline_guard = pipeline_span.enter();
        let pipeline_start = Instant::now();

        for (stage_index, config) in self.stage_configs.iter().enumerate() {
            let mut stage = config.build();
            let stage_dir = run_root.join(format!("{}_{}", stage_index, stage.name()));
            fs::create_dir_all(stage_dir.as_std_path())?;
            context.set_output_dir(stage_dir.clone());

            let input_label = context
                .rosbag_path
                .as_ref()
                .map(|path| path.as_str().to_string())
                .or_else(|| {
                    context
                        .bundle_root
                        .as_ref()
                        .map(|path| path.as_str().to_string())
                })
                .unwrap_or_else(|| self.input_path.as_str().to_string());
            let span = tracing::info_span!(
                "stage_run",
                stage = stage.name(),
                stage_index = stage_index as i32,
                input = input_label.as_str(),
                duration_ms = tracing::field::Empty,
                output_dir = %stage_dir
            );
            let _span_guard = span.enter();
            let start = Instant::now();

            match stage.run(context) {
                Ok(next) => {
                    context = next;
                    if self.save_contexts {
                        Orchestrator::save_context(&context, &stage_dir)
                            .map_err(|err| PipelineError::new(err.to_string()))?;
                    }
                    let duration = start.elapsed();
                    span.record("duration_ms", duration.as_millis() as i64);
                    tracing::info!(
                        stage = stage.name(),
                        stage_index,
                        duration_ms = { duration.as_millis() },
                        "stage completed"
                    );
                }
                Err(err) => {
                    let duration = start.elapsed();
                    span.record("duration_ms", duration.as_millis() as i64);
                    let failure_message = format_error_chain(&err);
                    if err.is_skip() {
                        tracing::warn!(
                            stage = stage.name(),
                            stage_index,
                            duration_ms = { duration.as_millis() },
                            "stage skipped: {}",
                            failure_message
                        );
                    } else {
                        tracing::warn!(
                            stage = stage.name(),
                            stage_index,
                            duration_ms = { duration.as_millis() },
                            "stage failed: {}",
                            failure_message
                        );
                    }
                    let total = pipeline_start.elapsed();
                    pipeline_span.record("duration_ms", total.as_millis() as i64);
                    tracing::warn!(
                        input = input_name,
                        total_ms = { total.as_millis() },
                        "pipeline aborted"
                    );
                    return Err(PipelineError::stage_failed(stage.name(), failure_message));
                }
            }
        }
        let total_duration = pipeline_start.elapsed();
        pipeline_span.record("duration_ms", total_duration.as_millis() as i64);
        tracing::info!(
            input = input_name,
            total_ms = { total_duration.as_millis() },
            "pipeline completed"
        );
        Ok(())
    }
}

impl PipelineInputKind {
    fn display_name(self) -> &'static str {
        match self {
            PipelineInputKind::Rosbag => "rosbag",
            PipelineInputKind::ParquetVideoBundle => "parquet-video bundle",
        }
    }
}

/// Errors that can occur during pipeline execution.
#[derive(Debug, Error)]
pub enum PipelineError {
    /// A stage failed to execute.
    #[error("stage {stage} failed: {message}")]
    StageFailed { stage: String, message: String },

    /// An I/O error occurred.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// A general error occurred.
    #[error("{0}")]
    Other(String),
}

impl PipelineError {
    fn new(reason: impl Into<String>) -> Self {
        Self::Other(reason.into())
    }

    fn stage_failed(stage: impl Into<String>, message: impl Into<String>) -> Self {
        Self::StageFailed {
            stage: stage.into(),
            message: message.into(),
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use std::fs;

    use serde::{Deserialize, Serialize};

    use super::*;

    use crate::core::error::OptionExt;
    use crate::core::stage::{Stage, StageError};
    use crate::encode::image_encoder::ImageEncoderConfig;
    use crate::encode::video_encoder::VideoEncoderConfig;
    use crate::enrich::delta_joint_position_enricher::DeltaJointPositionEnricherConfig;
    use crate::enrich::delta_transform_enricher::DeltaTransformEnricherConfig;
    use crate::enrich::head_command_enricher::HeadCommandEnricherConfig;
    use crate::enrich::tf_buffer_enricher::TfBufferEnricherConfig;
    use crate::enrich::tf_chain_enricher::{FramePair, TfChainEnricherConfig};
    use crate::ingest::rosbag1_ingestor::Rosbag1IngestorConfig;
    use crate::schema::RobotModelSource;
    use crate::synchronize::zero_order_hold_time_synchronizer::ZeroOrderHoldTimeSynchronizerConfig;
    use crate::transform::lerobot_v21::lerobot_v21_transformer::LeRobotV21TransformerConfig;
    use tempfile::tempdir;

    #[derive(Clone, Debug, Serialize, Deserialize)]
    struct FailOnDirectoryConfig {
        failing_directories: Vec<String>,
    }

    #[typetag::serde]
    impl StageConfig for FailOnDirectoryConfig {
        fn build(&self) -> Box<dyn Stage> {
            Box::new(FailOnDirectoryStage {
                failing_directories: self.failing_directories.clone(),
            })
        }
    }

    struct FailOnDirectoryStage {
        failing_directories: Vec<String>,
    }

    impl Stage for FailOnDirectoryStage {
        fn name(&self) -> &'static str {
            "fail_on_directory"
        }

        fn run(&mut self, context: Context) -> Result<Context, StageError> {
            let rosbag_path = context
                .rosbag_path()
                .cloned()
                .or_missing("rosbag_path in context")?;
            let directory_name = rosbag_path
                .parent()
                .and_then(Utf8Path::file_name)
                .ok_or_else(|| StageError::invalid("rosbag path must have a parent directory"))?;

            if self
                .failing_directories
                .iter()
                .any(|candidate| candidate == directory_name)
            {
                return Err(StageError::invalid(format!(
                    "intentional failure for {}",
                    directory_name
                )));
            }

            let output_dir = context
                .output_dir()
                .cloned()
                .or_missing("output_dir in context")?;
            let marker_path = output_dir.join("completed.txt");
            fs::write(marker_path.as_std_path(), b"ok")
                .map_err(|e| StageError::io("failed to write completion marker", e))?;

            Ok(context)
        }
    }

    fn create_dummy_rosbag(root: &Utf8Path, directory_name: &str) -> Utf8PathBuf {
        let rosbag_dir = root.join(directory_name);
        fs::create_dir_all(rosbag_dir.as_std_path()).unwrap();
        let rosbag_path = rosbag_dir.join(format!("{directory_name}_0.mcap"));
        fs::write(rosbag_path.as_std_path(), b"dummy").unwrap();
        rosbag_path
    }

    #[test]
    #[ignore]
    fn test_orchestrator() {
        let rosbag_path = crate::test_utils::rosbag_path().clone();
        let temp_dir = tempdir().unwrap();
        let work_dir = Utf8PathBuf::from_path_buf(temp_dir.path().join("work")).unwrap();
        let lerobot_out_dir = Utf8PathBuf::from_path_buf(temp_dir.path().join("lerobot")).unwrap();

        let ingector_config = Rosbag1IngestorConfig::new();
        let tf_buffer_enricher_config = TfBufferEnricherConfig {};
        let tf_chain_enricher_config = TfChainEnricherConfig {
            frame_pairs: vec![FramePair {
                source: "base_link".to_string(),
                target: "hand_palm_link".to_string(),
            }],
        };
        let head_command_enricher_config = HeadCommandEnricherConfig::new();
        let zero_order_hold_time_synchronizer_config =
            ZeroOrderHoldTimeSynchronizerConfig { fps: 10 };
        let delta_joint_position_enricher_config = DeltaJointPositionEnricherConfig {
            topic_names: vec!["/hsrb/joint_states".to_string()],
        };
        let delta_transform_enricher_config = DeltaTransformEnricherConfig {
            topic_names: vec!["/tf_chain".to_string()],
            delta_reference_frame:
                crate::enrich::delta_transform_enricher::DeltaReferenceFrame::PreviousTargetFrame,
        };
        let lerobot_v21_transformer_config = LeRobotV21TransformerConfig::new(
            lerobot_out_dir.as_ref(),
            RobotModelSource::Path("../config/robot_model/hsr.yaml".to_string()),
        );

        let orchestrator_config = OrchestratorConfig {
            work_dir: work_dir.to_string(),
            save_contexts: true,
            stage_configs: vec![
                Box::new(ingector_config),
                Box::new(ImageEncoderConfig::default()),
                Box::new(VideoEncoderConfig::default()),
                Box::new(tf_buffer_enricher_config),
                Box::new(tf_chain_enricher_config),
                Box::new(head_command_enricher_config),
                Box::new(zero_order_hold_time_synchronizer_config),
                Box::new(delta_joint_position_enricher_config),
                Box::new(delta_transform_enricher_config),
                Box::new(lerobot_v21_transformer_config),
            ],
            video_cache_root: None,
            stop_on_error: true,
        };
        serde_yaml::to_string(&orchestrator_config).unwrap();
        let orchestrator = orchestrator_config.build();
        orchestrator
            .run(vec![rosbag_path.clone()], 1, false)
            .expect("orchestrator pipeline should succeed");

        let expected_output = lerobot_out_dir.join("data/chunk-000/episode_000000.parquet");
        println!("Expected output path: {}", expected_output);
        if let Ok(entries) = std::fs::read_dir(&lerobot_out_dir) {
            println!("Contents of {}:", lerobot_out_dir);
            for entry in entries {
                println!("  {:?}", entry.unwrap().path());
            }
        } else {
            println!("Could not read directory {}", lerobot_out_dir);
        }
        // Check recursive
        if let Ok(entries) = std::fs::read_dir(lerobot_out_dir.join("data")) {
            println!("Contents of {}/data:", lerobot_out_dir);
            for entry in entries {
                println!("  {:?}", entry.unwrap().path());
            }
        }

        assert!(expected_output.as_std_path().exists());

        let bag_dir_name = rosbag_path.parent().unwrap().file_name().unwrap();
        let stage_dir = work_dir.join(bag_dir_name).join("0_rosbag1_ingestor");
        assert!(stage_dir.as_std_path().exists());
    }

    #[test]
    fn sequential_run_continues_when_stop_on_error_is_false() {
        let temp_dir = tempdir().unwrap();
        let root = Utf8PathBuf::from_path_buf(temp_dir.path().to_path_buf()).unwrap();
        let work_dir = root.join("work");
        let failing_bag = create_dummy_rosbag(&root, "001_fail");
        let succeeding_bag = create_dummy_rosbag(&root, "002_ok");

        let orchestrator = OrchestratorConfig {
            work_dir: work_dir.to_string(),
            save_contexts: false,
            stage_configs: vec![Box::new(FailOnDirectoryConfig {
                failing_directories: vec!["001_fail".to_string()],
            })],
            video_cache_root: None,
            stop_on_error: false,
        }
        .build();

        let result = orchestrator.run(vec![failing_bag, succeeding_bag], 1, false);
        assert!(result.is_err(), "run should still report failed rosbags");
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("1 rosbag(s) failed"),
            "aggregated error should summarize failed rosbags"
        );

        let success_marker = work_dir
            .join("002_ok")
            .join("0_fail_on_directory")
            .join("completed.txt");
        assert!(
            success_marker.exists(),
            "second rosbag should still complete when stop_on_error is false"
        );
    }

    #[test]
    fn sequential_run_stops_when_stop_on_error_is_true() {
        let temp_dir = tempdir().unwrap();
        let root = Utf8PathBuf::from_path_buf(temp_dir.path().to_path_buf()).unwrap();
        let work_dir = root.join("work");
        let failing_bag = create_dummy_rosbag(&root, "001_fail");
        let succeeding_bag = create_dummy_rosbag(&root, "002_ok");

        let orchestrator = OrchestratorConfig {
            work_dir: work_dir.to_string(),
            save_contexts: false,
            stage_configs: vec![Box::new(FailOnDirectoryConfig {
                failing_directories: vec!["001_fail".to_string()],
            })],
            video_cache_root: None,
            stop_on_error: true,
        }
        .build();

        let result = orchestrator.run(vec![failing_bag, succeeding_bag], 1, false);
        assert!(
            result.is_err(),
            "run should fail after the first rosbag error"
        );

        let success_marker = work_dir
            .join("002_ok")
            .join("0_fail_on_directory")
            .join("completed.txt");
        assert!(
            !success_marker.exists(),
            "second rosbag should not run when stop_on_error is true"
        );
    }

    #[test]
    fn single_rosbag_keeps_detailed_error_in_process_path() {
        let temp_dir = tempdir().unwrap();
        let root = Utf8PathBuf::from_path_buf(temp_dir.path().to_path_buf()).unwrap();
        let failing_bag = create_dummy_rosbag(&root, "001_fail");

        let orchestrator = OrchestratorConfig {
            work_dir: root.join("work").to_string(),
            save_contexts: false,
            stage_configs: vec![Box::new(FailOnDirectoryConfig {
                failing_directories: vec!["001_fail".to_string()],
            })],
            video_cache_root: None,
            stop_on_error: false,
        }
        .build();

        let err = orchestrator
            .run(vec![failing_bag], 8, false)
            .expect_err("single rosbag failure should be returned");
        let message = err.to_string();
        assert!(
            message.contains(
                "stage fail_on_directory failed: invalid data: intentional failure for 001_fail"
            ),
            "single rosbag path should preserve the underlying stage failure"
        );
        assert!(
            !message.contains("rosbag(s) failed"),
            "single rosbag path should not collapse into a batch summary"
        );
    }
}
