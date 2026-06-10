# 設定

`rebake-cli run` は 2 つの YAML で動きます。パイプライン設定が「どう作るか」（ステージの並び）、ロボットモデルが「何を出すか」（どのトピックをどの feature にするか）です。feature は完成したデータセットで学習コードが読む列・動画の名前です。

このページは両方のリファレンスです。コーデックの設定値は[エンコード](encoding_ja.md)、`meta.json` は[メタデータ](metadata_ja.md)、はじめての方は[ガイド](guide_ja.md)へ。

## パイプライン設定

最小のパイプライン設定です。YUBI の ROS 2 bag を読み、毎秒 30 行に揃え、LeRobot v2.1 を書きます。このまま保存して動きます。

```yaml
work_dir: "./orchestrator_work"
stage_configs:
  - Rosbag2IngestorConfig: {}
  - ZeroOrderHoldTimeSynchronizerConfig:
      fps: 30
  - LeRobotV21TransformerConfig:
      outdir: "./lerobot_yubi"
      robot_model: "./config/robot_model/yubi.yaml"
      video_config:
        fps: 30        # 同期の fps と同じ値に（違ってもエラーにならず、ずれたデータセットになります）
```

トップレベルのキー:

| キー | 型 | 既定 | 説明 |
|---|---|---|---|
| `work_dir` | パス | 必須 | 作業出力の置き場。入力 1 つにつき 1 フォルダできます |
| `stage_configs` | リスト | 必須 | ステージの順序付きリスト。各項目は `ステージ名: {そのステージのフィールド}` |
| `save_contexts` | true/false | `false` | 各ステージ直後のテーブルを `work_dir` に Parquet で保存。期待した列ができているか確かめるときに |
| `stop_on_error` | true/false | `true` | 1 つの入力が失敗したら残りを止める。`false` で全部流し切って最後にまとめて報告 |
| `video_cache_root` | パス | `./video_cache` | 動画キャッシュの置き場 |

設定内の相対パス（`robot_model` や `outdir`）は、設定ファイルの場所ではなく `rebake-cli` を実行したディレクトリから解決されます。

ステージ名は YAML に書く文字列そのもの（`Rosbag2IngestorConfig` など）で選びます。知らない名前は読み込み時に失敗します。フィールドの綴り間違いは多くのステージで素通りしますが、動画・コーデック設定だけは未知のキーを拒否します。

## ステージの並べ方

順序がすべてです。各ステージは前段までが作ったデータに対して働くので、頼りになる並びは:

```text
Ingestor → raw Enricher → Synchronizer → synced Enricher → Transformer / Exporter
```

Synchronizer の前に置くのは、元のメッセージの時刻を必要とするステージだけです。代表は `TfBufferEnricherConfig`（元の更新時刻で TF を組み立てるため）。[HSR 専用の指令再構成 2 つ](#hsr-専用のステージ)も Synchronizer の前です。残りの Enricher は Synchronizer の後ろに置きます。

順序を間違えると、そのステージで `missing required data: ...` という実行時エラーになります。エラー文と直し方は[うまくいかないとき](#うまくいかないとき)へ。

### Synchronizer が何をするか

等レートの Synchronizer（ZeroOrderHold / NearestNeighbor）は、`fps` 間隔の時間軸を引き、全トピックの値を各時刻に割り当てます。時間軸は「全トピックの開始時刻のうち最も遅いもの」から「終了時刻のうち最も遅いもの」までです。早く止まったトピックは最後の値を持ち越したまま末尾まで続きます。エピソード終盤で値が平坦に見えるときは、これが起きています。

Synchronizer は全トピックに 2 列を足します。`synched_timestamp_ns` がその行の時刻、`is_fresh` がその時刻に新しいメッセージが来ていたかどうかです（持ち越しなら false）。`is_fresh` を feature にしておくと、学習側が実測と持ち越しを区別できます。

## ステージリファレンス

各ステージは「何をするか、書く YAML、フィールド、注意」の順で並べています。YAML 例はどれも `stage_configs:` の 1 項目です。フィールド表の「既定」列が「必須」のものは省略できません。

### Ingestor

最初の 1 段は必ず Ingestor です。ROS bag を読むか、書き出し済みの中間フォーマットを読み直すかを選びます。

---

#### Rosbag2IngestorConfig

ROS 2 の `.mcap` を読み、全トピックをテーブルにします。

```yaml
- Rosbag2IngestorConfig: {}
```

| フィールド | 型 | 既定 | 説明 |
|---|---|---|---|
| `require_metadata` | true/false | `true` | ROS bag の隣の `meta.json` を読む。`false` にできるのは[メタデータを使うステージ](metadata_ja.md#どのステージに要るか)が無いパイプラインだけ |

画像・深度・点群の大きなペイロードはテーブルの外に出され、行には参照番号の `index` 列が残ります（行き先は[中間フォーマット](intermediate-format_ja.md#画像と深度と点群のペイロード)と同じ仕組みです）。

---

#### Rosbag1IngestorConfig

ROS 1 の `.bag` を読みます。フィールドも挙動も Rosbag2IngestorConfig と同じです。

```yaml
- Rosbag1IngestorConfig: {}
```

---

#### ParquetVideoIngestorConfig

書き出し済みの[中間フォーマット](intermediate-format_ja.md)を読み直します。ROS bag をもう一度デコードせずに、別の設定でデータセットを作り直すための入口です。`meta.json` は要りません（中間フォーマットの中に入っています）。

```yaml
- ParquetVideoIngestorConfig: {}
```

| フィールド | 型 | 既定 | 説明 |
|---|---|---|---|
| `input_dir` | パス | CLI の入力パス | 通常は書きません。`rebake-cli run <パス>` の入力がそのまま使われます |

### Synchronizer

全トピックを 1 本の時間軸に揃えます。3 種の違いは「各時刻にどの値を選ぶか」だけです。迷ったら ZeroOrderHold。

---

#### ZeroOrderHoldTimeSynchronizerConfig

各時刻に、そのトピックの「その時点までの最後の値」を入れます。未来を見ないので、関節位置のような状態量に安全です。

```yaml
- ZeroOrderHoldTimeSynchronizerConfig:
    fps: 30
```

| フィールド | 型 | 既定 | 説明 |
|---|---|---|---|
| `fps` | 整数 | 必須 | 1 秒あたりの行数。そのままデータセットの再生レートになります。Transformer の `video_config.fps` を同じ値に |

---

#### NearestNeighborTimeSynchronizerConfig

各時刻に「最も時刻が近い値」を入れます。少し未来のメッセージを使うことがあるので、未来を見てはいけない量には ZeroOrderHold を使ってください。

```yaml
- NearestNeighborTimeSynchronizerConfig:
    fps: 30
```

| フィールド | 型 | 既定 | 説明 |
|---|---|---|---|
| `fps` | 整数 | 必須 | 同上 |

---

#### TimestampMergeTimeSynchronizerConfig

全トピックの元タイムスタンプの和集合を時間軸にします。等間隔にはなりません。フィールドはありません。

```yaml
- TimestampMergeTimeSynchronizerConfig: {}
```

`fps` を持たず `is_fresh` も足さないので、等レート前提の変換とは普通組み合わせません。元の時刻を保ったまま中間フォーマットに書き出すような用途向けです。

### Enricher

Enricher は、既にあるトピックから新しいトピックや列を作ります。

---

#### TfBufferEnricherConfig

`/tf`（あれば `/tf_static` も）を読み、任意のフレーム対の変換を任意の時刻で引けるバッファを `/tf_buffer` トピックに作ります。フィールドはありません。

```yaml
- TfBufferEnricherConfig: {}
```

Synchronizer の前に置きます。`TfChainEnricherConfig` がこのバッファを使います。

---

#### TfChainEnricherConfig

指定したフレーム対の姿勢を計算し、`/tf_chain` トピックに書きます。

```yaml
- TfChainEnricherConfig:
    frame_pairs:
      - source: base_link
        target: hand_link
      - source: base_link
        target: camera_link
```

| フィールド | 型 | 既定 | 説明 |
|---|---|---|---|
| `frame_pairs` | リスト | 必須 | `source` と `target` の座標フレーム名の対 |

各対には `transform`（並進 x, y, z と回転クォータニオン x, y, z, w）と `is_fresh`（経路上のどれかの変換がその時刻に更新されていたら true）が入ります。ロボットモデルからは `/base_link/hand_link/transform` のような[フィールドパス](#フィールドパス)で取り出します。

---

#### DeltaJointPositionEnricherConfig

`position` 列を持つトピックに、前の行との差を `delta_position` 列として足します。最初の行は 0 です。

```yaml
- DeltaJointPositionEnricherConfig:
    topic_names:
      - /joint_states
```

| フィールド | 型 | 既定 | 説明 |
|---|---|---|---|
| `topic_names` | リスト | 必須 | `position` が float のリストであるトピック |

該当しないトピックはエラーにならず素通しです。列が現れないときは、まずトピック名の綴りを疑ってください。

---

#### DeltaTransformEnricherConfig

トピック内の transform 構造（入れ子も含む）の隣に、前の行からの変化を `delta_transform` として足します。

```yaml
- DeltaTransformEnricherConfig:
    topic_names: ["/tf_chain"]
    delta_reference_frame: previous_target_frame
```

| フィールド | 型 | 既定 | 説明 |
|---|---|---|---|
| `topic_names` | リスト | 必須 | transform を探すトピック。普通は `["/tf_chain"]` |
| `delta_reference_frame` | `previous_target_frame` か `source_frame` | 必須 | 変化をどの座標系で表すか。アクションに使うなら `previous_target_frame`（直前の target フレーム座標で見た運動）。`source_frame` は source フレーム座標での成分ごとの差 |

---

#### ShiftEnricherConfig

トピックを複製し、行を `shift_steps` だけずらした新トピックを作ります。`1` なら各行に「次の行の値」が入ります。観測の列から「次にとるべき行動」の列を作る定番です。

```yaml
- ShiftEnricherConfig:
    source_topic: /joint_states
    output_topic: /joint_states/action
    shift_steps: 1
```

| フィールド | 型 | 既定 | 説明 |
|---|---|---|---|
| `source_topic` | 文字列 | 必須 | 元のトピック。変更されません |
| `output_topic` | 文字列 | 必須 | 新しいトピックの名前 |
| `shift_steps` | 整数 | 必須 | ずらす行数。正で未来、負で過去 |
| `fill_strategy` | `edge` か `zero` | `edge` | 端で空く行の埋め方。`edge` はいちばん近い実値、`zero` は数値列だけ 0（それ以外は `edge` と同じ） |

時刻系の列（`synched_timestamp_ns`、`timestamp_ns`、`is_fresh`）はずれません。値だけが動きます。

---

#### UuidEnricherConfig

全トピックに録画の UUID を `rosbag_uuid` 列として足します。データセットを統合したあとも行の出どころを辿れます。フィールドはありません。

```yaml
- UuidEnricherConfig: {}
```

メタデータが無いと実行全体が止まります。`meta.json` なしのパイプラインには入れないでください。

---

#### HSR 専用のステージ

Toyota HSR の録画専用の指令再構成です。他のロボットでは使いません。どちらもフィールドはなく、対象トピックが既にあれば何もしません。Synchronizer の前に置きます。

`HandCommandEnricherConfig` は、録られていないグリッパ指令 `/hsrb/gripper_controller/command` をサーボの実測値から再構成します。`HeadCommandEnricherConfig` は、頭部指令 `/hsrb/head_trajectory_controller/command` を `/hsrb/joint_states` から再構成します。

```yaml
- HandCommandEnricherConfig: {}
- HeadCommandEnricherConfig: {}
```

### Encoder / Decoder

通常、ここのステージを自分で足す必要はありません。Transformer と Exporter は自分の `video_config` で動画を作ります。足すのは、動画を先に作っておく、画像を 1 枚ずつ取り出す、といった用途のときだけです。コーデックの設定値はすべて[エンコード](encoding_ja.md)にあります。

---

#### VideoEncoderConfig

カメラトピックを MP4 にし、後段が参照できるように登録します。フィールド（`fps`、`gop`、`crf`、`scaling`、`resize`、`codec_config`）は[エンコード](encoding_ja.md#rgb-動画)へ。

```yaml
- VideoEncoderConfig:
    fps: 30
    codec_config:
      codec: AV1
```

---

#### DepthVideoConfig

`compressedDepth` トピックを深度動画にします。距離をミリメートルのまま（FFV1）か、10 bit に量子化して（それ以外）保ちます。

```yaml
- DepthVideoConfig:
    fps: 30
    depth_max_mm: 4092
    codec_config:
      codec: FFV1
```

| フィールド | 型 | 既定 | 説明 |
|---|---|---|---|
| `depth_max_mm` | 整数 | `4092` | 量子化で残す最大距離。FFV1 では無視されます |
| `fps` | 整数 | `30` | 深度動画のフレームレート |
| `codec_config` | オブジェクト | AV1 | [エンコード](encoding_ja.md#深度動画)の深度コーデックから選ぶ |

---

#### ImageEncoderConfig

カメラの各フレームを動画にせず、画像ファイルのまま書き出します（`0.jpg`、`1.jpg`、…）。フィールドはありません。

```yaml
- ImageEncoderConfig: {}
```

---

#### DepthImageEncoderConfig

深度の各フレームを 16 bit の生ファイル（`0.bin`、…）と寸法を書いた `meta.json` で書き出します。フィールドはありません。

```yaml
- DepthImageEncoderConfig: {}
```

---

#### VideoDecoderConfig

登録済みの動画をメモリ上のフレームに戻します。全フレームを抱えるので重く、通常は不要です（変換は動画から直接フレームを読みます）。フィールドはありません。

```yaml
- VideoDecoderConfig: {}
```

### Transformer

---

#### LeRobotV21TransformerConfig

LeRobot v2.1 データセットを書きます。普通は最後の段です。`meta.json` のセグメントを同期後の時間軸に重ねてエピソードにし、テーブル・動画・メタデータ一式を出力します。

```yaml
- LeRobotV21TransformerConfig:
    outdir: "./lerobot_yubi"
    robot_model: "./config/robot_model/yubi.yaml"
    video_config:
      fps: 30
      codec_config:
        codec: AV1
    separate_per_primitive: false
```

| フィールド | 型 | 既定 | 説明 |
|---|---|---|---|
| `outdir` | パス | 必須 | この下に録画ごとの `<uuid>/` ができます |
| `robot_model` | パスかインライン | 必須 | [ロボットモデル](#ロボットモデル)のファイルパス、または entry のリストをそのまま |
| `video_config` | オブジェクト | AV1・fps 100 | カメラ feature のエンコード方法。`fps` は同期の値に合わせる。既定の 100 のままだと、30 fps の同期に対して動画だけ速いデータセットが黙ってできます |
| `separate_per_primitive` | true/false | `false` | `false` は全セグメントをつないで 1 エピソード（境界の行に `next.done` が立つ）。`true` はセグメントごとに 1 エピソード |

失敗する条件は 4 つ。メタデータが無い、ロボットモデルが挙げたトピックがデータに無い、セグメントが時間軸とひとつも重ならない、`Video` feature の動画素材が無い、です。

### Exporter

---

#### ParquetVideoExporterConfig

[中間フォーマット](intermediate-format_ja.md)を書きます。`rebake-cli export` と同じ出力を、パイプラインの途中の状態（同期後など）からも作れます。

```yaml
- ParquetVideoExporterConfig:
    output_dir: "./intermediate"
    depth_config:
      codec_config:
        codec: FFV1
```

| フィールド | 型 | 既定 | 説明 |
|---|---|---|---|
| `output_dir` | パス | 必須 | この下に録画ごとの `<uuid>/` ができます |
| `video_config` | オブジェクト | AV1 | カメラトピックのエンコード方法 |
| `depth_config` | オブジェクト | なし | 深度トピックのエンコード方法。省略すると深度は保存されません |

## ロボットモデル

ロボットモデルは平らな YAML リストで、1 項目が 1 つの feature を宣言します。

```yaml
- type: Parquet
  topic: /joint_states
  field: /position
  feature: observation.state
  names: [shoulder, elbow, wrist]

- type: Video
  topic: /camera/image_raw/compressed
  feature: observation.image.head
  names: [height, width, channel]
```

entry は `type` で決まる 3 種類です。

| `type` | キー | 働き |
|---|---|---|
| `Parquet` | `topic`、`field`、`feature`、任意で `names`、`description` | トピックの 1 フィールドをデータセットの列にする |
| `Video` | `topic`、`feature`、任意で `names`、`description` | カメラトピックをデータセットの動画にする |
| `Image` | `topic`、`feature`、任意で `names`、`description` | feature を登録だけして、ファイルは書かない。カメラには `Video` を使う |

`names` は feature の各次元の名前です。書いておくと個数が実データの幅と照合され、トピックの形が変わったときに気づけます。`description` は自由文で、そのままデータセットに載ります。

実データのトピック名と列名を確かめるには、一度 [export](cli_ja.md#export) して Parquet を覗くのが確実です（[ガイド手順 2](guide_ja.md#2-ros-bag-の中身を見る)）。

`robot_model` はパスでもインラインでも書けます。

```yaml
robot_model: "./config/robot_model/my_robot.yaml"
```

```yaml
robot_model:
  - type: Parquet
    topic: /joint_states
    field: /position
    feature: observation.state
```

### フィールドパス

`Parquet` entry の `field` は、メッセージの中のどの値を取るかを示すパスです。

- `/` で始まり、最初の区切りは列名です。
- 名前の区切りは struct のフィールドを選びます: `/wrench/force`
- 整数はリストの要素を選びます（負数は後ろから）: `/points/0/positions`
- `start:end` はリストを切り出します。以降のパスは切り出した各要素に適用されます。

| パス | 取れるもの |
|---|---|
| `/position` | `position` 列 |
| `/points/0/positions` | `points` の先頭要素の `positions` |
| `/base_link/hand_link/transform` | `/tf_chain` の姿勢 |
| `/base_link/hand_link/is_fresh` | その姿勢の鮮度フラグ |

## 出力レイアウト

Transformer は `outdir/<uuid>/` に LeRobot v2.1 を書きます。

```text
<outdir>/<uuid>/
├── data/
│   └── chunk-000/
│       ├── episode_000000.parquet
│       └── episode_000001.parquet     # セグメントごとに分けたときのみ複数
├── videos/
│   └── chunk-000/
│       └── <feature>/                  # Video feature ごとに 1 フォルダ
│           └── episode_000000.mp4
└── meta/
    ├── info.json
    ├── episodes.jsonl
    ├── episodes_stats.jsonl
    └── tasks.jsonl
```

チャンクは常に `chunk-000` の 1 つです。`info.json` の `splits` は `train` だけで、val / test の分割は書きません。出力は LeRobot v2.1 です（LeRobot 本家は v3 系に進んでいますが、rebake が書くのは v2.1）。

## うまくいかないとき

エラー文の `... in context` は、「前段までに作られているはずのデータが無い」という意味です。

| エラー文に含まれる断片 | 原因 | 直し方 |
|---|---|---|
| `missing required data: dataset` | 先頭が Ingestor になっていない | Ingestor を 1 段目に置く |
| `missing required data: rosbag_path` | ROS bag の入力が渡っていない | `rebake-cli run <パス> -c ...` の入力を確認 |
| `missing required data: bundle_root` | 中間フォーマットの入力が渡っていない | 同上 |
| `missing required data: tf_buffer` | TfChain の前に TfBuffer が無い | `TfBufferEnricherConfig` を前に足す |
| `missing required data: fps` | 等レートの同期が無い | ZeroOrderHold か NearestNeighbor を入れる |
| `unknown variant` | ステージ名の綴り間違い | このページの見出しと突き合わせる |
| エラーは出ないが列・トピックが増えていない | Enricher の対象が見つからず素通しされた | トピック名の綴りを確認。`save_contexts: true` で各段の出力を見る |

メタデータ起因のエラー（`failed to read meta.json`、`no segments overlap` など）は[メタデータの表](metadata_ja.md#うまくいかないとき)、コーデック起因は[エンコードの表](encoding_ja.md#うまくいかないとき)へ。
