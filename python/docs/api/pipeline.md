# rebake.pipeline

Run a sequence of stages defined as JSON (the same shape as the `stage_configs`
section of a pipeline YAML in `config/pipeline/*.yaml`).

```python
from rebake.core import Context
from rebake.pipeline import PipelineConfig

config_json = '{"stage_configs": [{"ZeroOrderHoldTimeSynchronizerConfig": {"fps": 10}}]}'
pipeline = PipelineConfig(config_json).build()

context = Context()
context.set_rosbag_path("/path/to/recording.mcap")
context = pipeline.run(context)
```

## PipelineConfig

::: rebake.pipeline.PipelineConfig
    options:
      members:
        - build

## Pipeline

::: rebake.pipeline.Pipeline
    options:
      members:
        - run
