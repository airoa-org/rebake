"""Image encoder for saving image frames to individual files."""

from __future__ import annotations

from pydantic import BaseModel

from .. import _internal
from ..core.context import Context


class ImageEncoderConfig(BaseModel):
    """Configuration for the image encoder.

    This encoder saves image frames from the Context to individual files.
    Unlike VideoEncoder, this has no configuration parameters - it simply
    saves all image data to the output directory.

    Output Structure:
        Images are saved to ``{output_dir}/{topic_path}/{index}.{extension}``.
        For example, topic "/camera/image" with frame index 0 becomes:
        ``{output_dir}/camera/image/0.jpg``

    Example:
        >>> config = ImageEncoderConfig()
        >>> encoder = config.build()
        >>> context.set_output_dir("./reference_images")
        >>> context = encoder.run(context)
    """

    def build(self) -> "ImageEncoder":
        """Create an ImageEncoder from this config.

        Returns:
            A new ImageEncoder instance.
        """
        return ImageEncoder(self)


class ImageEncoder:
    """Saves image frames to individual files.

    This encoder takes image data from the Context and saves each frame
    as an individual file. The output directory structure mirrors the
    topic name hierarchy.

    Note:
        The ``run()`` method requires ``output_dir`` to be set in the context.
        If ``image_data`` is not present, the stage returns early without error.

    Example:
        >>> config = ImageEncoderConfig()
        >>> encoder = config.build()
        >>> context.set_output_dir("./output")
        >>> context = encoder.run(context)
    """

    def __init__(self, config: ImageEncoderConfig):
        """Create a new ImageEncoder.

        Args:
            config: The configuration for this encoder.
        """
        self.config = config
        self._inner = _internal.encode.ImageEncoder(
            _internal.encode.ImageEncoderConfig()
        )

    def run(self, context: Context) -> Context:
        """Run the encoder on the given context.

        Saves all image data to individual files in the output directory.

        Args:
            context: The context containing image data. Must have:
                - ``output_dir`` set
                - ``image_data`` (optional - returns early if missing)

        Returns:
            The context (image_data is preserved for subsequent stages).

        Raises:
            RuntimeError: If output_dir is not set or I/O fails.
        """
        self._inner.run(context.inner)
        return context
