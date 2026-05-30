# Quick Start

This guide walks you through the basic workflow of using rebake to process a ROS bag file.

!!! note "Prerequisite — `meta.json`"
    The ingestors read an airoa `meta.json` from the rosbag's parent directory by
    default. To process a plain rosbag without one, pass
    `Rosbag2IngestorConfig(require_metadata=False)` — but the LeRobot transformer and
    the video encoders still require that metadata (dataset UUID, segments, labels).

## Basic Pipeline

### 1. Ingest a ROS Bag File

```python
from rebake.core import Context
from rebake.ingest import Rosbag2IngestorConfig

# For ROS 2 bags (.mcap): set the path on a Context, then run the ingestor.
context = Context()
context.set_rosbag_path("path/to/recording.mcap")
context = Rosbag2IngestorConfig().build().run(context)

# Check what topics were loaded
print(context.dataset_topics())
# ['/joint_states', '/camera/image_raw', '/tf', ...]
```

For ROS 1 bags, use `Rosbag1IngestorConfig` instead:

```python
from rebake.core import Context
from rebake.ingest import Rosbag1IngestorConfig

context = Context()
context.set_rosbag_path("path/to/recording.bag")
context = Rosbag1IngestorConfig().build().run(context)
```

### 2. Synchronize Data

ROS topics typically have different sampling rates. Use a synchronizer to align all data to a common timeline:

```python
from rebake.synchronize import ZeroOrderHoldTimeSynchronizerConfig

synchronizer = ZeroOrderHoldTimeSynchronizerConfig(fps=30).build()
context = synchronizer.run(context)

# Verify synchronization
print(f"FPS: {context.fps}")  # 30
```

Available synchronizers:

- `ZeroOrderHoldTimeSynchronizerConfig`: Holds the last known value (recommended)
- `NearestNeighborTimeSynchronizerConfig`: Uses the nearest sample

### 3. Access Data

```python
# Get data for a specific topic as PyArrow RecordBatch
batch = context.get_record_batch("/joint_states")
print(batch.schema)
print(f"Rows: {batch.num_rows}")

# Get image data
image_data = context.get_image_data()
if image_data:
    frames = image_data["/camera/image_raw"]
    print(f"Frames: {len(frames)}")
```

### 4. Enrich Data (Optional)

Add computed fields like TF transforms or joint deltas:

```python
from rebake.enrich import (
    TfBufferEnricherConfig,
    TfChainEnricherConfig,
    FramePair,
    DeltaJointPositionEnricherConfig,
)

# Build TF buffer first (required for TfChainEnricher)
context = TfBufferEnricherConfig().build().run(context)

# Compute transform chains between coordinate frames
tf_chain_enricher = TfChainEnricherConfig(
    frame_pairs=[
        FramePair(source="base_link", target="hand_palm_link"),
    ]
).build()
context = tf_chain_enricher.run(context)

# Compute joint position deltas (adds a `delta_position` column to each topic)
delta_enricher = DeltaJointPositionEnricherConfig(
    topic_names=["/joint_states"],
).build()
context = delta_enricher.run(context)
```

### 5. Encode Video (Optional)

Convert image sequences to video. The encoder writes to `{video_cache_dir}/{uuid}/{topic}.mp4`, where `uuid` comes from the bag's airoa metadata — so the context must carry `airoa_metadata` (loaded by the ingestor from `meta.json`).

```python
from rebake.encode import VideoEncoderConfig

context.set_video_cache_dir("./video_cache")
encoder = VideoEncoderConfig().build()
context = encoder.run(context)
# Videos are written to ./video_cache/<uuid>/camera/image_raw.mp4
```

### 6. Transform to LeRobot Format

Export the data to LeRobot v2.1 dataset format:

```python
from rebake.transform import LeRobotV21TransformerConfig

transformer = LeRobotV21TransformerConfig(
    outdir="./lerobot_dataset",
    robot_model="./robot.yaml",
).build()
context = transformer.run(context)
```

## Complete Example

```python
from rebake.core import Context
from rebake.ingest import Rosbag2IngestorConfig
from rebake.synchronize import ZeroOrderHoldTimeSynchronizerConfig
from rebake.enrich import (
    TfBufferEnricherConfig,
    TfChainEnricherConfig,
    FramePair,
)
from rebake.transform import LeRobotV21TransformerConfig

# Ingest
context = Context()
context.set_rosbag_path("recording.mcap")
context = Rosbag2IngestorConfig().build().run(context)

# Enrich (before sync): build the TF buffer from raw /tf timestamps
context = TfBufferEnricherConfig().build().run(context)

# Synchronize to 30 FPS
synchronizer = ZeroOrderHoldTimeSynchronizerConfig(fps=30).build()
context = synchronizer.run(context)

# Enrich (after sync): compute transform chains at synced timestamps
tf_chain_enricher = TfChainEnricherConfig(
    frame_pairs=[
        FramePair(source="base_link", target="hand_palm_link"),
    ]
).build()
context = tf_chain_enricher.run(context)

# Export to LeRobot format
transformer = LeRobotV21TransformerConfig(
    outdir="./lerobot_dataset",
    robot_model="./robot.yaml",
).build()
context = transformer.run(context)
```

## Error Handling

All rebake errors inherit from `RebakeError`:

```python
from rebake.core import Context
from rebake.ingest import Rosbag2IngestorConfig
from rebake.exceptions import RebakeError, IngestError

context = Context()
context.set_rosbag_path("nonexistent.mcap")
try:
    context = Rosbag2IngestorConfig().build().run(context)
except IngestError as e:
    print(f"Failed to ingest: {e}")
except RebakeError as e:
    print(f"Rebake error: {e}")
```
