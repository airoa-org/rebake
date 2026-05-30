import io

from PIL import Image
from rebake import _internal
from rebake.common import PyImageFrame, PyImageShape
from rebake.core import Context
from rebake.decode import VideoDecoderConfig
from rebake.encode import VideoEncoderConfig


def create_dummy_image(width=64, height=64, color=(255, 0, 0)):
    """Creates a dummy PNG image in memory."""
    img = Image.new("RGB", (width, height), color)
    byte_arr = io.BytesIO()
    img.save(byte_arr, format="PNG")
    return byte_arr.getvalue()


def create_dummy_frames(count=30, width=64, height=64):
    """Create dummy image frames for testing."""
    frames = []
    for i in range(count):
        color = (255, i * 5 % 256, 0)
        img_bytes = create_dummy_image(width=width, height=height, color=color)
        shape = PyImageShape(height, width, 3)
        frame = PyImageFrame(i, "png", list(img_bytes), shape)
        frames.append(frame)
    return frames


def test_video_encoder_decoder(tmp_path):
    """
    Verifies that VideoEncoder can encode images to video and VideoDecoder can decode them back.
    """
    # 1. Setup Context with dummy image data
    context = Context()
    topic_name = "/camera/image_raw"
    test_uuid = "test-uuid-12345"

    PyImageFrame = _internal.common.PyImageFrame
    PyImageShape = _internal.common.PyImageShape

    frames = []
    for i in range(30):
        color = (255, i * 5, 0)
        img_bytes = create_dummy_image(color=color)

        shape = PyImageShape(64, 64, 3)
        frame = PyImageFrame(i, "png", list(img_bytes), shape)
        frames.append(frame)

    context.set_image_data({topic_name: frames})
    context.set_video_cache_dir(str(tmp_path))
    # VideoEncoder requires airoa_metadata with uuid
    context.set_airoa_metadata(
        {
            "uuid": test_uuid,
            "version": "1.3",
            "files": [],
            "context": {"entities": [], "components": []},
            "run": {
                "total_time_s": 0.0,
                "instructions": [],
                "segments": [],
                "episode_label": "",
            },
        }
    )

    # 2. Run VideoEncoder
    encoder_config = VideoEncoderConfig(fps=30)
    encoder = encoder_config.build()
    context = encoder.run(context)

    # Verify video file exists (path includes uuid subdirectory)
    video_path = tmp_path / test_uuid / "camera" / "image_raw.mp4"
    assert video_path.exists()

    # 3. Run VideoDecoder
    # Clear image data to verify decoder restores it
    context.set_image_data({})

    decoder_config = VideoDecoderConfig()
    decoder = decoder_config.build()
    context = decoder.run(context)

    # Verify image data is restored
    image_data = context.get_image_data()
    assert topic_name in image_data
    decoded_frames = image_data[topic_name]
    assert len(decoded_frames) == 30

    first_frame = decoded_frames[0]
    img = Image.open(io.BytesIO(bytes(first_frame.bytes)))
    assert img.size == (64, 64)


def test_video_encoder_decoder_context_free(tmp_path):
    """
    Verifies that VideoEncoder.encode() and VideoDecoder.decode_registry() work without Context.

    This tests the Context-free API where:
    - VideoEncoder.encode(image_data, video_cache_dir, uuid) encodes frames directly
      and returns typed video artifacts
    - VideoDecoder.decode_registry(video_registry) decodes typed artifacts directly
    """
    topic_name = "/camera/image_raw"
    test_uuid = "test-uuid-context-free"

    # 1. Create dummy image data
    frames = create_dummy_frames(count=30, width=64, height=64)
    image_data = {topic_name: frames}

    # 2. Encode using Context-free API
    encoder_config = VideoEncoderConfig(fps=30)
    encoder = encoder_config.build()
    artifacts = encoder.encode(
        image_data, video_cache_dir=str(tmp_path), uuid=test_uuid
    )

    # Verify video file was created (path includes uuid subdirectory)
    assert topic_name in artifacts
    video_path = tmp_path / test_uuid / "camera" / "image_raw.mp4"
    assert video_path.exists()
    assert artifacts[topic_name].video_path == str(video_path)

    # 3. Decode using Context-free API
    decoder_config = VideoDecoderConfig()
    decoder = decoder_config.build()
    decoded_image_data = decoder.decode_registry(artifacts)

    # Verify decoded frames
    assert topic_name in decoded_image_data
    decoded_frames = decoded_image_data[topic_name]
    assert len(decoded_frames) == 30

    # Verify first frame is a valid image
    first_frame = decoded_frames[0]
    assert first_frame.extension == "png"
    img = Image.open(io.BytesIO(bytes(first_frame.bytes)))
    assert img.size == (64, 64)

    # Verify frame indices
    for i, frame in enumerate(decoded_frames):
        assert frame.index == i


def test_video_roundtrip_preserves_frame_count(tmp_path):
    """
    Verifies that encode -> decode preserves the correct number of frames.
    """
    topic_name = "/depth/image"
    frame_count = 50
    test_uuid = "test-uuid-roundtrip"

    # Create frames
    frames = create_dummy_frames(count=frame_count, width=128, height=96)
    image_data = {topic_name: frames}

    # Encode
    encoder = VideoEncoderConfig(fps=30).build()
    artifacts = encoder.encode(image_data, str(tmp_path), uuid=test_uuid)

    # Decode via the canonical typed artifact API
    decoder = VideoDecoderConfig().build()
    decoded_data = decoder.decode_registry(artifacts)

    # Verify frame count is preserved
    assert len(decoded_data[topic_name]) == frame_count


def test_video_decoder_decode_rgb_paths_compat(tmp_path):
    """RGB path-only decode should remain available as a compatibility helper."""
    topic_name = "/camera/image_raw"
    frames = create_dummy_frames(count=10, width=32, height=24)
    artifacts = VideoEncoderConfig(fps=30).build().encode(
        {topic_name: frames},
        str(tmp_path),
        uuid="test-uuid-rgb-paths",
    )

    decoder = VideoDecoderConfig().build()
    decoded = decoder.decode_rgb_paths(
        {topic: artifact.video_path for topic, artifact in artifacts.items()}
    )

    assert topic_name in decoded
    assert len(decoded[topic_name]) == 10
