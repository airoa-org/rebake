# Configuration

`rebake-cli run` uses two YAML files. The pipeline config says how to build the dataset, as an ordered stage list. The robot model says what to output, by mapping topics to dataset features. A feature is a column or video name read by training code.

This page is the reference for both. Codec settings are in [encoding](encoding.md), `meta.json` is in [metadata](metadata.md), and the first-time path is in the [guide](guide.md).

## Pipeline config

This minimal pipeline reads YUBI ROS 2 bags, synchronizes to 30 rows per second, and writes LeRobot v2.1. You can save and run it as-is for YUBI data.

```yaml
work_dir: "./orchestrator_work"
stage_configs:
  - Rosbag2IngestorConfig: {}
  - ZeroOrderHoldTimeSynchronizerConfig:
      fps: 30
  - LeRobotV21TransformerConfig:
      outdir: "./lerobot_yubi"
      robot_model: "./config/robot_model/yubi.yaml"
      video_config:
        fps: 30        # match the synchronizer fps, or rows and video will drift without an error
```

Top-level keys:

| Key | Type | Default | Meaning |
|---|---|---|---|
| `work_dir` | path | required | Working output directory. One folder is created per input |
| `stage_configs` | list | required | Ordered stage list. Each item is `StageName: {that stage's fields}` |
| `save_contexts` | true/false | `false` | Save tables after each stage as Parquet under `work_dir`; useful for checking expected columns |
| `stop_on_error` | true/false | `true` | Stop after one input fails. Set `false` to process the whole batch and report failures at the end |
| `video_cache_root` | path | `./video_cache` | Video cache location |

Relative paths inside the config, such as `robot_model` and `outdir`, are resolved from the directory where you run `rebake-cli`, not from the directory that contains the config file.

Stage names are the exact YAML strings, such as `Rosbag2IngestorConfig`. Unknown names fail when the config is loaded. Misspelled fields pass through many stages unnoticed, but video and codec configs reject unknown keys.

## Stage order

Order is the rule. Each stage works on data produced by the stages before it. The reliable shape is:

```text
Ingestor -> raw Enricher -> Synchronizer -> synced Enricher -> Transformer / Exporter
```

Put before the Synchronizer only stages that need original message timing. The main one is `TfBufferEnricherConfig`, because it builds TF from the raw update times. The two [HSR-specific stages](#hsr-specific-stages) also go before synchronization. Most other Enrichers go after the Synchronizer.

If the order is wrong, the stage usually fails at runtime with `missing required data: ...`. See [troubleshooting](#troubleshooting) for the message fragments and fixes.

### What Synchronizer does

Fixed-rate Synchronizers (ZeroOrderHold / NearestNeighbor) create a timeline at `fps` intervals and assign every topic's value to each time. The timeline starts at the latest start time among all topics and ends at the latest end time among all topics. If one topic stopped early, its last value is carried forward until the end. That is what you are seeing when a value looks flat near the end of an episode.

Synchronizer adds two columns to every topic. `synched_timestamp_ns` is the row time. `is_fresh` says whether a new message arrived at that time, or whether the value was carried forward. Mapping `is_fresh` into a feature lets training distinguish measured values from held values.

## Stage reference

Each stage below lists what it does, its YAML, fields, and notes. Every YAML snippet is one item under `stage_configs:`. In field tables, `required` in the Default column means the field cannot be omitted.

### Ingestor

The first stage must be an Ingestor. It either reads ROS bags or re-ingests an exported intermediate format.

---

#### Rosbag2IngestorConfig

Reads ROS 2 `.mcap` files and turns all topics into tables.

```yaml
- Rosbag2IngestorConfig: {}
```

| Field | Type | Default | Meaning |
|---|---|---|---|
| `require_metadata` | true/false | `true` | Read `meta.json` next to the ROS bag. Set `false` only for pipelines with no [metadata-dependent stages](metadata.md#which-stages-need-it) |

Large image, depth, and point-cloud payloads are moved out of tables, leaving an `index` column as a reference. This is the same mechanism used by the [intermediate format](intermediate-format.md#image-depth-and-point-cloud-payloads).

---

#### Rosbag1IngestorConfig

Reads ROS 1 `.bag` files. Fields and behavior are the same as `Rosbag2IngestorConfig`.

```yaml
- Rosbag1IngestorConfig: {}
```

---

#### ParquetVideoIngestorConfig

Reads an exported [intermediate format](intermediate-format.md). Use this to rebuild datasets with a different config without decoding the ROS bag again. No `meta.json` sidecar is needed because metadata is inside the intermediate format.

```yaml
- ParquetVideoIngestorConfig: {}
```

| Field | Type | Default | Meaning |
|---|---|---|---|
| `input_dir` | path | CLI input path | Usually omit this. The input passed to `rebake-cli run <PATH>` is used |

### Synchronizer

Synchronizers put all topics on one timeline. The three variants differ only in how they choose a value for each time. If unsure, use ZeroOrderHold.

---

#### ZeroOrderHoldTimeSynchronizerConfig

At each time, uses the last value that topic had at or before that time. It does not look into the future, so it is safe for state such as joint positions.

```yaml
- ZeroOrderHoldTimeSynchronizerConfig:
    fps: 30
```

| Field | Type | Default | Meaning |
|---|---|---|---|
| `fps` | integer | required | Rows per second. This becomes the dataset playback rate. Set Transformer `video_config.fps` to the same value |

---

#### NearestNeighborTimeSynchronizerConfig

At each time, uses the value nearest in time. It can use a slightly future message, so prefer ZeroOrderHold for values that must not look ahead.

```yaml
- NearestNeighborTimeSynchronizerConfig:
    fps: 30
```

| Field | Type | Default | Meaning |
|---|---|---|---|
| `fps` | integer | required | Same as above |

---

#### TimestampMergeTimeSynchronizerConfig

Uses the union of all original topic timestamps as the timeline. The result is not evenly spaced. There are no fields.

```yaml
- TimestampMergeTimeSynchronizerConfig: {}
```

It does not set `fps` and does not add `is_fresh`, so it is normally not paired with fixed-rate dataset transformation. It is for workflows that want to preserve original times and write them back to the intermediate format.

### Enricher

Enrichers create new topics or columns from data already present.

---

#### TfBufferEnricherConfig

Reads `/tf` and, if present, `/tf_static`, then builds a `/tf_buffer` topic that can answer transform queries between frames at arbitrary times. There are no fields.

```yaml
- TfBufferEnricherConfig: {}
```

Run it before the Synchronizer. `TfChainEnricherConfig` uses this buffer.

---

#### TfChainEnricherConfig

Computes poses for named frame pairs and writes them to `/tf_chain`.

```yaml
- TfChainEnricherConfig:
    frame_pairs:
      - source: base_link
        target: hand_link
      - source: base_link
        target: camera_link
```

| Field | Type | Default | Meaning |
|---|---|---|---|
| `frame_pairs` | list | required | Pairs of `source` and `target` frame names |

Each pair contains `transform` (translation x, y, z and rotation quaternion x, y, z, w) and `is_fresh` (true if any transform on the path updated at that time). From a robot model, select it with a [field path](#field-paths) such as `/base_link/hand_link/transform`.

---

#### DeltaJointPositionEnricherConfig

Adds a `delta_position` column beside a topic's `position` column. It is the difference from the previous row. The first row is 0.

```yaml
- DeltaJointPositionEnricherConfig:
    topic_names:
      - /joint_states
```

| Field | Type | Default | Meaning |
|---|---|---|---|
| `topic_names` | list | required | Topics whose `position` is a float list |

Topics that do not match are passed through without an error. If the column does not appear, first check the topic spelling.

---

#### DeltaTransformEnricherConfig

Adds `delta_transform` beside transform structures inside topics, including nested structures. The delta is the change from the previous row.

```yaml
- DeltaTransformEnricherConfig:
    topic_names: ["/tf_chain"]
    delta_reference_frame: previous_target_frame
```

| Field | Type | Default | Meaning |
|---|---|---|---|
| `topic_names` | list | required | Topics to scan for transforms. Usually `["/tf_chain"]` |
| `delta_reference_frame` | `previous_target_frame` or `source_frame` | required | Frame used to express the change. For actions, use `previous_target_frame`; it is motion in the previous target-frame coordinates. `source_frame` is component-wise difference in the source frame |

---

#### ShiftEnricherConfig

Copies a topic and shifts its rows by `shift_steps`. With `1`, each row gets the next row's values. This is the standard way to turn observation columns into "what the robot should do next" action columns.

```yaml
- ShiftEnricherConfig:
    source_topic: /joint_states
    output_topic: /joint_states/action
    shift_steps: 1
```

| Field | Type | Default | Meaning |
|---|---|---|---|
| `source_topic` | string | required | Original topic. It is not modified |
| `output_topic` | string | required | New topic name |
| `shift_steps` | integer | required | Rows to shift. Positive means future, negative means past |
| `fill_strategy` | `edge` or `zero` | `edge` | How to fill empty rows at the ends. `edge` uses the nearest real value. `zero` fills numeric columns with 0 and uses `edge` for other columns |

Time columns (`synched_timestamp_ns`, `timestamp_ns`, `is_fresh`) are not shifted. Only values move.

---

#### UuidEnricherConfig

Adds the recording UUID as `rosbag_uuid` to every topic, so rows remain traceable after datasets are merged. There are no fields.

```yaml
- UuidEnricherConfig: {}
```

Missing metadata stops the whole run. Do not add this stage to a pipeline without `meta.json`.

---

#### HSR-specific stages

These reconstruct command topics for Toyota HSR recordings. Do not use them for other robots. Both have no fields. If the target topic already exists, the stage does nothing. Run them before the Synchronizer.

`HandCommandEnricherConfig` reconstructs missing gripper command `/hsrb/gripper_controller/command` from servo measurements. `HeadCommandEnricherConfig` reconstructs head command `/hsrb/head_trajectory_controller/command` from `/hsrb/joint_states`.

```yaml
- HandCommandEnricherConfig: {}
- HeadCommandEnricherConfig: {}
```

### Encoder / Decoder

Usually you do not add these stages yourself. Transformer and Exporter create videos from their own `video_config`. Add these only when you need to pre-encode video or extract individual image frames. All codec settings are in [encoding](encoding.md).

---

#### VideoEncoderConfig

Encodes camera topics to MP4 and registers the result for later stages. Fields (`fps`, `gop`, `crf`, `scaling`, `resize`, `codec_config`) are in [encoding](encoding.md#rgb-video).

```yaml
- VideoEncoderConfig:
    fps: 30
    codec_config:
      codec: AV1
```

---

#### DepthVideoConfig

Encodes `compressedDepth` topics as depth video. It preserves metric distance either exactly (FFV1) or through 10-bit quantization (other codecs).

```yaml
- DepthVideoConfig:
    fps: 30
    depth_max_mm: 4092
    codec_config:
      codec: FFV1
```

| Field | Type | Default | Meaning |
|---|---|---|---|
| `depth_max_mm` | integer | `4092` | Maximum distance kept by quantization. Ignored by FFV1 |
| `fps` | integer | `30` | Depth video frame rate |
| `codec_config` | object | AV1 | Choose a depth codec from [encoding](encoding.md#depth-video) |

---

#### ImageEncoderConfig

Writes camera frames as image files instead of video (`0.jpg`, `1.jpg`, ...). There are no fields.

```yaml
- ImageEncoderConfig: {}
```

---

#### DepthImageEncoderConfig

Writes each depth frame as a 16-bit raw file (`0.bin`, ...) plus a `meta.json` with dimensions. There are no fields.

```yaml
- DepthImageEncoderConfig: {}
```

---

#### VideoDecoderConfig

Decodes registered videos back into in-memory frames. This is heavy because it holds all frames, and is usually unnecessary. Transformation reads frames directly from videos. There are no fields.

```yaml
- VideoDecoderConfig: {}
```

### Transformer

---

#### LeRobotV21TransformerConfig

Writes a LeRobot v2.1 dataset. This is usually the last stage. It overlaps `meta.json` segments with the synchronized timeline to build episodes, then writes tables, videos, and metadata.

```yaml
- LeRobotV21TransformerConfig:
    outdir: "./lerobot_yubi"
    robot_model: "./config/robot_model/yubi.yaml"
    video_config:
      fps: 30
      codec_config:
        codec: AV1
    separate_per_primitive: false
```

| Field | Type | Default | Meaning |
|---|---|---|---|
| `outdir` | path | required | One `<uuid>/` directory is created under this path |
| `robot_model` | path or inline | required | Path to a [robot model](#robot-model), or the entry list itself |
| `video_config` | object | AV1, fps 100 | How camera features are encoded. Set `fps` to the synchronizer rate. If left at the 100 fps default with a 30 fps sync pipeline, the dataset is written with misaligned videos |
| `separate_per_primitive` | true/false | `false` | `false` joins all segments into one episode, with `next.done` at boundaries. `true` writes one episode per segment |

It fails when metadata is missing, the robot model names a topic not present in the data, no segment overlaps the timeline, or a `Video` feature has no video source.

### Exporter

---

#### ParquetVideoExporterConfig

Writes the [intermediate format](intermediate-format.md). It produces the same output as `rebake-cli export`, but from the current pipeline state, such as after synchronization.

```yaml
- ParquetVideoExporterConfig:
    output_dir: "./intermediate"
    depth_config:
      codec_config:
        codec: FFV1
```

| Field | Type | Default | Meaning |
|---|---|---|---|
| `output_dir` | path | required | One `<uuid>/` directory is created under this path |
| `video_config` | object | AV1 | How camera topics are encoded |
| `depth_config` | object | none | How depth topics are encoded. If omitted, depth is not saved |

## Robot model

A robot model is a flat YAML list. Each item declares one feature.

```yaml
- type: Parquet
  topic: /joint_states
  field: /position
  feature: observation.state
  names: [shoulder, elbow, wrist]

- type: Video
  topic: /camera/image_raw/compressed
  feature: observation.image.head
  names: [height, width, channel]
```

Entries are selected by `type`.

| `type` | Keys | Meaning |
|---|---|---|
| `Parquet` | `topic`, `field`, `feature`, optional `names`, `description` | Map one topic field to a dataset column |
| `Video` | `topic`, `feature`, optional `names`, `description` | Map a camera topic to dataset video |
| `Image` | `topic`, `feature`, optional `names`, `description` | Register only the feature; it writes no files. Use `Video` for cameras |

`names` gives names to feature dimensions. If present, rebake checks the count against the real data width, which catches topic shape changes. `description` is free text and is copied into the dataset.

To find real topic and column names, [export](cli.md#export) once and inspect the Parquet files (see [guide step 2](guide.md#2-inspect-the-ros-bag-contents)).

`robot_model` can be a path or inline entries.

```yaml
robot_model: "./config/robot_model/my_robot.yaml"
```

```yaml
robot_model:
  - type: Parquet
    topic: /joint_states
    field: /position
    feature: observation.state
```

### Field paths

The `field` in a `Parquet` entry is a path selecting a value inside a message.

- It starts with `/`, and the first segment is a column name.
- Name segments select struct fields: `/wrench/force`
- Integers select list elements, including negative indices from the end: `/points/0/positions`
- `start:end` slices a list. The remaining path is applied to each selected element.

| Path | Selects |
|---|---|
| `/position` | the `position` column |
| `/points/0/positions` | the first element of `points`, then its `positions` |
| `/base_link/hand_link/transform` | a pose from `/tf_chain` |
| `/base_link/hand_link/is_fresh` | the freshness flag for that pose |

## Output layout

Transformer writes LeRobot v2.1 to `outdir/<uuid>/`.

```text
<outdir>/<uuid>/
├── data/
│   └── chunk-000/
│       ├── episode_000000.parquet
│       └── episode_000001.parquet     # multiple only when split by segment
├── videos/
│   └── chunk-000/
│       └── <feature>/                  # one folder per Video feature
│           └── episode_000000.mp4
└── meta/
    ├── info.json
    ├── episodes.jsonl
    ├── episodes_stats.jsonl
    └── tasks.jsonl
```

rebake always writes a single `chunk-000`. `info.json` has only the `train` split; it does not write val / test splits. The output format is LeRobot v2.1. Upstream LeRobot has moved on to v3, but rebake currently writes v2.1.

## Troubleshooting

`... in context` in an error means "data expected from an earlier stage is missing."

| Message fragment | Cause | Fix |
|---|---|---|
| `missing required data: dataset` | The first stage is not an Ingestor | Put an Ingestor first |
| `missing required data: rosbag_path` | No ROS bag input was passed | Check the input in `rebake-cli run <PATH> -c ...` |
| `missing required data: bundle_root` | No intermediate-format input was passed | Check the input path |
| `missing required data: tf_buffer` | TfChain ran before TfBuffer | Add `TfBufferEnricherConfig` earlier |
| `missing required data: fps` | There is no fixed-rate Synchronizer | Add ZeroOrderHold or NearestNeighbor |
| `unknown variant` | Stage name spelling is wrong | Compare with the headings on this page |
| No error, but a column or topic is missing | An Enricher did not find its target and passed data through | Check topic spelling. Use `save_contexts: true` to inspect each stage output |

Metadata errors (`failed to read meta.json`, `no segments overlap`, and similar) are in the [metadata table](metadata.md#troubleshooting). Codec errors are in the [encoding table](encoding.md#troubleshooting).
