# CLI

`rebake-cli` has three commands. `export` turns ROS bags into the intermediate format, `run` uses a YAML pipeline to write LeRobot datasets, and `merge` combines multiple LeRobot datasets into one.

This page is the command reference. For pipeline and robot-model YAML, see [configuration](configuration.md). For codecs, see [encoding](encoding.md). For the `meta.json` sidecar each ROS bag needs, see [metadata](metadata.md). For the first end-to-end path, see the [guide](guide.md). For building, see the [README](../README.md).

## Which command to use

| You have | You want | Command |
|---|---|---|
| ROS bags (`.bag` / `.mcap`) | A queryable, reusable intermediate format | [`export`](#export) |
| ROS bags, or the intermediate format | A LeRobot v2.1 dataset | [`run`](#run) |
| Multiple LeRobot v2.1 datasets | One combined dataset | [`merge`](#merge) |

The usual workflow is to write one dataset per ROS bag with `run`, then combine them with `merge`. If you expect to try several pipeline settings, run `export` first. Then later `run` commands can read the intermediate format instead of re-reading the original ROS bags.

## export

Converts ROS bags to the [intermediate format](intermediate-format.md): one Parquet table per topic plus videos, without a pipeline config.

```bash
rebake-cli export <PATH>... -o <DIR> [options]
```

Each ROS bag writes one output under `<DIR>/<uuid>/`. The directory contains `parquet/` and `videos/`. See [intermediate format](intermediate-format.md) for the full layout.

Each ROS bag needs a [meta.json](metadata.md) next to it. `.bag` files are read as ROS 1, and `.mcap` files are read as ROS 2.

### Basic options

| Option | Default | Meaning |
|---|---|---|
| `<PATH>...` | required | ROS bag files, or directories containing them |
| `-o`, `--output <DIR>` | required | Output directory. One `<uuid>/` directory is created per recording. Alias: `--output-dir` |
| `-j`, `--jobs <N>` | `1` | Number of ROS bags to convert in parallel |

### RGB options

| Option | Default | Meaning |
|---|---|---|
| `--fps <N>` | `100` | RGB video frame rate |
| `--codec <CODEC>` | `av1` | RGB codec: `av1`, `h264`, `h265`, `av1_vaapi`, `h264_vaapi`, `h265_vaapi`, `av1_nvenc`, `h264_nvenc`, `h265_nvenc` |
| `--qp <N>` | codec-specific | Override QP for `h264_vaapi` and the RGB NVENC codecs |
| `--video-config <FILE>` | none | Use a YAML `VideoEncoderConfig`. Cannot be combined with `--fps`, `--codec`, or `--qp` |

The `--qp` defaults are `h264_vaapi=21`, `h264_nvenc=26`, `h265_nvenc=25`, and `av1_nvenc=130`. Software codecs and `av1_vaapi` / `h265_vaapi` do not use `--qp`.

### Depth options

| Option | Default | Meaning |
|---|---|---|
| `--depth-codec <CODEC>` | `av1` | Depth codec: `ffv1`, `av1`, `av1_vaapi`, `h265_vaapi`, `av1_nvenc`, `h265_nvenc` |
| `--depth-fps <N>` | `30` | Depth video frame rate |
| `--depth-max-mm <MM>` | `4092` | Maximum distance kept by lossy depth encoding, in millimeters. Ignored by `ffv1` |
| `--depth-qp <N>` | codec-specific | Override QP for depth NVENC codecs |

The `--depth-qp` defaults are `h265_nvenc=10` and `av1_nvenc=20`. The `--depth-*` flags are independent from RGB settings, so they can be used with `--video-config`. For codec choices and detailed defaults, see [encoding](encoding.md).

### Examples

```bash
# Convert a directory of ROS bags to the intermediate format, eight at a time.
rebake-cli export ./yubi_recordings -o ./intermediate -j 8

# Store RGB as H.264 for faster decode during training.
rebake-cli export ./recording.mcap -o ./intermediate --fps 60 --codec h264

# Store depth with lossless FFV1.
rebake-cli export ./yubi_recordings -o ./intermediate --depth-codec ffv1 --depth-fps 15

# Read detailed RGB settings from YAML.
rebake-cli export ./yubi_recordings -o ./intermediate --video-config config/export/video_config_h265.yaml
```

If one recording fails, the remaining recordings are still processed. At the end, `export` reports the failures and exits with status 1.

## run

Runs inputs through a YAML pipeline. This is the normal command for writing LeRobot datasets.

```bash
rebake-cli run <PATH>... -c <CONFIG> [-j <N>]
```

| Option | Default | Meaning |
|---|---|---|
| `<PATH>...` | required | Input paths. The first Ingestor in the pipeline decides how they are read |
| `-c`, `--config <FILE>` | required | Pipeline config (YAML) |
| `-j`, `--jobs <N>` | `1` | Number of inputs to process in parallel |

If the pipeline starts with a ROS bag Ingestor, inputs are `.bag` / `.mcap` files or directories containing them. If it starts with `ParquetVideoIngestorConfig`, inputs are intermediate-format directories or parent directories containing them. You do not repeat the input path inside the YAML.

`run` has no output-directory flag. Working output is written under the config's `work_dir`. LeRobot datasets are written under `LeRobotV21TransformerConfig.outdir/<uuid>/`; that is usually the output you inspect. Whether a failed input stops the rest of the batch is controlled by `stop_on_error` in the config, not by a CLI flag. The default is `true`; see [pipeline config](configuration.md).

```bash
# Convert a directory of ROS bags to LeRobot, eight at a time.
rebake-cli run ./yubi_recordings -c config/pipeline/yubi.yaml -j 8

# Re-curate from an exported intermediate format.
rebake-cli run ./intermediate -c config/pipeline/yubi_from_parquet.yaml
```

## merge

Combines LeRobot datasets produced per recording into one training dataset. It renumbers episodes, deduplicates tasks, consolidates metadata, and copies videos without re-encoding them.

```bash
rebake-cli merge <SOURCE_DIR> -o <DIR> [--chunk-size <N>]
```

| Option | Default | Meaning |
|---|---|---|
| `<SOURCE_DIR>` | required | Immediate child directories containing `meta/info.json` are detected as datasets |
| `-o`, `--output <DIR>` | required | Output directory for the merged dataset |
| `--chunk-size <N>` | first dataset's value | Episodes per chunk. Alias: `--chunks-size` |

Discovery is alphabetical, so episode order is deterministic. Merging requires at least two datasets, and their `fps`, `codebase_version`, and feature schema must match. For video features, codec and pixel format must also match. `merge` has no `-j`.

```bash
rebake-cli merge ./lerobot_yubi -o ./lerobot_merged
```

On success, it prints `Merge completed successfully.`

## Shared conventions

### Input paths

ROS bag input (`export`, and `run` pipelines that start with a ROS bag Ingestor):

- A single file must end in `.bag` or `.mcap`; other extensions are rejected.
- Directories are searched recursively. Found paths are sorted and deduplicated.
- Backup files left by `rosbag reindex` (`.orig.bag` / `.orig.mcap`) are skipped during directory search and rejected when passed directly.
- A single `run` command cannot mix `.bag` and `.mcap`, because the pipeline has one Ingestor. `export` can mix them because it chooses the reader per file.

Intermediate-format input (`run` pipelines that start with `ParquetVideoIngestorConfig`):

- A directory is treated as an intermediate format if it contains both `parquet/_metadata.parquet` and `parquet/_topic_type_map.parquet`.
- `videos/` is not part of that detection, so state-only outputs are accepted.
- If the path itself is not an intermediate format, rebake searches below it.

### Parallelism

`-j` is input-level parallelism. When `export` or `run` uses a value greater than 1, each input is processed in a separate process. If NVENC starts failing because the GPU driver's concurrent encoding limit is exceeded, lower `-j`.

### Logs

Detailed logs are written to `logs/pipeline.log`.

| Environment variable | Default | Meaning |
|---|---|---|
| `REBAKE_LOG_DIR` | `logs` | Directory containing `pipeline.log` |
| `RUST_LOG` | `warn` | Log filter. Use `info` or `debug` for more stage-level detail |
| `REBAKE_LOG_STDERR` | unset | Any value other than `0` also writes logs to stderr |

```bash
RUST_LOG=info REBAKE_LOG_STDERR=1 rebake-cli run ./yubi_recordings -c config/pipeline/yubi.yaml
```

`pipeline.log` is appended to and not rotated. Delete it when it grows too large.

### Help and exit status

Use `rebake-cli --help`, `rebake-cli <command> --help`, and `rebake-cli --version`. On failure, the command exits with status 1 and prints `Error:` followed by `Caused by:`.

```text
Error: <what failed>
  Caused by: <root cause>
```

## Troubleshooting

| Message fragment | Cause | Fix |
|---|---|---|
| `input path must be provided` | `run` was called without an input path | Use `rebake-cli run <PATH> -c <CONFIG>` |
| `no rosbag files found under directory` | The directory has no `.bag` / `.mcap` files | Check the path |
| `rosbag files must have .bag or .mcap extension` | A non-ROS-bag file was passed directly | Pass a ROS bag file or a directory |
| `ignoring backup rosbag produced by rosbag reindex` | A `.orig.bag` / `.orig.mcap` backup was passed directly | Use the ROS bag without `.orig` |
| `bundle input must be a directory` | A file was passed as intermediate-format input | Pass an intermediate-format directory |
| `no rebake bundle directories found` | `run` input is not the intermediate format | Pass a directory containing `parquet/_metadata.parquet` and `parquet/_topic_type_map.parquet`, or its parent |
| `failed to read meta.json` | `meta.json` is missing next to the ROS bag | Add `meta.json`; see [metadata](metadata.md) |
| YAML parse errors such as `while parsing ...` | The config file is invalid YAML | Fix the line and column reported by the parser |
| `expected at least 2 dataset directories` | `merge` found fewer than two datasets | Pass the parent directory of datasets containing `meta/info.json` |
| `FPS mismatch` / `codebase_version mismatch` / `feature ... mismatch` | Datasets passed to `merge` do not share a schema | Recreate them with the same pipeline config |

For stage-order and `missing required data` errors, see the troubleshooting table in [configuration](configuration.md). For metadata errors, see [metadata](metadata.md).
