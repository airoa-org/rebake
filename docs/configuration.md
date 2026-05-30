# Configuration Reference

rebake uses two types of YAML configuration files:

1. **Pipeline Config** - Defines the processing stages
2. **Robot Model Config** - Maps ROS topics to LeRobot features

## Contents

- [Quick Start](#quick-start)
- [Pipeline Config](#pipeline-config)
- [Stage Order](#stage-order)
- [Metadata Requirement](#metadata-requirement)
- [Stage Reference](#stage-reference)
  - [Ingest Stages](#ingest-stages)
  - [Synchronize Stages](#synchronize-stages)
  - [Enrich Stages](#enrich-stages)
  - [Encode Stages](#encode-stages)
  - [Decode Stages](#decode-stages)
  - [Transform Stages](#transform-stages)
  - [Export Stages](#export-stages)
- [Codec Configuration Details](#codec-configuration-details)
  - [Glossary](#glossary)
- [Using Enriched Data](#using-enriched-data)
- [Robot Model Config](#robot-model-config)
- [Output Structure](#output-structure)
- [Troubleshooting](#troubleshooting)

## Quick Start

### Minimal Pipeline

This is a minimal pipeline to convert a ROS 2 bag to LeRobot format:

This example is a complete pipeline config file. You can save it as-is and run it.

```yaml
work_dir: "./output"
stage_configs:
  - Rosbag2IngestorConfig: {}
  - ZeroOrderHoldTimeSynchronizerConfig:
      fps: 10
  - LeRobotV21TransformerConfig:
      outdir: "./lerobot_output"
      robot_model: "./config/robot_model/myrobot.yaml"
```

### Common Pipeline Patterns

The examples in this section are also complete pipeline config files.

**Pattern 1: Basic Pipeline (no TF transforms)**

```yaml
work_dir: "./output"
stage_configs:
  - Rosbag2IngestorConfig: {}
  - ZeroOrderHoldTimeSynchronizerConfig:
      fps: 10
  - LeRobotV21TransformerConfig:
      outdir: "./output"
      robot_model: "./config/robot_model/myrobot.yaml"
```

**Pattern 2: Pipeline with TF Transforms**

```yaml
work_dir: "./output"
stage_configs:
  # 1. Ingest
  - Rosbag2IngestorConfig: {}

  # 2. Enrich (Before Sync) - build TF buffer
  - TfBufferEnricherConfig: {}

  # 3. Synchronize
  - ZeroOrderHoldTimeSynchronizerConfig:
      fps: 10

  # 4. Enrich (After Sync) - compute transforms and deltas
  - TfChainEnricherConfig:
      frame_pairs:
        - source: base_link
          target: hand_link
  - DeltaTransformEnricherConfig:
      topic_names: ["/tf_chain"]
      delta_reference_frame: previous_target_frame
  - ShiftEnricherConfig:
      source_topic: "/joint_states"
      output_topic: "/joint_states/action"
      shift_steps: 1

  # 5. Transform
  - LeRobotV21TransformerConfig:
      outdir: "./output"
      robot_model: "./config/robot_model/myrobot.yaml"
```

---

## Pipeline Config

Location: `config/pipeline/`

### Structure

```yaml
work_dir: "./output"          # Output directory for intermediate results
save_contexts: true           # Save each stage's output (useful for debugging)
stage_configs:                # List of stages to run (in order)
  - StageConfigName:
      parameter: value
```

### Parameters

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| work_dir | string | Yes | Directory for intermediate outputs |
| save_contexts | bool | No | Save each stage's output (default: false) |
| stop_on_error | bool | No | Stop dispatching additional rosbag files after the first rosbag failure (default: true) |
| stage_configs | list | Yes | Ordered list of stage configurations |

---

## Stage Order

Stages must be in a specific order. Follow these rules:

### Basic Order

```
Ingest → Enrich (Before Sync) → Synchronize → Enrich (After Sync) → Transform
```

### Enricher Timing

Enrichers must run at the correct time relative to Synchronization:

**Before Synchronization** (to preserve data precision):

| Enricher | Reason |
|----------|--------|
| TfBufferEnricherConfig | Builds TF buffer from raw timestamps |
| HandCommandEnricherConfig | HSR only - Synthesizes from raw servo data |
| HeadCommandEnricherConfig | HSR only - Synthesizes from raw joint data |

**After Synchronization** (needs synced timestamps):

| Enricher | Reason |
|----------|--------|
| TfChainEnricherConfig | Computes transforms at synced timestamps |
| DeltaJointPositionEnricherConfig | Computes frame-to-frame delta |
| DeltaTransformEnricherConfig | Computes frame-to-frame delta |
| ShiftEnricherConfig | Creates shifted copy of a topic (e.g., for action labels) |

### Example Pipeline Order

```yaml
stage_configs:
  # 1. Ingest
  - Rosbag2IngestorConfig: {}

  # 2. Enrich (Before Sync) - build TF buffer
  - TfBufferEnricherConfig: {}

  # 3. Synchronize - resample to fixed FPS
  - ZeroOrderHoldTimeSynchronizerConfig:
      fps: 10

  # 4. Enrich (After Sync) - compute transforms and deltas
  - TfChainEnricherConfig:
      frame_pairs:
        - source: base_link
          target: hand_link
  - DeltaJointPositionEnricherConfig:
      topic_names: ["/joint_states"]
  - DeltaTransformEnricherConfig:
      topic_names: ["/tf_chain"]
      delta_reference_frame: previous_target_frame
  - ShiftEnricherConfig:
      source_topic: "/joint_states"
      output_topic: "/joint_states/action"
      shift_steps: 1

  # 5. Transform
  - LeRobotV21TransformerConfig:
      outdir: "./lerobot_output"
      robot_model: "./config/robot_model/myrobot.yaml"
```

---

## Metadata Requirement

Some stages need airoa metadata (`meta.json`, loaded by an ingestor). Stages that need it read the dataset UUID and/or segment/label info and have no opt-out. The ingestors load `meta.json` by default (`require_metadata: true`); set `require_metadata: false` to skip it — but only for pipelines that avoid the stages marked **Yes** below.

| Stage | Needs airoa metadata? | Notes |
|-------|-----------------------|-------|
| Rosbag1IngestorConfig / Rosbag2IngestorConfig | Optional (default **required**) | `require_metadata: true` by default; set `false` for a plain rosbag |
| ParquetVideoIngestorConfig | Reads it from the bundle | Restored from the Parquet+Video bundle |
| ZeroOrderHold / NearestNeighbor / TimestampMerge | No | |
| TfBufferEnricher / TfChainEnricher | No | |
| DeltaJointPositionEnricher / DeltaTransformEnricher / ShiftEnricher | No | |
| HandCommandEnricher / HeadCommandEnricher | No | |
| UuidEnricherConfig | **Yes** | Adds `rosbag_uuid`; no-op if metadata absent |
| ImageEncoderConfig / DepthImageEncoderConfig | No | Write under `output_dir` |
| VideoEncoderConfig / DepthVideoConfig | **Yes** | Need the dataset UUID for the output path |
| VideoDecoderConfig | No | |
| ParquetVideoExporterConfig | **Yes** | UUID subdir + writes bundle metadata |
| LeRobotV21TransformerConfig | **Yes** | Needs UUID, segments, labels |

---

## Stage Reference

The examples below show individual `stage_configs` entries, not complete pipeline config files. Add them under the top-level `stage_configs:` key in a full config that also includes `work_dir`.

## Ingest Stages

### Rosbag1IngestorConfig

Reads a ROS 1 bag file (.bag) and loads all topics.

**Parameters**

| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| require_metadata | bool | true | Require meta.json file. Set false for testing. |

**Example**

```yaml
# Default: requires meta.json
- Rosbag1IngestorConfig: {}

# For testing without meta.json
- Rosbag1IngestorConfig:
    require_metadata: false
```

---

### Rosbag2IngestorConfig

Reads a ROS 2 bag file (.mcap) and loads all topics.

**Parameters**

| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| require_metadata | bool | true | Require meta.json file. Set false for testing. |

**Example**

```yaml
# Default: requires meta.json
- Rosbag2IngestorConfig: {}

# For testing without meta.json
- Rosbag2IngestorConfig:
    require_metadata: false
```

---

### ParquetVideoIngestorConfig

Reads rebake intermediate format data (`parquet/` + `videos/`) and restores the dataset, metadata, and video registry into `Context`.

Use this when you want to continue processing from previously exported Parquet/MP4 data instead of ingesting a rosbag again.

**Parameters**

| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| input_dir | string | unset | Optional input directory override. In normal `rebake-cli run` usage, the positional input path is injected automatically, so this usually does not need to be set in YAML. |

**Example**

```yaml
# Typical CLI usage: input path comes from `rebake-cli run <PATH> -c ...`
- ParquetVideoIngestorConfig: {}

# Optional override for direct library use or fixed-path configs
- ParquetVideoIngestorConfig:
    input_dir: "./exported_dataset/123e4567-e89b-12d3-a456-426614174000"
```

---

## Synchronize Stages

### ZeroOrderHoldTimeSynchronizerConfig

Resamples all topics to a fixed frame rate. Uses the last known value at each timestamp.

**Parameters**

| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| fps | int | (required) | Target frame rate |

**Example**

```yaml
- ZeroOrderHoldTimeSynchronizerConfig:
    fps: 10
```

---

### NearestNeighborTimeSynchronizerConfig

Resamples all topics to a fixed frame rate. Uses the nearest value at each timestamp.

**Parameters**

| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| fps | int | (required) | Target frame rate |

**Example**

```yaml
- NearestNeighborTimeSynchronizerConfig:
    fps: 10
```

---

### TimestampMergeTimeSynchronizerConfig

Aligns all topics to a merged timeline. Does NOT resample to fixed frame rate.

**Parameters**

No parameters.

**Example**

```yaml
- TimestampMergeTimeSynchronizerConfig: {}
```

**Notes**

- Does NOT set fps (non-uniform timeline)
- Use when you want to keep all original timestamps

---

## Enrich Stages

### TfBufferEnricherConfig

Builds a TF buffer from /tf, and also incorporates /tf_static when it is available.

/tf messages arrive sparsely — not every frame is reported at every timestamp. This stage fills in the gaps so that every frame has a known transform at every timestamp. The resulting buffer is required by TfChainEnricher to compute transform chains.

**Parameters**

No parameters.

**Example**

```yaml
- TfBufferEnricherConfig: {}
```

**Notes**

- **Run before Synchronization** - Builds TF buffer from raw timestamps
- Must run before TfChainEnricherConfig

---

### TfChainEnricherConfig

Computes transforms between frame pairs. Creates /tf_chain topic.

**Parameters**

| Parameter | Type | Description |
|-----------|------|-------------|
| frame_pairs | list | List of {source, target} frame pairs |

**Example**

```yaml
- TfChainEnricherConfig:
    frame_pairs:
      - source: base_link
        target: hand_link
      - source: base_link
        target: camera_link
```

**Robot Model Config Example**

```yaml
- type: Parquet
  topic: /tf_chain
  field: /base_link/hand_link/transform
  feature: observation.end_effector_pose
  names: [x, y, z, qx, qy, qz, qw]
```

**Notes**

- **Run after Synchronization** - Computes transforms at synced timestamps
- Requires TfBufferEnricherConfig to run first
- Field paths: `/tf_chain/{source}/{target}/transform` and `/tf_chain/{source}/{target}/is_fresh`

---

### DeltaJointPositionEnricherConfig

Computes joint position deltas (difference from previous frame).

**Parameters**

| Parameter | Type | Description |
|-----------|------|-------------|
| topic_names | list | Topics containing JointState messages |

**Example**

```yaml
- DeltaJointPositionEnricherConfig:
    topic_names:
      - /joint_states
```

**Robot Model Config Example**

```yaml
- type: Parquet
  topic: /joint_states
  field: /delta_position
  feature: action.delta_joint_position
  names: [joint1, joint2, joint3]
```

**Notes**

- **Run after Synchronization** - Delta computation requires uniform frame rate
- Adds `/delta_position` field to the specified topics

---

### DeltaTransformEnricherConfig

Computes transform deltas (difference from previous frame).

**Parameters**

| Parameter | Type | Description |
|-----------|------|-------------|
| topic_names | list | Topics containing transform data |
| delta_reference_frame | string | Required. Use `previous_target_frame` for body-frame action deltas, or `source_frame` for source-frame coordinate component deltas |

**Example**

```yaml
- DeltaTransformEnricherConfig:
    topic_names:
      - /tf_chain
    delta_reference_frame: previous_target_frame
```

**Robot Model Config Example**

```yaml
- type: Parquet
  topic: /tf_chain
  field: /base_link/hand_link/delta_transform
  feature: action.delta_end_effector
  names: [dx, dy, dz, dqx, dqy, dqz, dqw]
```

**Notes**

- **Run after Synchronization** - Delta computation requires uniform frame rate
- Adds `/delta_transform` field to transform structures
- Usually used with /tf_chain topic (created by TfChainEnricher)
- `previous_target_frame` computes translation as `inverse(R_previous) * (p_current - p_previous)` and keeps rotation as the existing relative quaternion delta
- `source_frame` computes translation as `p_current - p_previous` in source-frame coordinates, while rotation keeps the existing relative quaternion delta

---

### HandCommandEnricherConfig

> **HSR robot only**

Creates gripper command topic if missing. Extracts hand motor position from /hsrb/servo_states.

**Parameters**

No parameters.

**Example**

```yaml
- HandCommandEnricherConfig: {}
```

**Robot Model Config Example**

```yaml
- type: Parquet
  topic: /hsrb/gripper_controller/command
  field: /points/0/positions
  feature: action.gripper
  names: [hand_motor_joint]
```

**Notes**

- **HSR robot only** - Not applicable to other robots
- **Run before Synchronization** - Synthesizes from raw servo data
- Creates `/hsrb/gripper_controller/command` topic if missing

---

### HeadCommandEnricherConfig

> **HSR robot only**

Creates head trajectory command topic if missing. Extracts head pan/tilt from /hsrb/joint_states.

**Parameters**

No parameters.

**Example**

```yaml
- HeadCommandEnricherConfig: {}
```

**Robot Model Config Example**

```yaml
- type: Parquet
  topic: /hsrb/head_trajectory_controller/command
  field: /points/0/positions
  feature: action.head
  names: [head_pan_joint, head_tilt_joint]
```

**Notes**

- **HSR robot only** - Not applicable to other robots
- **Run before Synchronization** - Synthesizes from raw joint data
- Creates `/hsrb/head_trajectory_controller/command` topic if missing

---

### ShiftEnricherConfig

Creates a new topic by shifting column values by N steps. The source topic is preserved unchanged, so both state (original) and action (shifted) data can coexist.

This is primarily used for VLA model training where "action = future observation". Each config handles a single source-to-output pair. Use multiple configs to shift multiple topics.

**Parameters**

| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| source_topic | string | (required) | Source topic to read from. Not modified. |
| output_topic | string | (required) | New topic name for the shifted data. |
| shift_steps | int | (required) | Steps to shift. Positive = future, negative = past. |
| fill_strategy | string | "edge" | How to fill nulls: "edge" or "zero" (see below). |

**Fill Strategies**

| Strategy | Behavior |
|----------|----------|
| `edge` (default) | Forward fill then backward fill. Works for all types. |
| `zero` | Fill with 0 for numeric types. Falls back to `edge` for non-numeric types (string, list, struct, etc.). |

**Example**

```yaml
# Shift joint states 1 step into the future → action labels
- ShiftEnricherConfig:
    source_topic: "/joint_states"
    output_topic: "/joint_states/action"
    shift_steps: 1

# Zero-fill instead of edge-fill
- ShiftEnricherConfig:
    source_topic: "/joint_states"
    output_topic: "/joint_states/action"
    shift_steps: 1
    fill_strategy: zero
```

**Robot Model Config Example**

```yaml
# Observation: current joint positions (from source topic)
- type: Parquet
  topic: /joint_states
  field: /position
  feature: observation.joint_position
  names: [joint1, joint2, joint3]

# Action: next step's joint positions (from shifted topic)
- type: Parquet
  topic: /joint_states/action
  field: /position
  feature: action.joint_position
  names: [joint1, joint2, joint3]
```

**Notes**

- **Run after Synchronization** - Shift is applied to synced data columns
- Time metadata columns (`synched_timestamp_ns`, `timestamp_ns`, `is_fresh`) are not shifted
- `shift_steps=1` means each row gets the value from the next row (future). This is the typical setting for VLA action labels.

---

### UuidEnricherConfig

Adds a `rosbag_uuid` column (from the airoa metadata UUID) to every topic. Requires airoa metadata; no-op if it is absent.

**Parameters**

No parameters.

**Example**

```yaml
- UuidEnricherConfig: {}
```

---

## Encode Stages

### VideoEncoderConfig

Encodes images from image topics into video files (MP4).

> Unfamiliar with `gop`, `crf`, `qp`, or other encoding terms? See the [Glossary](#glossary) under "Codec Configuration Details" for short definitions and pointers to deeper references.

**Parameters**

| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| fps | int | 100 | Frame rate |
| gop | int | 20 | Keyframe interval |
| crf | string | "34" | Quality (lower = better) |
| scaling | string | "Bicubic" | Scaling algorithm |
| resize | object | (none) | Optional exact output size `{width, height}` in px (even, > 0); stretches frames without preserving aspect ratio |
| codec_config | object | AV1 | Codec settings (see below) |

**Example (AV1 - Default)**

```yaml
- VideoEncoderConfig:
    fps: 10
    gop: 2
    crf: "30"
```

**Example (H.264 - Fast decode)**

```yaml
- VideoEncoderConfig:
    fps: 10
    gop: 2
    crf: "23"
    codec_config:
      codec: "H264"
      preset: Fast
```

**Example (H.265 - Good compression)**

```yaml
- VideoEncoderConfig:
    fps: 10
    gop: 2
    crf: "28"
    codec_config:
      codec: "H265"
      preset: Medium
```

**Example (AV1 VA-API - Hardware accelerated)**

```yaml
- VideoEncoderConfig:
    fps: 30
    gop: 100
    codec_config:
      codec: "AV1_VAAPI"
      qp: 124
```

**Codec Selection Guide**

| Use Case | Codec | Quality Param |
|----------|-------|---------------|
| Storage/Archive | AV1 | crf 25-35 |
| Training (GPU decode) | H.264 | crf 18-28 |
| Balance | H.265 | crf 22-32 |
| Fast encoding (HW) | AV1_VAAPI | qp 100-150 |

See [Codec Configuration Details](#codec-configuration-details) for full parameters.

---

### ImageEncoderConfig

Extracts images from image topics and saves them as JPEG files.

**Parameters**

No parameters.

**Example**

```yaml
- ImageEncoderConfig: {}
```

**Notes**

- Use instead of VideoEncoderConfig when you need individual images

---

### DepthImageEncoderConfig

Decodes depth frames and saves them as raw binary files under `output_dir`.

**Parameters**

No parameters.

**Example**

```yaml
- DepthImageEncoderConfig: {}
```

---

### DepthVideoConfig

Encodes 16-bit depth frames into video. Lossy codecs quantize via Q10Clip4 (16-bit → 10-bit, P010LE); FFV1 stores lossless `gray16le`. Requires airoa metadata (for the UUID-based output path).

**Parameters**

| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| depth_max_mm | int | 4092 | Max depth (mm) for Q10Clip4; ignored by FFV1 |
| fps | int | 30 | Frame rate |
| codec_config | object | AV1 (crf 4, preset 4) | Depth codec; `codec:` selects it (`AV1`, `H265_VAAPI`, `AV1_VAAPI`, `H265_NVENC`, `AV1_NVENC`, `FFV1`) |

**Example**

```yaml
- DepthVideoConfig:
    depth_max_mm: 4092
    fps: 30
    codec_config:
      codec: "FFV1"
```

---

## Decode Stages

### VideoDecoderConfig

Decodes the videos registered in the Context (e.g. from a Parquet+Video bundle) back into in-memory image frames.

**Parameters**

No parameters.

**Example**

```yaml
- VideoDecoderConfig: {}
```

---

## Transform Stages

### LeRobotV21TransformerConfig

Converts pipeline data to LeRobot v2.1 format.

**Parameters**

| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| outdir | string | (required) | Output directory |
| robot_model | string or list | (required) | Path to robot model config, or inline robot model entries |
| video_config | object | AV1 | Video encoding settings |
| separate_per_primitive | bool | false | Episode mode (see below) |

**Episode Modes**

- `false` (default): All segments are combined into a single episode. `next.done` is set to `true` at each segment boundary.
- `true`: Each segment becomes a separate episode with independent frame indices and video files.

**Example (default: single episode)**

```yaml
- LeRobotV21TransformerConfig:
    outdir: "./lerobot_output"
    robot_model: "./config/robot_model/hsr2.yaml"
```

**Example (separate episodes per segment)**

```yaml
- LeRobotV21TransformerConfig:
    outdir: "./lerobot_output"
    robot_model: "./config/robot_model/hsr2.yaml"
    separate_per_primitive: true
```

**Example (with custom video settings)**

```yaml
- LeRobotV21TransformerConfig:
    outdir: "./lerobot_output"
    robot_model: "./config/robot_model/hsr2.yaml"
    video_config:
      fps: 10
      gop: 2
      crf: "30"
```

**Notes**

- Must run after a Synchronizer (needs synced timestamps)
- Usually the last stage in the pipeline
- Emits the **LeRobot v2.1** layout (`meta/info.json` → `codebase_version: "v2.1"`). Upstream LeRobot has since moved to v3.0; rebake currently emits v2.1.

---

## Export Stages

### ParquetVideoExporterConfig

Exports the dataset to the intermediate Parquet + Video bundle format (`{output_dir}/{uuid}/parquet/` + `videos/`), which can be re-ingested with `ParquetVideoIngestorConfig`. Requires airoa metadata.

**Parameters**

| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| output_dir | string | (required) | Root output dir; a `{uuid}` subdir is created |
| video_config | object | unset (AV1 default) | Optional RGB `VideoEncoderConfig` |
| depth_config | object | unset | Optional `DepthVideoConfig` for depth topics |

**Example**

```yaml
- ParquetVideoExporterConfig:
    output_dir: "./export"
```

---

## Codec Configuration Details

> New to video encoding terms like `gop`, `crf`, `qp`, `b_frames`, or `preset`? Skip ahead to the [Glossary](#glossary) at the end of this section for plain-language definitions.

### AV1 (SVT-AV1) - Best compression, recommended for storage

| Parameter | Type | Range | Default | Description |
|-----------|------|-------|---------|-------------|
| codec | string | - | - | Must be `"AV1"` (aliases: `"av1"`, `"Av1"`) |
| lp | int | 0-6 | 0 (auto) | Level of parallelism |
| pin | int | 0-N | 0 | CPU pinning (0=disabled) |
| preset | int | 0-13 | 10 | Quality preset (lower=better quality/slower) |
| film-grain | int | 0-50 | - | Film grain synthesis level |
| film-grain-denoise | bool | - | - | Apply denoising when film-grain enabled |
| lookahead | int | -1 to 120 | - | Frames to look ahead (-1=auto) |
| fast-decode | int | 0-2 | - | Fast decode optimization level |

```yaml
- VideoEncoderConfig:
    fps: 10
    gop: 2
    crf: "30"
    codec_config:
      codec: "AV1"
      lp: 6
      preset: 6
      film-grain: 8        # Optional: restore grain (8 for live-action, 4-6 for animation)
      lookahead: 60        # Optional: improve quality at cost of latency
```

### H.264 (libx264) - Fastest decode, best compatibility

| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| codec | string | - | Must be `"H264"` (aliases: `"h264"`, `"H.264"`) |
| threads | int | null (auto) | Thread count |
| preset | string | Medium | Ultrafast/Superfast/Veryfast/Faster/Fast/Medium/Slow/Slower/Veryslow |
| tune | list | [] | Tuning options (see below) |

**Tune Options** (can combine, but only one PSY tuning allowed):
- **PSY tunings** (mutually exclusive): `Film`, `Animation`, `Grain`, `StillImage`, `Psnr`, `Ssim`
- **Non-PSY tunings** (can combine with one PSY): `FastDecode`, `ZeroLatency`

```yaml
- VideoEncoderConfig:
    fps: 10
    gop: 2
    crf: "23"              # H.264 typical range: 18-28
    codec_config:
      codec: "H264"
      threads: 4
      preset: Fast
      tune: [Film, FastDecode]   # One PSY + non-PSY is OK
```

### H.265 (libx265) - Good compression, medium decode speed

| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| codec | string | - | Must be `"H265"` (aliases: `"h265"`, `"H.265"`) |
| threads | int | null (auto) | Thread count (via x265 pools) |
| preset | string | Medium | Same presets as H.264 |
| tune | list | [] | Tuning options (see below) |
| frame-threads | int | null (auto) | Frame-level parallelism threads |

**Tune Options**:
- **PSY tunings** (mutually exclusive): `Psnr`, `Ssim`, `Grain`, `Animation`
- **Non-PSY tunings**: `FastDecode`, `ZeroLatency`

```yaml
- VideoEncoderConfig:
    fps: 10
    gop: 2
    crf: "28"              # H.265 typical range: 20-32
    codec_config:
      codec: "H265"
      threads: 4
      preset: Medium
      tune: [Grain, ZeroLatency]
      frame-threads: 3     # Optional: frame-level parallelism
```

---

### VA-API Hardware Codecs

VA-API (Video Acceleration API) enables hardware-accelerated encoding on AMD and Intel GPUs. These codecs provide significantly faster encoding than software codecs, with some quality trade-offs.

> **Requirements:**
> - Linux with VA-API support
> - AMD GPU (RDNA 2+ / Ryzen 6000+) or Intel GPU (Gen 8+ / Broadwell+)
> - Docker: `/dev/dri` device passthrough required

#### H.264 VA-API (h264_vaapi) - Best compatibility

| Parameter | Type | Range | Default | Description |
|-----------|------|-------|---------|-------------|
| codec | string | - | - | Must be `"H264_VAAPI"` (alias: `"h264_vaapi"`) |
| qp | int | 0-51 | 21 | Quantization parameter (lower = better quality) |
| device | string | - | `/dev/dri/renderD128` | VA-API device path |
| profile | string | - | high | `constrained_baseline`, `main`, `high`, `high10` |
| b-depth | int | 0-7 | unset | B-frame reference depth (AMD VCN 3.0+) |
| async-depth | int | 1-64 | 16 | Parallel encoding depth |

```yaml
- VideoEncoderConfig:
    fps: 30
    gop: 2
    codec_config:
      codec: "H264_VAAPI"
      qp: 29                    # Quality (0-51, lower = better)
      profile: high             # Optional: constrained_baseline/main/high
      b-depth: 2                # Optional: B-frame depth (AMD VCN 3.0+)
      async-depth: 4            # Optional: parallel encoding
```

#### H.265 VA-API (hevc_vaapi) - Better compression

| Parameter | Type | Range | Default | Description |
|-----------|------|-------|---------|-------------|
| codec | string | - | - | Must be `"H265_VAAPI"` (aliases: `"hevc_vaapi"`, `"h265_vaapi"`) |
| qp | int | 0-51 | 29 | Quantization parameter (lower = better quality) |
| device | string | - | `/dev/dri/renderD128` | VA-API device path |
| profile | string | - | auto | `main`, `main10`, `rext` |
| async-depth | int | 1-64 | unset | Parallel encoding depth (unset = encoder default) |

> **AMD VCN Limitation:** HEVC does NOT support B-frames on any AMD VCN generation.
> This is a hardware limitation, not a software issue.

```yaml
- VideoEncoderConfig:
    fps: 30
    gop: 2
    codec_config:
      codec: "H265_VAAPI"
      qp: 29                    # Quality (0-51, lower = better)
      profile: main             # Optional: main/main10
      async-depth: 4            # Optional: parallel encoding
```

#### AV1 VA-API (av1_vaapi) - Best compression (VCN 4.0+ / Intel Arc)

| Parameter | Type | Range | Default | Description |
|-----------|------|-------|---------|-------------|
| codec | string | - | - | Must be `"AV1_VAAPI"` (alias: `"av1_vaapi"`) |
| qp | int | 0-255 | 110 | Quantization parameter (lower = better quality) |
| device | string | - | `/dev/dri/renderD128` | VA-API device path |
| profile | string | - | auto | `main`, `high`, `professional` |
| b-depth | int | 0-7 | unset | B-frame reference depth |
| async-depth | int | 1-64 | unset | Parallel encoding depth |

> **Hardware Requirements:**
> - AMD: RDNA 3 (RX 7000 / Ryzen 7000) with VCN 4.0+
> - Intel: Arc GPUs (DG2+)

```yaml
- VideoEncoderConfig:
    fps: 30
    gop: 100
    codec_config:
      codec: "AV1_VAAPI"
      qp: 124                   # Quality (0-255, lower = better)
      profile: main             # Optional: main/high/professional
      b-depth: 2                # Optional: B-frame depth
      async-depth: 4            # Optional: parallel encoding
```

---

### NVIDIA NVENC Hardware Codecs

NVENC enables hardware-accelerated encoding on NVIDIA GPUs. Like the VA-API codecs, the NVENC variants are invoked through the FFmpeg CLI subprocess, so the FFmpeg binary in your environment must include the matching `*_nvenc` encoder. The NVENC Docker image (`rebake:nvenc`) builds an FFmpeg with `h264_nvenc`, `hevc_nvenc`, and `av1_nvenc` enabled.

> **Requirements:**
> - NVIDIA driver compatible with NVENC
> - H.264 and H.265 NVENC are supported on most NVIDIA GPUs from Maxwell onwards (GTX 900-series and later)
> - AV1 NVENC additionally requires Ada Lovelace or newer (RTX 4000-series and later)
> - Container/runtime with NVIDIA GPU access (e.g. `nvidia-container-toolkit`). The provided Docker setup uses NVIDIA CDI devices.
> - See [README - NVIDIA NVENC hardware encoding](../README.md#nvidia-nvenc-hardware-encoding) for the Docker setup

#### Common Parameters

The three NVENC variants share these parameters. Codec-specific ranges and defaults are listed in the per-codec subsections below.

| Parameter | Type | Description |
|-----------|------|-------------|
| qp | int | Quantization parameter (lower = better quality). Range and default depend on the codec |
| gpu | int | NVIDIA GPU index passed to FFmpeg's `-gpu`. Omit for the default device |
| preset | string | Speed/quality preset, `P1` (fastest) to `P7` (slowest, best compression) |
| tune | string | Encoder tune: `Hq`, `Ll`, `Ull`. Omit for FFmpeg/NVENC defaults |
| profile | string | Encoder profile (codec-specific). Omit for the default |
| b_frames | int | Number of B-frames (0-7). Default: `0` for H.265/AV1 NVENC, `1` for H.264 NVENC |
| rc_lookahead | int | Frames to look ahead for rate control (0-120). Optional |

> **B-frames default to 0 for H.265 and AV1 NVENC**, which keeps frame-indexed packaging predictable and avoids interactions between FFmpeg/NVENC's hidden defaults and short GOP values. **H.264 NVENC defaults to 1** (its measured VMAF≥93 profile). When you set `b_frames > 0`, rebake automatically adds `-b_ref_mode middle`, and `gop` must be greater than `b_frames + 1`. `b_ref_mode` is intentionally not exposed as a public setting.

#### H.264 NVENC (h264_nvenc) - Best compatibility

| Parameter | Type | Range | Default | Description |
|-----------|------|-------|---------|-------------|
| codec | string | - | - | Must be `"H264_NVENC"` (alias: `"h264_nvenc"`) |
| qp | int | 0-51 | 26 | Quantization parameter |
| preset | string | P1-P7 | P5 | Speed/quality preset |
| profile | string | - | high | `baseline`, `main`, `high` |
| tune | string | - | Hq | `Hq`, `Ll`, `Ull` |
| b_frames | int | 0-7 | 1 | Number of B-frames |
| rc_lookahead | int | 0-120 | 32 | Rate-control lookahead |

```yaml
- VideoEncoderConfig:
    fps: 100
    gop: 20
    codec_config:
      codec: "H264_NVENC"
      qp: 26                  # Quality (0-51, lower = better)
      preset: P5              # P1 fastest ... P7 best compression
      tune: Hq                # Optional: Hq/Ll/Ull
```

#### H.265 NVENC (hevc_nvenc) - Better compression

| Parameter | Type | Range | Default | Description |
|-----------|------|-------|---------|-------------|
| codec | string | - | - | Must be `"H265_NVENC"` (aliases: `"hevc_nvenc"`, `"h265_nvenc"`) |
| qp | int | 0-51 | 25 | Quantization parameter |
| preset | string | P1-P7 | P4 | Speed/quality preset |
| profile | string | - | auto | `main`, `main10`, `rext` |
| tune | string | - | auto | `Hq`, `Ll`, `Ull` |
| b_frames | int | 0-7 | 0 | Number of B-frames |
| rc_lookahead | int | 0-120 | auto | Rate-control lookahead |

```yaml
- VideoEncoderConfig:
    fps: 100
    gop: 100
    codec_config:
      codec: "H265_NVENC"
      qp: 25
      preset: P4
      tune: Hq
```

#### AV1 NVENC (av1_nvenc) - Best compression (Ada Lovelace+)

| Parameter | Type | Range | Default | Description |
|-----------|------|-------|---------|-------------|
| codec | string | - | - | Must be `"AV1_NVENC"` (alias: `"av1_nvenc"`) |
| qp | int | 0-255 | 130 | Quantization parameter |
| preset | string | P1-P7 | P7 | Speed/quality preset |
| profile | string | - | auto | `main` |
| tune | string | - | auto | `Hq`, `Ll`, `Ull` |
| b_frames | int | 0-7 | 0 | Number of B-frames |
| rc_lookahead | int | 0-120 | auto | Rate-control lookahead |

> **Default tuning:** `qp: 130` and `preset: P7` were measured on RTX 5090 with UMI-style RGB recordings to keep VMAF >= 93 while reducing file size versus lower QP values.

```yaml
- VideoEncoderConfig:
    fps: 100
    gop: 20
    codec_config:
      codec: "AV1_NVENC"
      qp: 130
      preset: P7
```

#### Depth NVENC

`DepthCodecConfig` supports `H265_NVENC` and `AV1_NVENC` for depth video. The depth pipeline applies Q10Clip4 quantization (16-bit -> 10-bit P010LE) and `-color_range pc` automatically, just like the depth VA-API path.

| Codec | QP range | Default QP | Default preset |
|-------|----------|-----------:|----------------|
| `H265_NVENC` (depth) | 0-51 | 10 | P4 |
| `AV1_NVENC` (depth) | 0-255 | 20 | P4 |

```yaml
- DepthVideoConfig:
    fps: 30
    depth_max_mm: 4092
    codec_config:
      codec: "AV1_NVENC"
      qp: 20
      preset: P4
```

---

### Glossary

Short definitions of the encoding terms used throughout this section. If a parameter feels opaque — `gop`, `qp`, `b_frames`, and the like — start here. Follow the linked references for deeper background. Terms are listed alphabetically.

#### color_range

Two ways of mapping pixel values to display levels:

- `tv` (limited / studio range) — pixel values use only the inner band (for example 64-940 for 10-bit). This is the broadcast default.
- `pc` (full range) — pixel values use the full range (for example 0-1023 for 10-bit).

rebake's depth pipeline always uses `pc` so that quantized depth values are not clipped or rescaled by the encoder.

#### CRF (Constant Rate Factor)

A quality target used by software encoders. The encoder picks an internal QP per frame so that the perceived quality stays near the target. Lower values mean higher quality and larger files. Valid ranges depend on the codec:

- SVT-AV1: 0-63
- x264: 0-51
- x265: 0-51

CRF only applies to software encoders. The hardware encoders in rebake (VA-API, NVENC) use QP directly instead.

#### GOP (Group of Pictures)

The number of frames between two keyframes (I-frames). A short GOP makes seeking faster and recovers from data loss within a few frames, but raises the file size. A long GOP gives better compression at the cost of slower seeking. The right value depends on the codec and the use case; rebake picks a per-codec default that suits typical robot-camera workloads. See [Wikipedia: Group of pictures](https://en.wikipedia.org/wiki/Group_of_pictures).

#### I-frame, P-frame, B-frame

The three kinds of compressed frames used by modern video codecs:

- **I-frame** (intra-coded) — encoded on its own, like a single image. Decodable without any other frame, and the largest of the three.
- **P-frame** (predicted) — predicted from earlier frames. Smaller than an I-frame.
- **B-frame** (bidirectionally predicted) — predicted from both past and future frames. The smallest of the three, but it reorders the stream: the order of frames in the file no longer matches the source order, which complicates per-frame lookup by index. rebake disables B-frames by default for H.265/AV1 NVENC (`b_frames: 0`; H.264 NVENC defaults to `1`); increase it only when frame reordering is acceptable for your use case.

See [Wikipedia: Video compression picture types](https://en.wikipedia.org/wiki/Video_compression_picture_types).

#### Pixel formats

The byte layout of pixel data passed to FFmpeg. rebake uses three:

- `yuv420p` — 8-bit YUV with 4:2:0 chroma subsampling. The standard format for software-encoded H.264/H.265 RGB videos.
- `p010le` — 10-bit YUV 4:2:0, little-endian. Used for hardware-encoded depth (Q10Clip4 quantized) and high-bit-depth content.
- `gray16le` — 16-bit grayscale, little-endian. Used by FFV1 lossless depth so that the original 16-bit depth values stay intact.

#### preset

A bundled speed-vs-quality trade-off. Slower presets explore more encoding options and produce smaller files at the same quality target. The names differ by codec family:

- SVT-AV1: `0` (slowest, best) - `13` (fastest)
- x264 / x265: `ultrafast`, `superfast`, ..., `slower`, `veryslow`
- NVENC: `P1` (fastest) - `P7` (slowest, best compression)

There is no universal "right" preset. Pick a slower one for archive output, a faster one when encoding throughput matters more than the last few percent of size.

#### profile

A subset of the codec specification that the encoder targets. The profile controls which features are used and which decoders can play the result. Examples:

- H.264: `baseline`, `main`, `high`
- H.265: `main`, `main10`, `rext`
- AV1: `main`

Leave the profile unset unless you have a specific decoder constraint (for example, a mobile player that only supports H.264 `main`).

#### Q10Clip4

rebake's quantization scheme for depth video. It maps 16-bit millimeter depth values into a 10-bit range and clips anything beyond `depth_max_mm` to zero (treated as invalid). Q10Clip4 is what makes depth values fit into the 10-bit `p010le` format expected by HEVC and AV1 hardware encoders. FFV1 lossless depth skips Q10Clip4 and stores the raw `gray16le` values instead.

#### QP (Quantization Parameter)

How aggressively the encoder discards detail in each block. Lower values keep more detail and produce larger files. Valid ranges depend on the codec:

- H.264 / H.265: 0-51
- AV1 (NVENC, VA-API): 0-255

The hardware encoders in rebake use the constant-QP rate-control mode (CQP), which encodes every frame with the same QP. This gives predictable per-frame size at the cost of slightly less efficient bit allocation than CRF.

#### rc_lookahead

How many future frames the rate controller reads before encoding the current frame. Looking ahead helps the encoder spend more bits where they matter, especially around scene changes. NVENC only. Leave it unset for the FFmpeg default; raise it (up to 120) when archive quality matters more than encoding speed.

#### tune (NVENC)

Selects an optimization profile for a specific use case:

- `Hq` — high quality. The safest pick for offline encoding.
- `Ll` — low latency.
- `Ull` — ultra-low latency, used for live streaming.

rebake's NVENC defaults aim at offline encoding, so leaving `tune` unset is usually fine. Set `tune: Hq` if you want to be explicit.

#### VMAF (Video Multi-Method Assessment Fusion)

A perceptual video quality metric developed at Netflix. It scores videos from 0 to 100; higher means closer to the source. A common rule of thumb is that **VMAF >= 93** means "indistinguishable from the source for most viewers." rebake's hardware-codec QP defaults are tuned to keep VMAF above 93 on internal test datasets. See the [VMAF GitHub repository](https://github.com/Netflix/vmaf).

---

### Codec Parameter References

#### Software Encoders
- **SVT-AV1**: [SVT-AV1 Documentation](https://gitlab.com/AOMediaCodec/SVT-AV1/-/blob/master/Docs/Parameters.md)
- **x264**: [x264 FFmpeg Options](https://trac.ffmpeg.org/wiki/Encode/H.264)
- **x265**: [x265 FFmpeg Options](https://trac.ffmpeg.org/wiki/Encode/H.265)

#### VA-API Hardware Encoders
- **FFmpeg VAAPI Encoding**: [FFmpeg Hardware Encoding Guide](https://trac.ffmpeg.org/wiki/Hardware/VAAPI)
- **AMD VCN**: [AMD Video Core Next (Wikipedia)](https://en.wikipedia.org/wiki/Video_Core_Next)
- **Intel Quick Sync**: [ArchWiki Hardware Acceleration](https://wiki.archlinux.org/title/Hardware_video_acceleration)
- **VA-API Setup Guide**: [Brainiarc7's VAAPI Gist](https://gist.github.com/Brainiarc7/95c9338a737aa36d9bb2931bed379219)

#### NVIDIA NVENC Hardware Encoders
- **NVIDIA FFmpeg GPU Guide**: [Using FFmpeg with NVIDIA GPU Hardware Acceleration](https://docs.nvidia.com/video-technologies/video-codec-sdk/13.0/ffmpeg-with-nvidia-gpu/index.html)
- **NVENC SDK**: [NVIDIA Video Codec SDK 13.0](https://docs.nvidia.com/video-technologies/video-codec-sdk/13.0/index.html)
- **List the FFmpeg encoder's options locally**: `ffmpeg -hide_banner -h encoder=h264_nvenc` (also `hevc_nvenc`, `av1_nvenc`)

#### H.264/H.265 Profiles
- **H.264 Profiles Explained**: [RGB Spectrum - H.264 Profiles](https://www.rgb.com/h264-profiles)
- **H.264 Profile Comparison**: [Streamio - H.264 High, Main, or Baseline](https://www.streamio.com/support/h-264-high-main-or-baseline/)
- **HEVC Profiles**: [HEVC Wikipedia](https://en.wikipedia.org/wiki/High_Efficiency_Video_Coding)
- **HEVC Main10 and HDR**: [Intel - Enable 10-Bit HEVC](https://www.intel.com/content/www/us/en/developer/articles/technical/enable-10bpp.html)

#### B-frame and Encoding Quality
- **B-frame Recommendations**: [Xilinx Video SDK - Tuning Quality](https://xilinx.github.io/video-sdk/v2.0/tuning_encoding_quality.html)
- **AMD B-frame Support**: [Intel Media Driver - B frame numbers](https://github.com/intel/media-driver/issues/766)

---

## Using Enriched Data

Enrichers add new topics or fields to the dataset. To use these in LeRobot output, you must add entries to your Robot Model Config.

### Enricher → Robot Model Config Reference

| Enricher | Creates | Robot Model Config Example |
|----------|---------|---------------------------|
| TfChainEnricher | `/tf_chain` topic | See [TF Transform Example](#tf-transform-example) |
| DeltaJointPositionEnricher | `delta_position` field | See [Delta Position Example](#delta-position-example) |
| DeltaTransformEnricher | `delta_transform` field | See [Delta Transform Example](#delta-transform-example) |
| ShiftEnricher | New topic (e.g., `/joint_states/action`) | See [Shift Example](#shift-example) |
| HandCommandEnricher | `/hsrb/gripper_controller/command` topic | See [HSR Command Example](#hsr-command-example) |
| HeadCommandEnricher | `/hsrb/head_trajectory_controller/command` topic | See [HSR Command Example](#hsr-command-example) |

### TF Transform Example

TfChainEnricher creates the `/tf_chain` topic with transform data.

**Pipeline Config:**

```yaml
- TfChainEnricherConfig:
    frame_pairs:
      - source: base_link
        target: hand_link
```

**Robot Model Config:**

```yaml
# Use absolute transform (position + rotation)
- type: Parquet
  topic: /tf_chain
  field: /base_link/hand_link/transform
  feature: observation.end_effector_pose
  names: [x, y, z, qx, qy, qz, qw]

# Use pair freshness (true if any edge on the chain updated at this timestamp)
- type: Parquet
  topic: /tf_chain
  field: /base_link/hand_link/is_fresh
  feature: observation.end_effector_pose.is_fresh
```

**Field Path Structure:**

```
/tf_chain
└── /{source_frame}
    └── /{target_frame}
        ├── /transform
        │   ├── /translation  → x, y, z
        │   └── /rotation     → qx, qy, qz, qw (quaternion)
        └── /is_fresh         → bool
```

### Delta Position Example

DeltaJointPositionEnricher adds the `delta_position` field to existing topics.

**Pipeline Config:**

```yaml
- DeltaJointPositionEnricherConfig:
    topic_names:
      - /joint_states
```

**Robot Model Config:**

```yaml
# Original joint positions (observation)
- type: Parquet
  topic: /joint_states
  field: /position
  feature: observation.joint_position
  names: [joint1, joint2, joint3]

# Delta joint positions (action)
- type: Parquet
  topic: /joint_states
  field: /delta_position
  feature: action.delta_joint_position
  names: [joint1, joint2, joint3]
```

### Delta Transform Example

DeltaTransformEnricher adds the `delta_transform` field to transform topics.

**Pipeline Config:**

```yaml
- TfChainEnricherConfig:
    frame_pairs:
      - source: base_link
        target: hand_link

- DeltaTransformEnricherConfig:
    topic_names:
      - /tf_chain
    delta_reference_frame: previous_target_frame
```

**Robot Model Config:**

```yaml
# Absolute transform (observation)
- type: Parquet
  topic: /tf_chain
  field: /base_link/hand_link/transform
  feature: observation.end_effector_pose
  names: [x, y, z, qx, qy, qz, qw]

# Delta transform (action)
- type: Parquet
  topic: /tf_chain
  field: /base_link/hand_link/delta_transform
  feature: action.delta_end_effector
  names: [dx, dy, dz, dqx, dqy, dqz, dqw]
```

### Shift Example

ShiftEnricher creates a new topic with shifted column values. The source topic is unchanged, so you can map both to LeRobot features.

**Pipeline Config:**

```yaml
- ShiftEnricherConfig:
    source_topic: "/joint_states"
    output_topic: "/joint_states/action"
    shift_steps: 1
```

**Robot Model Config:**

```yaml
# Observation: current joint positions (source topic, unchanged)
- type: Parquet
  topic: /joint_states
  field: /position
  feature: observation.joint_position
  names: [joint1, joint2, joint3]

# Action: next step's joint positions (shifted topic)
- type: Parquet
  topic: /joint_states/action
  field: /position
  feature: action.joint_position
  names: [joint1, joint2, joint3]
```

### HSR Command Example

HandCommandEnricher and HeadCommandEnricher create command topics for HSR robots.

**Pipeline Config:**

```yaml
- HandCommandEnricherConfig: {}
- HeadCommandEnricherConfig: {}
```

**Robot Model Config:**

```yaml
# Gripper command
- type: Parquet
  topic: /hsrb/gripper_controller/command
  field: /points/0/positions
  feature: action.gripper
  names: [hand_motor_joint]

# Head command
- type: Parquet
  topic: /hsrb/head_trajectory_controller/command
  field: /points/0/positions
  feature: action.head
  names: [head_pan_joint, head_tilt_joint]
```

### Complete Pipeline Example

Here is a complete example using TF transforms and delta computations:

**Pipeline Config (pipeline.yaml):**

```yaml
work_dir: "./output"
stage_configs:
  # Ingest
  - Rosbag2IngestorConfig: {}

  # Enrich (Before Sync) - build TF buffer
  - TfBufferEnricherConfig: {}

  # Synchronize
  - ZeroOrderHoldTimeSynchronizerConfig:
      fps: 10

  # Enrich (After Sync) - compute transforms and deltas
  - TfChainEnricherConfig:
      frame_pairs:
        - source: base_link
          target: hand_link
  - DeltaJointPositionEnricherConfig:
      topic_names: ["/joint_states"]
  - DeltaTransformEnricherConfig:
      topic_names: ["/tf_chain"]
      delta_reference_frame: previous_target_frame
  - ShiftEnricherConfig:
      source_topic: "/joint_states"
      output_topic: "/joint_states/action"
      shift_steps: 1
  - ShiftEnricherConfig:
      source_topic: "/tf_chain"
      output_topic: "/tf_chain/action"
      shift_steps: 1

  # Transform
  - LeRobotV21TransformerConfig:
      outdir: "./lerobot_output"
      robot_model: "./config/robot_model/myrobot.yaml"
```

**Robot Model Config (myrobot.yaml):**

```yaml
# Observation: Joint states
- type: Parquet
  topic: /joint_states
  field: /position
  feature: observation.joint_position
  names: [joint1, joint2, joint3]

# Observation: End effector pose
- type: Parquet
  topic: /tf_chain
  field: /base_link/hand_link/transform
  feature: observation.end_effector_pose
  names: [x, y, z, qx, qy, qz, qw]

# Observation: Camera
- type: Video
  topic: /camera/image_raw/compressed
  feature: observation.image.camera
  names: [height, width, channel]

# Action: Next step's joint positions (from ShiftEnricher)
- type: Parquet
  topic: /joint_states/action
  field: /position
  feature: action.joint_position
  names: [joint1, joint2, joint3]

# Action: Next step's end effector pose (from ShiftEnricher)
- type: Parquet
  topic: /tf_chain/action
  field: /base_link/hand_link/transform
  feature: action.end_effector_pose
  names: [x, y, z, qx, qy, qz, qw]

# Action: Delta joint position
- type: Parquet
  topic: /joint_states
  field: /delta_position
  feature: action.delta_joint_position
  names: [joint1, joint2, joint3]

# Action: Delta end effector
- type: Parquet
  topic: /tf_chain
  field: /base_link/hand_link/delta_transform
  feature: action.delta_end_effector
  names: [dx, dy, dz, dqx, dqy, dqz, dqw]
```

---

## Robot Model Config

Location: `config/robot_model/`

This file maps ROS topics to LeRobot features. It is used by `LeRobotV21TransformerConfig`.

The examples in this section are standalone robot model config files, not `stage_configs` snippets.

### Entry Types

#### Parquet Entry

Maps a field from a ROS topic to a parquet column.

```yaml
- type: Parquet
  topic: /joint_states           # ROS topic name
  field: /position               # JSON Pointer to the field
  feature: observation.state     # LeRobot feature name
  names:                         # Optional: names for each dimension
    - joint1
    - joint2
  description: "Joint positions" # Optional: description
```

#### Video Entry

Maps an image topic to a video feature.

```yaml
- type: Video
  topic: /camera/image_raw/compressed
  feature: observation.image.camera
  names: [height, width, channel]
```

#### Image Entry

Maps an image topic to an image feature (individual frames rather than a video stream). Prefer `Video` for normal camera streams.

```yaml
- type: Image
  topic: /camera/image_raw/compressed
  feature: observation.image.camera
  names: [height, width, channel]
```

### Field Path Syntax

The `field` parameter uses JSON Pointer syntax (RFC 6901):

| Path | Meaning |
|------|---------|
| `/position` | Top-level field named "position" |
| `/points/0/positions` | First element of "points" array, then "positions" field |
| `/linear/x` | Nested field: linear.x |
| `/wrench/force` | Nested field: wrench.force |

### Example

```yaml
# config/robot_model/example.yaml

# Observation: Joint states
- type: Parquet
  topic: /joint_states
  field: /position
  feature: observation.state
  names:
    - joint1
    - joint2
    - joint3

# Observation: Camera image
- type: Video
  topic: /camera/image_raw/compressed
  feature: observation.image.camera
  names: [height, width, channel]

# Action: Arm trajectory
- type: Parquet
  topic: /arm_trajectory_controller/command
  field: /points/0/positions
  feature: action.arm
  names:
    - shoulder_joint
    - elbow_joint
    - wrist_joint

# Action: Base velocity
- type: Parquet
  topic: /cmd_vel
  field: /linear
  feature: action.twist_linear
  names: [linear_x, linear_y, linear_z]
```

---

## Output Structure

`work_dir` is primarily for intermediate/debugging output. The exact layout is an implementation detail and may change in future versions.

Current layout when `save_contexts: true`:

```
./orchestrator_work/
└── {rosbag_parent_dir_name}/
    ├── 0_rosbag2_ingestor/
    │   ├── joint_states.parquet
    │   ├── tf.parquet
    │   └── ...
    ├── 1_tf_buffer_enricher/
    │   └── ...
    ├── 2_zero_order_hold_time_synchronizer/
    │   └── ...
    └── ...
```

`rebake-cli run` uses a per-input directory under `work_dir`:

- For rosbag pipelines, it uses the rosbag's parent directory name
- For `ParquetVideoIngestor` pipelines, it uses the intermediate format dataset directory name

LeRobot output is saved under `{outdir}/{uuid}/`:

```
./lerobot_output/
└── {uuid}/
    ├── data/
    │   └── chunk-000/
    │       └── episode_000000.parquet
    ├── videos/
    │   └── chunk-000/
    │       └── observation.image.head/
    │           └── episode_000000.mp4
    └── meta/
        ├── info.json
        ├── episodes.jsonl
        ├── episodes_stats.jsonl
        └── tasks.jsonl
```

---

## Troubleshooting

### Quick Reference

| Error | Cause | Solution |
|-------|-------|----------|
| `missing required data: rosbag_path` | Rosbag ingestor input path not set | Check that a rosbag path was provided as a positional argument |
| `missing required data: bundle_root` | `ParquetVideoIngestor` input directory not set | Check that an intermediate format dataset directory was provided as a positional argument |
| `missing required data: dataset` | Ingestor missing | Add Ingestor as first stage |
| `missing required data: tf_buffer` | TfBufferEnricher not run | Add TfBufferEnricherConfig before TfChainEnricher |
| `missing required data: fps` | No synchronizer ran | Add ZeroOrderHold or NearestNeighbor synchronizer |
| `I/O error: failed to read meta.json` | meta.json not found | Set require_metadata: false or add meta.json |
| `I/O error: failed to open mcap` | File not found | Check rosbag file path |
| `I/O error: failed to open parquet file: .../_topic_type_map.parquet` | Input is not an exported intermediate format dataset directory | Point `rebake-cli run` to a dataset directory, or to a parent directory containing multiple such datasets |
| `I/O error: failed to read robot model` | File not found or invalid YAML | Check `robot_model` path or inline config |
| `TF lookup failed` | Frame not found | Check frame names in frame_pairs |
| `invalid data: no segments overlap` | Segments outside timeline | Check meta.json segment times |

### Stage Order Issues

**Problem:** Error about missing data that should exist.

**Solution:** Check stage order. Some stages need others to run first:

1. Ingestor (always first)
2. TfBufferEnricher (before TfChainEnricher)
3. TfChainEnricher (before DeltaTransformEnricher)
4. Synchronizer (before LeRobotTransformer)
5. LeRobotTransformer (usually last)

### Common Pipeline Issues

**Problem:** Videos are not encoded.

**Check:**
- Is image_data in context? (Ingestor loads this)
- Does `robot_model` contain `Video` entries?

**Problem:** TF transforms are missing.

**Check:**
- Did you add TfBufferEnricherConfig before TfChainEnricherConfig?
- Does the rosbag have a /tf topic? (/tf_static is optional)
- Are the frame names correct in frame_pairs?

**Problem:** LeRobot output is empty.

**Check:**
- Did a synchronizer run? (Needed for synched_timestamp_ns)
- Does meta.json have valid segments?
- Do segment times overlap with the rosbag timeline?
