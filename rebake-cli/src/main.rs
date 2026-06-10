mod commands;
mod error_display;
mod input_utils;
mod rosbag_utils;

use anyhow::Result;
use camino::Utf8PathBuf;
use clap::{Parser, Subcommand};
use once_cell::sync::OnceCell;
use rebake::core::stage::PipelineInputKind;
use rebake::orchestrator::OrchestratorConfig;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::sync::Once;
use tracing_appender::{non_blocking::WorkerGuard, rolling};
use tracing_log::LogTracer;
use tracing_subscriber::{EnvFilter, prelude::*};

use commands::{ExportArgs, MergeArgs};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Run a YAML-configured pipeline over ROS bags or an intermediate format.
    ///
    /// Use this for multi-stage processing: time synchronization, transform-tree
    /// math, action labels, and LeRobot v2.1 output. For a plain conversion to the
    /// intermediate format, use 'rebake export' instead.
    Run(RunArgs),

    /// Export ROS bags to a reusable intermediate format (Parquet + video).
    ///
    /// This is the fastest way out of the bag format, without writing a YAML
    /// configuration. For custom pipelines with TF transforms, time
    /// synchronization, or LeRobot v2.1 output, use 'rebake run' instead.
    Export(ExportArgs),

    /// Merge multiple LeRobot v2.1 datasets into a single dataset.
    ///
    /// Discovers all dataset subdirectories (containing meta/info.json) within
    /// the given source directory, then merges them with renumbered indices,
    /// task deduplication, and consolidated metadata.
    ///
    /// Typical workflow: rebake run -> rebake merge
    Merge(MergeArgs),
}

#[derive(clap::Args, Debug)]
struct RunArgs {
    /// Input path(s). How they're read depends on the pipeline's first stage.
    ///
    /// A ROS bag ingestor reads .bag/.mcap files or directories of them.
    ///
    /// ParquetVideoIngestorConfig reads rebake intermediate-format directories.
    #[arg(value_name = "PATH", value_hint = clap::ValueHint::AnyPath)]
    pub input_paths: Vec<Utf8PathBuf>,

    /// Path to the pipeline configuration file (YAML).
    #[arg(
        short,
        long,
        value_name = "FILE",
        value_hint = clap::ValueHint::FilePath,
        required_unless_present = "config_data"
    )]
    config: Option<Utf8PathBuf>,

    /// Inline pipeline configuration (YAML string).
    /// Used internally for subprocess spawning.
    #[arg(long, hide = true, conflicts_with = "config")]
    config_data: Option<String>,

    /// Inputs to process in parallel.
    #[arg(short = 'j', long = "jobs", default_value_t = 1, value_name = "N")]
    jobs: usize,
}

fn main() {
    if let Err(err) = run() {
        error_display::print_error(&err);
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    init_tracing()?;
    let cli = Cli::parse();

    match cli.command {
        Commands::Run(args) => run_pipeline(args),
        Commands::Export(args) => commands::export::run_export(args),
        Commands::Merge(args) => commands::merge::run_merge(args),
    }
}

/// Initialize tracing so that both stderr and `logs/pipeline.log` receive events.
///
/// Returns the `WorkerGuard`s that must be kept alive for the duration of the process to flush
/// buffered logs.
fn init_tracing() -> Result<()> {
    static INIT: Once = Once::new();
    static GUARDS: OnceCell<Vec<WorkerGuard>> = OnceCell::new();

    INIT.call_once(|| {
        let _ = LogTracer::init();

        let make_env_filter =
            || EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn"));

        let log_dir = std::env::var("REBAKE_LOG_DIR").unwrap_or_else(|_| "logs".to_string());
        let log_path = Path::new(&log_dir);
        if let Err(err) = std::fs::create_dir_all(log_path) {
            eprintln!(
                "warning: failed to create log directory '{}': {}",
                log_dir, err
            );
        }

        let file_appender = rolling::never(log_path, "pipeline.log");
        let (file_writer, file_guard) = tracing_appender::non_blocking(file_appender);
        let log_to_stderr = std::env::var("REBAKE_LOG_STDERR")
            .map(|value| value != "0")
            .unwrap_or(false);
        let guards = if log_to_stderr {
            let (stderr_writer, stderr_guard) = tracing_appender::non_blocking(std::io::stderr());
            tracing_subscriber::registry()
                .with(make_env_filter())
                .with(
                    tracing_subscriber::fmt::layer()
                        .with_writer(file_writer)
                        .with_ansi(false)
                        .with_target(true),
                )
                .with(
                    tracing_subscriber::fmt::layer()
                        .with_writer(stderr_writer)
                        .with_ansi(true)
                        .with_target(false),
                )
                .try_init()
                .ok();
            vec![file_guard, stderr_guard]
        } else {
            tracing_subscriber::registry()
                .with(make_env_filter())
                .with(
                    tracing_subscriber::fmt::layer()
                        .with_writer(file_writer)
                        .with_ansi(false)
                        .with_target(true),
                )
                .try_init()
                .ok();
            vec![file_guard]
        };

        let _ = GUARDS.set(guards);
    });

    Ok(())
}

fn run_pipeline(args: RunArgs) -> Result<()> {
    // When invoked with --config-data, this is a child process spawned by
    // another orchestrator. The parent handles progress reporting.
    let is_child_process = args.config_data.is_some();

    // Load configuration from file or inline data
    let config: OrchestratorConfig = if let Some(config_data) = args.config_data {
        serde_yaml::from_str(&config_data)?
    } else if let Some(config_path) = &args.config {
        let file = File::open(config_path)?;
        let reader = BufReader::new(file);
        serde_yaml::from_reader(reader)?
    } else {
        return Err(anyhow::anyhow!(
            "--config or --config-data must be provided"
        ));
    };

    let input_kind = config.pipeline_input_kind();

    let input_paths = if !args.input_paths.is_empty() {
        match input_kind {
            PipelineInputKind::Rosbag => {
                rosbag_utils::collect_rosbags_from_paths(&args.input_paths)?
            }
            PipelineInputKind::ParquetVideoBundle => {
                input_utils::collect_bundle_roots_from_paths(&args.input_paths)?
            }
        }
    } else {
        return Err(anyhow::anyhow!(
            "input path must be provided: rebake run <PATH> -c <CONFIG>"
        ));
    };

    let input_count = input_paths.len();
    let input_label = match input_kind {
        PipelineInputKind::Rosbag => "rosbag",
        PipelineInputKind::ParquetVideoBundle => "parquet-video bundle",
    };

    if input_count == 0 {
        return Err(anyhow::anyhow!(
            "no {}s found at provided path",
            input_label
        ));
    }

    let max_parallel = args.jobs.max(1);
    let work_dir_display = config.work_dir.clone();
    let orchestrator = config.build();

    orchestrator
        .run(input_paths, max_parallel, is_child_process)
        .map_err(
            boxed_error_to_anyhow as fn(Box<dyn std::error::Error + Send + Sync + 'static>) -> _,
        )?;

    if !is_child_process {
        println!("Processed {} {}(s).", input_count, input_label);
        println!("Output written to base directory: {}", work_dir_display);
    }

    Ok(())
}

/// Wrapper for boxed errors that implements std::error::Error.
///
/// This enables conversion to anyhow::Error while preserving the error chain.
struct BoxedError(Box<dyn std::error::Error + Send + Sync + 'static>);

impl std::fmt::Display for BoxedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl std::fmt::Debug for BoxedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(&self.0, f)
    }
}

impl std::error::Error for BoxedError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.0.source()
    }
}

/// Convert a boxed error to anyhow::Error while preserving the error chain.
fn boxed_error_to_anyhow(e: Box<dyn std::error::Error + Send + Sync + 'static>) -> anyhow::Error {
    anyhow::Error::new(BoxedError(e))
}
