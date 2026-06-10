# CLI

`rebake-cli` のコマンドは 3 つです。`export` は ROS bag を中間フォーマットへ、`run` は YAML のパイプラインで LeRobot データセットへ、`merge` は複数の LeRobot データセットを 1 つへまとめます。

このページはコマンドのリファレンスです。パイプラインとロボットモデルの書き方は[設定](configuration_ja.md)、コーデックは[エンコード](encoding_ja.md)、各 ROS bag に添える `meta.json` は[メタデータ](metadata_ja.md)、はじめての流れは[ガイド](guide_ja.md)、ビルドは [README](../README_ja.md) へ。

## どのコマンドを使うか

| 手元にあるもの | 欲しいもの | コマンド |
|---|---|---|
| ROS bag（`.bag` / `.mcap`） | 問い合わせ・再利用に向く中間フォーマット | [`export`](#export) |
| ROS bag、または中間フォーマット | LeRobot v2.1 データセット | [`run`](#run) |
| 複数の LeRobot v2.1 データセット | 1 つにまとまったデータセット | [`merge`](#merge) |

通常は `run` で ROS bag ごとに 1 データセットを書き出し、最後に `merge` で結合します。何度も設定を試すなら、先に `export` で中間フォーマットへ変換しておくと、元の ROS bag を読み直さずに `run` をやり直せます。

## export

ROS bag を、設定ファイルなしで[中間フォーマット](intermediate-format_ja.md)（トピックごとの Parquet + 動画）へ変換します。

```bash
rebake-cli export <PATH>... -o <DIR> [options]
```

出力は ROS bag ごとに `<DIR>/<uuid>/` へ作られます。中身は `parquet/` と `videos/` です。詳しいレイアウトは[中間フォーマット](intermediate-format_ja.md#レイアウト)を参照してください。

各 ROS bag の隣には [meta.json](metadata_ja.md) が必要です。`.bag` は ROS 1 として、`.mcap` は ROS 2 として読まれます。

### 基本オプション

| オプション | 既定 | 意味 |
|---|---|---|
| `<PATH>...` | 必須 | ROS bag ファイル、またはそれらを含むディレクトリ |
| `-o`, `--output <DIR>` | 必須 | 出力ディレクトリ。録画ごとに `<uuid>/` ができます。別名: `--output-dir` |
| `-j`, `--jobs <N>` | `1` | 並列に変換する ROS bag の数 |

### RGB オプション

| オプション | 既定 | 意味 |
|---|---|---|
| `--fps <N>` | `100` | RGB 動画のフレームレート |
| `--codec <CODEC>` | `av1` | RGB コーデック。`av1`、`h264`、`h265`、`av1_vaapi`、`h264_vaapi`、`h265_vaapi`、`av1_nvenc`、`h264_nvenc`、`h265_nvenc` |
| `--qp <N>` | コーデック別 | `h264_vaapi` と RGB の NVENC コーデックの QP を上書きします |
| `--video-config <FILE>` | なし | YAML の `VideoEncoderConfig` を使います。`--fps`、`--codec`、`--qp` とは併用できません |

`--qp` の既定値は `h264_vaapi=21`、`h264_nvenc=26`、`h265_nvenc=25`、`av1_nvenc=130` です。ソフトウェアコーデックと `av1_vaapi` / `h265_vaapi` では `--qp` は使いません。

### 深度オプション

| オプション | 既定 | 意味 |
|---|---|---|
| `--depth-codec <CODEC>` | `av1` | 深度コーデック。`ffv1`、`av1`、`av1_vaapi`、`h265_vaapi`、`av1_nvenc`、`h265_nvenc` |
| `--depth-fps <N>` | `30` | 深度動画のフレームレート |
| `--depth-max-mm <MM>` | `4092` | 非可逆の深度動画で残す最大距離（mm）。`ffv1` では無視されます |
| `--depth-qp <N>` | コーデック別 | 深度の NVENC コーデックの QP を上書きします |

`--depth-qp` の既定値は `h265_nvenc=10`、`av1_nvenc=20` です。`--depth-*` は RGB 設定から独立しているので、`--video-config` とも併用できます。コーデック選びと詳細な既定値は[エンコード](encoding_ja.md#export-の既定値)へ。

### 例

```bash
# ディレクトリ内の ROS bag を 8 並列で中間フォーマットへ
rebake-cli export ./yubi_recordings -o ./intermediate -j 8

# 学習時のデコードが速い H.264 で RGB を保存
rebake-cli export ./recording.mcap -o ./intermediate --fps 60 --codec h264

# 深度をロスレス FFV1 で保存
rebake-cli export ./yubi_recordings -o ./intermediate --depth-codec ffv1 --depth-fps 15

# RGB の詳細設定を YAML から読む
rebake-cli export ./yubi_recordings -o ./intermediate --video-config config/export/video_config_h265.yaml
```

どれかの録画が失敗しても残りは処理され、最後に失敗の一覧を出して終了コード 1 で終わります。

## run

YAML のパイプライン設定に従って入力を処理します。LeRobot データセットを作る常用コマンドです。

```bash
rebake-cli run <PATH>... -c <CONFIG> [-j <N>]
```

| オプション | 既定 | 意味 |
|---|---|---|
| `<PATH>...` | 必須 | 入力。読み方はパイプラインの最初の Ingestor で決まります |
| `-c`, `--config <FILE>` | 必須 | パイプライン設定（YAML） |
| `-j`, `--jobs <N>` | `1` | 並列に処理する入力の数 |

ROS bag の Ingestor で始まるパイプラインなら、入力は `.bag` / `.mcap` ファイルか、それらを含むディレクトリです。`ParquetVideoIngestorConfig` で始まるパイプラインなら、入力は中間フォーマットのディレクトリか、その親ディレクトリです。入力パスを YAML の中に書く必要はありません。

`run` に出力先フラグはありません。作業出力は設定の `work_dir` の下へ、LeRobot データセットは `LeRobotV21TransformerConfig.outdir/<uuid>/` へ書かれます。ふだん見るのは後者です。1 つの入力が失敗したとき残りを続けるかは、フラグではなく設定の `stop_on_error`（既定: `true`）で決まります。詳しくは[パイプライン設定](configuration_ja.md#パイプライン設定)へ。

```bash
# ROS bag のディレクトリを 8 並列で LeRobot へ
rebake-cli run ./yubi_recordings -c config/pipeline/yubi.yaml -j 8

# 書き出し済みの中間フォーマットから作り直す
rebake-cli run ./intermediate -c config/pipeline/yubi_from_parquet.yaml
```

## merge

`run` が録画ごとに作った LeRobot データセットを、1 つの学習用データセットへまとめます。エピソード番号の振り直し、タスクの重複除去、メタデータの統合を行います。動画は再エンコードせずコピーするので、画質は変わりません。

```bash
rebake-cli merge <SOURCE_DIR> -o <DIR> [--chunk-size <N>]
```

| オプション | 既定 | 意味 |
|---|---|---|
| `<SOURCE_DIR>` | 必須 | この直下で `meta/info.json` を持つディレクトリがデータセットとして検出されます |
| `-o`, `--output <DIR>` | 必須 | 統合後のデータセットの置き場 |
| `--chunk-size <N>` | 先頭のデータセットに合わせる | 1 チャンクあたりのエピソード数。別名: `--chunks-size` |

検出は名前の辞書順なので、結果のエピソード順は毎回同じです。統合できるのは 2 つ以上のデータセットで、`fps`、`codebase_version`、feature の構成（名前・型・形、動画ならコーデックとピクセルフォーマット）が一致している必要があります。`merge` に `-j` はありません。

```bash
rebake-cli merge ./lerobot_yubi -o ./lerobot_merged
```

成功すると `Merge completed successfully.` と表示されます。

## 共通の規約

### 入力パス

ROS bag 入力（`export` と、ROS bag Ingestor で始まる `run`）:

- 単一ファイルは `.bag` か `.mcap` であること。それ以外の拡張子は拒否されます。
- ディレクトリは再帰的に探索されます。見つかったパスはソートされ、重複は除かれます。
- `rosbag reindex` が残すバックアップ（`.orig.bag` / `.orig.mcap`）は、探索では飛ばされ、直接指定すると拒否されます。
- 1 回の `run` に `.bag` と `.mcap` は混ぜられません。パイプラインの Ingestor は 1 種類だからです。`export` はファイルごとに読み分けるので混在できます。

中間フォーマット入力（`ParquetVideoIngestorConfig` で始まる `run`）:

- `parquet/_metadata.parquet` と `parquet/_topic_type_map.parquet` の両方を持つディレクトリが中間フォーマットとみなされます。
- `videos/` の有無は判定に使われません。状態だけの中間フォーマットも認識されます。
- 渡したディレクトリ自体が中間フォーマットでなければ、その下を探索します。

### 並列実行

`-j` は入力単位の並列数です。`export` と `run` は、並列数が 2 以上のとき入力ごとに別プロセスで処理します。NVENC を使う場合、GPU ドライバ側の同時エンコード上限を超えると Encoder が失敗し始めるので、その場合は `-j` を下げてください。

### ログ

詳しいログは `logs/pipeline.log` に書かれます。

| 環境変数 | 既定 | 意味 |
|---|---|---|
| `REBAKE_LOG_DIR` | `logs` | `pipeline.log` を置くディレクトリ |
| `RUST_LOG` | `warn` | ログフィルタ。`info` か `debug` にするとステージ単位の情報が増えます |
| `REBAKE_LOG_STDERR` | 未設定 | `0` 以外の値を入れると stderr にもログを出します |

```bash
RUST_LOG=info REBAKE_LOG_STDERR=1 rebake-cli run ./yubi_recordings -c config/pipeline/yubi.yaml
```

`pipeline.log` は追記され、ローテーションされません。肥大したら削除してください。

### ヘルプと終了コード

`rebake-cli --help`、`rebake-cli <command> --help`、`rebake-cli --version` が使えます。失敗すると終了コード 1 で終わり、画面には `Error:` と `Caused by:` が表示されます。

```text
Error: <何が失敗したか>
  Caused by: <原因>
```

## うまくいかないとき

| エラー文に含まれる断片 | 原因 | 直し方 |
|---|---|---|
| `input path must be provided` | `run` に入力パスが無い | `rebake-cli run <PATH> -c <CONFIG>` の形にする |
| `no rosbag files found under directory` | ディレクトリに `.bag` / `.mcap` が無い | パスを確認する |
| `rosbag files must have .bag or .mcap extension` | 対応しない拡張子のファイルを直接指定した | ROS bag ファイルかディレクトリを指定する |
| `ignoring backup rosbag produced by rosbag reindex` | `.orig.bag` / `.orig.mcap` を直接指定した | `.orig` の付かない方の ROS bag を使う |
| `bundle input must be a directory` | 中間フォーマット入力にファイルを渡した | 中間フォーマットのディレクトリを渡す |
| `no rebake bundle directories found` | `run` の入力が中間フォーマットではない | `parquet/_metadata.parquet` と `parquet/_topic_type_map.parquet` を持つディレクトリ、またはその親を渡す |
| `failed to read meta.json` | ROS bag の隣に `meta.json` が無い | [メタデータ](metadata_ja.md)の形式で `meta.json` を置く |
| YAML の解析エラー（`while parsing ...`） | 設定ファイルの書式が壊れている | 行・列番号の指す箇所を直す |
| `expected at least 2 dataset directories` | `merge` の入力にデータセットが 2 つ未満 | `meta/info.json` を持つデータセット群の親ディレクトリを渡す |
| `FPS mismatch` / `codebase_version mismatch` / `feature ... mismatch` | `merge` するデータセットの構成が揃っていない | 同じパイプライン設定で作り直す |

ステージ順序や `missing required data` 系は[設定の表](configuration_ja.md#うまくいかないとき)、メタデータ起因は[メタデータの表](metadata_ja.md#うまくいかないとき)へ。
