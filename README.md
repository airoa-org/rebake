**English** | [日本語](README_ja.md)

<p align="center">
  <img src="assets/rebake_concept.png" alt="rebake — turn your ROS bags into a queryable dataset, then into LeRobot training data" width="820">
</p>

<p align="center">
  <b>Decode a ROS bag once. Query it like a database — and bake it into LeRobot training data, as many times as you like.</b>
</p>

<p align="center">
  <a href="LICENSE"><img alt="License: Apache-2.0" src="https://img.shields.io/badge/license-Apache--2.0-blue.svg"></a>
  <img alt="Rust 1.88+" src="https://img.shields.io/badge/rust-1.88%2B-orange.svg">
  <img alt="Python 3.9+" src="https://img.shields.io/badge/python-3.9%2B-3776ab.svg">
</p>

rebake converts ROS bags (`.bag` / `.mcap`) into a queryable **Parquet + video** dataset, and from that into [LeRobot v2.1](https://github.com/huggingface/lerobot) training data — as a CLI, a Python package, and a Rust library.

---

## Why rebake

A ROS bag is built for *recording*, not for *using*. It's large, slow to load, and serialized message by message — so every analysis or training run begins by re-parsing the ROS bag, rebuilding the transform tree, and re-aligning clocks that never matched. Training formats like LeRobot fix the loading problem but are deliberately lossy: they freeze in one set of choices — which topics, which rate, which features — so changing your mind means starting over from the ROS bag.

rebake decodes a ROS bag **once** into a queryable Parquet + video intermediate format, and runs everything else — synchronization, transform-tree math, the LeRobot export — on *that*:

- ⚡ **Query without deserialization.** Each topic becomes a columnar Parquet table, so you read just the fields you need — never the whole message — straight from DuckDB, Polars, or pandas.
- 📦 **Smaller, and archival.** Per-column compression, plus video for camera streams, makes the intermediate typically 7–10× smaller than the ROS bag — while keeping state data losslessly. A durable archive, not just a temp file.
- 🧱 **Typed and structured.** Nested ROS messages stay nested, with their types intact — so the fragile, rewrite-it-every-time parsing code disappears.
- 🌍 **Part of the data ecosystem.** Parquet and video are first-class everywhere — DuckDB, Polars, pandas, Arrow, Spark, FFmpeg — so your robot data drops into the tools your team already runs, with nothing rebake-specific to install.

Because the heavy work lives in the intermediate format, re-curating with a different topic set or sample rate never touches the ROS bag again — and LeRobot v2.1 is simply the first export target the pipeline knows how to write.

## Quickstart

> [!NOTE]
> Each ROS bag needs a small `meta.json` sidecar (its dataset id, plus segment labels for the full pipeline); the shipped configs already expect it. See [docs/configuration.md](docs/configuration.md#metadata-requirement).

Build it in the dev container:

```bash
git clone --recursive https://github.com/airoa-org/rebake.git
cd rebake
docker compose -f docker/docker-compose.yml up -d --build
docker compose -f docker/docker-compose.yml exec rebake-dev bash

# inside the container
just build      # → ./target/release/rebake-cli
```

**Decode your ROS bags into a queryable intermediate format.** Point at a single `.bag`/`.mcap` or a whole directory of them; `-j` converts the ROS bags in parallel, each in its own process:

```bash
rebake-cli export ./bags -o ./out -j 8
```

Your opaque ROS bags are now plain Parquet and video — explore them with anything, no rebake required:

```python
import pandas as pd
pd.read_parquet("out/<id>/parquet/joint_states.parquet")   # also: polars, pyarrow
```
```bash
duckdb -c "SELECT * FROM 'out/*/parquet/joint_states.parquet' LIMIT 5"
```

**Bake LeRobot v2.1 datasets** when you're ready to train — one declarative pipeline, no per-robot code, the same parallel batch over a directory:

```bash
rebake-cli run ./bags -c config/pipeline/yubi.yaml -j 8
```
```text
lerobot_dataset/
├── meta/      info.json, episodes.jsonl, tasks.jsonl, episodes_stats.jsonl
├── data/      one Parquet file per episode
└── videos/    one video per camera, per episode
```

It's a standard LeRobot v2.1 dataset — load it with the `lerobot` library and start training. To re-curate it — a different topic set, sample rate, or feature mapping — re-run pointed at the intermediate format: rebake re-ingests that instead of re-parsing the original ROS bags.

## How it works

A pipeline is a declarative list of **stages** that share a **context** — reorder, add, or drop stages in YAML, no code changes.

```yaml
# abridged from config/pipeline/yubi.yaml
stage_configs:
  - Rosbag2IngestorConfig: {}             # read .bag / .mcap
  - TfBufferEnricherConfig: {}            # build the transform tree
  - TfChainEnricherConfig:                # compute poses between frame pairs
      frame_pairs:
        - source: quest_origin
          target: quest_hmd
  - ZeroOrderHoldTimeSynchronizerConfig:  # resample to one timeline
      fps: 30
  - LeRobotV21TransformerConfig:          # write the LeRobot dataset
      robot_model: ./config/robot_model/yubi.yaml
```

- **Synchronize** — resample topics recorded at different rates onto one timeline (zero-order-hold, nearest-neighbor, or timestamp-merge), marking each row `is_fresh` so a held value is never mistaken for a fresh measurement.
- **Enrich** — build the transform tree from `/tf` and `/tf_static`, then compute end-effector and camera poses between any two frames with forward kinematics (SE(3) composition, lowest-common-ancestor lookup), and derive action labels — the things a policy needs but the ROS bag never stored directly.
- **Encode** — write camera streams to AV1/H.264/H.265 video, and keep depth metric — lossless FFV1 or a compact 10-bit form — instead of crushing 16-bit millimeters into 8-bit RGB.

<details>
<summary><b>The full stage list</b></summary>

Ingest (ROS 1/2, or re-ingest a rebake intermediate) · Synchronize (ZOH / nearest-neighbor / timestamp-merge) · Enrich (TF buffer & chain, joint/transform deltas, action shift, uuid) · Encode (RGB & depth video, software or VA-API/NVENC) · Export (Parquet + video intermediate) · Transform (LeRobot v2.1) · Merge (combine datasets without re-encoding). Full reference: [docs/configuration.md](docs/configuration.md).
</details>

## Bring your own robot

A new robot is one YAML file that maps its ROS topics and fields (JSON Pointer) to LeRobot features:

```yaml
# robot_model.yaml
- type: Parquet
  topic: /joint_states
  field: /position
  feature: observation.state
- type: Video
  topic: /camera/color/image_raw/compressed
  feature: observation.image.head
- type: Parquet
  topic: /right_hand/command
  field: /position
  feature: action.right_hand
- type: Parquet
  topic: /left_hand/command
  field: /position
  feature: action.left_hand
```

See [config/robot_model/](config/robot_model/) for complete examples.

## Python

The same stages from Python — built from source with [maturin](https://github.com/PyO3/maturin), with zero-copy Arrow/PyArrow exchange. Every stage is `Config().build().run(context)`, all the way to a LeRobot dataset:

```python
from rebake.core import Context
from rebake.encode import VideoEncoderConfig
from rebake.enrich import FramePair, TfBufferEnricherConfig, TfChainEnricherConfig
from rebake.ingest import Rosbag2IngestorConfig
from rebake.synchronize import ZeroOrderHoldTimeSynchronizerConfig
from rebake.transform import LeRobotV21TransformerConfig

context = Context()
context.set_rosbag_path("recording.mcap")
context = Rosbag2IngestorConfig().build().run(context)
context = TfBufferEnricherConfig().build().run(context)
context = TfChainEnricherConfig(
    frame_pairs=[FramePair(source="quest_origin", target="quest_hmd")],
).build().run(context)
context = ZeroOrderHoldTimeSynchronizerConfig(fps=10).build().run(context)
context = LeRobotV21TransformerConfig(
    outdir="./lerobot_dataset",
    robot_model="config/robot_model/yubi.yaml",
    video_config=VideoEncoderConfig(fps=10),
).build().run(context)
```

See [python/](python/) for the full API and examples.

## Learn more

- **[CLI](docs/cli.md)** — `run`, `export`, `merge`
- **[Configuration](docs/configuration.md)** — pipelines, robot models, encoding
- **[Hardware acceleration](docs/hardware.md)** — VA-API and NVENC
- **[Changelog](CHANGELOG.md)** — what's in this release (rebake is pre-1.0; expect changes)

## Contributing

Issues and pull requests are welcome — see [CONTRIBUTING.md](CONTRIBUTING.md) and come say hello in [Discussions](https://github.com/airoa-org/rebake/discussions).

## License

Licensed under the Apache License, Version 2.0 — see the [LICENSE](LICENSE) file for details.

Copyright © 2026 AI Robot Association.
