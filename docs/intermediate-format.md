# Intermediate Format

This is the directory format written by `rebake-cli export` or by `ParquetVideoExporterConfig` in a pipeline. One directory corresponds to one ROS bag, with one Parquet table per topic and video files for camera and depth streams.

You can read this format without rebake; see [reading without rebake](#reading-without-rebake). To feed it back into rebake and rebuild a dataset, start the pipeline with [`ParquetVideoIngestorConfig`](configuration.md#parquetvideoingestorconfig). For the `export` command, see [CLI](cli.md#export).

## Layout

```text
<output>/
└── <uuid>/                          # UUID from meta.json
    ├── parquet/
    │   ├── <topic>.parquet          # one per topic
    │   ├── _metadata.parquet        # recording metadata, one row
    │   ├── _topic_type_map.parquet  # topic -> ROS message type
    │   └── _video_registry.parquet  # topic -> video; absent when there are no videos
    └── videos/
        ├── <topic>.mp4              # camera and lossy depth
        └── <topic>.mkv              # lossless FFV1 depth
```

## Topic tables

The file name is the topic name with the leading `/` removed and the remaining `/` replaced with `__`. For example, `/camera/rgb/image_raw` becomes `camera__rgb__image_raw.parquet`.

Columns preserve the ROS message structure. Nested messages become struct columns, arrays become list columns, and nested paths such as `header.stamp` remain nested.

Columns present in every table:

- `timestamp_ns` (u64): recording timestamp in nanoseconds. For MCAP this is log time. Rows keep the order recorded in the ROS bag.
- `publish_timestamp_ns` (nullable u64): publish timestamp.

Output from `rebake-cli export` also always has a `rosbag_uuid` string column. When writing with `ParquetVideoExporterConfig` inside a pipeline, that column appears if the pipeline passed through `UuidEnricherConfig`. If you export after synchronization or enrichment, the columns present at that point, such as `synched_timestamp_ns` and `is_fresh`, are written as well.

One type caveat: ROS `uint16` fields are widened to u32 columns.

### Image, depth, and point cloud payloads

For the following topics, the byte `data` field is replaced by a u32 `index` column.

| Topic | Detection | Where the bytes go |
|---|---|---|
| Compressed camera image | Topic name ends with `/compressed` | RGB video under `videos/`. Frame number = `index` |
| Compressed depth | Topic name ends with `/compressedDepth` | Depth video under `videos/`. Frame number = `index` |
| Raw image | Type is `Image` | JPEG frames encoded into RGB video |
| Point cloud | Type is `PointCloud2` | Not saved in the current version; see [known limits](#known-limits) |

This keeps Parquet files from being dominated by image bytes; the table keeps only the reference (`index`).

Depth videos are written by `rebake-cli export` and by `ParquetVideoExporterConfig` when `depth_config` is set. If a pipeline exporter omits `depth_config`, depth payloads are not saved.

## System tables

`_metadata.parquet` is the recording's [meta.json](metadata.md) as a one-row table. Field meanings are defined by the metadata page.

`_topic_type_map.parquet`:

| Column | Meaning |
|---|---|
| `rosbag_uuid` | Recording UUID |
| `topic_name` | Topic name, starting with `/` |
| `message_type` | ROS message type, for example `sensor_msgs/msg/Image` |

`_video_registry.parquet` is the ledger for topics that became videos:

| Column | Meaning |
|---|---|
| `rosbag_uuid` | Recording UUID |
| `topic_name` | Original topic name |
| `video_path` | Path relative to the directory, for example `videos/camera__rgb__image_raw.mp4` |
| `media_type` | `rgb` or `depth` |
| `codec_family` / `encoder_name` / `pix_fmt` | Codec identity, for example `av1` / `libsvtav1` / `yuv420p` |
| `width` / `height` / `fps` | Video dimensions and frame rate (u32) |
| `encoding_config_json` | Full encoding config as JSON. For depth, this includes `depth_max_mm` |

`video_path` is relative so the whole directory can move or be stored in object storage without breaking the registry.

## Videos

Camera (RGB) videos are mp4. Lossy depth is also mp4. Lossless FFV1 depth is mkv because FFV1 does not have an mp4 container path.

Lossy depth stores 10-bit distance values. The original 16-bit millimeter values are quantized to `[1, 1023]` and stored in the Y plane of P010LE as `q10 << 6`; chroma is neutral. Zero means invalid pixel. For choosing the quantization range, see [encoding](encoding.md#depth-video).

Lossless FFV1 uses gray16le, with millimeter values unchanged.

## Reading without rebake

The tables are ordinary Parquet.

```bash
duckdb -c "SELECT timestamp_ns, position FROM '<uuid>/parquet/joint_states.parquet' LIMIT 5"
```

```python
import pandas as pd
df = pd.read_parquet("<uuid>/parquet/joint_states.parquet")   # polars / pyarrow work too
```

Camera videos are ordinary mp4 files, readable by FFmpeg or OpenCV. The table's `index` column is the frame number.

To restore lossy depth to millimeters, decode each 16-bit pixel value `y`:

```text
q10 = y >> 6
mm  = 0                                  # when q10 = 0 (invalid pixel)
mm  = (q10 * depth_max_mm + 511) / 1023  # otherwise, integer division
```

`depth_max_mm` is in `encoding_config_json` in `_video_registry.parquet`; the default is 4092. FFV1 depth needs no conversion, because the pixel value is already millimeters.

## Re-ingest round-trip

Feed the directory to a pipeline whose first stage is [`ParquetVideoIngestorConfig`](configuration.md#parquetvideoingestorconfig). rebake restores the topic tables, metadata, topic type map, and video registry. You no longer need a `meta.json` file because `_metadata.parquet` replaces it.

Videos are not decoded into in-memory frames on ingest. LeRobot transformation reads frames directly from the video files. The output dataset's videos are re-encoded with the transformer's `video_config`.

## Known limits

- PointCloud2 payloads are not saved. The table keeps only an `index` column, and point cloud data is still unavailable after re-ingest.
- ROS `uint16` columns are widened to u32.
- rebake is pre-1.0, and this format may change in later versions. Re-ingest with the same rebake version that wrote the output when you need maximum certainty.
