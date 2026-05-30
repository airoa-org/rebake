"""rebake: A Python library for converting ROS bag files to ML-ready datasets.

rebake provides tools to process robotics data from ROS bag files and
convert them to formats suitable for VLA training, such as LeRobot.

Submodules:
    rebake.core: Core data structures (Context)
    rebake.analysis: Topic timing analysis helpers
    rebake.ingest: ROS bag file readers
    rebake.synchronize: Time synchronization
    rebake.enrich: Data enrichment (transforms, deltas)
    rebake.encode: Video encoding
    rebake.decode: Video decoding
    rebake.transform: Output format transformers (LeRobot)
    rebake.pipeline: Pipeline configuration and execution
    rebake.exceptions: Exception classes

Quick Start (Stage API with Context):
    >>> from rebake.core import Context
    >>> from rebake.ingest import Rosbag2IngestorConfig
    >>> from rebake.synchronize import ZeroOrderHoldTimeSynchronizerConfig
    >>> from rebake.transform import LeRobotV21TransformerConfig
    >>>
    >>> # Load a ROS bag file
    >>> context = Context()
    >>> context.set_rosbag_path("data.mcap")
    >>> context = Rosbag2IngestorConfig().build().run(context)
    >>>
    >>> # Synchronize to 30 FPS
    >>> synchronizer = ZeroOrderHoldTimeSynchronizerConfig(fps=30).build()
    >>> context = synchronizer.run(context)
    >>>
    >>> # Output as LeRobot dataset
    >>> transformer = LeRobotV21TransformerConfig(
    ...     outdir="./output",
    ...     robot_model="./robot.yaml",
    ... ).build()
    >>> context = transformer.run(context)

All imports should be done from submodules:

    - rebake.core: Context
    - rebake.analysis: compute_topic_timing_metrics, compute_interval_stats, etc.
    - rebake.ingest: Rosbag1IngestorConfig, Rosbag2IngestorConfig
    - rebake.synchronize: ZeroOrderHoldTimeSynchronizerConfig, etc.
    - rebake.enrich: TfChainEnricherConfig, DeltaJointPositionEnricherConfig, etc.
    - rebake.encode: VideoEncoderConfig
    - rebake.decode: VideoDecoderConfig
    - rebake.transform: LeRobotV21TransformerConfig
    - rebake.exceptions: RebakeError, IngestError, etc.
"""

__all__: list[str] = []
