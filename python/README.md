# rebake Python Bindings

Python bindings for the `rebake-rs` robotics data processing library. This package lets you convert ROS bag files to ML-ready datasets (like LeRobot format) using Python.

## Installation

### Requirements

- Python 3.9 or later
- Rust toolchain (for building from source)
- [uv](https://github.com/astral-sh/uv) (Python package manager)

### Install with uv

```bash
cd rebake-rs/python
uv sync
```

### Optional extras

Use extras when you need rebake's development-only dependencies:

```bash
# Python test dependencies (pytest, Pillow, rosbags)
uv sync --extra test

# Documentation dependencies
uv sync --extra docs
```

## Quick Start

Here is a simple example that converts a ROS bag file to a LeRobot dataset:

```python
import rebake

# Step 1: Load a ROS bag file
config = rebake.Rosbag1IngestorConfig()
ingestor = config.build()
context = ingestor.ingest("/path/to/your/rosbag.bag")

# Step 2: Synchronize data to a fixed frame rate
sync_config = rebake.ZeroOrderHoldTimeSynchronizerConfig(fps=30)
synchronizer = sync_config.build()
context = synchronizer.run(context)

# Step 3: Convert to LeRobot format
transformer_config = rebake.LeRobotV21TransformerConfig(
    outdir="./output",
    robot_model="/path/to/robot_model.yaml",
)
transformer = transformer_config.build()
context = transformer.run(context)

print("Done! Output saved to ./output")
```

## Documentation

Full API documentation is available locally via MkDocs.

### Building Documentation

```bash
# Install docs dependencies
uv sync --extra docs

# Build documentation (output in site/ directory)
uv run mkdocs build

# Or serve locally with hot-reload at http://127.0.0.1:8000
uv run mkdocs serve
```

### Viewing in VSCode

You can preview the built documentation directly in VSCode using the Live Preview extension:

1. Install the [Live Preview](https://marketplace.visualstudio.com/items?itemName=ms-vscode.live-server) extension
2. Build the documentation:
   ```bash
   uv run mkdocs build
   ```
3. Right-click the `site/index.html` file and select **"Show Preview"**

Alternatively, use `uv run mkdocs serve` and open http://127.0.0.1:8000 in your browser for live-reloading during development.
