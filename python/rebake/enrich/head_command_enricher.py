"""Head Command enricher for extracting head command data."""

from __future__ import annotations

from pydantic import BaseModel, ConfigDict
from .. import _internal
from ..core.context import Context


class HeadCommandEnricherConfig(BaseModel):
    """Configuration for the Head Command enricher.

    This enricher extracts head command data from the dataset.
    It processes head-related control messages and adds them
    as structured columns.

    Example:
        >>> config = HeadCommandEnricherConfig()
        >>> enricher = config.build()
        >>> context = enricher.run(context)
    """

    model_config = ConfigDict(arbitrary_types_allowed=True)

    def build(self) -> HeadCommandEnricher:
        """Create a HeadCommandEnricher from this config.

        Returns:
            A new HeadCommandEnricher instance.
        """
        return HeadCommandEnricher(self)

    def _to_inner(self) -> _internal.enrich.PyHeadCommandEnricherConfig:
        """Convert to internal Rust config object."""
        return _internal.enrich.PyHeadCommandEnricherConfig()


class HeadCommandEnricher:
    """Extracts head command data from the dataset.

    This enricher processes head-related messages and creates
    structured data columns for head commands like pan and tilt.

    Example:
        >>> config = HeadCommandEnricherConfig()
        >>> enricher = config.build()
        >>> context = enricher.run(context)
    """

    def __init__(self, config: HeadCommandEnricherConfig):
        """Create a new HeadCommandEnricher.

        Args:
            config: The configuration for this enricher.
        """
        self.config = config
        self._inner = _internal.enrich.PyHeadCommandEnricher(config._to_inner())

    def run(self, context: Context) -> Context:
        """Run the enricher on the given context.

        This extracts head command data and adds it to the dataset.

        Args:
            context: The context containing head-related messages.

        Returns:
            The same context with head command data added.

        Examples:
            ```python
            context = enricher.run(context)
            ```
        """
        self._inner.run(context.inner)
        return context
