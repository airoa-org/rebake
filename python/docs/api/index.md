# API Reference

This section provides detailed API documentation for all rebake modules.

## Module Overview

### Core

- [`rebake.core`](core.md) - Core data structures including the `Context` class
- [`rebake.analysis`](analysis.md) - Timestamp metrics and topic-level timing helpers

### Pipeline Stages

- [`rebake.ingest`](ingest.md) - ROS bag file readers
- [`rebake.synchronize`](synchronize.md) - Time synchronization algorithms
- [`rebake.enrich`](enrich.md) - Data enrichment (transforms, deltas)
- [`rebake.encode`](encode.md) - Video encoding
- [`rebake.decode`](decode.md) - Video decoding
- [`rebake.transform`](transform.md) - Output format transformers (LeRobot)
- [`rebake.pipeline`](pipeline.md) - Run a YAML/JSON-defined pipeline of stages

### Utilities

- [`rebake.exceptions`](exceptions.md) - Exception classes

## Design Pattern

All pipeline stages follow a consistent pattern:

```python
# 1. Create a config
config = SomeStageConfig(param1=value1, param2=value2)

# 2. Build the stage
stage = config.build()

# 3. Run the stage
context = stage.run(context)
```

This pattern allows for:

- **Configuration validation**: Pydantic validates config parameters
- **Separation of concerns**: Config and execution are separate
- **Composability**: Stages can be easily chained together
