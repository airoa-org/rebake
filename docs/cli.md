# CLI Usage

The `rebake-cli` tool provides three modes of operation:

1. **Run mode** (`rebake-cli run`) - Full pipeline execution with YAML configuration
2. **Export mode** (`rebake-cli export`) - Simple export to Parquet + Video format
3. **Merge mode** (`rebake-cli merge`) - Merge multiple LeRobot v2.1 datasets into one

## Install

```bash
cargo install --path rebake-cli
```

This installs the `rebake-cli` binary into your Cargo bin directory (typically `~/.cargo/bin/`).

---

## Run Command

The `run` command executes full pipelines defined in YAML configuration files.

For simple rosbag-to-Parquet conversion without YAML, use `rebake-cli export` instead.

### Basic Usage

```bash
rebake-cli run <PATH> -c <CONFIG_FILE>
```

### Arguments

| Argument | Short | Required | Default | Description |
|----------|-------|----------|---------|-------------|
| `<PATH>` | - | Yes | - | Input path(s). Rosbag ingestors accept `.bag`/`.mcap`; `ParquetVideoIngestor` accepts intermediate format data directories (`parquet/` + `videos/`) |
| `--config` | `-c` | Yes* | - | Pipeline config file (YAML) |
| `--jobs` | `-j` | No | 1 | Number of parallel pipelines |

\* Either `--config` or `--config-data` must be provided. `--config-data` is an internal option used for subprocess spawning and should not be used directly.

### Examples

#### Process a Directory

When the first stage is a rosbag ingestor, passing a directory makes the CLI recursively find all `.bag` and `.mcap` files. Backup files (`.orig.bag`, `.orig.mcap`) created by `rosbag reindex` are automatically excluded.

```bash
rebake-cli run \
    /data/recordings/ \
    -c config/pipeline/hsr2.yaml
```

#### Parallel Processing

Process multiple files in parallel:

```bash
rebake-cli run \
    /data/recordings/ \
    -c config/pipeline/hsr2.yaml \
    -j 4
```

#### Single File

You can also pass a single rosbag file directly:

```bash
rebake-cli run \
    /data/recording.mcap \
    -c config/pipeline/hsr2.yaml
```

#### Intermediate-Format Data (Parquet/MP4)

When the first stage is `ParquetVideoIngestor`, you can pass either:

- A single intermediate format dataset directory directly, or
- A parent directory containing multiple intermediate format datasets

If the provided directory is not itself a dataset directory, the CLI recursively discovers dataset directories by looking for `parquet/_topic_type_map.parquet` and `parquet/_metadata.parquet`.

You do not need to repeat the input path in the YAML config.

```bash
rebake-cli run \
    /data/exported_bundle/ \
    -c config/pipeline/yubi_from_parquet.yaml
```

---

## Export Command

The `export` command provides a simple way to export rosbag files without a YAML configuration file. It outputs structured Parquet files and encoded videos suitable for data lake ingestion.

The command supports both ROS 1 bag (`.bag`) and ROS 2 MCAP (`.mcap`) input.

### Basic Usage

```bash
rebake-cli export <PATH> -o <DIR>
```

### Output Structure

```
<DIR>/
  <UUID>/
    parquet/
      <topic>.parquet           # Topic data
      _metadata.parquet         # Rosbag metadata
      _topic_type_map.parquet   # Topic name to message type mapping
      _video_registry.parquet   # Topic name to video path mapping
    videos/
      <topic>.mp4               # Encoded video files
```

### Arguments

| Argument | Short | Required | Default | Description |
|----------|-------|----------|---------|-------------|
| `<PATH>` | - | Yes | - | Input `.bag`/`.mcap` file(s) or directory containing rosbag files |
| `--output` | `-o` | Yes | - | Output directory |
| `--video-config` | - | No | - | Path to VideoEncoderConfig YAML file (for fine-tuning) |
| `--fps` | - | No | 100 | Video frame rate |
| `--codec` | - | No | av1 | Video codec (av1, h264, h265, av1_vaapi, h264_vaapi, h265_vaapi, av1_nvenc, h264_nvenc, h265_nvenc) |
| `--qp` | - | No | - | QP override for QP-based hardware codecs. Applies to `h264_vaapi`, `h264_nvenc`, `h265_nvenc`, `av1_nvenc`. Defaults: h264_vaapi=21, h264_nvenc=26, h265_nvenc=25, av1_nvenc=130. Ignored by `av1_vaapi`/`h265_vaapi` (fixed defaults) and software codecs (which use crf) |
| `--depth-codec` | - | No | av1 | Depth video codec (ffv1, av1, av1_vaapi, h265_vaapi, av1_nvenc, h265_nvenc) |
| `--depth-fps` | - | No | 30 | Depth video frame rate |
| `--depth-max-mm` | - | No | 4092 | Maximum depth in mm for Q10Clip4 quantization (ignored for FFV1) |
| `--depth-qp` | - | No | - | Depth NVENC QP override. Defaults: h265_nvenc=10, av1_nvenc=20 |
| `--jobs` | `-j` | No | 1 | Maximum parallel processes |

> **Note**: `--video-config` is mutually exclusive with `--fps`, `--codec`, and `--qp`. The `--depth-*` options are independent and can be used alongside either `--video-config` or `--fps`/`--codec`.

### Examples

#### Directory Export

```bash
rebake-cli export /data/recordings/ -o /data/output
```

#### Parallel Processing

```bash
rebake-cli export /data/recordings/ -o /data/output -j 4
```

For NVENC codecs on a single GeForce GPU, keep `-j` at `8` or lower — NVIDIA caps concurrent NVENC sessions per GPU on consumer cards (Quadro/RTX Pro/Tesla cards are uncapped). Use a smaller value if other NVENC jobs are running on the same GPU.

#### Custom Video Settings

```bash
rebake-cli export /data/recordings/ -o /data/output \
    --fps 60 --codec h264
```

When using `--codec av1_vaapi` without `--video-config`, `rebake export` defaults to `gop: 100` and `qp: 124`.

NVENC codecs work the same way. Without `--video-config`, the CLI applies these codec-specific defaults:

| Codec | Default QP | Default preset | Default GOP |
|-------|-----------:|----------------|------------:|
| `h264_nvenc` | 26 | P5 | 20 |
| `h265_nvenc` | 25 | P4 | 100 |
| `av1_nvenc` | 130 | P7 | 20 |

```bash
rebake-cli export /data/recordings/ -o /data/output \
    --fps 100 --codec av1_nvenc
```

`--qp` overrides the QP for `h264_vaapi` and the NVENC codecs (`h264_nvenc`, `h265_nvenc`, `av1_nvenc`); `av1_vaapi` and `h265_vaapi` use fixed defaults and ignore it. To set other options (`b_frames`, `rc_lookahead`, `tune`, `profile`, `gpu`), use `--video-config` with one of the example files in `config/export/`.

#### Custom Depth Video Settings

```bash
rebake-cli export /data/recordings/ -o /data/output \
    --depth-codec ffv1 --depth-fps 15
```

Depth NVENC codecs follow the same pattern. Without `--video-config`, the CLI uses conservative QP defaults tuned for `--depth-max-mm 4092`:

| Depth codec | Default QP | Default preset |
|-------------|-----------:|----------------|
| `h265_nvenc` | 10 | P4 |
| `av1_nvenc` | 20 | P4 |

Override with `--depth-qp`:

```bash
rebake-cli export /data/recordings/ -o /data/output \
    --depth-codec av1_nvenc --depth-qp 24
```

#### Using a Video Config File

For parameter optimization experiments, use a YAML config file:

```bash
rebake-cli export /data/recordings/ -o /data/output \
    --video-config video_config.yaml
```

Example video config file (`video_config.yaml`):

```yaml
fps: 30
gop: 10
crf: "23"
scaling: Bicubic
codec_config:
  codec: H264
  preset: medium
  # Optional: tune: [film, fastdecode]
```

See `config/export/` for more video config examples:
- `video_config_av1.yaml` - AV1 (SVT-AV1) software encoder
- `video_config_h264.yaml` - H.264 (x264) software encoder
- `video_config_h265.yaml` - H.265 (x265) software encoder
- `video_config_av1_vaapi.yaml` - AV1 VA-API hardware encoder
- `video_config_av1_nvenc.yaml` - AV1 NVENC hardware encoder
- `video_config_h264_nvenc.yaml` - H.264 NVENC hardware encoder
- `video_config_h265_nvenc.yaml` - H.265 NVENC hardware encoder

---

## Merge Command

The `merge` command combines multiple LeRobot v2.1 datasets into a single dataset. It handles episode renumbering, task deduplication, parquet column remapping, video file copying, and metadata consolidation.

Typical workflow: `rebake run` -> `rebake merge`

### Basic Usage

```bash
rebake-cli merge <SOURCE_DIR> -o <DIR>
```

The `SOURCE_DIR` directory should contain multiple LeRobot dataset subdirectories. Each subdirectory with a `meta/info.json` file is automatically detected as a dataset.

```
SOURCE_DIR/
├── dataset_a/        ← detected (has meta/info.json)
│   ├── meta/
│   ├── data/
│   └── videos/
├── dataset_b/        ← detected
│   ├── meta/
│   ├── data/
│   └── videos/
└── README.txt        ← ignored (no meta/info.json)
```

### Arguments

| Argument | Short | Required | Default | Description |
|----------|-------|----------|---------|-------------|
| `<SOURCE_DIR>` | - | Yes | - | Directory containing LeRobot dataset subdirectories |
| `--output` | `-o` | Yes | - | Output directory for the merged dataset |
| `--chunk-size` | - | No | (from first source) | Override the number of episodes per chunk |

### Examples

#### Basic Merge

```bash
rebake-cli merge /data/datasets -o /data/merged
```

#### With Custom Chunk Size

```bash
rebake-cli merge /data/datasets -o /data/merged --chunk-size 500
```

---

## Configuration

For detailed configuration file format, see [Configuration Reference](configuration.md).

Quick links:
- [Pipeline Config](configuration.md#pipeline-config) - Stage definitions
- [Stage Reference](configuration.md#stage-reference) - Per-stage options
- [Robot Model Config](configuration.md#robot-model-config) - Topic-to-feature mapping

## Supported File Types

### Rosbag Input

| Extension | Format |
|-----------|--------|
| `.bag` | ROS 1 bag |
| `.mcap` | ROS 2 bag (MCAP) |

### Intermediate Format Input

| Directory Structure | Format |
|---------------------|--------|
| `parquet/` + `videos/` | rebake intermediate format (Parquet + MP4) |

Make sure your pipeline config uses the correct ingestor for your input type:

- `Rosbag1IngestorConfig` for `.bag` files (ROS 1)
- `Rosbag2IngestorConfig` for `.mcap` files (ROS 2)
- `ParquetVideoIngestorConfig` for intermediate format directories (`parquet/` + `videos/`)

> **Note**: If a directory contains mixed file types (`.bag` and `.mcap`), you should either separate them into different directories or run the CLI separately with different config files for each type.

> **Note**: Backup files (`.orig.bag`, `.orig.mcap`) are not supported. If you specify one directly, the CLI will return an error.

## Troubleshooting

### Common Errors and Solutions

#### File not found

```text
Error: failed to access /path/to/file
  Caused by: No such file or directory (os error 2)
```

**Solution**: Check the file path and ensure the file exists.

#### Permission denied

```text
Error: failed to access /path/to/file
  Caused by: Permission denied (os error 13)
```

**Solution**: Grant read permission with `chmod +r <file>`.

#### Invalid YAML configuration

```text
Error: while parsing a block mapping, did not find expected key at line 2 column 1
```

**Solution**: Validate your YAML file with `yamllint config/your_config.yaml`.

#### No rosbag files found

```text
Error: no rosbag files found under directory: /path/to/dir
```

**Solution**: Verify that the directory contains `.bag` or `.mcap` files.

#### Invalid rosbag extension

```text
Error: rosbag files must have .bag or .mcap extension: /path/to/file.txt
```

**Solution**: Rename the file to have `.bag` or `.mcap` extension, or provide a valid rosbag file.

#### Stage failure

```text
Error: stage rosbag2_ingestor failed: missing required data: dataset in context
  Caused by: missing required data: dataset in context
```

**Solution**: Examine `logs/pipeline.log` for detailed error traces and stage-specific information.

### Logging

The CLI writes detailed logs to `logs/pipeline.log`. To also output logs to stderr, set:

```bash
export REBAKE_LOG_STDERR=1
```

To change the log level:

```bash
export RUST_LOG=debug  # Options: error, warn, info, debug, trace
```
