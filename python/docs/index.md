# rebake (Python)

Python bindings for rebake.

## Installation

```bash
cd docker
docker compose up -d --build
docker compose exec rebake-dev bash

cd python
uv sync
```

See the main [README](../../README.md) for more details.

## Quick Example

```python
from rebake.core import Context
from rebake.ingest import Rosbag2IngestorConfig
from rebake.synchronize import ZeroOrderHoldTimeSynchronizerConfig
from rebake.enrich import TfBufferEnricherConfig

# Load ROS bag: set the path on a Context, then run the ingestor.
context = Context()
context.set_rosbag_path("/path/to/recording.mcap")
context = Rosbag2IngestorConfig().build().run(context)

# Build TF buffer
context = TfBufferEnricherConfig().build().run(context)

# Synchronize to 10 FPS
context = ZeroOrderHoldTimeSynchronizerConfig(fps=10).build().run(context)

# Access the data
print(context.dataset_topics())
batch = context.get_record_batch("/joint_states")
```

## Documentation

For general documentation, see:

- [Configuration Reference](../../docs/configuration.md) - YAML config details
- [CLI Usage](../../docs/cli.md) - Command-line interface

## API Reference

| Module | Description |
|--------|-------------|
| [rebake.analysis](api/analysis.md) | Topic timing metrics, segment quality metrics, and exported-Parquet helpers |
| [rebake.core](api/core.md) | Core data structures (Context) |
| [rebake.ingest](api/ingest.md) | ROS bag file readers |
| [rebake.synchronize](api/synchronize.md) | Time synchronization |
| [rebake.enrich](api/enrich.md) | Data enrichment (transforms, deltas) |
| [rebake.encode](api/encode.md) | Video encoding |
| [rebake.decode](api/decode.md) | Video decoding |
| [rebake.transform](api/transform.md) | Output format transformers |
| [rebake.pipeline](api/pipeline.md) | Run a YAML/JSON-defined pipeline of stages |
| [rebake.exceptions](api/exceptions.md) | Exception classes |
