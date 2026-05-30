# Changelog

All notable changes to **rebake** are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project follows [Semantic Versioning](https://semver.org/). **rebake is pre-1.0 (0.x): anything — public APIs, YAML keys, the Python surface, and the intermediate bundle schema — may change in a minor release until 1.0.**

## [Unreleased]

_Nothing yet._

## [0.1.0] - 2026-05-31

First public release.

**rebake converts robot recordings (ROS 2 `.mcap`, ROS 1 `.bag`) into a queryable, near-lossless Parquet + video intermediate format, and from that intermediate produces training-ready datasets in the [LeRobot v2.1](https://github.com/huggingface/lerobot) layout — all from one declarative pipeline, available as a Rust library, a CLI, and a Python package.**

It addresses two problems robotics ML teams hit early. **(1) Rosbags are well suited to recording but awkward for analysis** — large, slow to load, message-serialized rather than columnar, and forcing every analyst to write the same per-bag deserialization, TF reconstruction, and across-recording aggregation code. **(2) Trainer-specific dataset formats are by design selective and lossy** — they encode curator choices (which topics, which sync rate, which feature mapping) and pin data to one format version, so changing any of those means re-running the full ingest.

rebake puts a **Parquet + video intermediate format** in the middle. The intermediate keeps nearly all of the original data in standard columnar/video formats that DuckDB, Polars, PyArrow, and FFmpeg can read directly. Synchronization, enrichment, and the LeRobot v2.1 export all then run *on* the intermediate — so re-curating with a different topic set or sync rate does not require re-reading the original rosbag. The LeRobot v2.1 transformer is the first downstream "head" of the pipeline; the architecture is built so additional output targets can be added without changing the intermediate.

A pipeline is a sequence of **stages** sharing a **context**, configured in YAML or driven from code. The capabilities below are all available in this release.

### Highlights

- **A queryable Parquet + video intermediate format** for ROS recordings — small, columnar, near-lossless, and inspectable with standard tools. Useful as a research/analysis artifact in its own right, not only as a step in the pipeline.
- **A declarative stage pipeline** — synchronization, enrichment, encoding, and transformation are composed declaratively from YAML configuration. Stage parameters (sync rate, codec and encoding settings, TF frame pairs, feature mappings, …) and the stage list itself — adding, removing, or reordering stages — are adjustable in config, so re-shaping a pipeline does not require code changes.
- **LeRobot v2.1 dataset output**, the first downstream "head" of the pipeline; the architecture is designed to host additional output targets.
- **A storage-efficient archive of the recording** — most bundles are **typically 7–10× smaller than the source rosbag** (state/transform topics stored losslessly in columnar Parquet, camera/depth streams encoded as video), so the bundle doubles as a long-lived archive, not just a pipeline intermediate.

### Added

**Pipeline core**

- Stage + Context architecture; pipelines defined in YAML (stages referenced by name) or composed programmatically.
- Batch processing of many recordings with configurable parallelism; parallel runs are process-isolated so SVT-AV1's global state cannot race across concurrent jobs.

**Inputs**

- Read **ROS 2 `.mcap`** (memory-mapped) and **ROS 1 `.bag`** recordings, with native in-Rust parsing of both ROS 1 and ROS 2 message wire formats (ROS 2 uses CDR).
- Re-ingest rebake's own **Parquet + video intermediate bundle**, so processing can be resumed without touching the original recording.
- Large image / depth / point-cloud payloads are kept out of the columnar tables and routed to side tables, so Parquet stays small and pixels go to video/image files. ROS `compressed_depth_image_transport` is decoded from both its PNG and RVL forms.

**Intermediate format — Parquet + video bundle**

- The canonical on-disk representation of a recording in rebake: one Parquet file per topic for columnar/state data, encoded video files for camera streams (and depth streams when configured), and small sidecars (`_metadata.parquet`, `_topic_type_map.parquet`, `_video_registry.parquet`) that describe topic types and video routing.
- **Retains the full recording, not only the training subset.** Every ingested topic gets its own Parquet table, so the intermediate stays usable as a research/analysis artifact even after downstream curation has selected a smaller subset for a training format.
- **Storage-efficient.** Numeric topics are stored in columnar Parquet (typically far smaller than the original message-serialized rosbag), and camera streams (and depth streams when configured) are written as video. For recordings dominated by camera data — the common case — the bundle is **typically 7–10× smaller than the source rosbag** while keeping state data losslessly intact.
- **Designed for analysis without rebake-specific tooling.** The Parquet files can be queried directly with DuckDB, Polars, or PyArrow; the video files are readable with any FFmpeg-based tool.
- **Round-trippable.** `ParquetVideoIngestorConfig` reads a bundle back as a Context, so synchronization, enrichment, and downstream export can be re-run with a different configuration without re-reading the original rosbag.
- Produced by `ParquetVideoExporterConfig`, or by `rebake-cli export` (the no-YAML fast path from rosbag to bundle).

**Time synchronization**

- **Zero-order hold** and **nearest-neighbor** resampling to a fixed frame rate, and a **timestamp-merge** mode that unions all topic timestamps into one non-uniform timeline.
- Adds `synched_timestamp_ns` and an `is_fresh` flag distinguishing real samples from held/filled ones.

**Enrichment**

- **TF buffer** built from `/tf` (and `/tf_static` when present), back-filling transforms so every frame has every transform.
- **TF chain**: a forward-kinematics engine (SE(3) composition with lowest-common-ancestor lookup) that computes the transform between requested frame pairs and emits `/tf_chain`, with per-pair freshness — so each analyst does not have to rebuild TF reasoning code.
- **Delta joint position** and **delta transform** (body-frame `previous_target_frame` or `source_frame` deltas) for action labels.
- **Shift**: derive an "action = next observation" copy of a topic, leaving the source intact; time-metadata columns are never shifted.
- **UUID** stamping (`rosbag_uuid`) on every topic for warehouse-style joins across recordings.
- **HSR command synthesis** (Toyota HSR-specific): synthesize gripper and head-trajectory command topics from servo/joint states when they weren't recorded.

**Encoding**

- RGB video in **AV1 (SVT-AV1)**, **H.264 (x264)**, and **H.265 (x265)**, plus hardware acceleration via **VA-API** (AMD/Intel) and **NVENC** (NVIDIA) for H.264/H.265/AV1.
- **Depth video** via a purpose-built Q10Clip4 quantization (16-bit mm → 10-bit, packed into P010LE) for HEVC (VA-API / NVENC) and AV1 (sw / VA-API / NVENC), or **FFV1 lossless**; full color range is forced so depth values aren't clipped by the encoder.
- Hardware-codec quality defaults were measured (targeting VMAF ≥ 93), not guessed.
- Also: save frames as individual **image files**, and depth frames as raw binary.

**Decoding**

- Decode encoded video back into frames, with a sequential-access fast path that avoids re-seeking for forward reads.

**LeRobot v2.1 export — the first downstream "head"**

- Emit a complete **LeRobot v2.1** dataset (`data/`, `videos/`, and `meta/{info.json,episodes.jsonl,episodes_stats.jsonl,tasks.jsonl}`).
- Two episode modes: a single merged episode with segment boundaries marked, or one episode per labeled segment.
- A robot-model YAML maps ROS topic fields to LeRobot features using JSON-Pointer paths.
- When videos are pre-encoded in the intermediate bundle, frames are pulled lazily from the video files at export time instead of decoding everything up front.
- This is the first output target wired into the pipeline. Because synchronization, enrichment, and encoding all run on the intermediate format, additional output targets can be added without redoing that work.

**Dataset merge**

- Combine multiple LeRobot v2.1 datasets into one: deterministic, sorted discovery so the same inputs always produce the same dataset; episode renumbering; task de-duplication; video files copied (not re-encoded); and consolidated metadata.

**Interfaces**

- **Rust library** (`rebake`) — the Stage/Context API. Rust edition 2024 with a minimum supported Rust version of 1.88.
- **CLI** (`rebake-cli`) — `run` (YAML pipeline), `export` (no-config bag → Parquet + video intermediate bundle), and `merge`.
- **Python package** (`rebake`) — PyO3 bindings shipped as an `abi3` wheel compatible with Python 3.9+, with zero-copy Apache Arrow / PyArrow data exchange, plus a dataset-analysis module.

### Notes & known limitations

- **Pre-1.0:** see the SemVer note above — expect breaking changes in 0.x.
- **HSR command enrichers are hardware-specific** (hard-coded Toyota HSR topics/joint indices) and are not general-purpose.
- **Codecs require FFmpeg.** GPU encoding (VA-API/NVENC) is optional and needs the matching hardware and drivers. Builds that bundle FFmpeg with x264/x265 link GPL-licensed codecs — relevant when redistributing a binary or container image.
- **Metadata:** some stages (the LeRobot transform, the video/depth encoders, the bundle exporter, the UUID enricher) need airoa `meta.json` for the dataset UUID/segments. The ingestors enforce this by default; disable with `require_metadata: false` for ingest/inspect-only pipelines.
- **`compressedDepth` `32FC1` PNG payloads** are recognized but not decoded in this release; use the `16UC1` (PNG or RVL) form for depth ingest.
- **`PointCloud2` binary payloads** are externalized during ingest but are not currently serialized into the Parquet + video bundle in this release; the topic table is exported, but the binary payload is not.

### License

- Apache-2.0.

[Unreleased]: https://github.com/airoa-org/rebake/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/airoa-org/rebake/releases/tag/v0.1.0
