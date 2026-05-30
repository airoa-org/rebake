"""Unit tests for video metadata and artifact helpers."""

import json
from types import SimpleNamespace

from rebake.common import PyImageFrame, PyImageShape
from rebake.encode import (
    CodecConfig,
    VideoEncoder,
    VideoEncoderConfig,
    build_video_artifact,
    build_video_metadata,
    validate_video_config_json,
)


def test_build_video_metadata_returns_canonical_fields() -> None:
    """Video metadata should come from rebake's canonical codec knowledge."""
    config = VideoEncoderConfig(
        fps=24,
        codec_config=CodecConfig.av1_vaapi(qp=110),
    )
    config_json = validate_video_config_json(config.model_dump_json(exclude_none=True))

    metadata = build_video_metadata(
        config_json,
        width=1280,
        height=720,
    )

    assert metadata.media_type == "rgb"
    assert metadata.codec_family == "av1"
    assert metadata.encoder_name == "av1_vaapi"
    assert metadata.pix_fmt == "yuv420p"
    assert metadata.width == 1280
    assert metadata.height == 720
    assert metadata.fps == 24
    assert metadata.encoding_config_json == config_json


def test_build_video_metadata_rejects_zero_dimensions() -> None:
    """Zero dimensions should fail fast instead of creating partial metadata."""
    config_json = validate_video_config_json(VideoEncoderConfig().model_dump_json())

    try:
        build_video_metadata(config_json, width=0, height=480)
    except ValueError as exc:
        assert "positive width and height" in str(exc)
    else:
        raise AssertionError("Expected ValueError for zero width")


def test_build_video_artifact_wraps_path_and_metadata() -> None:
    """Artifacts should combine the local path with canonical video metadata."""
    config_json = validate_video_config_json(VideoEncoderConfig().model_dump_json())

    artifact = build_video_artifact(
        config_json,
        video_path="/tmp/video.mp4",
        width=640,
        height=480,
    )

    assert artifact.video_path == "/tmp/video.mp4"
    assert artifact.metadata.media_type == "rgb"
    assert artifact.metadata.width == 640
    assert artifact.metadata.height == 480


def test_video_encoder_encode_builds_typed_results(
    monkeypatch,
) -> None:
    """encode should combine encoded paths with canonical metadata."""
    encoder = VideoEncoderConfig(fps=12).build()
    shape = PyImageShape(240, 320, 3)
    image_data = {
        "/camera": [PyImageFrame(0, "png", [0], shape)]
    }

    def fake_run(self: VideoEncoder, context):
        return SimpleNamespace(video_paths={"/camera": "/tmp/camera.mp4"})

    monkeypatch.setattr(VideoEncoder, "run", fake_run)

    artifacts = encoder.encode(image_data, "/tmp/cache", "uuid-123")

    assert artifacts["/camera"].video_path == "/tmp/camera.mp4"
    assert artifacts["/camera"].metadata.fps == 12
    assert artifacts["/camera"].metadata.width == 320
    assert artifacts["/camera"].metadata.height == 240


def test_video_encoder_config_defaults_match_canonical_av1_defaults() -> None:
    """Default VideoEncoderConfig should use the canonical AV1 policy."""
    config_json = validate_video_config_json(VideoEncoderConfig().model_dump_json())
    normalized = json.loads(config_json)

    assert normalized["fps"] == 100
    assert normalized["gop"] == 20
    assert normalized["crf"] == "34"
    assert normalized["codec_config"]["codec"] == "AV1"
    assert normalized["codec_config"]["preset"] == 10
