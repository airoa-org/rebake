from __future__ import annotations

import json
from pathlib import Path

from rebake.core import metadata_to_arrow, normalize_metadata_to_v2_0


def _load_v1_metadata() -> dict:
    root = Path(__file__).resolve().parents[3]
    metadata_path = root / "rebake" / "testdata" / "metadata" / "v1.3" / "meta.json"
    return json.loads(metadata_path.read_text())


def test_normalize_metadata_to_v2_0_converts_v1_metadata() -> None:
    metadata = _load_v1_metadata()

    normalized = normalize_metadata_to_v2_0(metadata)

    assert normalized["schema_version"] == "2.0"
    assert "version" not in normalized
    assert normalized["uuid"] == metadata["uuid"]
    assert "robot" in normalized
    assert "episode" in normalized


def test_metadata_to_arrow_uses_v2_schema_after_normalization() -> None:
    metadata = normalize_metadata_to_v2_0(_load_v1_metadata())

    table = metadata_to_arrow(metadata)

    assert "schema_version" in table.column_names
    assert "version" not in table.column_names
