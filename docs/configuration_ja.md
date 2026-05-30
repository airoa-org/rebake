# 設定リファレンス

rebake は2種類の YAML 設定ファイルを使用します：

1. **Pipeline 設定** - 処理ステージを定義する
2. **Robot Model 設定** - ROS トピックを LeRobot の feature にマッピングする

## 目次

- [クイックスタート](#クイックスタート)
- [Pipeline 設定](#pipeline-設定)
- [ステージの実行順序](#ステージの実行順序)
- [Metadata Requirement](#metadata-requirement)
- [ステージリファレンス](#ステージリファレンス)
  - [Ingest ステージ](#ingest-ステージ)
  - [Synchronize ステージ](#synchronize-ステージ)
  - [Enrich ステージ](#enrich-ステージ)
  - [Encode ステージ](#encode-ステージ)
  - [Decode ステージ](#decode-ステージ)
  - [Transform ステージ](#transform-ステージ)
  - [Export ステージ](#export-ステージ)
- [Codec Configuration Details](#codec-configuration-details)
  - [用語集](#用語集)
- [Enrich されたデータの使用](#enrich-されたデータの使用)
- [Robot Model 設定](#robot-model-設定)
- [出力構造](#出力構造)
- [トラブルシューティング](#トラブルシューティング)

## クイックスタート

### 最小構成の Pipeline

ROS 2 bag を LeRobot フォーマットに変換するための最小構成の pipeline です：

この例はそのまま使える完全な pipeline 設定ファイルです。そのまま保存して実行できます。

```yaml
work_dir: "./output"
stage_configs:
  - Rosbag2IngestorConfig: {}
  - ZeroOrderHoldTimeSynchronizerConfig:
      fps: 10
  - LeRobotV21TransformerConfig:
      outdir: "./lerobot_output"
      robot_model: "./config/robot_model/myrobot.yaml"
```

### よくある Pipeline パターン

このセクションの例も完全な pipeline 設定ファイルです。

**パターン 1: 基本的な Pipeline（TF 変換なし）**

```yaml
work_dir: "./output"
stage_configs:
  - Rosbag2IngestorConfig: {}
  - ZeroOrderHoldTimeSynchronizerConfig:
      fps: 10
  - LeRobotV21TransformerConfig:
      outdir: "./output"
      robot_model: "./config/robot_model/myrobot.yaml"
```

**パターン 2: TF 変換付き Pipeline**

```yaml
work_dir: "./output"
stage_configs:
  # 1. 取り込み
  - Rosbag2IngestorConfig: {}

  # 2. Enrich（同期前） - TF バッファの構築
  - TfBufferEnricherConfig: {}

  # 3. 同期
  - ZeroOrderHoldTimeSynchronizerConfig:
      fps: 10

  # 4. Enrich（同期後） - 変換とデルタの計算
  - TfChainEnricherConfig:
      frame_pairs:
        - source: base_link
          target: hand_link
  - DeltaTransformEnricherConfig:
      topic_names: ["/tf_chain"]
      delta_reference_frame: previous_target_frame
  - ShiftEnricherConfig:
      source_topic: "/joint_states"
      output_topic: "/joint_states/action"
      shift_steps: 1

  # 5. 変換
  - LeRobotV21TransformerConfig:
      outdir: "./output"
      robot_model: "./config/robot_model/myrobot.yaml"
```

---

## Pipeline 設定

配置場所: `config/pipeline/`

### 構造

```yaml
work_dir: "./output"          # 中間結果の出力ディレクトリ
save_contexts: true           # 各ステージの出力を保存する（デバッグに有用）
stage_configs:                # 実行するステージのリスト（順序通り）
  - StageConfigName:
      parameter: value
```

### パラメータ

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| work_dir | string | Yes | 中間出力用ディレクトリ |
| save_contexts | bool | No | 各ステージの出力を保存する（デフォルト: false） |
| stop_on_error | bool | No | 最初の rosbag 処理が失敗した場合、残りの rosbag の処理を中止する（デフォルト: true） |
| stage_configs | list | Yes | ステージ設定の順序付きリスト |

---

## ステージの実行順序

ステージは特定の順序で配置する必要があります。以下のルールに従ってください：

### 基本的な順序

```
Ingest → Enrich (同期前) → Synchronize → Enrich (同期後) → Transform
```

### Enricher の実行タイミング

Enricher は同期処理に対して適切なタイミングで実行する必要があります：

**同期前に実行**（データの精度を保持するため）：

| Enricher | 理由 |
|----------|------|
| TfBufferEnricherConfig | 生のタイムスタンプから TF バッファを構築する |
| HandCommandEnricherConfig | HSR 専用 - 生のサーボデータから合成する |
| HeadCommandEnricherConfig | HSR 専用 - 生のジョイントデータから合成する |

**同期後に実行**（同期済みタイムスタンプが必要）：

| Enricher | 理由 |
|----------|------|
| TfChainEnricherConfig | 同期済みタイムスタンプで変換を計算する |
| DeltaJointPositionEnricherConfig | フレーム間のデルタを計算する |
| DeltaTransformEnricherConfig | フレーム間のデルタを計算する |
| ShiftEnricherConfig | トピックのシフトされたコピーを作成する（例：アクションラベル用） |

### Pipeline の順序例

```yaml
stage_configs:
  # 1. 取り込み
  - Rosbag2IngestorConfig: {}

  # 2. Enrich（同期前） - TF バッファの構築
  - TfBufferEnricherConfig: {}

  # 3. 同期 - 固定 FPS にリサンプリング
  - ZeroOrderHoldTimeSynchronizerConfig:
      fps: 10

  # 4. Enrich（同期後） - 変換とデルタの計算
  - TfChainEnricherConfig:
      frame_pairs:
        - source: base_link
          target: hand_link
  - DeltaJointPositionEnricherConfig:
      topic_names: ["/joint_states"]
  - DeltaTransformEnricherConfig:
      topic_names: ["/tf_chain"]
      delta_reference_frame: previous_target_frame
  - ShiftEnricherConfig:
      source_topic: "/joint_states"
      output_topic: "/joint_states/action"
      shift_steps: 1

  # 5. 変換
  - LeRobotV21TransformerConfig:
      outdir: "./lerobot_output"
      robot_model: "./config/robot_model/myrobot.yaml"
```

---

## Metadata Requirement

一部のステージは airoa メタデータ（ingestor が読み込む `meta.json`）を必要とします。必要とするステージはデータセット UUID やセグメント/ラベル情報を読み取り、無効化できません。ingestor は既定で `meta.json` を読み込みます（`require_metadata: true`）。`require_metadata: false` でスキップできますが、下表で **Yes** のステージを使わない pipeline に限ります。

| Stage | airoa メタデータが必要? | 備考 |
|-------|-----------------------|------|
| Rosbag1IngestorConfig / Rosbag2IngestorConfig | 任意（既定で**必要**） | 既定 `require_metadata: true`。プレーンな rosbag では `false` に設定 |
| ParquetVideoIngestorConfig | バンドルから読み込む | Parquet+Video バンドルから復元 |
| ZeroOrderHold / NearestNeighbor / TimestampMerge | 不要 | |
| TfBufferEnricher / TfChainEnricher | 不要 | |
| DeltaJointPositionEnricher / DeltaTransformEnricher / ShiftEnricher | 不要 | |
| HandCommandEnricher / HeadCommandEnricher | 不要 | |
| UuidEnricherConfig | **必要** | `rosbag_uuid` を追加。メタデータが無ければ no-op |
| ImageEncoderConfig / DepthImageEncoderConfig | 不要 | `output_dir` 配下に書き出し |
| VideoEncoderConfig / DepthVideoConfig | **必要** | 出力パスにデータセット UUID が必要 |
| VideoDecoderConfig | 不要 | |
| ParquetVideoExporterConfig | **必要** | UUID サブディレクトリ + バンドルメタデータを書き出し |
| LeRobotV21TransformerConfig | **必要** | UUID・セグメント・ラベルが必要 |

---

## ステージリファレンス

以下の例は個別の `stage_configs` エントリを示しており、完全な pipeline 設定ファイルではありません。`work_dir` を含む完全な設定ファイルのトップレベルの `stage_configs:` キーの下に追加してください。

## Ingest ステージ

### Rosbag1IngestorConfig

ROS 1 bag ファイル（.bag）を読み込み、すべてのトピックをロードします。

**パラメータ**

| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| require_metadata | bool | true | meta.json ファイルを必須とする。テスト時は false に設定。 |

**例**

```yaml
# デフォルト: meta.json が必要
- Rosbag1IngestorConfig: {}

# meta.json なしでテストする場合
- Rosbag1IngestorConfig:
    require_metadata: false
```

---

### Rosbag2IngestorConfig

ROS 2 bag ファイル（.mcap）を読み込み、すべてのトピックをロードします。

**パラメータ**

| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| require_metadata | bool | true | meta.json ファイルを必須とする。テスト時は false に設定。 |

**例**

```yaml
# デフォルト: meta.json が必要
- Rosbag2IngestorConfig: {}

# meta.json なしでテストする場合
- Rosbag2IngestorConfig:
    require_metadata: false
```

---

### ParquetVideoIngestorConfig

rebake の中間形式データ（`parquet/` + `videos/`）を読み込み、データセット、メタデータ、ビデオレジストリを `Context` に復元します。

rosbag を再度取り込む代わりに、以前エクスポートした Parquet/MP4 データから処理を継続したい場合に使用します。

**パラメータ**

| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| input_dir | string | unset | 入力ディレクトリの任意オーバーライド。通常の `rebake-cli run` 使用時には位置引数の入力パスが自動的に注入されるため、YAML で設定する必要はありません。 |

**例**

```yaml
# 一般的な CLI 使用法: 入力パスは `rebake-cli run <PATH> -c ...` から取得される
- ParquetVideoIngestorConfig: {}

# ライブラリから直接使用する場合や、パスを固定する場合の override
- ParquetVideoIngestorConfig:
    input_dir: "./exported_dataset/123e4567-e89b-12d3-a456-426614174000"
```

---

## Synchronize ステージ

### ZeroOrderHoldTimeSynchronizerConfig

すべてのトピックを固定フレームレートにリサンプリングします。各タイムスタンプで最後の既知の値を使用します。

**パラメータ**

| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| fps | int | (required) | 目標フレームレート |

**例**

```yaml
- ZeroOrderHoldTimeSynchronizerConfig:
    fps: 10
```

---

### NearestNeighborTimeSynchronizerConfig

すべてのトピックを固定フレームレートにリサンプリングします。各タイムスタンプで最も近い値を使用します。

**パラメータ**

| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| fps | int | (required) | 目標フレームレート |

**例**

```yaml
- NearestNeighborTimeSynchronizerConfig:
    fps: 10
```

---

### TimestampMergeTimeSynchronizerConfig

すべてのトピックをマージされたタイムラインに揃えます。固定フレームレートへのリサンプリングは行いません。

**パラメータ**

パラメータなし。

**例**

```yaml
- TimestampMergeTimeSynchronizerConfig: {}
```

**注意事項**

- fps を設定しません（不均一なタイムライン）
- 元のタイムスタンプをすべて保持したい場合に使用してください

---

## Enrich ステージ

### TfBufferEnricherConfig

/tf から TF バッファを構築し、/tf_static があればそれも取り込みます。

/tf メッセージはスパースに配信されるため、すべてのタイムスタンプですべてのフレームの変換が揃っているとは限りません。このステージはその欠けを補完し、任意のタイムスタンプで任意のフレームの変換を参照できる状態にします。構築されたバッファは TfChainEnricher が変換チェーンを計算するために必要です。

**パラメータ**

パラメータなし。

**例**

```yaml
- TfBufferEnricherConfig: {}
```

**注意事項**

- **同期前に実行** - 生のタイムスタンプから TF バッファを構築します
- TfChainEnricherConfig の前に実行する必要があります

---

### TfChainEnricherConfig

フレームペア間の変換を計算します。/tf_chain トピックを作成します。

**パラメータ**

| Parameter | Type | Description |
|-----------|------|-------------|
| frame_pairs | list | {source, target} フレームペアのリスト |

**例**

```yaml
- TfChainEnricherConfig:
    frame_pairs:
      - source: base_link
        target: hand_link
      - source: base_link
        target: camera_link
```

**Robot Model 設定例**

```yaml
- type: Parquet
  topic: /tf_chain
  field: /base_link/hand_link/transform
  feature: observation.end_effector_pose
  names: [x, y, z, qx, qy, qz, qw]
```

**注意事項**

- **同期後に実行** - 同期済みタイムスタンプで変換を計算します
- 事前に TfBufferEnricherConfig を実行する必要があります
- フィールドパス: `/tf_chain/{source}/{target}/transform` と `/tf_chain/{source}/{target}/is_fresh`

---

### DeltaJointPositionEnricherConfig

関節位置のデルタ（前フレームとの差分）を計算します。

**パラメータ**

| Parameter | Type | Description |
|-----------|------|-------------|
| topic_names | list | JointState メッセージを含むトピック |

**例**

```yaml
- DeltaJointPositionEnricherConfig:
    topic_names:
      - /joint_states
```

**Robot Model 設定例**

```yaml
- type: Parquet
  topic: /joint_states
  field: /delta_position
  feature: action.delta_joint_position
  names: [joint1, joint2, joint3]
```

**注意事項**

- **同期後に実行** - デルタ計算には均一なフレームレートが必要です
- 指定されたトピックに `/delta_position` フィールドを追加します

---

### DeltaTransformEnricherConfig

変換のデルタ（前フレームとの差分）を計算します。

**パラメータ**

| Parameter | Type | Description |
|-----------|------|-------------|
| topic_names | list | 変換データを含むトピック |
| delta_reference_frame | string | 必須。body-frame のアクションデルタには `previous_target_frame`、source-frame 座標成分の translation delta には `source_frame` を指定します |

**例**

```yaml
- DeltaTransformEnricherConfig:
    topic_names:
      - /tf_chain
    delta_reference_frame: previous_target_frame
```

**Robot Model 設定例**

```yaml
- type: Parquet
  topic: /tf_chain
  field: /base_link/hand_link/delta_transform
  feature: action.delta_end_effector
  names: [dx, dy, dz, dqx, dqy, dqz, dqw]
```

**注意事項**

- **同期後に実行** - デルタ計算には均一なフレームレートが必要です
- 変換構造に `/delta_transform` フィールドを追加します
- 通常は /tf_chain トピック（TfChainEnricher が作成）と共に使用されます
- `previous_target_frame` は translation を `inverse(R_previous) * (p_current - p_previous)` として計算し、rotation は既存の相対 quaternion delta を維持します
- `source_frame` は translation を source frame 座標で `p_current - p_previous` として計算し、rotation は既存の相対 quaternion delta を維持します

---

### HandCommandEnricherConfig

> **HSR ロボット専用**

グリッパーコマンドトピックが存在しない場合に作成します。/hsrb/servo_states からハンドモーターの位置を抽出します。

**パラメータ**

パラメータなし。

**例**

```yaml
- HandCommandEnricherConfig: {}
```

**Robot Model 設定例**

```yaml
- type: Parquet
  topic: /hsrb/gripper_controller/command
  field: /points/0/positions
  feature: action.gripper
  names: [hand_motor_joint]
```

**注意事項**

- **HSR ロボット専用** - 他のロボットには適用されません
- **同期前に実行** - 生のサーボデータから合成します
- `/hsrb/gripper_controller/command` トピックが存在しない場合に作成します

---

### HeadCommandEnricherConfig

> **HSR ロボット専用**

ヘッドtrajectoryコマンドトピックが存在しない場合に作成します。/hsrb/joint_states からヘッドのパン/チルトを抽出します。

**パラメータ**

パラメータなし。

**例**

```yaml
- HeadCommandEnricherConfig: {}
```

**Robot Model 設定例**

```yaml
- type: Parquet
  topic: /hsrb/head_trajectory_controller/command
  field: /points/0/positions
  feature: action.head
  names: [head_pan_joint, head_tilt_joint]
```

**注意事項**

- **HSR ロボット専用** - 他のロボットには適用されません
- **同期前に実行** - 生のジョイントデータから合成します
- `/hsrb/head_trajectory_controller/command` トピックが存在しない場合に作成します

---

### ShiftEnricherConfig

カラム値を N ステップシフトした新しいトピックを作成します。元のトピックは変更されずに保持されるため、状態（元データ）とアクション（シフトされたデータ）の両方を共存させることができます。

これは主に「アクション = 未来の観測値」となる VLA モデルの学習に使用されます。各設定は単一のソースから出力へのペアを処理します。複数のトピックをシフトするには、複数の設定を使用してください。

**パラメータ**

| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| source_topic | string | (required) | 読み取り元のソーストピック。変更されません。 |
| output_topic | string | (required) | シフトされたデータの新しいトピック名。 |
| shift_steps | int | (required) | シフトするステップ数。正の値 = 未来、負の値 = 過去。 |
| fill_strategy | string | "edge" | null の埋め方: "edge" または "zero"（以下参照）。 |

**埋め戦略**

| Strategy | Behavior |
|----------|----------|
| `edge`（デフォルト） | 前方埋め、次に後方埋め。すべての型で動作します。 |
| `zero` | 数値型の場合は 0 で埋めます。非数値型（文字列、リスト、構造体など）の場合は `edge` にフォールバックします。 |

**例**

```yaml
# ジョイント状態を1ステップ未来にシフト → アクションラベル
- ShiftEnricherConfig:
    source_topic: "/joint_states"
    output_topic: "/joint_states/action"
    shift_steps: 1

# edge の代わりに zero で埋める
- ShiftEnricherConfig:
    source_topic: "/joint_states"
    output_topic: "/joint_states/action"
    shift_steps: 1
    fill_strategy: zero
```

**Robot Model 設定例**

```yaml
# 観測値: 現在の関節位置（ソーストピックから）
- type: Parquet
  topic: /joint_states
  field: /position
  feature: observation.joint_position
  names: [joint1, joint2, joint3]

# アクション: 次のステップの関節位置（シフトされたトピックから）
- type: Parquet
  topic: /joint_states/action
  field: /position
  feature: action.joint_position
  names: [joint1, joint2, joint3]
```

**注意事項**

- **同期後に実行** - シフトは同期済みデータのカラムに対して適用されます
- 時間メタデータカラム（`synched_timestamp_ns`、`timestamp_ns`、`is_fresh`）はシフトされません
- `shift_steps=1` は各行が次の行（未来）の値を取得することを意味します。これは VLA のアクションラベルの一般的な設定です。

---

### UuidEnricherConfig

airoa メタデータの UUID から `rosbag_uuid` カラムを全トピックに追加します。airoa メタデータが必要で、無ければ no-op です。

**パラメータ**

パラメータはありません。

**例**

```yaml
- UuidEnricherConfig: {}
```

---

## Encode ステージ

### VideoEncoderConfig

画像トピックの画像を動画ファイル（MP4）にエンコードします。

> `gop`、`crf`、`qp` などのエンコード関連用語に馴染みがない場合は、「Codec Configuration Details」セクション内の[用語集](#用語集)で簡潔な定義と、より詳しい資料へのリンクを参照できます。

**パラメータ**

| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| fps | int | 100 | フレームレート |
| gop | int | 20 | キーフレーム間隔 |
| crf | string | "34" | 品質（低い値 = 高品質） |
| scaling | string | "Bicubic" | スケーリングアルゴリズム |
| resize | object | (なし) | 出力サイズ `{width, height}`（px、偶数、> 0）を指定。アスペクト比を保たず引き伸ばす |
| codec_config | object | AV1 | codec 設定（以下参照） |

**例 (AV1 - デフォルト)**

```yaml
- VideoEncoderConfig:
    fps: 10
    gop: 2
    crf: "30"
```

**例 (H.264 - 高速デコード)**

```yaml
- VideoEncoderConfig:
    fps: 10
    gop: 2
    crf: "23"
    codec_config:
      codec: "H264"
      preset: Fast
```

**例 (H.265 - 高圧縮)**

```yaml
- VideoEncoderConfig:
    fps: 10
    gop: 2
    crf: "28"
    codec_config:
      codec: "H265"
      preset: Medium
```

**例 (AV1 VA-API - ハードウェアアクセラレーション)**

```yaml
- VideoEncoderConfig:
    fps: 30
    gop: 100
    codec_config:
      codec: "AV1_VAAPI"
      qp: 124
```

**Codec 選択ガイド**

| Use Case | Codec | Quality Param |
|----------|-------|---------------|
| 保存/アーカイブ | AV1 | crf 25-35 |
| 学習（GPU デコード） | H.264 | crf 18-28 |
| バランス型 | H.265 | crf 22-32 |
| 高速エンコード（HW） | AV1_VAAPI | qp 100-150 |

詳細なパラメータについては [Codec Configuration Details](#codec-configuration-details) を参照してください。

---

### ImageEncoderConfig

画像トピックから画像を抽出し、JPEG ファイルとして保存します。

**パラメータ**

パラメータなし。

**例**

```yaml
- ImageEncoderConfig: {}
```

**注意事項**

- 個別の画像が必要な場合は VideoEncoderConfig の代わりに使用してください

---

### DepthImageEncoderConfig

深度フレームをデコードし、`output_dir` 配下に raw バイナリファイルとして保存します。

**パラメータ**

パラメータなし。

**例**

```yaml
- DepthImageEncoderConfig: {}
```

---

### DepthVideoConfig

16-bit の深度フレームを動画にエンコードします。lossy codec は Q10Clip4（16-bit → 10-bit、P010LE）で量子化し、FFV1 は `gray16le` をロスレス保存します。airoa メタデータ（UUID ベースの出力パス用）が必要です。

**パラメータ**

| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| depth_max_mm | int | 4092 | Q10Clip4 の最大深度（mm）。FFV1 では無視される |
| fps | int | 30 | フレームレート |
| codec_config | object | AV1 (crf 4, preset 4) | 深度 codec。`codec:` で選択（`AV1`, `H265_VAAPI`, `AV1_VAAPI`, `H265_NVENC`, `AV1_NVENC`, `FFV1`） |

**例**

```yaml
- DepthVideoConfig:
    depth_max_mm: 4092
    fps: 30
    codec_config:
      codec: "FFV1"
```

---

## Decode ステージ

### VideoDecoderConfig

Context に登録された動画（例: Parquet+Video バンドル）をメモリ上の画像フレームにデコードします。

**パラメータ**

パラメータなし。

**例**

```yaml
- VideoDecoderConfig: {}
```

---

## Transform ステージ

### LeRobotV21TransformerConfig

pipeline データを LeRobot v2.1 フォーマットに変換します。

**パラメータ**

| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| outdir | string | (required) | 出力ディレクトリ |
| robot_model | string or list | (required) | Robot Model 設定ファイルへのパス、またはインラインの Robot Model エントリ |
| video_config | object | AV1 | 動画エンコード設定 |
| separate_per_primitive | bool | false | エピソードモード（以下参照） |

**エピソードモード**

- `false`（デフォルト）: すべてのセグメントを1つのエピソードに結合します。各セグメント境界で `next.done` が `true` に設定されます。
- `true`: 各セグメントが独立したエピソードになります。フレームインデックスと動画ファイルはエピソードごとに独立します。

**例（デフォルト: 単一エピソード）**

```yaml
- LeRobotV21TransformerConfig:
    outdir: "./lerobot_output"
    robot_model: "./config/robot_model/hsr2.yaml"
```

**例（セグメントごとに個別エピソード）**

```yaml
- LeRobotV21TransformerConfig:
    outdir: "./lerobot_output"
    robot_model: "./config/robot_model/hsr2.yaml"
    separate_per_primitive: true
```

**例 (カスタム動画設定)**

```yaml
- LeRobotV21TransformerConfig:
    outdir: "./lerobot_output"
    robot_model: "./config/robot_model/hsr2.yaml"
    video_config:
      fps: 10
      gop: 2
      crf: "30"
```

**注意事項**

- Synchronizer の後に実行する必要があります（同期済みタイムスタンプが必要）
- 通常は pipeline の最後のステージです
- **LeRobot v2.1** レイアウト（`meta/info.json` → `codebase_version: "v2.1"`）を出力します。upstream の LeRobot はその後 v3.0 へ移行していますが、rebake は現在 v2.1 を出力します。

---

## Export ステージ

### ParquetVideoExporterConfig

データセットを中間形式の Parquet + Video バンドル（`{output_dir}/{uuid}/parquet/` + `videos/`）にエクスポートします。`ParquetVideoIngestorConfig` で再 ingest できます。airoa メタデータが必要です。

**パラメータ**

| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| output_dir | string | (required) | ルート出力ディレクトリ。`{uuid}` サブディレクトリが作成される |
| video_config | object | 未設定（AV1 既定） | 任意の RGB `VideoEncoderConfig` |
| depth_config | object | 未設定 | 深度トピック用の任意の `DepthVideoConfig` |

**例**

```yaml
- ParquetVideoExporterConfig:
    output_dir: "./export"
```

---

## Codec Configuration Details

> `gop`、`crf`、`qp`、`b_frames`、`preset` のような動画エンコード関連用語に馴染みがない場合は、本セクション末尾の[用語集](#用語集)に平易な定義をまとめてあります。

### AV1 (SVT-AV1) - 最高の圧縮率、保存に推奨

| Parameter | Type | Range | Default | Description |
|-----------|------|-------|---------|-------------|
| codec | string | - | - | `"AV1"` を指定（エイリアス: `"av1"`, `"Av1"`） |
| lp | int | 0-6 | 0 (auto) | 並列処理レベル |
| pin | int | 0-N | 0 | CPU ピンニング（0=無効） |
| preset | int | 0-13 | 10 | 品質プリセット（低い値=高品質/低速） |
| film-grain | int | 0-50 | - | フィルムグレイン合成レベル |
| film-grain-denoise | bool | - | - | film-grain 有効時にノイズ除去を適用 |
| lookahead | int | -1 to 120 | - | 先読みフレーム数（-1=自動） |
| fast-decode | int | 0-2 | - | 高速デコード最適化レベル |

```yaml
- VideoEncoderConfig:
    fps: 10
    gop: 2
    crf: "30"
    codec_config:
      codec: "AV1"
      lp: 6
      preset: 6
      film-grain: 8        # オプション: グレインを復元（実写は8、アニメーションは4-6）
      lookahead: 60        # オプション: レイテンシと引き換えに品質を向上
```

### H.264 (libx264) - 最速のデコード、最高の互換性

| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| codec | string | - | `"H264"` を指定（エイリアス: `"h264"`, `"H.264"`） |
| threads | int | null (auto) | スレッド数 |
| preset | string | Medium | Ultrafast/Superfast/Veryfast/Faster/Fast/Medium/Slow/Slower/Veryslow |
| tune | list | [] | チューニングオプション（以下参照） |

**チューニングオプション**（組み合わせ可能、ただし PSY チューニングは1つのみ）：
- **PSY チューニング**（相互排他）: `Film`, `Animation`, `Grain`, `StillImage`, `Psnr`, `Ssim`
- **非 PSY チューニング**（PSY と1つ組み合わせ可能）: `FastDecode`, `ZeroLatency`

```yaml
- VideoEncoderConfig:
    fps: 10
    gop: 2
    crf: "23"              # H.264 の一般的な範囲: 18-28
    codec_config:
      codec: "H264"
      threads: 4
      preset: Fast
      tune: [Film, FastDecode]   # PSY 1つ + 非 PSY は可
```

### H.265 (libx265) - 良好な圧縮率、中程度のデコード速度

| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| codec | string | - | `"H265"` を指定（エイリアス: `"h265"`, `"H.265"`） |
| threads | int | null (auto) | スレッド数（x265 pools 経由） |
| preset | string | Medium | H.264 と同じプリセット |
| tune | list | [] | チューニングオプション（以下参照） |
| frame-threads | int | null (auto) | フレームレベルの並列処理スレッド |

**チューニングオプション**：
- **PSY チューニング**（相互排他）: `Psnr`, `Ssim`, `Grain`, `Animation`
- **非 PSY チューニング**: `FastDecode`, `ZeroLatency`

```yaml
- VideoEncoderConfig:
    fps: 10
    gop: 2
    crf: "28"              # H.265 の一般的な範囲: 20-32
    codec_config:
      codec: "H265"
      threads: 4
      preset: Medium
      tune: [Grain, ZeroLatency]
      frame-threads: 3     # オプション: フレームレベルの並列処理
```

---

### VA-API ハードウェア Codec

VA-API（Video Acceleration API）は、AMD および Intel GPU でのハードウェアアクセラレーションによるエンコードを可能にします。これらの codec はソフトウェア codec と比較して大幅に高速なエンコードを提供しますが、品質面でのトレードオフがあります。

> **要件：**
> - VA-API をサポートする Linux
> - AMD GPU（RDNA 2+ / Ryzen 6000+）または Intel GPU（Gen 8+ / Broadwell+）
> - Docker: `/dev/dri` デバイスのパススルーが必要

#### H.264 VA-API (h264_vaapi) - 最高の互換性

| Parameter | Type | Range | Default | Description |
|-----------|------|-------|---------|-------------|
| codec | string | - | - | `"H264_VAAPI"` を指定（エイリアス: `"h264_vaapi"`） |
| qp | int | 0-51 | 21 | 量子化パラメータ（低い値 = 高品質） |
| device | string | - | `/dev/dri/renderD128` | VA-API デバイスパス |
| profile | string | - | high | `constrained_baseline`, `main`, `high`, `high10` |
| b-depth | int | 0-7 | unset | B フレーム参照深度（AMD VCN 3.0+） |
| async-depth | int | 1-64 | 16 | 並列エンコード深度 |

```yaml
- VideoEncoderConfig:
    fps: 30
    gop: 2
    codec_config:
      codec: "H264_VAAPI"
      qp: 29                    # 品質（0-51、低い値 = 高品質）
      profile: high             # オプション: constrained_baseline/main/high
      b-depth: 2                # オプション: B フレーム深度（AMD VCN 3.0+）
      async-depth: 4            # オプション: 並列エンコード
```

#### H.265 VA-API (hevc_vaapi) - より高い圧縮率

| Parameter | Type | Range | Default | Description |
|-----------|------|-------|---------|-------------|
| codec | string | - | - | `"H265_VAAPI"` を指定（エイリアス: `"hevc_vaapi"`, `"h265_vaapi"`） |
| qp | int | 0-51 | 29 | 量子化パラメータ（低い値 = 高品質） |
| device | string | - | `/dev/dri/renderD128` | VA-API デバイスパス |
| profile | string | - | auto | `main`, `main10`, `rext` |
| async-depth | int | 1-64 | unset | 並列エンコード深度（未設定 = エンコーダ既定） |

> **AMD VCN の制限事項:** HEVC はすべての AMD VCN 世代で B フレームをサポートしていません。
> これはハードウェアの制限であり、ソフトウェアの問題ではありません。

```yaml
- VideoEncoderConfig:
    fps: 30
    gop: 2
    codec_config:
      codec: "H265_VAAPI"
      qp: 29                    # 品質（0-51、低い値 = 高品質）
      profile: main             # オプション: main/main10
      async-depth: 4            # オプション: 並列エンコード
```

#### AV1 VA-API (av1_vaapi) - 最高の圧縮率（VCN 4.0+ / Intel Arc）

| Parameter | Type | Range | Default | Description |
|-----------|------|-------|---------|-------------|
| codec | string | - | - | `"AV1_VAAPI"` を指定（エイリアス: `"av1_vaapi"`） |
| qp | int | 0-255 | 110 | 量子化パラメータ（低い値 = 高品質） |
| device | string | - | `/dev/dri/renderD128` | VA-API デバイスパス |
| profile | string | - | auto | `main`, `high`, `professional` |
| b-depth | int | 0-7 | unset | B フレーム参照深度 |
| async-depth | int | 1-64 | unset | 並列エンコード深度 |

> **ハードウェア要件：**
> - AMD: RDNA 3（RX 7000 / Ryzen 7000）VCN 4.0+ 搭載
> - Intel: Arc GPU（DG2+）

```yaml
- VideoEncoderConfig:
    fps: 30
    gop: 100
    codec_config:
      codec: "AV1_VAAPI"
      qp: 124                   # 品質（0-255、低い値 = 高品質）
      profile: main             # オプション: main/high/professional
      b-depth: 2                # オプション: B フレーム深度
      async-depth: 4            # オプション: 並列エンコード
```

---

### NVIDIA NVENC ハードウェア Codec

NVENC は NVIDIA GPU でのハードウェアアクセラレーションエンコードを可能にします。VA-API codec と同様、NVENC variant は FFmpeg CLI subprocess 経由で呼び出されるため、環境にある FFmpeg バイナリに対応する `*_nvenc` エンコーダが含まれている必要があります。本リポジトリの NVENC 用 Docker image (`rebake:nvenc`) は、`h264_nvenc`, `hevc_nvenc`, `av1_nvenc` を有効化した FFmpeg を build します。

> **要件：**
> - NVENC をサポートする NVIDIA driver
> - H.264 と H.265 NVENC は Maxwell 世代以降のほとんどの NVIDIA GPU で動作（GTX 900 シリーズ以降）
> - AV1 NVENC を使う場合は、加えて Ada Lovelace 世代以降の GPU が必要（RTX 4000 シリーズ以降）
> - container/runtime から NVIDIA GPU にアクセスできること（例: `nvidia-container-toolkit`）。本リポジトリの Docker setup では NVIDIA CDI devices を使います。
> - Docker のセットアップは [README - NVIDIA NVENC ハードウェアエンコーディング](../README_ja.md#nvidia-nvenc-ハードウェアエンコーディング) を参照

#### 共通 Parameters

3 つの NVENC variant は以下の parameter を共通で持ちます。codec 固有の range と既定値は、後続の codec 別サブセクションを参照してください。

| Parameter | Type | Description |
|-----------|------|-------------|
| qp | int | 量子化パラメータ（低いほど高品質）。range と既定値は codec ごとに異なる |
| gpu | int | FFmpeg の `-gpu` に渡す NVIDIA GPU index。省略時は default device |
| preset | string | 速度/品質プリセット。`P1`（最速）〜 `P7`（最遅・最高圧縮率） |
| tune | string | エンコーダの tune: `Hq`, `Ll`, `Ull`。省略時は FFmpeg/NVENC の既定値 |
| profile | string | エンコーダ profile（codec 固有）。省略時は既定値 |
| b_frames | int | B フレーム数（0-7）。既定: `0`（H.265/AV1 NVENC）、`1`（H.264 NVENC） |
| rc_lookahead | int | レート制御の lookahead フレーム数（0-120）。任意指定 |

> **H.265 / AV1 NVENC の B-frames 既定値は `0`** です。frame index に基づく packaging の挙動を予測しやすくし、FFmpeg/NVENC の暗黙の既定 B-frame 数によって短い `gop` 値が invalid になるのを避けるためです。**H.264 NVENC の既定値は `1`** です（測定された VMAF≥93 プロファイル）。`b_frames > 0` を指定すると、rebake は `-b_ref_mode middle` を自動で付加し、その場合 `gop` は `b_frames + 1` より大きい必要があります。`b_ref_mode` は意図的に公開設定として出していません。

#### H.264 NVENC (h264_nvenc) - 最高の互換性

| Parameter | Type | Range | Default | Description |
|-----------|------|-------|---------|-------------|
| codec | string | - | - | `"H264_NVENC"` を指定（エイリアス: `"h264_nvenc"`） |
| qp | int | 0-51 | 26 | 量子化パラメータ |
| preset | string | P1-P7 | P5 | 速度/品質プリセット |
| profile | string | - | high | `baseline`, `main`, `high` |
| tune | string | - | Hq | `Hq`, `Ll`, `Ull` |
| b_frames | int | 0-7 | 1 | B フレーム数 |
| rc_lookahead | int | 0-120 | 32 | レート制御 lookahead |

```yaml
- VideoEncoderConfig:
    fps: 100
    gop: 20
    codec_config:
      codec: "H264_NVENC"
      qp: 26                  # 品質（0-51、低い値 = 高品質）
      preset: P5              # P1 最速 ... P7 最高圧縮率
      tune: Hq                # オプション: Hq/Ll/Ull
```

#### H.265 NVENC (hevc_nvenc) - より高い圧縮率

| Parameter | Type | Range | Default | Description |
|-----------|------|-------|---------|-------------|
| codec | string | - | - | `"H265_NVENC"` を指定（エイリアス: `"hevc_nvenc"`, `"h265_nvenc"`） |
| qp | int | 0-51 | 25 | 量子化パラメータ |
| preset | string | P1-P7 | P4 | 速度/品質プリセット |
| profile | string | - | auto | `main`, `main10`, `rext` |
| tune | string | - | auto | `Hq`, `Ll`, `Ull` |
| b_frames | int | 0-7 | 0 | B フレーム数 |
| rc_lookahead | int | 0-120 | auto | レート制御 lookahead |

```yaml
- VideoEncoderConfig:
    fps: 100
    gop: 100
    codec_config:
      codec: "H265_NVENC"
      qp: 25
      preset: P4
      tune: Hq
```

#### AV1 NVENC (av1_nvenc) - 最高の圧縮率（Ada Lovelace+）

| Parameter | Type | Range | Default | Description |
|-----------|------|-------|---------|-------------|
| codec | string | - | - | `"AV1_NVENC"` を指定（エイリアス: `"av1_nvenc"`） |
| qp | int | 0-255 | 130 | 量子化パラメータ |
| preset | string | P1-P7 | P7 | 速度/品質プリセット |
| profile | string | - | auto | `main` |
| tune | string | - | auto | `Hq`, `Ll`, `Ull` |
| b_frames | int | 0-7 | 0 | B フレーム数 |
| rc_lookahead | int | 0-120 | auto | レート制御 lookahead |

> **既定値の根拠:** `qp: 130` と `preset: P7` は、UMI 系 RGB データで VMAF >= 93 を維持しつつ低い QP より file size を抑えられる値として RTX 5090 で測定したものです。

```yaml
- VideoEncoderConfig:
    fps: 100
    gop: 20
    codec_config:
      codec: "AV1_NVENC"
      qp: 130
      preset: P7
```

#### Depth NVENC

`DepthCodecConfig` は深度動画用に `H265_NVENC` と `AV1_NVENC` をサポートします。深度パイプラインは Q10Clip4 量子化（16-bit -> 10-bit P010LE）と `-color_range pc` を深度 VA-API パスと同じく自動で適用します。

| Codec | QP range | Default QP | Default preset |
|-------|----------|-----------:|----------------|
| `H265_NVENC`（深度） | 0-51 | 10 | P4 |
| `AV1_NVENC`（深度） | 0-255 | 20 | P4 |

```yaml
- DepthVideoConfig:
    fps: 30
    depth_max_mm: 4092
    codec_config:
      codec: "AV1_NVENC"
      qp: 20
      preset: P4
```

---

### 用語集

このセクションで使われているエンコード関連用語の簡潔な定義集です。`gop`、`qp`、`b_frames` のような parameter の意味が掴みづらいときは、まずここを起点にしてください。さらに詳しい背景は、各項目のリンク先を参照してください。アルファベット順に並べています。

#### color_range

ピクセル値とディスプレイ階調のマッピング方法です:

- `tv`（limited / studio range）— ピクセル値は内側の帯域のみを使用します（例: 10-bit なら 64-940）。放送系で標準的に使われる範囲です。
- `pc`（full range）— ピクセル値はフル range を使用します（例: 10-bit なら 0-1023）。

rebake の深度パイプラインは、量子化済みの深度値が誤って clip されたり再スケールされたりしないよう、常に `pc` を使用します。

#### CRF (Constant Rate Factor)

ソフトウェアエンコーダで使われる品質目標値です。エンコーダは、知覚品質が目標値に近づくよう、フレームごとに内部 QP を調整します。値が低いほど高品質・大きなファイルになります。有効な range はコーデックによって異なります:

- SVT-AV1: 0-63
- x264: 0-51
- x265: 0-51

CRF はソフトウェアエンコーダ専用です。rebake のハードウェアエンコーダ（VA-API、NVENC）は CRF ではなく QP を直接使用します。

#### GOP (Group of Pictures)

連続する 2 つのキーフレーム（I-frame）の間のフレーム数です。GOP が短いと seek が高速になり、データ欠損からの復旧も数フレーム以内に収まりますが、ファイルサイズは大きくなります。GOP が長いと圧縮率は良くなりますが、seek は遅くなります。最適値はコーデックと用途によって変わるため、rebake はロボットカメラの典型的なワークロードに合わせて、コーデック別の既定値を選んでいます。詳細は [Wikipedia: Group of pictures](https://en.wikipedia.org/wiki/Group_of_pictures) を参照してください。

#### I-frame、P-frame、B-frame

近年の動画コーデックで使われる、圧縮済みフレームの 3 種類です:

- **I-frame**（イントラ符号化）— 単独で符号化される、1 枚の画像のようなフレームです。他のフレームを参照せずに復号でき、3 種類の中で最もサイズが大きくなります。
- **P-frame**（前方予測）— 過去のフレームから予測されます。I-frame よりサイズが小さくなります。
- **B-frame**（双方向予測）— 過去と未来の両方のフレームから予測されます。最もサイズが小さくなりますが、ストリームの並び順を変えます。つまりファイル内のフレーム並びがソースの並びと一致しなくなり、frame index による検索が複雑になります。rebake は既定で H.265/AV1 NVENC の B-frame を無効化しています（`b_frames: 0`。H.264 NVENC の既定は `1`）。フレームの並び替えが許容できるユースケース（archive 用の圧縮など）でのみ増やしてください。

詳細は [Wikipedia: Video compression picture types](https://en.wikipedia.org/wiki/Video_compression_picture_types) を参照してください。

#### Pixel formats

FFmpeg に渡すピクセルデータのバイト配置です。rebake は次の 3 種類を使用します:

- `yuv420p` — 8-bit YUV、クロマサブサンプリング 4:2:0。ソフトウェアエンコードの H.264/H.265 RGB 動画における標準フォーマットです。
- `p010le` — 10-bit YUV 4:2:0、リトルエンディアン。ハードウェアエンコードの深度（Q10Clip4 量子化済み）や高 bit 深度コンテンツに使用されます。
- `gray16le` — 16-bit グレースケール、リトルエンディアン。FFV1 lossless 深度で、元の 16-bit 深度値をそのまま保持するために使用されます。

#### preset

速度と品質のトレードオフをまとめたプリセットです。遅い preset ほど多くの符号化オプションを試行し、同じ品質目標でより小さなファイルを生成します。値はコーデックによって異なります:

- SVT-AV1: `0`（最遅・最高品質）〜 `13`（最速）
- x264 / x265: `ultrafast`, `superfast`, ..., `slower`, `veryslow`
- NVENC: `P1`（最速）〜 `P7`（最遅・最高圧縮率）

普遍的に「正しい」preset はありません。archive 用途には遅め、エンコードスループットを重視する場合は速めを選んでください。

#### profile

エンコーダがターゲットとするコーデック仕様のサブセットです。profile によって、使用される機能と、結果を再生できるデコーダが決まります。例:

- H.264: `baseline`, `main`, `high`
- H.265: `main`, `main10`, `rext`
- AV1: `main`

特定のデコーダ制約がない限り、profile は未指定のままにしてください（例: H.264 の `main` profile しかサポートしないモバイル機器を対象にする場合などに指定が必要になります）。

#### Q10Clip4

rebake が深度動画に使用する量子化方式です。16-bit のミリメートル深度値を 10-bit の range にマッピングし、`depth_max_mm` を超える値は 0（invalid として扱われる）にクリップします。Q10Clip4 によって、深度値を HEVC や AV1 ハードウェアエンコーダが要求する 10-bit `p010le` フォーマットに収めることができます。FFV1 lossless 深度は Q10Clip4 をスキップし、生の `gray16le` 値をそのまま保存します。

#### QP (Quantization Parameter)

エンコーダがブロックごとにどれだけ細部を捨てるかを表す値です。低い値ほど細部が残り、ファイルサイズが大きくなります。有効な range はコーデックによって異なります:

- H.264 / H.265: 0-51
- AV1（NVENC、VA-API）: 0-255

rebake のハードウェアエンコーダは constant-QP レート制御モード（CQP）を使用し、すべてのフレームを同じ QP でエンコードします。これによりフレームごとのサイズが予測しやすくなりますが、CRF と比べてビット配分の効率はわずかに劣ります。

#### rc_lookahead

レート制御がエンコード前に何フレーム先まで読み取るかを表します。先読みすることで、シーン変化など重要な箇所にビットを多く割り当てられるようになります。NVENC 専用です。FFmpeg の既定値で良い場合は未指定のままにし、エンコード速度より archive 品質を優先する場合は値を上げてください（最大 120）。

#### tune (NVENC)

特定の用途に最適化された profile を選択します:

- `Hq` — 高品質。オフラインエンコードでの安全な選択肢です。
- `Ll` — 低レイテンシ。
- `Ull` — 超低レイテンシ。ライブストリーミング用途。

rebake の NVENC 既定値はオフラインエンコードを想定しているため、`tune` は未指定でも問題ありません。明示的に指定したい場合は `tune: Hq` をお試しください。

#### VMAF (Video Multi-Method Assessment Fusion)

Netflix が開発した知覚的動画品質メトリクスです。0 から 100 のスコアで評価し、高いほどソースに近いことを示します。一般的な目安として、**VMAF >= 93** は「ほとんどの視聴者にとってソースと区別がつかない」品質とされています。rebake のハードウェアコーデックの既定 QP は、内部テストデータセットで VMAF を 93 以上に保てるよう調整しています。詳細は [VMAF の GitHub リポジトリ](https://github.com/Netflix/vmaf) を参照してください。

---

### Codec パラメータリファレンス

#### ソフトウェアエンコーダ
- **SVT-AV1**: [SVT-AV1 Documentation](https://gitlab.com/AOMediaCodec/SVT-AV1/-/blob/master/Docs/Parameters.md)
- **x264**: [x264 FFmpeg Options](https://trac.ffmpeg.org/wiki/Encode/H.264)
- **x265**: [x265 FFmpeg Options](https://trac.ffmpeg.org/wiki/Encode/H.265)

#### VA-API ハードウェアエンコーダ
- **FFmpeg VAAPI Encoding**: [FFmpeg Hardware Encoding Guide](https://trac.ffmpeg.org/wiki/Hardware/VAAPI)
- **AMD VCN**: [AMD Video Core Next (Wikipedia)](https://en.wikipedia.org/wiki/Video_Core_Next)
- **Intel Quick Sync**: [ArchWiki Hardware Acceleration](https://wiki.archlinux.org/title/Hardware_video_acceleration)
- **VA-API Setup Guide**: [Brainiarc7's VAAPI Gist](https://gist.github.com/Brainiarc7/95c9338a737aa36d9bb2931bed379219)

#### NVIDIA NVENC ハードウェアエンコーダ
- **NVIDIA FFmpeg GPU Guide**: [Using FFmpeg with NVIDIA GPU Hardware Acceleration](https://docs.nvidia.com/video-technologies/video-codec-sdk/13.0/ffmpeg-with-nvidia-gpu/index.html)
- **NVENC SDK**: [NVIDIA Video Codec SDK 13.0](https://docs.nvidia.com/video-technologies/video-codec-sdk/13.0/index.html)
- **ローカルでエンコーダの options を確認**: `ffmpeg -hide_banner -h encoder=h264_nvenc`（`hevc_nvenc`, `av1_nvenc` も同様）

#### H.264/H.265 プロファイル
- **H.264 Profiles Explained**: [RGB Spectrum - H.264 Profiles](https://www.rgb.com/h264-profiles)
- **H.264 Profile Comparison**: [Streamio - H.264 High, Main, or Baseline](https://www.streamio.com/support/h-264-high-main-or-baseline/)
- **HEVC Profiles**: [HEVC Wikipedia](https://en.wikipedia.org/wiki/High_Efficiency_Video_Coding)
- **HEVC Main10 and HDR**: [Intel - Enable 10-Bit HEVC](https://www.intel.com/content/www/us/en/developer/articles/technical/enable-10bpp.html)

#### B フレームとエンコード品質
- **B-frame Recommendations**: [Xilinx Video SDK - Tuning Quality](https://xilinx.github.io/video-sdk/v2.0/tuning_encoding_quality.html)
- **AMD B-frame Support**: [Intel Media Driver - B frame numbers](https://github.com/intel/media-driver/issues/766)

---

## Enrich されたデータの使用

Enricher はデータセットに新しいトピックやフィールドを追加します。これらを LeRobot の出力で使用するには、Robot Model 設定にエントリを追加する必要があります。

### Enricher → Robot Model 設定リファレンス

| Enricher | 作成するもの | Robot Model 設定例 |
|----------|---------|---------------------------|
| TfChainEnricher | `/tf_chain` トピック | [TF Transform の例](#tf-transform-example) を参照 |
| DeltaJointPositionEnricher | `delta_position` フィールド | [Delta Position の例](#delta-position-example) を参照 |
| DeltaTransformEnricher | `delta_transform` フィールド | [Delta Transform の例](#delta-transform-example) を参照 |
| ShiftEnricher | 新しいトピック（例: `/joint_states/action`） | [Shift の例](#shift-example) を参照 |
| HandCommandEnricher | `/hsrb/gripper_controller/command` トピック | [HSR Command の例](#hsr-command-example) を参照 |
| HeadCommandEnricher | `/hsrb/head_trajectory_controller/command` トピック | [HSR Command の例](#hsr-command-example) を参照 |

### TF Transform Example

TfChainEnricher は変換データを含む `/tf_chain` トピックを作成します。

**Pipeline 設定:**

```yaml
- TfChainEnricherConfig:
    frame_pairs:
      - source: base_link
        target: hand_link
```

**Robot Model 設定:**

```yaml
# 絶対変換（位置 + 回転）を使用
- type: Parquet
  topic: /tf_chain
  field: /base_link/hand_link/transform
  feature: observation.end_effector_pose
  names: [x, y, z, qx, qy, qz, qw]

# ペア freshness を使用（その時刻に chain 上のどこかの edge が更新されたら true）
- type: Parquet
  topic: /tf_chain
  field: /base_link/hand_link/is_fresh
  feature: observation.end_effector_pose.is_fresh
```

**フィールドパス構造:**

```
/tf_chain
└── /{source_frame}
    └── /{target_frame}
        ├── /transform
        │   ├── /translation  → x, y, z
        │   └── /rotation     → qx, qy, qz, qw (quaternion)
        └── /is_fresh         → bool
```

### Delta Position Example

DeltaJointPositionEnricher は既存のトピックに `delta_position` フィールドを追加します。

**Pipeline 設定:**

```yaml
- DeltaJointPositionEnricherConfig:
    topic_names:
      - /joint_states
```

**Robot Model 設定:**

```yaml
# 元の関節位置（観測値）
- type: Parquet
  topic: /joint_states
  field: /position
  feature: observation.joint_position
  names: [joint1, joint2, joint3]

# デルタ関節位置（アクション）
- type: Parquet
  topic: /joint_states
  field: /delta_position
  feature: action.delta_joint_position
  names: [joint1, joint2, joint3]
```

### Delta Transform Example

DeltaTransformEnricher は変換トピックに `delta_transform` フィールドを追加します。

**Pipeline 設定:**

```yaml
- TfChainEnricherConfig:
    frame_pairs:
      - source: base_link
        target: hand_link

- DeltaTransformEnricherConfig:
    topic_names:
      - /tf_chain
    delta_reference_frame: previous_target_frame
```

**Robot Model 設定:**

```yaml
# 絶対変換（観測値）
- type: Parquet
  topic: /tf_chain
  field: /base_link/hand_link/transform
  feature: observation.end_effector_pose
  names: [x, y, z, qx, qy, qz, qw]

# デルタ変換（アクション）
- type: Parquet
  topic: /tf_chain
  field: /base_link/hand_link/delta_transform
  feature: action.delta_end_effector
  names: [dx, dy, dz, dqx, dqy, dqz, dqw]
```

### Shift Example

ShiftEnricher はカラム値をシフトした新しいトピックを作成します。元のトピックは変更されないため、両方を LeRobot の feature にマッピングできます。

**Pipeline 設定:**

```yaml
- ShiftEnricherConfig:
    source_topic: "/joint_states"
    output_topic: "/joint_states/action"
    shift_steps: 1
```

**Robot Model 設定:**

```yaml
# 観測値: 現在の関節位置（ソーストピック、変更なし）
- type: Parquet
  topic: /joint_states
  field: /position
  feature: observation.joint_position
  names: [joint1, joint2, joint3]

# アクション: 次のステップの関節位置（シフトされたトピック）
- type: Parquet
  topic: /joint_states/action
  field: /position
  feature: action.joint_position
  names: [joint1, joint2, joint3]
```

### HSR Command Example

HandCommandEnricher と HeadCommandEnricher は HSR ロボット用のコマンドトピックを作成します。

**Pipeline 設定:**

```yaml
- HandCommandEnricherConfig: {}
- HeadCommandEnricherConfig: {}
```

**Robot Model 設定:**

```yaml
# グリッパーコマンド
- type: Parquet
  topic: /hsrb/gripper_controller/command
  field: /points/0/positions
  feature: action.gripper
  names: [hand_motor_joint]

# ヘッドコマンド
- type: Parquet
  topic: /hsrb/head_trajectory_controller/command
  field: /points/0/positions
  feature: action.head
  names: [head_pan_joint, head_tilt_joint]
```

### 完全な Pipeline の例

TF 変換とデルタ計算を使用した完全な例を以下に示します：

**Pipeline 設定 (pipeline.yaml):**

```yaml
work_dir: "./output"
stage_configs:
  # 取り込み
  - Rosbag2IngestorConfig: {}

  # Enrich（同期前） - TF バッファの構築
  - TfBufferEnricherConfig: {}

  # 同期
  - ZeroOrderHoldTimeSynchronizerConfig:
      fps: 10

  # Enrich（同期後） - 変換とデルタの計算
  - TfChainEnricherConfig:
      frame_pairs:
        - source: base_link
          target: hand_link
  - DeltaJointPositionEnricherConfig:
      topic_names: ["/joint_states"]
  - DeltaTransformEnricherConfig:
      topic_names: ["/tf_chain"]
      delta_reference_frame: previous_target_frame
  - ShiftEnricherConfig:
      source_topic: "/joint_states"
      output_topic: "/joint_states/action"
      shift_steps: 1
  - ShiftEnricherConfig:
      source_topic: "/tf_chain"
      output_topic: "/tf_chain/action"
      shift_steps: 1

  # 変換
  - LeRobotV21TransformerConfig:
      outdir: "./lerobot_output"
      robot_model: "./config/robot_model/myrobot.yaml"
```

**Robot Model 設定 (myrobot.yaml):**

```yaml
# 観測値: 関節状態
- type: Parquet
  topic: /joint_states
  field: /position
  feature: observation.joint_position
  names: [joint1, joint2, joint3]

# 観測値: エンドエフェクタの姿勢
- type: Parquet
  topic: /tf_chain
  field: /base_link/hand_link/transform
  feature: observation.end_effector_pose
  names: [x, y, z, qx, qy, qz, qw]

# 観測値: カメラ
- type: Video
  topic: /camera/image_raw/compressed
  feature: observation.image.camera
  names: [height, width, channel]

# アクション: 次のステップの関節位置（ShiftEnricher から）
- type: Parquet
  topic: /joint_states/action
  field: /position
  feature: action.joint_position
  names: [joint1, joint2, joint3]

# アクション: 次のステップのエンドエフェクタの姿勢（ShiftEnricher から）
- type: Parquet
  topic: /tf_chain/action
  field: /base_link/hand_link/transform
  feature: action.end_effector_pose
  names: [x, y, z, qx, qy, qz, qw]

# アクション: デルタ関節位置
- type: Parquet
  topic: /joint_states
  field: /delta_position
  feature: action.delta_joint_position
  names: [joint1, joint2, joint3]

# アクション: デルタエンドエフェクタ
- type: Parquet
  topic: /tf_chain
  field: /base_link/hand_link/delta_transform
  feature: action.delta_end_effector
  names: [dx, dy, dz, dqx, dqy, dqz, dqw]
```

---

## Robot Model 設定

配置場所: `config/robot_model/`

このファイルは ROS トピックを LeRobot の feature にマッピングします。`LeRobotV21TransformerConfig` で使用されます。

このセクションの例はスタンドアロンの Robot Model 設定ファイルであり、`stage_configs` のsnippetではありません。

### エントリの種類

#### Parquet エントリ

ROS トピックのフィールドを Parquet カラムにマッピングします。

```yaml
- type: Parquet
  topic: /joint_states           # ROS トピック名
  field: /position               # フィールドへの JSON Pointer
  feature: observation.state     # LeRobot の feature 名
  names:                         # オプション: 各次元の名前
    - joint1
    - joint2
  description: "Joint positions" # オプション: 説明
```

#### Video エントリ

画像トピックを video feature にマッピングします。

```yaml
- type: Video
  topic: /camera/image_raw/compressed
  feature: observation.image.camera
  names: [height, width, channel]
```

#### Image エントリ

画像トピックを（動画ストリームではなく）個別フレームの image feature にマッピングします。通常のカメラストリームには `Video` を推奨します。

```yaml
- type: Image
  topic: /camera/image_raw/compressed
  feature: observation.image.camera
  names: [height, width, channel]
```

### フィールドパス構文

`field` パラメータは JSON Pointer 構文（RFC 6901）を使用します：

| Path | Meaning |
|------|---------|
| `/position` | "position" という名前のトップレベルフィールド |
| `/points/0/positions` | "points" 配列の最初の要素の "positions" フィールド |
| `/linear/x` | ネストされたフィールド: linear.x |
| `/wrench/force` | ネストされたフィールド: wrench.force |

### 例

```yaml
# config/robot_model/example.yaml

# 観測値: 関節状態
- type: Parquet
  topic: /joint_states
  field: /position
  feature: observation.state
  names:
    - joint1
    - joint2
    - joint3

# 観測値: カメラ画像
- type: Video
  topic: /camera/image_raw/compressed
  feature: observation.image.camera
  names: [height, width, channel]

# アクション: アームtrajectory
- type: Parquet
  topic: /arm_trajectory_controller/command
  field: /points/0/positions
  feature: action.arm
  names:
    - shoulder_joint
    - elbow_joint
    - wrist_joint

# アクション: ベース速度
- type: Parquet
  topic: /cmd_vel
  field: /linear
  feature: action.twist_linear
  names: [linear_x, linear_y, linear_z]
```

---

## 出力構造

`work_dir` は主に中間/デバッグ出力用です。正確なレイアウトは実装の詳細であり、将来のバージョンで変更される可能性があります。

`save_contexts: true` の場合の現在のレイアウト：

```
./orchestrator_work/
└── {rosbag_parent_dir_name}/
    ├── 0_rosbag2_ingestor/
    │   ├── joint_states.parquet
    │   ├── tf.parquet
    │   └── ...
    ├── 1_tf_buffer_enricher/
    │   └── ...
    ├── 2_zero_order_hold_time_synchronizer/
    │   └── ...
    └── ...
```

`rebake-cli run` は `work_dir` 配下に入力ごとのディレクトリを作成します：

- Rosbag パイプラインの場合、rosbag の親ディレクトリ名を使用します
- `ParquetVideoIngestor` パイプラインの場合、中間形式データセットディレクトリ名を使用します

LeRobot の出力は `{outdir}/{uuid}/` に保存されます：

```
./lerobot_output/
└── {uuid}/
    ├── data/
    │   └── chunk-000/
    │       └── episode_000000.parquet
    ├── videos/
    │   └── chunk-000/
    │       └── observation.image.head/
    │           └── episode_000000.mp4
    └── meta/
        ├── info.json
        ├── episodes.jsonl
        ├── episodes_stats.jsonl
        └── tasks.jsonl
```

---

## トラブルシューティング

### クイックリファレンス

| Error | Cause | Solution |
|-------|-------|----------|
| `missing required data: rosbag_path` | Rosbag ingestorの入力パスが設定されていない | 位置引数として rosbag パスが指定されているか確認してください |
| `missing required data: bundle_root` | `ParquetVideoIngestor` の入力ディレクトリが設定されていない | 位置引数として中間形式データセットディレクトリが指定されているか確認してください |
| `missing required data: dataset` | Ingestor が不足している | 最初のステージとして Ingestor を追加してください |
| `missing required data: tf_buffer` | TfBufferEnricher が実行されていない | TfChainEnricher の前に TfBufferEnricherConfig を追加してください |
| `missing required data: fps` | Synchronizer が実行されていない | ZeroOrderHold または NearestNeighbor synchronizer を追加してください |
| `I/O error: failed to read meta.json` | meta.json が見つからない | require_metadata: false を設定するか、meta.json を追加してください |
| `I/O error: failed to open mcap` | ファイルが見つからない | rosbag ファイルのパスを確認してください |
| `I/O error: failed to open parquet file: .../_topic_type_map.parquet` | 入力がエクスポート済み中間形式データセットディレクトリではない | `rebake-cli run` にデータセットディレクトリ、またはそれらを含む親ディレクトリを指定してください |
| `I/O error: failed to read robot model` | ファイルが見つからないか、無効な YAML | `robot_model` のパスまたはインライン設定を確認してください |
| `TF lookup failed` | フレームが見つからない | frame_pairs のフレーム名を確認してください |
| `invalid data: no segments overlap` | セグメントがタイムラインの範囲外 | meta.json のセグメント時間を確認してください |

### ステージ順序の問題

**問題:** 存在するはずのデータが見つからないというエラーが発生する。

**解決策:** ステージの順序を確認してください。一部のステージは他のステージが先に実行されている必要があります：

1. Ingestor（常に最初）
2. TfBufferEnricher（TfChainEnricher の前）
3. TfChainEnricher（DeltaTransformEnricher の前）
4. Synchronizer（LeRobotTransformer の前）
5. LeRobotTransformer（通常は最後）

### よくある Pipeline の問題

**問題:** 動画がエンコードされない。

**確認事項:**
- コンテキストに image_data が含まれていますか？（Ingestor がロードします）
- `robot_model` に `Video` エントリが含まれていますか？

**問題:** TF 変換が見つからない。

**確認事項:**
- TfChainEnricherConfig の前に TfBufferEnricherConfig を追加しましたか？
- rosbag に /tf トピックがありますか？（/tf_static は任意）
- frame_pairs のフレーム名は正しいですか？

**問題:** LeRobot の出力が空になる。

**確認事項:**
- Synchronizer は実行されましたか？（synched_timestamp_ns に必要です）
- meta.json に有効なセグメントがありますか？
- セグメントの時間が rosbag のタイムラインと重なっていますか？
