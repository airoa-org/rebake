# Metadata (meta.json)

ROS bags only contain messages. `meta.json` tells rebake which recording this is, what task it contains, and which time ranges should become episodes. The UUID becomes the output folder name and the `rosbag_uuid` column; labels and segments become LeRobot tasks and episodes. Without this file, Transformer and Exporter stages do not run.

This page is the `meta.json` reference. For pipeline settings, see [configuration](configuration.md).

## Location

Place a file named `meta.json` in the same directory as the ROS bag.

```text
recordings/episode_0001/
├── data.mcap
└── meta.json
```

Ingestors read it by default, equivalent to `require_metadata: true`. Set that to `false` only for inspection-only pipelines that have no [metadata-dependent stages](#which-stages-need-it).

When re-ingesting an exported [intermediate format](intermediate-format.md), no sidecar `meta.json` is needed because the metadata is already inside the directory.

## Schema v2.0

This is a minimal copy-and-fill shape. Use `null` when a value is unavailable.

```json
{
  "$schema": "https://raw.githubusercontent.com/airoa-org/airoa-metadata/main/airoa_metadata/schemas/v2_0.json",
  "schema_version": "2.0",
  "uuid": "123e4567-e89b-12d3-a456-426614174000",
  "robot": {
    "uri": null,
    "type": "my_robot",
    "id": "my_robot_001",
    "checksum": null
  },
  "files": [
    {
      "type": "mcap",
      "name": "data.mcap",
      "checksum": null
    }
  ],
  "environment": {
    "type": "real_world",
    "site": "lab_a",
    "location": null
  },
  "runner": {
    "type": "operator",
    "organization": "my_org",
    "name": "operator_001"
  },
  "devices": [],
  "programs": [
    {
      "role": "interface",
      "name": "teleop",
      "source": {
        "git": {
          "uri": "https://example.com/teleop.git",
          "hash": "abc123",
          "branch": "main",
          "tag": null
        }
      }
    },
    {
      "role": "data_collection",
      "name": "recorder",
      "source": {
        "git": {
          "uri": "https://example.com/recorder.git",
          "hash": "def456",
          "branch": "main",
          "tag": null
        }
      }
    }
  ],
  "episode": {
    "start_time": 0.0,
    "end_time": 8.0,
    "success": true,
    "label": "put the cup on the shelf"
  },
  "labels": [
    "pick up the cup",
    "place the cup"
  ],
  "segments": [
    {
      "start_time": 0.0,
      "end_time": 4.0,
      "label_idx": 0,
      "success": true
    },
    {
      "start_time": 4.0,
      "end_time": 8.0,
      "label_idx": 1,
      "success": true
    }
  ]
}
```

Field meanings:

- `uuid`: unique ID per recording. It becomes the output folder name and `rosbag_uuid` column.
- `robot.type` / `robot.id`: robot model and individual robot. `type` is written to the dataset's `robot_type`.
- `files[]`: recording files. One item with `type` equal to `mcap`, `rosbag2`, or `rosbag` is required.
- `environment`: `type` is `real_world` or `simulation`. `site` is written to episode metadata.
- `runner`: who operated the system. `type` is `operator` or `model`.
- `devices[]`: hardware used. It may be empty.
- `programs[]`: software provenance. One `interface` (or `teleoperation`) entry and one `data_collection` (or `data_capture`) entry are required, each with `source.git`.
- `episode`: task for the whole recording. `label` becomes the whole-recording task name; use an empty string for no whole-recording task.
- `labels[]`: subtask names, written to LeRobot `tasks.jsonl`.
- `segments[]`: time ranges that become episodes. `label_idx` indexes into `labels[]`, and `success` is the segment result.

All times are seconds in the same clock as the recording messages. If the ROS bag timestamps are Unix-time seconds, use Unix-time seconds here too. Segments written from zero when the bag uses Unix time will not overlap the recording, and transformation fails with `no segments overlap`.

## How it affects output

`LeRobotV21TransformerConfig` uses this file as follows.

- Each `labels[]` item becomes a task. If `episode.label` is not empty, it is also registered as a whole-recording task.
- Each `segment` becomes a training range. With `separate_per_primitive: false` (default), all overlapping segments become one episode, with `next.done` set at segment boundaries. With `true`, each segment becomes one episode.
- Only segments that overlap the synchronized timeline are used. If none overlap, the run fails.
- `robot`, `environment.site`, `files[]`, and `programs[]` are written into dataset metadata.

## Which stages need it

| Stage | meta.json |
|---|---|
| Rosbag1 / Rosbag2 Ingestor | Read by default. Can be disabled with `require_metadata: false` |
| `UuidEnricherConfig` | Required. Missing metadata stops the run |
| `VideoEncoderConfig` / `DepthVideoConfig` | Required, because the UUID names output paths |
| `ParquetVideoExporterConfig` | Required |
| `LeRobotV21TransformerConfig` | Required |
| Synchronizer, TF / delta / shift Enricher, ImageEncoderConfig / DepthImageEncoderConfig | Not required |

## Pre-run checklist

Use this to avoid a failed run.

- `meta.json` exists next to each ROS bag, with this exact name.
- `uuid` is unique per recording.
- `files[]` has an item whose `type` is `mcap`, `rosbag2`, or `rosbag`.
- `programs[]` has one `interface`-family entry and one `data_collection`-family entry, each with `source.git`.
- Every `segments[].label_idx` is within the `labels[]` range.
- Segment seconds use the recording clock and lie within the recording time range.

## Troubleshooting

| Message fragment | Cause | Fix |
|---|---|---|
| `failed to read meta.json` | No `meta.json` next to the ROS bag | Put it in the same directory, not a subdirectory |
| `missing required data: airoa_metadata` | A metadata-dependent stage ran without metadata | Restore `require_metadata: true` or remove that stage |
| `skipped:` stops the run | `UuidEnricherConfig` ran without metadata | Add `meta.json` or remove that stage |
| `no segments overlap` | Segment times do not overlap the recording | Check the time clock. Also check that a Synchronizer is present |
| missing errors mentioning program or git fields | The two required `programs[]` roles are incomplete | Add `interface` and `data_collection` entries with `source.git` |

For stage-order errors, see the [configuration troubleshooting table](configuration.md#troubleshooting).
