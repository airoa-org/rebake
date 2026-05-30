"""Pipeline configuration and execution.

Provides PipelineConfig and Pipeline for running a sequence of stages
defined by JSON configuration. The format is identical to the stage_configs
section of rebake's pipeline YAML files (config/pipeline/*.yaml).

Follows rebake's standard Config -> build() -> entity pattern:

    >>> from rebake.pipeline import PipelineConfig
    >>> from rebake.core import Context
    >>>
    >>> config_json = '{"stage_configs": [{"ZeroOrderHoldTimeSynchronizerConfig": {"fps": 10}}]}'
    >>> config = PipelineConfig(config_json)
    >>> pipeline = config.build()
    >>> context = pipeline.run(context)
"""

from .. import _internal
from ..core.context import Context

__all__ = ["Pipeline", "PipelineConfig"]


class PipelineConfig:
    """Configuration for a pipeline of stages.

    Deserializes stage definitions from a JSON string using Rust's
    serde_json + typetag::serde. Call :meth:`build` to create a
    :class:`Pipeline` that can execute the stages.

    The JSON format matches rebake's pipeline YAML format:

        {"stage_configs": [{"ConfigClassName": {params}}, ...]}

    Available stage config names (same as YAML):
        Synchronizers:
            - ZeroOrderHoldTimeSynchronizerConfig: {"fps": int}
            - NearestNeighborTimeSynchronizerConfig: {"fps": int}
        Enrichers:
            - TfChainEnricherConfig: {"frame_pairs": [{"source": str, "target": str}]}
            - DeltaJointPositionEnricherConfig: {"topic_names": [str]}
            - DeltaTransformEnricherConfig: {"topic_names": [str], "delta_reference_frame": "previous_target_frame" | "source_frame"}
            - HeadCommandEnricherConfig: {}
            - HandCommandEnricherConfig: {}

    Args:
        json_str: JSON string defining the pipeline stages.

    Raises:
        ValueError: If JSON is invalid or contains unknown stage config names.

    Example:
        >>> config = PipelineConfig('{"stage_configs": [{"ZeroOrderHoldTimeSynchronizerConfig": {"fps": 10}}]}')
        >>> pipeline = config.build()
        >>> context = pipeline.run(context)
    """

    def __init__(self, json_str: str) -> None:
        self._inner = _internal.pipeline.PyPipelineConfig(json_str)

    def build(self) -> "Pipeline":
        """Build a Pipeline from this configuration.

        Returns:
            A new Pipeline instance ready to execute stages.
        """
        return Pipeline(self)


class Pipeline:
    """An executable pipeline that runs stages sequentially.

    Created by :meth:`PipelineConfig.build`. Each call to :meth:`run`
    executes all stages in order, passing the Context through each stage.

    Args:
        config: The PipelineConfig to build from.

    Example:
        >>> config = PipelineConfig(json_str)
        >>> pipeline = config.build()
        >>> context = pipeline.run(context)
    """

    def __init__(self, config: PipelineConfig) -> None:
        self._inner = config._inner.build()

    def run(self, context: Context) -> Context:
        """Execute all stages sequentially on the given context.

        Args:
            context: Input context with dataset loaded via Context.from_tables().

        Returns:
            Modified context after all stages have run.

        Raises:
            RuntimeError: If any stage fails during execution.
        """
        self._inner.run(context._inner)
        return context
