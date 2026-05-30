"""TF Chain enricher for computing transform chains."""

from __future__ import annotations

import pyarrow as pa
from pydantic import BaseModel, ConfigDict

from .. import _internal
from ..core.context import Context


class FramePair(BaseModel):
    """A pair of coordinate frames for computing transforms.

    This defines a transform to compute from a source frame to a
    target frame. For example, to compute the hand position relative
    to the robot base, use source="base_link" and target="hand_palm_link".

    Attributes:
        source: The source coordinate frame name.
        target: The target coordinate frame name.

    Example:
        >>> pair = FramePair(source="base_link", target="hand_palm_link")
    """

    source: str
    target: str

    def _to_inner(self) -> _internal.enrich.PyFramePair:
        """Convert to internal Rust object."""
        return _internal.enrich.PyFramePair(self.source, self.target)


class TfChainEnricherConfig(BaseModel):
    """Configuration for the TF Chain enricher.

    This enricher computes transform chains between coordinate frames.
    It uses the TF buffer (built by TfBufferEnricher) to look up
    transforms at each timestamp.

    You must run TfBufferEnricher before this enricher.

    Attributes:
        frame_pairs: List of frame pairs to compute transforms for.

    Example:
        >>> config = TfChainEnricherConfig(frame_pairs=[
        ...     FramePair(source="base_link", target="hand_palm_link")
        ... ])
        >>> enricher = config.build()
        >>> # Using run() with Context
        >>> context = enricher.run(context)
        >>> # Using enrich() with Arrow Tables
        >>> tf_chain = enricher.enrich(tf_buffer)
    """

    model_config = ConfigDict(arbitrary_types_allowed=True)
    frame_pairs: list[FramePair]

    def build(self) -> TfChainEnricher:
        """Create a TfChainEnricher from this config.

        Returns:
            A new TfChainEnricher instance.
        """
        return TfChainEnricher(self)

    def _to_inner(self) -> _internal.enrich.PyTfChainEnricherConfig:
        """Convert to internal Rust config object."""
        inner_pairs = [pair._to_inner() for pair in self.frame_pairs]
        return _internal.enrich.PyTfChainEnricherConfig(inner_pairs)


class TfChainEnricher:
    """Computes transform chains between coordinate frames.

    This enricher uses the TF buffer to compute the full transform
    chain between specified coordinate frames. For each frame pair,
    it adds a new topic "/tf_chain" with the computed transforms.

    The output contains position (x, y, z) and orientation
    (quaternion: qx, qy, qz, qw) for each timestamp.

    This class provides two methods for processing data:

    - ``run(context)``: Process data within a Context object.
    - ``enrich(tf_buffer)``: Process Arrow Tables directly.

    Example:
        >>> config = TfChainEnricherConfig(frame_pairs=[
        ...     FramePair(source="base_link", target="hand_palm_link")
        ... ])
        >>> enricher = config.build()
        >>> # Using run() with Context
        >>> context = enricher.run(context)
        >>> # Using enrich() with Arrow Tables
        >>> tf_chain = enricher.enrich(tf_buffer)
    """

    def __init__(self, config: TfChainEnricherConfig):
        """Create a new TfChainEnricher.

        Args:
            config: The configuration for this enricher.
        """
        self.config = config
        self._inner = _internal.enrich.PyTfChainEnricher(config._to_inner())

    def run(self, context: Context) -> Context:
        """Run the enricher on the given context.

        This computes transform chains for all configured frame pairs.

        Args:
            context: The context with TF buffer already built.

        Returns:
            The context with transform chain data added.

        Example:
            >>> context = enricher.run(context)
        """
        self._inner.run(context.inner)
        return context

    def enrich(self, tf_buffer: pa.Table) -> pa.Table:
        """Enrich TF buffer data to compute transform chains.

        Args:
            tf_buffer: Arrow Table containing /tf_buffer topic data
                (output from TfBufferEnricher).

        Returns:
            Arrow Table containing the transform chain with columns:

            - timestamp_ns: The timestamp in nanoseconds
            - x, y, z: Position components
            - qx, qy, qz, qw: Orientation quaternion components

        Example:
            >>> tf_chain = enricher.enrich(tf_buffer)
        """
        context = Context.from_tables({"/tf_buffer": tf_buffer})
        context = self.run(context)

        return pa.Table.from_batches([context.get_record_batch("/tf_chain")])
