# 中間フォーマット

`rebake-cli export`（またはパイプラインの `ParquetVideoExporterConfig`）が書き出すディレクトリの仕様です。1 ディレクトリが ROS bag 1 本に対応し、トピックごとの Parquet テーブルと、カメラ・深度の動画からなります。

この形式は rebake なしで読めます。読み方は[rebake なしで読む](#rebake-なしで読む)へ。rebake に読み直してデータセットを作り直すには、[`ParquetVideoIngestorConfig`](configuration_ja.md#parquetvideoingestorconfig) を先頭にしたパイプラインを使います。`export` コマンドの使い方は [CLI](cli_ja.md#export) へ。

## レイアウト

```text
<出力先>/
└── <uuid>/                          # meta.json の UUID
    ├── parquet/
    │   ├── <topic>.parquet          # トピックごとに 1 つ
    │   ├── _metadata.parquet        # 録画のメタデータ（1 行）
    │   ├── _topic_type_map.parquet  # トピック → ROS メッセージ型
    │   └── _video_registry.parquet  # トピック → 動画。動画が 1 本もなければ無い
    └── videos/
        ├── <topic>.mp4              # カメラと、損失あり深度
        └── <topic>.mkv              # 無損失（FFV1）深度
```

## トピックテーブル

ファイル名は、トピック名の先頭の `/` を除き、残りの `/` を `__` に置き換えたものです。`/camera/rgb/image_raw` なら `camera__rgb__image_raw.parquet`。

列は ROS メッセージの構造がそのまま入ります。入れ子のメッセージは struct 列、配列は list 列になり、`header.stamp` のような階層も保たれます。

どのテーブルにもある列:

- `timestamp_ns`（u64）: 記録時刻のナノ秒（MCAP では log time）。行は ROS bag に記録された順に並びます。
- `publish_timestamp_ns`（u64、null 許容）: パブリッシュ時刻。

`rebake-cli export` の出力には、さらに `rosbag_uuid`（文字列）列が必ず付きます。パイプラインから `ParquetVideoExporterConfig` で書く場合は、`UuidEnricherConfig` を通したときに付きます。同期などのステージを通してから書き出した場合は、その時点の列（`synched_timestamp_ns` や `is_fresh` など）もそのまま入ります。

型の対応で 1 つだけ注意: ROS の `uint16` フィールドは u32 列に広げて格納されます。

### 画像と深度と点群のペイロード

次のトピックでは、バイト列の `data` フィールドの代わりに `index`（u32）列が入ります。

| トピック | 判定 | バイト列の行き先 |
|---|---|---|
| 圧縮カメラ画像 | トピック名が `/compressed` で終わる | `videos/` の RGB 動画。フレーム番号 = `index` |
| 圧縮深度 | トピック名が `/compressedDepth` で終わる | `videos/` の深度動画。フレーム番号 = `index` |
| 生画像 | 型が `Image` | JPEG 化して RGB 動画へ |
| 点群 | 型が `PointCloud2` | 現バージョンでは保存されません（下記[既知の制限](#既知の制限)） |

Parquet が画像バイトで肥大しないための設計で、テーブル側には参照（`index`）だけが残ります。

深度動画が書かれるのは、`rebake-cli export`（常に書く）と、`depth_config` を指定した `ParquetVideoExporterConfig` です。`depth_config` を省略したパイプライン経由の書き出しでは、深度ペイロードは保存されません。

## システムテーブル

`_metadata.parquet` は、録画の [meta.json](metadata_ja.md) を 1 行のテーブルにしたものです。フィールドの意味は[メタデータ](metadata_ja.md)のページが正です。

`_topic_type_map.parquet`:

| 列 | 内容 |
|---|---|
| `rosbag_uuid` | 録画の UUID |
| `topic_name` | トピック名（`/` 始まり） |
| `message_type` | ROS メッセージ型（例: `sensor_msgs/msg/Image`） |

`_video_registry.parquet` は動画になったトピックの台帳です:

| 列 | 内容 |
|---|---|
| `rosbag_uuid` | 録画の UUID |
| `topic_name` | 元のトピック名 |
| `video_path` | ディレクトリからの相対パス（例: `videos/camera__rgb__image_raw.mp4`） |
| `media_type` | `rgb` または `depth` |
| `codec_family` / `encoder_name` / `pix_fmt` | コーデックの素性（例: `av1` / `libsvtav1` / `yuv420p`） |
| `width` / `height` / `fps` | 動画の寸法とフレームレート（u32） |
| `encoding_config_json` | エンコード設定全体の JSON。深度では `depth_max_mm` を含む |

`video_path` が相対パスなのは、ディレクトリごと移動してもオブジェクトストレージに置いても台帳が壊れないようにするためです。

## 動画

カメラ（RGB）は mp4 です。深度は、損失あり（既定）が mp4、無損失の FFV1 だけが mkv です（FFV1 に mp4 コンテナがないため）。

損失あり深度の中身は 10 bit 化した距離値です。16 bit のミリメートル値を `[1, 1023]` に量子化し、P010LE（10 bit の YUV 形式）の Y プレーンに `q10 << 6` で格納します（色差は中立値）。0 は「無効画素」を意味します。量子化幅の選び方は[エンコード](encoding_ja.md#深度動画)へ。

無損失（FFV1）は gray16le で、ミリメートル値そのままです。

## rebake なしで読む

テーブルは普通の Parquet です。

```bash
duckdb -c "SELECT timestamp_ns, position FROM '<uuid>/parquet/joint_states.parquet' LIMIT 5"
```

```python
import pandas as pd
df = pd.read_parquet("<uuid>/parquet/joint_states.parquet")   # polars / pyarrow も同様
```

カメラ動画は普通の mp4 なので、FFmpeg や OpenCV でそのまま読めます。テーブルの `index` 列がフレーム番号です。

損失あり深度をミリメートルに戻すには、デコードした各画素の 16 bit 値 `y` から:

```text
q10 = y >> 6
mm  = 0                                  # q10 = 0 のとき（無効画素）
mm  = (q10 × depth_max_mm + 511) ÷ 1023  # それ以外（整数除算）
```

`depth_max_mm` は `_video_registry.parquet` の `encoding_config_json` にあります（既定 4092）。FFV1 の深度は変換不要で、画素値がそのままミリメートルです。

## 再取込（round-trip）

[`ParquetVideoIngestorConfig`](configuration_ja.md#parquetvideoingestorconfig) を先頭にしたパイプラインに食わせると、トピックテーブル・メタデータ・トピック型対応・動画台帳が復元されます。`meta.json` ファイルはもう要りません（`_metadata.parquet` が代わりになります）。

動画はメモリ上のフレームには戻されず、LeRobot 変換は動画ファイルから直接フレームを読みます。変換先のデータセットの動画は、そのときの `video_config` で再エンコードされます。

## 既知の制限

- 点群（PointCloud2）のペイロードは保存されません。テーブルに `index` 列だけが残り、再取込後も点群は使えません。
- ROS の `uint16` 列は u32 に広がります。
- rebake は 1.0 前で、この形式は今後の版で変わることがあります。書き出した版と同じ版の rebake で再取込するのが確実です。
