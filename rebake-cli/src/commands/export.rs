//! Export subcommand for structured Parquet + Video output.
//!
//! This is the simplest way to convert rosbag files without writing a YAML
//! configuration. For custom pipelines with TF transforms, time synchronization,
//! or LeRobot v2.1 output, use `rebake run` instead.
//!
//! # Example Usage
//!
//! ```bash
//! # Directory containing .mcap files
//! rebake export /data/recordings/ -o /data/output
//!
//! # Multiple directories with parallel processing
//! rebake export /data/recordings/ -o /data/output -j 4
//!
//! # Custom video settings via CLI options
//! rebake export /data/recordings/ -o /data/output --fps 60 --codec h264
//!
//! # Full control via YAML config file (for parameter optimization experiments)
//! rebake export /data/recordings/ -o /data/output --video-config video.yaml
//! ```

use std::fs::File;
use std::io::BufReader;

use anyhow::{Context, Result};
use camino::{Utf8Path, Utf8PathBuf};
use clap::{Args, ValueEnum};

use rebake::core::stage::{Context as PipelineContext, Stage};
use rebake::encode::depth_video_encoder::{
    DEFAULT_DEPTH_NVENC_AV1_QP, DEFAULT_DEPTH_NVENC_H265_QP, DepthCodecConfig, DepthVideoConfig,
};
use rebake::encode::nvenc::{
    DEFAULT_NVENC_AV1_PRESET, DEFAULT_NVENC_AV1_QP, DEFAULT_NVENC_B_FRAMES,
    DEFAULT_NVENC_H264_B_FRAMES, DEFAULT_NVENC_H264_PRESET, DEFAULT_NVENC_H264_PROFILE,
    DEFAULT_NVENC_H264_QP, DEFAULT_NVENC_H264_RC_LOOKAHEAD, DEFAULT_NVENC_H264_TUNE, NvencPreset,
};
use rebake::encode::video_encoder::{CodecConfig, VideoEncoderConfig, X264Preset};
use rebake::enrich::uuid_enricher::{UuidEnricher, UuidEnricherConfig};
use rebake::export::parquet_video_exporter::{ParquetVideoExporter, ParquetVideoExporterConfig};
use rebake::ingest::rosbag1_ingestor::{Rosbag1Ingestor, Rosbag1IngestorConfig};
use rebake::ingest::rosbag2_ingestor::{Rosbag2Ingestor, Rosbag2IngestorConfig};

use crate::error_display::print_error_indented;
use crate::rosbag_utils;

/// Video codec for encoding.
#[derive(clap::ValueEnum, Clone, Debug)]
pub enum Codec {
    /// AV1 via SVT-AV1 software encoder.
    Av1,
    /// H.264 via x264 software encoder.
    H264,
    /// HEVC via x265 software encoder.
    H265,
    /// AV1 via VA-API hardware encoder.
    #[value(name = "av1_vaapi")]
    Av1Vaapi,
    /// H.264 via VA-API hardware encoder.
    #[value(name = "h264_vaapi")]
    H264Vaapi,
    /// HEVC via VA-API hardware encoder.
    #[value(name = "h265_vaapi")]
    H265Vaapi,
    /// AV1 via NVIDIA NVENC hardware encoder.
    #[value(name = "av1_nvenc")]
    Av1Nvenc,
    /// H.264 via NVIDIA NVENC hardware encoder.
    #[value(name = "h264_nvenc")]
    H264Nvenc,
    /// HEVC via NVIDIA NVENC hardware encoder.
    #[value(name = "h265_nvenc")]
    H265Nvenc,
}

/// Depth video codec for encoding.
#[derive(clap::ValueEnum, Clone, Debug)]
pub enum DepthCodec {
    /// FFV1 lossless software encoder.
    Ffv1,
    /// AV1 via VA-API hardware encoder.
    #[value(name = "av1_vaapi")]
    Av1Vaapi,
    /// HEVC via VA-API hardware encoder.
    #[value(name = "h265_vaapi")]
    H265Vaapi,
    /// AV1 via NVIDIA NVENC hardware encoder.
    #[value(name = "av1_nvenc")]
    Av1Nvenc,
    /// HEVC via NVIDIA NVENC hardware encoder.
    #[value(name = "h265_nvenc")]
    H265Nvenc,
    /// AV1 via SVT-AV1 software encoder.
    Av1,
}

impl std::fmt::Display for DepthCodec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let pv = self.to_possible_value().ok_or(std::fmt::Error)?;
        f.write_str(pv.get_name())
    }
}

/// Export .bag/.mcap files to structured Parquet + Video format.
///
/// This command runs a minimal pipeline:
/// Rosbag1Ingestor/Rosbag2Ingestor → UuidEnricher → ParquetVideoExporter
///
/// For custom multi-stage pipelines, use 'rebake run' instead.
#[derive(Args, Debug, Clone)]
pub struct ExportArgs {
    /// Input ROS bag file(s), or a directory of them (.bag or .mcap).
    #[arg(
        required_unless_present = "export_single",
        value_name = "PATH",
        value_hint = clap::ValueHint::AnyPath
    )]
    pub rosbag_paths: Vec<Utf8PathBuf>,

    /// Output directory. UUID subdirectories will be created automatically.
    #[arg(
        short,
        long,
        alias = "output-dir",
        value_name = "DIR",
        value_hint = clap::ValueHint::DirPath
    )]
    pub output: Utf8PathBuf,

    /// Path to a YAML file containing VideoEncoderConfig.
    /// Mutually exclusive with --fps and --codec.
    #[arg(
        long,
        value_name = "FILE",
        value_hint = clap::ValueHint::FilePath,
        conflicts_with_all = ["fps", "codec", "qp"],
        help_heading = "RGB video options"
    )]
    pub video_config: Option<Utf8PathBuf>,

    /// RGB video frame rate.
    #[arg(long, default_value_t = 100, value_name = "N", help_heading = "RGB video options")]
    pub fps: u32,

    /// RGB video codec.
    #[arg(long, default_value_t = Codec::Av1, value_enum, help_heading = "RGB video options")]
    pub codec: Codec,

    /// Quality (QP) override for the QP-based hardware codecs.
    ///
    /// Defaults: h264_vaapi=21, h264_nvenc=26, h265_nvenc=25, av1_nvenc=130.
    #[arg(long, value_name = "N", conflicts_with = "video_config", help_heading = "RGB video options")]
    pub qp: Option<u32>,

    /// Depth video codec.
    #[arg(long, default_value_t = DepthCodec::Av1, value_enum, help_heading = "Depth video options")]
    pub depth_codec: DepthCodec,

    /// Depth video frame rate.
    #[arg(long, default_value_t = 30, value_name = "N", help_heading = "Depth video options")]
    pub depth_fps: u32,

    /// Maximum depth in millimeters for Q10Clip4 quantization.
    /// Only used for lossy depth codecs (ignored for FFV1).
    #[arg(long, default_value_t = 4092, value_name = "MM", help_heading = "Depth video options")]
    pub depth_max_mm: u16,

    /// Quality (QP) for the depth NVENC codecs.
    ///
    /// Defaults: h265_nvenc=10, av1_nvenc=20.
    #[arg(long, value_name = "N", help_heading = "Depth video options")]
    pub depth_qp: Option<u32>,

    /// ROS bags to convert in parallel.
    #[arg(short = 'j', long = "jobs", default_value_t = 1, value_name = "N")]
    pub jobs: usize,

    /// Internal: Single rosbag path for child process mode.
    /// Used by parent process to spawn child processes for parallel execution.
    #[arg(long, hide = true)]
    pub export_single: Option<Utf8PathBuf>,
}

/// Codec-specific default settings.
struct CodecDefaults {
    crf: &'static str,
    gop: u32,
}

impl ExportArgs {
    /// Builds the VideoEncoderConfig from CLI arguments or config file.
    ///
    /// If `--video-config` is specified, loads from YAML file.
    /// Otherwise, builds from --fps and --codec with optimized defaults.
    fn build_video_config(&self) -> Result<VideoEncoderConfig> {
        if let Some(config_path) = &self.video_config {
            // Load from YAML file
            let file = File::open(config_path.as_std_path())
                .with_context(|| format!("failed to open video config file: {}", config_path))?;
            let reader = BufReader::new(file);
            let config: VideoEncoderConfig = serde_yaml::from_reader(reader)
                .with_context(|| format!("failed to parse video config file: {}", config_path))?;
            Ok(config)
        } else if matches!(self.codec, Codec::Av1) {
            let mut config = VideoEncoderConfig::default();
            config.fps = self.fps;
            Ok(config)
        } else {
            // Build from CLI options with codec-specific defaults
            let defaults = self.codec_defaults();
            let codec_config = self.parse_codec_config()?;
            Ok(VideoEncoderConfig::new(self.fps)
                .set_gop(defaults.gop)
                .set_crf(defaults.crf.to_string())
                .set_codec_config(codec_config))
        }
    }

    /// Builds the DepthVideoConfig from CLI arguments.
    fn build_depth_config(&self) -> Result<DepthVideoConfig> {
        let codec_config = match self.depth_codec {
            DepthCodec::Ffv1 => DepthCodecConfig::Ffv1,
            DepthCodec::Av1Vaapi => DepthCodecConfig::Av1Vaapi {
                global_quality: 35,
                device: None,
            },
            DepthCodec::H265Vaapi => DepthCodecConfig::H265Vaapi {
                qp: 18,
                device: None,
            },
            DepthCodec::Av1Nvenc => DepthCodecConfig::Av1Nvenc {
                qp: self.depth_qp.unwrap_or(DEFAULT_DEPTH_NVENC_AV1_QP),
                gpu: None,
                preset: NvencPreset::P4,
                tune: None,
                b_frames: DEFAULT_NVENC_B_FRAMES,
                rc_lookahead: None,
            },
            DepthCodec::H265Nvenc => DepthCodecConfig::H265Nvenc {
                qp: self.depth_qp.unwrap_or(DEFAULT_DEPTH_NVENC_H265_QP),
                gpu: None,
                preset: NvencPreset::P4,
                tune: None,
                b_frames: DEFAULT_NVENC_B_FRAMES,
                rc_lookahead: None,
            },
            DepthCodec::Av1 => DepthCodecConfig::AV1 { crf: 4, preset: 4 },
        };

        Ok(DepthVideoConfig {
            depth_max_mm: self.depth_max_mm,
            codec_config,
            fps: self.depth_fps,
        })
    }

    /// Returns codec-specific default settings.
    fn codec_defaults(&self) -> CodecDefaults {
        match self.codec {
            Codec::H264 => CodecDefaults { crf: "15", gop: 2 },
            Codec::H264Vaapi => CodecDefaults { crf: "0", gop: 20 },
            Codec::H265 => CodecDefaults {
                crf: "18",
                gop: 100,
            },
            Codec::H265Vaapi => CodecDefaults {
                crf: "18",
                gop: 100,
            },
            Codec::Av1Vaapi => CodecDefaults {
                crf: "28",
                gop: 100,
            },
            // H.264 NVENC ignores crf; quality is controlled by qp.
            Codec::H264Nvenc => CodecDefaults { crf: "15", gop: 20 },
            Codec::H265Nvenc => CodecDefaults {
                crf: "18",
                gop: 100,
            },
            Codec::Av1Nvenc => CodecDefaults { crf: "28", gop: 20 },
            Codec::Av1 => CodecDefaults { crf: "34", gop: 20 },
        }
    }

    /// Converts the Codec enum into a CodecConfig with sensible defaults.
    fn parse_codec_config(&self) -> Result<CodecConfig> {
        Ok(match self.codec {
            Codec::Av1 => CodecConfig::AV1 {
                lp: None,
                pin: None,
                preset: 10,
                film_grain: None,
                film_grain_denoise: None,
                lookahead: None,
                fast_decode: None,
            },
            Codec::H264 => CodecConfig::H264 {
                threads: None,
                preset: X264Preset::Fast,
                tune: vec![],
            },
            Codec::H265 => CodecConfig::H265 {
                threads: None,
                preset: X264Preset::Superfast,
                tune: vec![],
                frame_threads: Some(6),
            },
            Codec::Av1Vaapi => CodecConfig::Av1Vaapi {
                qp: 124,
                device: None,
                profile: None,
                b_depth: None,
                async_depth: None,
            },
            Codec::H264Vaapi => CodecConfig::H264Vaapi {
                qp: self.qp.unwrap_or(21),
                device: None,
                profile: Some("high".to_string()),
                b_depth: None,
                async_depth: Some(16),
            },
            Codec::H265Vaapi => CodecConfig::H265Vaapi {
                qp: 29,
                device: None,
                profile: None,
                async_depth: None,
            },
            Codec::H264Nvenc => CodecConfig::H264Nvenc {
                qp: self.qp.unwrap_or(DEFAULT_NVENC_H264_QP),
                gpu: None,
                preset: DEFAULT_NVENC_H264_PRESET,
                tune: Some(DEFAULT_NVENC_H264_TUNE),
                profile: Some(DEFAULT_NVENC_H264_PROFILE.to_string()),
                b_frames: DEFAULT_NVENC_H264_B_FRAMES,
                rc_lookahead: Some(DEFAULT_NVENC_H264_RC_LOOKAHEAD),
            },
            Codec::H265Nvenc => CodecConfig::H265Nvenc {
                qp: self.qp.unwrap_or(25),
                gpu: None,
                preset: NvencPreset::P4,
                tune: None,
                profile: None,
                b_frames: DEFAULT_NVENC_B_FRAMES,
                rc_lookahead: None,
            },
            Codec::Av1Nvenc => CodecConfig::Av1Nvenc {
                qp: self.qp.unwrap_or(DEFAULT_NVENC_AV1_QP),
                gpu: None,
                preset: DEFAULT_NVENC_AV1_PRESET,
                tune: None,
                profile: None,
                b_frames: DEFAULT_NVENC_B_FRAMES,
                rc_lookahead: None,
            },
        })
    }
}

impl std::fmt::Display for Codec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let pv = self.to_possible_value().ok_or(std::fmt::Error)?;
        f.write_str(pv.get_name())
    }
}

/// Runs the export command.
pub fn run_export(args: ExportArgs) -> Result<()> {
    // Child process mode: process a single rosbag
    if let Some(rosbag_path) = &args.export_single {
        return run_single_export(&args, rosbag_path);
    }

    // Parent process mode: collect rosbags and dispatch
    let rosbag_paths = rosbag_utils::collect_rosbags_from_paths(&args.rosbag_paths)?;

    if rosbag_paths.len() == 1 || args.jobs <= 1 {
        run_sequential_export(&args, &rosbag_paths)
    } else {
        run_parallel_export(&args, rosbag_paths)
    }
}

/// Runs a single rosbag export (used by child processes or single-file mode).
fn run_single_export(args: &ExportArgs, rosbag_path: &Utf8PathBuf) -> Result<()> {
    let video_config = args.build_video_config()?;
    let depth_config = args.build_depth_config()?;

    let uuid_enricher_config = UuidEnricherConfig::default();
    let exporter_config = ParquetVideoExporterConfig::new(args.output.as_str())
        .with_video_config(video_config)
        .with_depth_config(depth_config);

    let mut uuid_enricher = UuidEnricher::new(uuid_enricher_config);
    let mut exporter = ParquetVideoExporter::new(exporter_config);

    let output_dir = process_single_rosbag(rosbag_path, &mut uuid_enricher, &mut exporter)?;

    println!("Exported to: {}", output_dir);
    Ok(())
}

/// Runs exports sequentially in the current process.
fn run_sequential_export(args: &ExportArgs, rosbag_paths: &[Utf8PathBuf]) -> Result<()> {
    let video_config = args.build_video_config()?;
    let depth_config = args.build_depth_config()?;

    let uuid_enricher_config = UuidEnricherConfig::default();
    let exporter_config = ParquetVideoExporterConfig::new(args.output.as_str())
        .with_video_config(video_config)
        .with_depth_config(depth_config);

    let mut uuid_enricher = UuidEnricher::new(uuid_enricher_config);
    let mut exporter = ParquetVideoExporter::new(exporter_config);

    let total = rosbag_paths.len();
    let mut success_count = 0;
    let mut error_count = 0;

    for (idx, rosbag_path) in rosbag_paths.iter().enumerate() {
        println!("[{}/{}] Processing: {}", idx + 1, total, rosbag_path);

        match process_single_rosbag(rosbag_path, &mut uuid_enricher, &mut exporter) {
            Ok(output_dir) => {
                println!("  -> Exported to: {}", output_dir);
                success_count += 1;
            }
            Err(e) => {
                print_error_indented(&e);
                error_count += 1;
            }
        }
    }

    println!();
    println!(
        "Export complete: {} succeeded, {} failed",
        success_count, error_count
    );

    if error_count > 0 {
        anyhow::bail!("{} file(s) failed to export", error_count);
    }

    Ok(())
}

/// Runs exports in parallel using process isolation.
///
/// Each rosbag is processed in a separate child process to avoid
/// SVT-AV1's global state race conditions.
#[allow(clippy::expect_used)] // clap ValueEnum guarantees all variants have values
fn run_parallel_export(args: &ExportArgs, rosbag_paths: Vec<Utf8PathBuf>) -> Result<()> {
    use clap::ValueEnum;
    use std::process::Command;

    let current_exe = std::env::current_exe().context("failed to get current executable")?;
    let parallelism = args.jobs.max(1);

    let mut pending: Vec<(usize, Utf8PathBuf)> = rosbag_paths.into_iter().enumerate().collect();
    let mut running: Vec<(usize, Utf8PathBuf, std::process::Child)> = Vec::new();
    let mut success_count = 0;
    let mut failed: Vec<(Utf8PathBuf, String)> = Vec::new();

    let total = pending.len();

    while !pending.is_empty() || !running.is_empty() {
        // Spawn new processes up to parallelism limit
        while running.len() < parallelism && !pending.is_empty() {
            let (index, path) = pending.remove(0);
            println!("[{}/{}] Starting: {}", index + 1, total, path);

            let mut cmd = Command::new(&current_exe);
            cmd.arg("export")
                .arg("--export-single")
                .arg(path.as_str())
                .arg("-o")
                .arg(args.output.as_str());

            // Pass video config: either file path or individual options
            if let Some(video_config_path) = &args.video_config {
                cmd.arg("--video-config").arg(video_config_path.as_str());
            } else {
                let codec_name = args
                    .codec
                    .to_possible_value()
                    .expect("all variants have values")
                    .get_name()
                    .to_string();
                cmd.arg("--fps")
                    .arg(args.fps.to_string())
                    .arg("--codec")
                    .arg(codec_name);
                if let Some(qp) = args.qp {
                    cmd.arg("--qp").arg(qp.to_string());
                }
            }

            // Pass depth config
            let depth_codec_name = args
                .depth_codec
                .to_possible_value()
                .expect("all variants have values")
                .get_name()
                .to_string();
            cmd.arg("--depth-codec")
                .arg(depth_codec_name)
                .arg("--depth-fps")
                .arg(args.depth_fps.to_string())
                .arg("--depth-max-mm")
                .arg(args.depth_max_mm.to_string());
            if let Some(depth_qp) = args.depth_qp {
                cmd.arg("--depth-qp").arg(depth_qp.to_string());
            }

            match cmd.spawn() {
                Ok(child) => running.push((index, path, child)),
                Err(e) => {
                    let msg = format!("spawn failed: {}", e);
                    println!("[{}/{}] Failed: {} ({})", index + 1, total, path, msg);
                    failed.push((path, msg));
                }
            }
        }

        // Poll running processes
        let mut still_running = Vec::new();
        for (index, path, mut child) in running {
            match child.try_wait() {
                Ok(Some(status)) => {
                    if status.success() {
                        println!("[{}/{}] Finished: {}", index + 1, total, path);
                        success_count += 1;
                    } else {
                        let msg = status
                            .code()
                            .map(|c| format!("exit code {}", c))
                            .unwrap_or_else(|| "terminated by signal".into());
                        println!("[{}/{}] Failed: {} ({})", index + 1, total, path, msg);
                        failed.push((path, msg));
                    }
                }
                Ok(None) => still_running.push((index, path, child)),
                Err(e) => {
                    let msg = format!("status check failed: {}", e);
                    println!("[{}/{}] Failed: {} ({})", index + 1, total, path, msg);
                    failed.push((path, msg));
                }
            }
        }
        running = still_running;

        // Avoid busy-waiting
        if !running.is_empty() {
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }

    println!();
    println!(
        "Export complete: {} succeeded, {} failed",
        success_count,
        failed.len()
    );

    if !failed.is_empty() {
        eprintln!("\nFailed rosbags:");
        for (path, reason) in &failed {
            eprintln!("  - {} ({})", path, reason);
        }
        anyhow::bail!("{} file(s) failed to export", failed.len());
    }

    Ok(())
}

fn is_ros1_bag(path: &Utf8Path) -> bool {
    path.extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("bag"))
}

/// Processes a single rosbag file through the pipeline.
fn process_single_rosbag(
    rosbag_path: &Utf8PathBuf,
    uuid_enricher: &mut UuidEnricher,
    exporter: &mut ParquetVideoExporter,
) -> Result<Utf8PathBuf> {
    // Create initial context with rosbag path
    let mut context = PipelineContext::default();
    context.set_rosbag_path(rosbag_path.clone());

    // Run pipeline: Ingest → Enrich → Export
    let context = if is_ros1_bag(rosbag_path) {
        let mut ingestor = Rosbag1Ingestor::new(Rosbag1IngestorConfig {
            require_metadata: true,
        });
        ingestor
            .run(context)
            .with_context(|| format!("Failed to ingest ROS1 bag {}", rosbag_path))?
    } else {
        let mut ingestor = Rosbag2Ingestor::new(Rosbag2IngestorConfig {
            require_metadata: true,
        });
        ingestor
            .run(context)
            .with_context(|| format!("Failed to ingest ROS2 bag {}", rosbag_path))?
    };

    let context = uuid_enricher
        .run(context)
        .with_context(|| "Failed to enrich with UUID")?;

    let context = exporter.run(context).with_context(|| "Failed to export")?;

    // Return the output directory
    context
        .output_dir()
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("Export completed but output_dir not set"))
}

#[cfg(test)]
mod tests {
    use super::{
        Codec, DEFAULT_DEPTH_NVENC_AV1_QP, DEFAULT_DEPTH_NVENC_H265_QP, DEFAULT_NVENC_AV1_PRESET,
        DEFAULT_NVENC_AV1_QP, DEFAULT_NVENC_H264_B_FRAMES, DEFAULT_NVENC_H264_PRESET,
        DEFAULT_NVENC_H264_PROFILE, DEFAULT_NVENC_H264_QP, DEFAULT_NVENC_H264_RC_LOOKAHEAD,
        DEFAULT_NVENC_H264_TUNE, DepthCodec, ExportArgs, is_ros1_bag,
    };
    use camino::Utf8Path;
    use rebake::encode::depth_video_encoder::DepthCodecConfig;
    use rebake::encode::video_encoder::CodecConfig;

    fn minimal_args(codec: Codec) -> ExportArgs {
        ExportArgs {
            rosbag_paths: vec![Utf8Path::new("/tmp/test.mcap").to_owned()],
            output: Utf8Path::new("/tmp/out").to_owned(),
            video_config: None,
            fps: 100,
            codec,
            qp: None,
            depth_codec: super::DepthCodec::Av1,
            depth_fps: 30,
            depth_max_mm: 4092,
            depth_qp: None,
            jobs: 1,
            export_single: None,
        }
    }

    #[test]
    fn detects_ros1_bag_by_extension() {
        assert!(is_ros1_bag(Utf8Path::new("/tmp/test.bag")));
        assert!(is_ros1_bag(Utf8Path::new("/tmp/test.BAG")));
        assert!(!is_ros1_bag(Utf8Path::new("/tmp/test.mcap")));
    }

    #[test]
    fn av1_vaapi_cli_defaults_match_production_video_config() {
        let args = minimal_args(Codec::Av1Vaapi);
        let config = args
            .build_video_config()
            .expect("video config should build");

        assert_eq!(config.fps, 100);
        assert_eq!(config.gop, 100);
        assert_eq!(config.crf, "28");
        match config.codec_config {
            CodecConfig::Av1Vaapi {
                qp,
                device,
                profile,
                b_depth,
                async_depth,
            } => {
                assert_eq!(qp, 124);
                assert_eq!(device, None);
                assert_eq!(profile, None);
                assert_eq!(b_depth, None);
                assert_eq!(async_depth, None);
            }
            other => panic!("expected AV1_VAAPI codec config, got {other:?}"),
        }
    }

    #[test]
    fn av1_nvenc_cli_uses_measured_defaults() {
        let args = minimal_args(Codec::Av1Nvenc);
        let config = args
            .build_video_config()
            .expect("av1_nvenc should use measured defaults");

        match config.codec_config {
            CodecConfig::Av1Nvenc { qp, preset, .. } => {
                assert_eq!(qp, DEFAULT_NVENC_AV1_QP);
                assert_eq!(preset, DEFAULT_NVENC_AV1_PRESET);
            }
            other => panic!("expected AV1_NVENC codec config, got {other:?}"),
        }
    }

    #[test]
    fn h264_nvenc_cli_uses_measured_general_defaults() {
        let args = minimal_args(Codec::H264Nvenc);
        let config = args
            .build_video_config()
            .expect("h264_nvenc should use measured defaults");

        assert_eq!(config.fps, 100);
        assert_eq!(config.gop, 20);
        assert_eq!(config.crf, "15");
        match config.codec_config {
            CodecConfig::H264Nvenc {
                qp,
                preset,
                tune,
                profile,
                b_frames,
                rc_lookahead,
                ..
            } => {
                assert_eq!(qp, DEFAULT_NVENC_H264_QP);
                assert_eq!(preset, DEFAULT_NVENC_H264_PRESET);
                assert_eq!(tune, Some(DEFAULT_NVENC_H264_TUNE));
                assert_eq!(profile.as_deref(), Some(DEFAULT_NVENC_H264_PROFILE));
                assert_eq!(b_frames, DEFAULT_NVENC_H264_B_FRAMES);
                assert_eq!(rc_lookahead, Some(DEFAULT_NVENC_H264_RC_LOOKAHEAD));
            }
            other => panic!("expected H264_NVENC codec config, got {other:?}"),
        }
    }

    #[test]
    fn av1_nvenc_cli_uses_explicit_qp() {
        let mut args = minimal_args(Codec::Av1Nvenc);
        args.qp = Some(80);
        let config = args
            .build_video_config()
            .expect("video config should build");

        match config.codec_config {
            CodecConfig::Av1Nvenc { qp, preset, .. } => {
                assert_eq!(qp, 80);
                assert_eq!(preset, DEFAULT_NVENC_AV1_PRESET);
            }
            other => panic!("expected AV1_NVENC codec config, got {other:?}"),
        }
    }

    #[test]
    fn depth_av1_nvenc_cli_uses_default_qp() {
        let mut args = minimal_args(Codec::Av1);
        args.depth_codec = DepthCodec::Av1Nvenc;
        let config = args
            .build_depth_config()
            .expect("depth config should build");

        match config.codec_config {
            DepthCodecConfig::Av1Nvenc { qp, .. } => assert_eq!(qp, DEFAULT_DEPTH_NVENC_AV1_QP),
            other => panic!("expected AV1_NVENC depth codec config, got {other:?}"),
        }
    }

    #[test]
    fn depth_h265_nvenc_cli_uses_default_qp() {
        let mut args = minimal_args(Codec::Av1);
        args.depth_codec = DepthCodec::H265Nvenc;
        let config = args
            .build_depth_config()
            .expect("depth config should build");

        match config.codec_config {
            DepthCodecConfig::H265Nvenc { qp, .. } => assert_eq!(qp, DEFAULT_DEPTH_NVENC_H265_QP),
            other => panic!("expected H265_NVENC depth codec config, got {other:?}"),
        }
    }
}
