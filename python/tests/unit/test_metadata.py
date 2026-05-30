"""Unit tests for metadata V2.0 parsing helpers."""

from __future__ import annotations

import json
from pathlib import Path
from typing import Any

from rebake.core import EnvType, MetadataV2_0, RunnerType, parse_metadata_as_v2_0


def test_parse_metadata_as_v2_0_converts_v1_3_input() -> None:
    metadata_json = json.dumps(
        {
            "uuid": "550e8400-e29b-41d4-a716-446655440000",
            "version": "1.3",
            "files": [{"type": "rosbag2", "name": "recording_0.mcap"}],
            "context": {
                "entities": [
                    {"role": "robot", "id": "robot-1"},
                    {"role": "location", "name": "site-a"},
                    {"role": "organization", "name": "airoa"},
                ],
                "components": [
                    {
                        "role": "interface",
                        "name": "umi",
                        "source": {"git": {"uri": "u", "hash": "h", "branch": "b"}},
                    }
                ],
            },
            "run": {
                "total_time_s": 1.0,
                "instructions": [{"idx": 0, "text": ["task"]}],
                "segments": [
                    {
                        "start_time": 10.0,
                        "end_time": 11.0,
                        "instruction_idx": 0,
                        "controlled_by": "operator",
                        "success": True,
                        "is_composite": True,
                    }
                ],
                "episode_label": "episode-a",
            },
        }
    )

    normalized = parse_metadata_as_v2_0(metadata_json)

    assert isinstance(normalized, MetadataV2_0)
    assert normalized.schema_version == "2.0"
    assert normalized.uuid == "550e8400-e29b-41d4-a716-446655440000"
    assert normalized.runner.organization == "airoa"
    assert normalized.environment.site == "site-a"
    assert normalized.robot.id == "robot-1"
    assert normalized.robot.robot_type == "HSR"


def test_parse_metadata_as_v2_0_preserves_v2_input() -> None:
    testdata = (
        Path(__file__).resolve().parents[3]
        / "rebake"
        / "testdata"
        / "metadata"
        / "v2.0"
        / "meta.json"
    )
    normalized = parse_metadata_as_v2_0(testdata.read_text())

    assert isinstance(normalized, MetadataV2_0)
    assert normalized.schema_version == "2.0"
    assert normalized.runner.runner_type == RunnerType.Operator
    assert normalized.environment.env_type == EnvType.RealWorld
    assert normalized.environment.site == "location001"
    assert json.loads(normalized.to_json())["$schema"].startswith("https://")


def _to_public_jsonish(value: Any) -> Any:
    if value is None or isinstance(value, (str, int, float, bool)):
        return value
    if isinstance(value, list):
        return [_to_public_jsonish(item) for item in value]
    if isinstance(value, (EnvType, RunnerType)):
        return str(value)

    attr_renames = {
        ("MetadataV2_0", "schema"): "$schema",
        ("Robot", "robot_type"): "type",
        ("File", "file_type"): "type",
        ("Environment", "env_type"): "type",
        ("Runner", "runner_type"): "type",
        ("Device", "device_type"): "type",
    }

    public_fields = {}
    for attr in dir(value):
        if attr.startswith("_") or attr == "to_json":
            continue
        attr_value = getattr(value, attr)
        if callable(attr_value):
            continue
        json_key = attr_renames.get((type(value).__name__, attr), attr)
        public_fields[json_key] = _to_public_jsonish(attr_value)

    return public_fields


def test_metadata_v2_public_surface_matches_serialized_rust_shape() -> None:
    testdata = (
        Path(__file__).resolve().parents[3]
        / "rebake"
        / "testdata"
        / "metadata"
        / "v2.0"
        / "meta.json"
    )
    payload = json.loads(testdata.read_text())
    payload["devices"] = [
        {
            "role": "controller",
            "type": "joystick",
            "id": "joy001",
        }
    ]

    metadata = parse_metadata_as_v2_0(json.dumps(payload))

    assert _to_public_jsonish(metadata) == json.loads(metadata.to_json())


# =============================================================================
# Phase 1: typed constructor API
# =============================================================================


import pytest  # noqa: E402

from rebake.core import (  # noqa: E402
    Context,
    Device,
    Environment,
    Episode,
    File,
    GitSource,
    Program,
    Robot,
    Runner,
)


def _valid_metadata() -> MetadataV2_0:
    return MetadataV2_0(
        episode=Episode(label="pick and place", start_time=0.0, end_time=0.0),
        files=[File(name="bag.mcap")],
        programs=[Program(role="interface", name="teleop")],
    )


# ---- Construction ----


def test_typed_minimal_construction() -> None:
    m = _valid_metadata()
    assert m.uuid
    assert m.schema_version == "2.0"
    assert m.episode.label == "pick and place"
    assert m.files[0].name == "bag.mcap"
    assert m.programs[0].name == "teleop"


def test_typed_defaults_are_applied() -> None:
    m = _valid_metadata()
    assert m.robot.robot_type == "unknown"
    assert m.robot.id == ""  # honest empty default, not a fake uuid
    assert m.environment.site == "unknown"
    assert m.devices == []
    assert m.labels == []
    assert m.segments == []


def test_metadata_rejects_empty_programs() -> None:
    with pytest.raises(ValueError, match="programs"):
        MetadataV2_0(
            episode=Episode(label="task", start_time=0.0, end_time=0.0),
            files=[File(name="bag.mcap")],
            programs=[],
        )


def test_device_id_defaults_to_empty_string() -> None:
    d = Device(role="controller", device_type="joystick")
    assert d.id == ""


def test_typed_full_construction() -> None:
    m = MetadataV2_0(
        episode=Episode(label="task", start_time=1.0, end_time=2.0),
        files=[File(name="bag.mcap", file_type="mcap")],
        uuid="11111111-2222-3333-4444-555555555555",
        robot=Robot(robot_type="hsr2", id="r-001"),
        environment=Environment(env_type=EnvType.RealWorld, site="lab"),
        runner=Runner(runner_type=RunnerType.Operator, organization="airoa", name="u"),
        programs=[Program(role="interface", name="teleop_v1")],
        labels=["pick", "place"],
    )
    assert m.uuid == "11111111-2222-3333-4444-555555555555"
    assert m.robot.robot_type == "hsr2"
    assert m.runner.name == "u"


# ---- Required field validation ----


def test_metadata_rejects_empty_files() -> None:
    with pytest.raises(ValueError, match="files"):
        MetadataV2_0(
            episode=Episode(label="task", start_time=0.0, end_time=0.0),
            files=[],
            programs=[Program(role="interface", name="teleop")],
        )


def test_episode_rejects_empty_label() -> None:
    with pytest.raises(ValueError, match="label"):
        Episode(label="", start_time=0.0, end_time=0.0)


def test_file_rejects_empty_name() -> None:
    with pytest.raises(ValueError, match="name"):
        File(name="")


def test_program_requires_role_and_name() -> None:
    with pytest.raises(ValueError, match="role"):
        Program(role="", name="x")
    with pytest.raises(ValueError, match="name"):
        Program(role="x", name="")


def test_device_requires_role_and_device_type() -> None:
    with pytest.raises(ValueError, match="role"):
        Device(role="", device_type="x")
    with pytest.raises(ValueError, match="device_type"):
        Device(role="x", device_type="")


def test_git_source_requires_all_three_fields() -> None:
    with pytest.raises(ValueError, match="uri"):
        GitSource(uri="", hash="h", branch="b")
    with pytest.raises(ValueError, match="hash"):
        GitSource(uri="u", hash="", branch="b")
    with pytest.raises(ValueError, match="branch"):
        GitSource(uri="u", hash="h", branch="")


# ---- Mutation ----


def test_setter_mutates_top_level() -> None:
    m = _valid_metadata()
    m.uuid = "new"
    assert m.uuid == "new"
    m.episode = Episode(label="task2", start_time=0.0, end_time=0.0)
    assert m.episode.label == "task2"


def test_setter_does_not_validate_eagerly_but_boundary_does() -> None:
    m = _valid_metadata()
    m.files = []  # accepted; invariant broken intentionally
    with pytest.raises(ValueError):
        m.to_json()


# ---- Serialization ----


def test_to_json_from_json_roundtrip() -> None:
    m = _valid_metadata()
    m2 = MetadataV2_0.from_json(m.to_json())
    assert m == m2


def test_to_dict_from_dict_roundtrip() -> None:
    m = _valid_metadata()
    d = m.to_dict()
    assert isinstance(d, dict)
    m2 = MetadataV2_0.from_dict(d)
    assert m == m2


# ---- Context boundary validation ----


def test_context_accepts_typed_metadata() -> None:
    ctx = Context()
    m = _valid_metadata()
    ctx.set_airoa_metadata(m)
    got = ctx.get_airoa_metadata()
    assert got is not None and got["uuid"] == m.uuid


def test_context_accepts_dict_metadata_for_back_compat() -> None:
    ctx = Context()
    m = _valid_metadata()
    ctx.set_airoa_metadata(json.loads(m.to_json()))
    assert ctx.get_airoa_metadata() is not None


def test_context_rejects_invalid_typed_metadata_at_boundary() -> None:
    ctx = Context()
    m = _valid_metadata()
    m.files = []
    with pytest.raises(ValueError, match="files"):
        ctx.set_airoa_metadata(m)


def test_context_rejects_invalid_dict_metadata_at_boundary() -> None:
    ctx = Context()
    m = _valid_metadata()
    bad = json.loads(m.to_json())
    bad["files"] = []
    with pytest.raises(ValueError, match="files"):
        ctx.set_airoa_metadata(bad)


# ---- Equality ----


def test_metadata_equality_is_field_based() -> None:
    programs = [Program(role="interface", name="teleop")]
    m1 = MetadataV2_0(
        episode=Episode(label="task", start_time=0.0, end_time=0.0),
        files=[File(name="bag.mcap")],
        programs=programs,
        uuid="same",
    )
    m2 = MetadataV2_0(
        episode=Episode(label="task", start_time=0.0, end_time=0.0),
        files=[File(name="bag.mcap")],
        programs=programs,
        uuid="same",
    )
    assert m1 == m2
    m2.uuid = "different"
    assert m1 != m2
