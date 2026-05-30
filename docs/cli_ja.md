# CLI の使い方

`rebake-cli` ツールには 3 つの動作モードがあります:

1. **Run モード** (`rebake-cli run`) - YAML 設定ファイルによるフルパイプライン実行
2. **Export モード** (`rebake-cli export`) - Parquet + Video 形式へのシンプルなエクスポート
3. **Merge モード** (`rebake-cli merge`) - 複数の LeRobot v2.1 データセットを 1 つに統合

## インストール

```bash
cargo install --path rebake-cli
```

これにより `rebake-cli` バイナリが Cargo の bin ディレクトリ（通常は `~/.cargo/bin/`）にインストールされます。

---

## Run コマンド

`run` コマンドは、YAML 設定ファイルで定義されたフルパイプラインを実行します。

YAML なしでシンプルに rosbag を Parquet に変換したい場合は、代わりに `rebake-cli export` を使用してください。

### 基本的な使い方

```bash
rebake-cli run <PATH> -c <CONFIG_FILE>
```

### 引数

| Argument | Short | Required | Default | Description |
|----------|-------|----------|---------|-------------|
| `<PATH>` | - | Yes | - | 入力パス。Rosbag ingestorは `.bag`/`.mcap` を受け付け、`ParquetVideoIngestor` は中間形式データディレクトリ（`parquet/` + `videos/`）を受け付けます |
| `--config` | `-c` | Yes* | - | パイプライン設定ファイル（YAML） |
| `--jobs` | `-j` | No | 1 | 並列パイプライン数 |

\* `--config` または `--config-data` のいずれかを指定する必要があります。`--config-data` はサブプロセス起動用の内部オプションであり、直接使用しないでください。

### 使用例

#### ディレクトリの処理

ディレクトリを渡すと、CLI は再帰的にすべての `.bag` および `.mcap` ファイルを検索します。`rosbag reindex` によって作成されたバックアップファイル（`.orig.bag`、`.orig.mcap`）は自動的に除外されます。

```bash
rebake-cli run \
    /data/recordings/ \
    -c config/pipeline/hsr2.yaml
```

#### 並列処理

複数ファイルを並列に処理します:

```bash
rebake-cli run \
    /data/recordings/ \
    -c config/pipeline/hsr2.yaml \
    -j 4
```

#### 単一ファイル

単一の rosbag ファイルを直接指定することもできます:

```bash
rebake-cli run \
    /data/recording.mcap \
    -c config/pipeline/hsr2.yaml
```

#### 中間形式データ (Parquet/MP4)

最初のステージが `ParquetVideoIngestor` の場合、以下のいずれかを渡すことができます:

- 中間形式データセットディレクトリを直接指定
- 複数の中間形式データセットを含む親ディレクトリを指定

指定されたディレクトリ自体がデータセットディレクトリでない場合、CLI は `parquet/_topic_type_map.parquet` と `parquet/_metadata.parquet` を探して、再帰的にデータセットディレクトリを検出します。

YAML 設定に入力パスを繰り返し記述する必要はありません。

```bash
rebake-cli run \
    /data/exported_bundle/ \
    -c config/pipeline/yubi_from_parquet.yaml
```

---

## Export コマンド

`export` コマンドは、YAML 設定ファイルなしで rosbag ファイルをエクスポートするシンプルな方法を提供します。データレイクへの取り込みに適した、構造化された Parquet ファイルとエンコード済み動画を出力します。

ROS 1 bag（`.bag`）と ROS 2 MCAP（`.mcap`）の両方の入力に対応しています。

### 基本的な使い方

```bash
rebake-cli export <PATH> -o <DIR>
```

### 出力構造

```
<DIR>/
  <UUID>/
    parquet/
      <topic>.parquet           # トピックデータ
      _metadata.parquet         # Rosbag メタデータ
      _topic_type_map.parquet   # トピック名からメッセージ型へのマッピング
      _video_registry.parquet   # トピック名から動画パスへのマッピング
    videos/
      <topic>.mp4               # エンコード済み動画ファイル
```

### 引数

| Argument | Short | Required | Default | Description |
|----------|-------|----------|---------|-------------|
| `<PATH>` | - | Yes | - | 入力 `.bag`/`.mcap` ファイルまたは rosbag ファイルを含むディレクトリ |
| `--output` | `-o` | Yes | - | 出力ディレクトリ |
| `--video-config` | - | No | - | VideoEncoderConfig YAML ファイルのパス（微調整用） |
| `--fps` | - | No | 100 | 動画フレームレート |
| `--codec` | - | No | av1 | 動画コーデック (av1, h264, h265, av1_vaapi, h264_vaapi, h265_vaapi, av1_nvenc, h264_nvenc, h265_nvenc) |
| `--qp` | - | No | - | QP ベースのハードウェア codec の QP override。`h264_vaapi`, `h264_nvenc`, `h265_nvenc`, `av1_nvenc` に適用。既定値は h264_vaapi=21, h264_nvenc=26, h265_nvenc=25, av1_nvenc=130。`av1_vaapi`/`h265_vaapi`（固定の既定値）とソフトウェア codec（crf を使用）では無視されます |
| `--depth-codec` | - | No | av1 | 深度動画コーデック (ffv1, av1, av1_vaapi, h265_vaapi, av1_nvenc, h265_nvenc) |
| `--depth-fps` | - | No | 30 | 深度動画フレームレート |
| `--depth-max-mm` | - | No | 4092 | Q10Clip4 量子化における最大深度（mm 単位、FFV1 では無視されます） |
| `--depth-qp` | - | No | - | 深度 NVENC の QP override。既定値は h265_nvenc=10, av1_nvenc=20 |
| `--jobs` | `-j` | No | 1 | 最大並列プロセス数 |

> **注意**: `--video-config` は `--fps`, `--codec`, `--qp` と排他的です。`--depth-*` オプションは独立しており、`--video-config` または `--fps`/`--codec` のどちらとも併用できます。

### 使用例

#### ディレクトリのエクスポート

```bash
rebake-cli export /data/recordings/ -o /data/output
```

#### 並列処理

```bash
rebake-cli export /data/recordings/ -o /data/output -j 4
```

単一の GeForce GPU で NVENC codec を使う場合は `-j` を `8` 以下に抑えてください — consumer 向け GPU では NVIDIA が同時 NVENC session 数を制限しているためです（Quadro/RTX Pro/Tesla は制限なし）。同じ GPU で他の NVENC job が動いている場合はさらに小さい値にします。

#### カスタム動画設定

```bash
rebake-cli export /data/recordings/ -o /data/output \
    --fps 60 --codec h264
```

`--video-config` を使わずに `--codec av1_vaapi` を指定した場合、`rebake export` は `gop: 100`, `qp: 124` を既定値として使います。

NVENC コーデックも同様に動作します。`--video-config` を使わない場合は、コーデックごとに次の既定値が適用されます:

| Codec | Default QP | Default preset | Default GOP |
|-------|-----------:|----------------|------------:|
| `h264_nvenc` | 26 | P5 | 20 |
| `h265_nvenc` | 25 | P4 | 100 |
| `av1_nvenc` | 130 | P7 | 20 |

```bash
rebake-cli export /data/recordings/ -o /data/output \
    --fps 100 --codec av1_nvenc
```

`--qp` で `h264_vaapi` と NVENC コーデック（`h264_nvenc`, `h265_nvenc`, `av1_nvenc`）の QP を上書きできます。`av1_vaapi` と `h265_vaapi` は固定の既定値を使い、`--qp` は無視されます。`b_frames`, `rc_lookahead`, `tune`, `profile`, `gpu` といった他のオプションを指定したい場合は、`config/export/` のサンプルを参考に `--video-config` を使ってください。

#### カスタム深度動画設定

```bash
rebake-cli export /data/recordings/ -o /data/output \
    --depth-codec ffv1 --depth-fps 15
```

深度 NVENC コーデックも同じパターンで使えます。`--video-config` を使わない場合は、`--depth-max-mm 4092` 向けに保守寄りに調整した QP を既定値として適用します:

| Depth codec | Default QP | Default preset |
|-------------|-----------:|----------------|
| `h265_nvenc` | 10 | P4 |
| `av1_nvenc` | 20 | P4 |

`--depth-qp` で QP を上書きできます:

```bash
rebake-cli export /data/recordings/ -o /data/output \
    --depth-codec av1_nvenc --depth-qp 24
```

#### 動画設定ファイルの使用

パラメータ最適化の実験には、YAML 設定ファイルを使用してください:

```bash
rebake-cli export /data/recordings/ -o /data/output \
    --video-config video_config.yaml
```

動画設定ファイルの例（`video_config.yaml`）:

```yaml
fps: 30
gop: 10
crf: "23"
scaling: Bicubic
codec_config:
  codec: H264
  preset: medium
  # オプション: tune: [film, fastdecode]
```

その他の動画設定例は `config/export/` を参照してください:
- `video_config_av1.yaml` - AV1 (SVT-AV1) ソフトウェアエンコーダ
- `video_config_h264.yaml` - H.264 (x264) ソフトウェアエンコーダ
- `video_config_h265.yaml` - H.265 (x265) ソフトウェアエンコーダ
- `video_config_av1_vaapi.yaml` - AV1 VA-API ハードウェアエンコーダ
- `video_config_av1_nvenc.yaml` - AV1 NVENC ハードウェアエンコーダ
- `video_config_h264_nvenc.yaml` - H.264 NVENC ハードウェアエンコーダ
- `video_config_h265_nvenc.yaml` - H.265 NVENC ハードウェアエンコーダ

---

## Merge コマンド

`merge` コマンドは、複数の LeRobot v2.1 データセットを 1 つのデータセットに統合します。エピソードの再番号付け、タスクの重複排除、Parquet カラムの再マッピング、動画ファイルのコピー、メタデータの統合を処理します。

一般的なワークフロー: `rebake run` -> `rebake merge`

### 基本的な使い方

```bash
rebake-cli merge <SOURCE_DIR> -o <DIR>
```

`SOURCE_DIR` ディレクトリには複数の LeRobot データセットのサブディレクトリを配置してください。`meta/info.json` ファイルを持つ各サブディレクトリがデータセットとして自動検出されます。

```
SOURCE_DIR/
├── dataset_a/        ← 検出される（meta/info.json あり）
│   ├── meta/
│   ├── data/
│   └── videos/
├── dataset_b/        ← 検出される
│   ├── meta/
│   ├── data/
│   └── videos/
└── README.txt        ← 無視される（meta/info.json なし）
```

### 引数

| Argument | Short | Required | Default | Description |
|----------|-------|----------|---------|-------------|
| `<SOURCE_DIR>` | - | Yes | - | LeRobot データセットのサブディレクトリを含むディレクトリ |
| `--output` | `-o` | Yes | - | 統合後のデータセットの出力ディレクトリ |
| `--chunk-size` | - | No | (最初のソースに準拠) | チャンクあたりのエピソード数を上書き |

### 使用例

#### 基本的な統合

```bash
rebake-cli merge /data/datasets -o /data/merged
```

#### カスタムチャンクサイズの指定

```bash
rebake-cli merge /data/datasets -o /data/merged --chunk-size 500
```

---

## 設定

設定ファイルの詳細な書式については、[設定リファレンス](configuration_ja.md) を参照してください。

クイックリンク:
- [Pipeline 設定](configuration_ja.md#pipeline-設定) - ステージ定義
- [Stage Reference](configuration_ja.md#stage-reference) - ステージごとのオプション
- [Robot Model 設定](configuration_ja.md#robot-model-設定) - トピックからfeatureへのマッピング

## 対応ファイル形式

### Rosbag 入力

| Extension | Format |
|-----------|--------|
| `.bag` | ROS 1 bag |
| `.mcap` | ROS 2 bag (MCAP) |

### 中間形式入力

| ディレクトリ構造 | Format |
|-----------------|--------|
| `parquet/` + `videos/` | rebake 中間形式 (Parquet + MP4) |

パイプライン設定では、入力形式に対応した正しいingestorを使用してください:

- `Rosbag1IngestorConfig` - `.bag` ファイル（ROS 1）用
- `Rosbag2IngestorConfig` - `.mcap` ファイル（ROS 2）用
- `ParquetVideoIngestorConfig` - 中間形式ディレクトリ（`parquet/` + `videos/`）用

> **注意**: ディレクトリに異なるファイル形式（`.bag` と `.mcap`）が混在している場合は、別々のディレクトリに分けるか、それぞれの形式に対応した設定ファイルで CLI を個別に実行してください。

> **注意**: バックアップファイル（`.orig.bag`、`.orig.mcap`）はサポートされていません。直接指定した場合、CLI はエラーを返します。

## トラブルシューティング

### よくあるエラーと解決策

#### ファイルが見つからない

```text
Error: failed to access /path/to/file
  Caused by: No such file or directory (os error 2)
```

**解決策**: ファイルパスを確認し、ファイルが存在することを確認してください。

#### 権限エラー

```text
Error: failed to access /path/to/file
  Caused by: Permission denied (os error 13)
```

**解決策**: `chmod +r <file>` で読み取り権限を付与してください。

#### 無効な YAML 設定

```text
Error: while parsing a block mapping, did not find expected key at line 2 column 1
```

**解決策**: `yamllint config/your_config.yaml` で YAML ファイルを検証してください。

#### rosbag ファイルが見つからない

```text
Error: no rosbag files found under directory: /path/to/dir
```

**解決策**: ディレクトリに `.bag` または `.mcap` ファイルが含まれていることを確認してください。

#### 無効な rosbag 拡張子

```text
Error: rosbag files must have .bag or .mcap extension: /path/to/file.txt
```

**解決策**: ファイルの拡張子を `.bag` または `.mcap` に変更するか、有効な rosbag ファイルを指定してください。

#### ステージの失敗

```text
Error: stage rosbag2_ingestor failed: missing required data: dataset in context
  Caused by: missing required data: dataset in context
```

**解決策**: `logs/pipeline.log` を確認し、詳細なエラートレースとステージ固有の情報を調査してください。

### ログ

CLI は詳細なログを `logs/pipeline.log` に出力します。stderr にもログを出力するには、以下を設定してください:

```bash
export REBAKE_LOG_STDERR=1
```

ログレベルを変更するには:

```bash
export RUST_LOG=debug  # 選択肢: error, warn, info, debug, trace
```
