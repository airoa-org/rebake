"""Hand Command enricher for extracting hand command data."""

from __future__ import annotations

from pydantic import BaseModel, ConfigDict
from .. import _internal
from ..core.context import Context


class HandCommandEnricherConfig(BaseModel):
    """Configuration for the Hand Command enricher.

    This enricher extracts hand command data from the dataset.
    It processes hand-related control messages (like gripper
    commands) and adds them as structured columns.

    Example:
        >>> config = HandCommandEnricherConfig()
        >>> enricher = config.build()
        >>> context = enricher.run(context)
    """

    model_config = ConfigDict(arbitrary_types_allowed=True)

    def build(self) -> HandCommandEnricher:
        """Create a HandCommandEnricher from this config.

        Returns:
            A new HandCommandEnricher instance.
        """
        return HandCommandEnricher(self)

    def _to_inner(self) -> _internal.enrich.PyHandCommandEnricherConfig:
        """Convert to internal Rust config object."""
        return _internal.enrich.PyHandCommandEnricherConfig()


class HandCommandEnricher:
    """Extracts hand command data from the dataset.

    This enricher processes hand-related messages and creates
    structured data columns for hand commands like gripper
    open/close commands.

    Example:
        >>> config = HandCommandEnricherConfig()
        >>> enricher = config.build()
        >>> context = enricher.run(context)
    """

    def __init__(self, config: HandCommandEnricherConfig):
        """Create a new HandCommandEnricher.

        Args:
            config: The configuration for this enricher.
        """
        self.config = config
        self._inner = _internal.enrich.PyHandCommandEnricher(config._to_inner())

    def run(self, context: Context) -> Context:
        """Run the enricher on the given context.

        This extracts hand command data and adds it to the dataset.

        Args:
            context: The context containing hand-related messages.

        Returns:
            The same context with hand command data added.

        Examples:
            ```python
            context = enricher.run(context)
            ```
        """
        self._inner.run(context.inner)
        return context
