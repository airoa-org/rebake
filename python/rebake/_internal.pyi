"""Type stubs for the rebake._internal Rust extension module.

This file provides type hints for the binary module compiled from Rust
using PyO3. It enables IDE autocompletion and static type checking
with tools like mypy and pyright.
"""

from typing import Optional

import pyarrow

# =============================================================================
# common module
# =============================================================================

class common:
    """Common data structures used across the rebake library."""

    class PyImageShape:
        """Shape information for an image (height, width, channels).

        Attributes:
            height: Image height in pixels.
            width: Image width in pixels.
            channels: Number of color channels (e.g., 3 for RGB).
        """

        def __init__(self, height: int, width: int, channels: int) -> None: ...
        @property
        def height(self) -> int: ...
        @property
        def width(self) -> int: ...
        @property
        def channels(self) -> int: ...

    class PyImageFrame:
        """A single image frame with metadata.

        Stores image data as bytes along with frame index and format info.

        Attributes:
            index: Frame index in the sequence.
            extension: File extension (e.g., "png", "jpg").
            bytes: Raw image data.
            shape: Optional image dimensions.
        """

        def __init__(
            self,
            index: int,
            extension: str,
            bytes: bytes,
            shape: Optional[common.PyImageShape] = None,
        ) -> None: ...
        @property
        def index(self) -> int: ...
        @property
        def extension(self) -> str: ...
        @property
        def bytes(self) -> bytes: ...
        @property
        def shape(self) -> Optional[common.PyImageShape]: ...

    class PyDepthFrame:
        """A single depth frame with metadata.

        Stores depth data as bytes along with frame index and format info.

        Attributes:
            index: Frame index in the sequence.
            extension: File extension (e.g., "png").
            bytes: Raw depth data.
            ros_format: Optional original ROS CompressedImage format string.
        """

        def __init__(
            self,
            index: int,
            extension: str,
            bytes: bytes,
            ros_format: Optional[str] = None,
        ) -> None: ...
        @property
        def index(self) -> int: ...
        @property
        def extension(self) -> str: ...
        @property
        def bytes(self) -> bytes: ...
        @property
        def ros_format(self) -> Optional[str]: ...

# =============================================================================
# core module
# =============================================================================

class core:
    """Core data structures for the rebake pipeline."""

    @staticmethod
    def metadata_to_arrow(metadata_json: str) -> pyarrow.RecordBatch: ...

    @staticmethod
    def parse_metadata_as_v2_0(metadata_json: str) -> core.MetadataV2_0: ...

    class EnvType:
        RealWorld: core.EnvType
        Simulation: core.EnvType

    class RunnerType:
        Operator: core.RunnerType
        Model: core.RunnerType

    class Robot:
        def __init__(
            self,
            *,
            robot_type: Optional[str] = None,
            id: Optional[str] = None,
            uri: Optional[str] = None,
            checksum: Optional[str] = None,
        ) -> None: ...
        uri: Optional[str]
        robot_type: str
        id: str
        checksum: Optional[str]

    class File:
        def __init__(
            self,
            name: str,
            *,
            file_type: Optional[str] = None,
            checksum: Optional[str] = None,
        ) -> None: ...
        file_type: str
        name: str
        checksum: Optional[str]

    class Environment:
        def __init__(
            self,
            *,
            env_type: Optional[core.EnvType] = None,
            site: Optional[str] = None,
            location: Optional[str] = None,
        ) -> None: ...
        env_type: core.EnvType
        site: str
        location: Optional[str]

    class Runner:
        def __init__(
            self,
            *,
            runner_type: Optional[core.RunnerType] = None,
            organization: Optional[str] = None,
            name: Optional[str] = None,
        ) -> None: ...
        runner_type: core.RunnerType
        organization: str
        name: str

    class Device:
        def __init__(
            self,
            role: str,
            device_type: str,
            *,
            id: Optional[str] = None,
        ) -> None: ...
        role: str
        device_type: str
        id: str

    class GitSource:
        def __init__(
            self,
            uri: str,
            hash: str,
            branch: str,
            *,
            tag: Optional[str] = None,
        ) -> None: ...
        uri: str
        hash: str
        branch: str
        tag: Optional[str]

    class Source:
        def __init__(self, *, git: Optional[core.GitSource] = None) -> None: ...
        git: Optional[core.GitSource]

    class Program:
        def __init__(
            self,
            role: str,
            name: str,
            *,
            source: Optional[core.Source] = None,
        ) -> None: ...
        role: str
        name: str
        source: core.Source

    class Episode:
        def __init__(
            self,
            label: str,
            start_time: float,
            end_time: float,
            *,
            success: bool = True,
        ) -> None: ...
        start_time: float
        end_time: float
        success: bool
        label: str

    class Segment:
        def __init__(
            self,
            start_time: float,
            end_time: float,
            label_idx: int,
            *,
            success: bool = True,
        ) -> None: ...
        start_time: float
        end_time: float
        label_idx: int
        success: bool

    class MetadataV2_0:
        def __init__(
            self,
            episode: core.Episode,
            files: list[core.File],
            programs: list[core.Program],
            *,
            uuid: Optional[str] = None,
            robot: Optional[core.Robot] = None,
            environment: Optional[core.Environment] = None,
            runner: Optional[core.Runner] = None,
            devices: Optional[list[core.Device]] = None,
            labels: Optional[list[str]] = None,
            segments: Optional[list[core.Segment]] = None,
        ) -> None: ...
        @property
        def schema(self) -> str: ...
        @property
        def schema_version(self) -> str: ...
        uuid: str
        robot: core.Robot
        files: list[core.File]
        environment: core.Environment
        runner: core.Runner
        devices: list[core.Device]
        programs: list[core.Program]
        episode: core.Episode
        labels: list[str]
        segments: list[core.Segment]
        def to_json(self) -> str: ...
        def to_dict(self) -> dict: ...
        @classmethod
        def from_json(cls, json_str: str) -> core.MetadataV2_0: ...
        @classmethod
        def from_dict(cls, data: dict) -> core.MetadataV2_0: ...

    class PyContext:
        """Container that holds data as it moves through the pipeline.

        The Context stores ROS topic data as Arrow RecordBatches and carries
        metadata like fps, output directory, and image data between stages.
        """

        def __init__(self) -> None: ...
        @staticmethod
        def from_record_batches(
            batches: dict[str, pyarrow.RecordBatch],
        ) -> core.PyContext: ...
        def dataset_topics(self) -> list[str]: ...
        def topic_message_type(self, topic: str) -> Optional[str]: ...
        def get_record_batch(self, topic: str) -> pyarrow.RecordBatch: ...
        def set_record_batch(self, topic: str, batch: pyarrow.RecordBatch) -> None: ...
        def to_record_batches(self) -> dict[str, pyarrow.RecordBatch]: ...
        @property
        def fps(self) -> Optional[int]: ...
        @fps.setter
        def fps(self, value: Optional[int]) -> None: ...
        @property
        def output_dir(self) -> Optional[str]: ...
        @output_dir.setter
        def output_dir(self, value: Optional[str]) -> None: ...
        @property
        def rosbag_path(self) -> Optional[str]: ...
        @rosbag_path.setter
        def rosbag_path(self, value: Optional[str]) -> None: ...
        @property
        def bundle_root(self) -> Optional[str]: ...
        @bundle_root.setter
        def bundle_root(self, value: Optional[str]) -> None: ...
        def get_image_data(
            self,
        ) -> Optional[dict[str, list[common.PyImageFrame]]]: ...
        def set_image_data(
            self, data: Optional[dict[str, list[common.PyImageFrame]]]
        ) -> None: ...
        def get_depth_data(
            self,
        ) -> Optional[dict[str, list[common.PyDepthFrame]]]: ...
        def set_depth_data(
            self, data: Optional[dict[str, list[common.PyDepthFrame]]]
        ) -> None: ...
        def get_airoa_metadata_json(self) -> Optional[str]: ...
        def set_airoa_metadata_json(self, json: str) -> None: ...
        def set_airoa_metadata(self, metadata: core.MetadataV2_0) -> None: ...
        def get_metadata_record_batch(self) -> pyarrow.RecordBatch:
            """Get the airoa metadata as an Arrow RecordBatch.

            This preserves the full nested structure of the metadata, including:
            - context.entities as List<Struct>
            - context.components as List<Struct>
            - run.instructions as List<Struct>
            - run.segments as List<Struct>

            Returns:
                Arrow RecordBatch with a single row containing the metadata.

            Raises:
                RuntimeError: If no metadata is available.
            """
            ...
        def get_video_paths(self) -> Optional[dict[str, str]]:
            """Get the video paths stored in the context.

            Returns:
                A dictionary mapping topic names to video file paths,
                or None if no video paths are set.
            """
            ...
        def set_video_registry_json(self, json: str) -> None:
            """Set the canonical video registry from a JSON string."""
            ...
        def get_topic_message_type_map(self) -> Optional[dict[str, str]]:
            """Get the full topic to message type mapping.

            Returns:
                A dictionary mapping topic names to their ROS message types,
                or None if no mapping is available.
            """
            ...
        def set_topic_message_type_map(self, map: Optional[dict[str, str]]) -> None:
            """Set the topic to message type mapping.

            Args:
                map: A dictionary mapping topic names to their ROS message types,
                    or None to clear the mapping.
            """
            ...
        def save_to_parquet(self, output_dir: str) -> None:
            """Save context dataset to Parquet files.

            Args:
                output_dir: Directory to save Parquet files to.
            """
            ...
    def normalize_metadata_json_to_v2_0(json: str) -> str: ...

# =============================================================================
# ingest module
# =============================================================================

class ingest:
    """ROS bag file readers."""

    class PyRosbag1IngestorConfig:
        """Configuration for the ROS1 bag file ingestor."""

        def __init__(self) -> None: ...

    class PyRosbag1Ingestor:
        """Reads ROS1 bag files (.bag format)."""

        def __init__(self, config: ingest.PyRosbag1IngestorConfig) -> None: ...
        def run(self, context: core.PyContext) -> None: ...

    class PyRosbag2IngestorConfig:
        """Configuration for the ROS2 bag file ingestor."""

        def __init__(self) -> None: ...
        def build(self) -> ingest.PyRosbag2Ingestor: ...

    class PyRosbag2Ingestor:
        """Reads ROS2 bag files (SQLite3/MCAP format)."""

        def __init__(self, config: ingest.PyRosbag2IngestorConfig) -> None: ...
        def run(self, context: core.PyContext) -> None: ...

# =============================================================================
# synchronize module
# =============================================================================

class synchronize:
    """Time synchronization for multi-topic data."""

    class PyNearestNeighborTimeSynchronizerConfig:
        """Configuration for nearest neighbor time synchronizer.

        Args:
            fps: Target frames per second for output.
        """

        def __init__(self, fps: int) -> None: ...

    class PyNearestNeighborTimeSynchronizer:
        """Synchronizes data using nearest neighbor interpolation."""

        def __init__(
            self, config: synchronize.PyNearestNeighborTimeSynchronizerConfig
        ) -> None: ...
        def run(self, context: core.PyContext) -> None: ...

    class PyZeroOrderHoldTimeSynchronizerConfig:
        """Configuration for zero-order hold time synchronizer.

        Args:
            fps: Target frames per second for output.
        """

        def __init__(self, fps: int) -> None: ...
        def build(self) -> synchronize.PyZeroOrderHoldTimeSynchronizer: ...

    class PyZeroOrderHoldTimeSynchronizer:
        """Synchronizes data using zero-order hold interpolation."""

        def __init__(
            self, config: synchronize.PyZeroOrderHoldTimeSynchronizerConfig
        ) -> None: ...
        def run(self, context: core.PyContext) -> None: ...

# =============================================================================
# enrich module
# =============================================================================

class enrich:
    """Data enrichment for adding computed features."""

    class PyFramePair:
        """A pair of coordinate frame names for transform computation.

        Args:
            source: Source coordinate frame name.
            target: Target coordinate frame name.
        """

        def __init__(self, source: str, target: str) -> None: ...
        @property
        def source(self) -> str: ...
        @property
        def target(self) -> str: ...

    class PyTfBufferEnricherConfig:
        """Configuration for TF buffer enricher."""

        def __init__(self) -> None: ...
        def build(self) -> enrich.PyTfBufferEnricher: ...

    class PyTfBufferEnricher:
        """Builds a TF buffer from /tf and /tf_static messages."""

        def __init__(self, config: enrich.PyTfBufferEnricherConfig) -> None: ...
        def run(self, context: core.PyContext) -> None: ...

    class PyTfChainEnricherConfig:
        """Configuration for TF chain enricher.

        Args:
            frame_pairs: List of frame pairs to compute transforms for.
        """

        def __init__(self, frame_pairs: list[enrich.PyFramePair]) -> None: ...
        def build(self) -> enrich.PyTfChainEnricher: ...

    class PyTfChainEnricher:
        """Computes transform chains between coordinate frames."""

        def __init__(self, config: enrich.PyTfChainEnricherConfig) -> None: ...
        def run(self, context: core.PyContext) -> None: ...

    class PyDeltaJointPositionEnricherConfig:
        """Configuration for delta joint position enricher.

        Args:
            topic_names: List of joint state topic names to process.
        """

        def __init__(self, topic_names: list[str]) -> None: ...
        @property
        def topic_names(self) -> list[str]: ...
        @topic_names.setter
        def topic_names(self, value: list[str]) -> None: ...
        def build(self) -> enrich.PyDeltaJointPositionEnricher: ...

    class PyDeltaJointPositionEnricher:
        """Calculates changes in joint positions between frames."""

        def __init__(
            self, config: enrich.PyDeltaJointPositionEnricherConfig
        ) -> None: ...
        def run(self, context: core.PyContext) -> None: ...

    class PyDeltaTransformEnricherConfig:
        """Configuration for delta transform enricher.

        Args:
            topic_names: List of transform topic names to process.
        """

        def __init__(self, topic_names: list[str], delta_reference_frame: str) -> None: ...
        delta_reference_frame: str
        @property
        def topic_names(self) -> list[str]: ...
        @topic_names.setter
        def topic_names(self, value: list[str]) -> None: ...
        def build(self) -> enrich.PyDeltaTransformEnricher: ...

    class PyDeltaTransformEnricher:
        """Calculates changes in transforms between frames."""

        def __init__(self, config: enrich.PyDeltaTransformEnricherConfig) -> None: ...
        def run(self, context: core.PyContext) -> None: ...

    class PyHandCommandEnricherConfig:
        """Configuration for hand command enricher."""

        def __init__(self) -> None: ...
        def build(self) -> enrich.PyHandCommandEnricher: ...

    class PyHandCommandEnricher:
        """Extracts hand command data from robot messages."""

        def __init__(self, config: enrich.PyHandCommandEnricherConfig) -> None: ...
        def run(self, context: core.PyContext) -> None: ...

    class PyHeadCommandEnricherConfig:
        """Configuration for head command enricher."""

        def __init__(self) -> None: ...
        def build(self) -> enrich.PyHeadCommandEnricher: ...

    class PyHeadCommandEnricher:
        """Extracts head command data from robot messages."""

        def __init__(self, config: enrich.PyHeadCommandEnricherConfig) -> None: ...
        def run(self, context: core.PyContext) -> None: ...

# =============================================================================
# encode module
# =============================================================================

class encode:
    """Image and video encoding for image data."""

    class PyImageEncoderConfig:
        """Configuration for the image encoder.

        This encoder has no configuration parameters - it simply saves
        all image data to individual files in the output directory.
        """

        def __new__(cls) -> encode.PyImageEncoderConfig: ...

    class PyImageEncoder:
        """Saves image frames to individual files.

        This encoder takes image data from the Context and saves each frame
        as an individual file. The output directory structure mirrors the
        topic name hierarchy.
        """

        def __init__(self, config: encode.PyImageEncoderConfig) -> None: ...
        def run(self, context: core.PyContext) -> None: ...

    class PyScalingFlag:
        """Scaling algorithm flags for video encoding.

        These flags control the scaling algorithm used when resizing
        video frames during encoding.
        """

        FastBilinear: encode.PyScalingFlag
        Bilinear: encode.PyScalingFlag
        Bicubic: encode.PyScalingFlag
        Bicublin: encode.PyScalingFlag
        Gauss: encode.PyScalingFlag
        Sinc: encode.PyScalingFlag
        Lanczos: encode.PyScalingFlag
        Spline: encode.PyScalingFlag
        SrcVChrDropMask: encode.PyScalingFlag
        SrcVChrDropShift: encode.PyScalingFlag
        ParamDefault: encode.PyScalingFlag
        PrintInfo: encode.PyScalingFlag
        FullChrHInt: encode.PyScalingFlag
        FullChrHInp: encode.PyScalingFlag
        DirectBgr: encode.PyScalingFlag
        AccurateRnd: encode.PyScalingFlag
        BitExact: encode.PyScalingFlag
        ErrorDiffusion: encode.PyScalingFlag

    class PyX264Preset:
        """x264/x265 encoder preset for speed vs compression tradeoff."""

        Ultrafast: encode.PyX264Preset
        Superfast: encode.PyX264Preset
        Veryfast: encode.PyX264Preset
        Faster: encode.PyX264Preset
        Fast: encode.PyX264Preset
        Medium: encode.PyX264Preset
        Slow: encode.PyX264Preset
        Slower: encode.PyX264Preset
        Veryslow: encode.PyX264Preset

    class PyX264Tune:
        """x264 (H.264) encoder tuning options."""

        Film: encode.PyX264Tune
        Animation: encode.PyX264Tune
        Grain: encode.PyX264Tune
        StillImage: encode.PyX264Tune
        Psnr: encode.PyX264Tune
        Ssim: encode.PyX264Tune
        FastDecode: encode.PyX264Tune
        ZeroLatency: encode.PyX264Tune

    class PyX265Tune:
        """x265 (H.265/HEVC) encoder tuning options."""

        Psnr: encode.PyX265Tune
        Ssim: encode.PyX265Tune
        Grain: encode.PyX265Tune
        FastDecode: encode.PyX265Tune
        ZeroLatency: encode.PyX265Tune
        Animation: encode.PyX265Tune

    class PyNvencPreset:
        """NVIDIA NVENC preset (P1 fastest through P7 slowest/best compression)."""

        P1: encode.PyNvencPreset
        P2: encode.PyNvencPreset
        P3: encode.PyNvencPreset
        P4: encode.PyNvencPreset
        P5: encode.PyNvencPreset
        P6: encode.PyNvencPreset
        P7: encode.PyNvencPreset

    class PyNvencTune:
        """NVIDIA NVENC tuning mode."""

        Hq: encode.PyNvencTune
        Ll: encode.PyNvencTune
        Ull: encode.PyNvencTune

    class PyCodecConfig:
        """Codec-specific configuration for video encoding."""

        def to_yaml(self) -> str:
            """Serialize to YAML string."""
            ...

        @staticmethod
        def av1(
            lp: Optional[int] = None,
            pin: Optional[int] = None,
            preset: int = 10,
            film_grain: Optional[int] = None,
            film_grain_denoise: Optional[bool] = None,
            lookahead: Optional[int] = None,
            fast_decode: Optional[int] = None,
        ) -> encode.PyCodecConfig:
            """Create AV1 codec configuration (SVT-AV1)."""
            ...

        @staticmethod
        def h264(
            threads: Optional[int] = None,
            preset: encode.PyX264Preset = ...,
            tune: Optional[list[encode.PyX264Tune]] = None,
        ) -> encode.PyCodecConfig:
            """Create H.264 codec configuration (libx264)."""
            ...

        @staticmethod
        def h265(
            threads: Optional[int] = None,
            preset: encode.PyX264Preset = ...,
            tune: Optional[list[encode.PyX265Tune]] = None,
            frame_threads: Optional[int] = None,
        ) -> encode.PyCodecConfig:
            """Create H.265 codec configuration (libx265)."""
            ...

        @staticmethod
        def h264_vaapi(
            qp: int = 21,
            device: Optional[str] = None,
            profile: Optional[str] = "high",
            b_depth: Optional[int] = None,
            async_depth: Optional[int] = 16,
        ) -> encode.PyCodecConfig:
            """Create H.264 VA-API hardware encoder configuration.

            Requires VA-API compatible hardware (AMD VCN or Intel QSV).
            """
            ...

        @staticmethod
        def h265_vaapi(
            qp: int = 29,
            device: Optional[str] = None,
            profile: Optional[str] = None,
            async_depth: Optional[int] = None,
        ) -> encode.PyCodecConfig:
            """Create H.265 VA-API hardware encoder configuration.

            Requires VA-API compatible hardware (AMD VCN or Intel QSV).
            Note: AMD VCN does NOT support B-frames for HEVC.
            """
            ...

        @staticmethod
        def av1_vaapi(
            qp: int = 110,
            device: Optional[str] = None,
            profile: Optional[str] = None,
            b_depth: Optional[int] = None,
            async_depth: Optional[int] = None,
        ) -> encode.PyCodecConfig:
            """Create AV1 VA-API hardware encoder configuration.

            Requires AMD VCN 4.0+ (RDNA 3) or Intel Arc graphics.
            """
            ...

        @staticmethod
        def h264_nvenc(
            qp: int = 26,
            gpu: Optional[int] = None,
            preset: encode.PyNvencPreset = ...,
            tune: Optional[encode.PyNvencTune] = None,
            profile: Optional[str] = None,
            b_frames: int = 1,
            rc_lookahead: Optional[int] = None,
        ) -> encode.PyCodecConfig:
            """Create H.264 NVENC hardware encoder configuration."""
            ...

        @staticmethod
        def h265_nvenc(
            qp: int = 25,
            gpu: Optional[int] = None,
            preset: encode.PyNvencPreset = ...,
            tune: Optional[encode.PyNvencTune] = None,
            profile: Optional[str] = None,
            b_frames: int = 0,
            rc_lookahead: Optional[int] = None,
        ) -> encode.PyCodecConfig:
            """Create H.265/HEVC NVENC hardware encoder configuration."""
            ...

        @staticmethod
        def av1_nvenc(
            qp: int = 130,
            gpu: Optional[int] = None,
            preset: encode.PyNvencPreset = ...,
            tune: Optional[encode.PyNvencTune] = None,
            profile: Optional[str] = None,
            b_frames: int = 0,
            rc_lookahead: Optional[int] = None,
        ) -> encode.PyCodecConfig:
            """Create AV1 NVENC hardware encoder configuration."""
            ...

    def py_is_vaapi_available() -> bool:
        """Check if VA-API hardware acceleration is available.

        Returns True if the VA-API device (/dev/dri/renderD128) exists.
        """
        ...

    def py_validate_video_config_json(
        config_json: str,
        preflight: bool = False,
    ) -> str:
        """Validate and normalize a VideoEncoderConfig JSON string."""
        ...

    def py_build_video_metadata_json(
        config_json: str,
        width: int,
        height: int,
    ) -> str:
        """Build canonical video metadata JSON from a VideoEncoderConfig JSON string."""
        ...

    def py_build_video_artifact_json(
        config_json: str,
        video_path: str,
        width: int,
        height: int,
    ) -> str:
        """Build canonical video artifact JSON from config, path, and dimensions."""
        ...

    class PyVideoEncoderConfig:
        """Configuration for the video encoder.

        Args:
            fps: Frames per second (default: 100).
            gop: Group of pictures size (default: 20).
            crf: Constant rate factor for quality (default: "34").
            scaling: Scaling algorithm (default: Bicubic).
            codec_config: Codec-specific configuration (default: AV1).
        """

        def __init__(
            self,
            fps: int = 100,
            gop: int = 20,
            crf: str = "34",
            scaling: encode.PyScalingFlag = ...,
            codec_config: Optional[encode.PyCodecConfig] = None,
        ) -> None: ...
        def to_yaml(self) -> str:
            """Serialize to YAML string."""
            ...
        def to_dict(self) -> dict:
            """Convert to dictionary suitable for YAML serialization."""
            ...

    class PyVideoEncoder:
        """Encodes image sequences to video files."""

        def __init__(self, config: encode.PyVideoEncoderConfig) -> None: ...
        def run(self, context: core.PyContext) -> None: ...

    class DepthCodecConfig:
        """Codec-specific configuration for depth video encoding.

        Use static factory methods to create instances:
        - ``av1()``: AV1 via SVT-AV1 software encoder (default)
        - ``h265_vaapi()``: HEVC via VA-API hardware encoder
        - ``av1_vaapi()``: AV1 via VA-API hardware encoder
        - ``ffv1()``: FFV1 lossless encoder
        """

        @staticmethod
        def av1(crf: int = 4, preset: int = 4) -> encode.DepthCodecConfig:
            """Create AV1 (SVT-AV1) software encoder configuration."""
            ...
        @staticmethod
        def h265_vaapi(
            qp: int = 18, device: Optional[str] = None,
        ) -> encode.DepthCodecConfig:
            """Create HEVC VA-API hardware encoder configuration."""
            ...
        @staticmethod
        def av1_vaapi(
            global_quality: int = 35, device: Optional[str] = None,
        ) -> encode.DepthCodecConfig:
            """Create AV1 VA-API hardware encoder configuration."""
            ...
        @staticmethod
        def h265_nvenc(
            qp: int = 10,
            gpu: Optional[int] = None,
            preset: encode.PyNvencPreset = ...,
            tune: Optional[encode.PyNvencTune] = None,
            b_frames: int = 0,
            rc_lookahead: Optional[int] = None,
        ) -> encode.DepthCodecConfig:
            """Create HEVC NVENC hardware encoder configuration."""
            ...
        @staticmethod
        def av1_nvenc(
            qp: int = 20,
            gpu: Optional[int] = None,
            preset: encode.PyNvencPreset = ...,
            tune: Optional[encode.PyNvencTune] = None,
            b_frames: int = 0,
            rc_lookahead: Optional[int] = None,
        ) -> encode.DepthCodecConfig:
            """Create AV1 NVENC hardware encoder configuration."""
            ...
        @staticmethod
        def ffv1() -> encode.DepthCodecConfig:
            """Create FFV1 lossless encoder configuration."""
            ...
        def to_yaml(self) -> str:
            """Serialize to YAML string."""
            ...
        def is_lossless(self) -> bool:
            """Check if this codec is lossless (FFV1)."""
            ...
        def video_extension(self) -> str:
            """Get the video file extension ('mkv' for FFV1, 'mp4' for others)."""
            ...

    class DepthVideoConfig:
        """Depth video encoding configuration.

        Args:
            depth_max_mm: Maximum depth in mm for Q10Clip4 quantization. Default: 4092.
            fps: Frames per second. Default: 30.
            codec_config: Codec-specific configuration. Default: AV1 (CRF=4, preset=4).
        """

        def __init__(
            self,
            depth_max_mm: int = 4092,
            fps: int = 30,
            codec_config: Optional[encode.DepthCodecConfig] = None,
        ) -> None: ...
        def to_yaml(self) -> str:
            """Serialize to YAML string."""
            ...
        def to_dict(self) -> dict:
            """Convert to dictionary suitable for YAML serialization."""
            ...

# =============================================================================
# decode module
# =============================================================================

class decode:
    """Video decoding for compressed image data."""

    class PyVideoDecoderConfig:
        """Configuration for the video decoder.

        The decoder automatically processes all video artifacts found in
        context's internal video registry. No configuration is needed.
        """

        def __init__(self) -> None: ...

    class PyVideoDecoder:
        """Decodes compressed video data to image frames."""

        def __init__(self, config: decode.PyVideoDecoderConfig) -> None: ...
        def decode_rgb_paths(
            self, video_paths: dict[str, str]
        ) -> dict[str, list[common.PyImageFrame]]: ...
        def decode_video_registry_json(
            self, video_registry_json: str, bundle_root: Optional[str]
        ) -> dict[str, list[common.PyImageFrame]]: ...
        def run(self, context: core.PyContext) -> None: ...

# =============================================================================
# transform module
# =============================================================================

class transform:
    """Output format transformers."""

    class PyLeRobotV21TransformerConfig:
        """Configuration for the LeRobot v2.1 transformer.

        Args:
            config_json: JSON string containing the transformer configuration.
        """

        def __init__(self, config_json: str) -> None: ...
        def build(self) -> transform.PyLeRobotV21Transformer: ...

    class PyLeRobotV21Transformer:
        """Transforms data into LeRobot v2.1 dataset format."""

        def __init__(self, config: transform.PyLeRobotV21TransformerConfig) -> None: ...
        def run(self, context: core.PyContext) -> None: ...

# =============================================================================
# merge module
# =============================================================================

class merge:
    """Dataset merging operations."""

    @staticmethod
    def discover_datasets(
        source_dir: str,
    ) -> list[str]:
        """Discover LeRobot dataset directories inside a parent directory.

        Scans immediate subdirectories of `source_dir` for those containing
        `meta/info.json`. Returns the discovered paths sorted alphabetically
        for deterministic merge ordering.

        Returns an empty list if `source_dir` does not exist.

        Args:
            source_dir: Path to a directory containing multiple LeRobot dataset subdirectories.

        Returns:
            Sorted list of dataset directory paths.
        """
        ...

    @staticmethod
    def merge_datasets(
        source_dir: str,
        output: str,
        chunks_size: int | None = None,
    ) -> int:
        """Merge multiple LeRobot v2.1 datasets into a single dataset.

        Discovers all LeRobot dataset subdirectories within `source_dir`
        (those containing `meta/info.json`), then merges them with renumbered
        episode indices, task deduplication, and consolidated metadata.

        Args:
            source_dir: Path to a directory containing multiple LeRobot dataset subdirectories.
            output: Path to output merged dataset directory.
            chunks_size: Override chunks_size (default: from first source).

        Returns:
            Number of datasets merged.

        Raises:
            RuntimeError: If fewer than 2 datasets are found in source_dir.
        """
        ...
