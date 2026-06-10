# メタデータ（meta.json）

ROS bag はメッセージしか持っていません。どの収録か、何のタスクか、どこからどこまでがエピソードか、は `meta.json` で与えます。UUID が出力のフォルダ名と `rosbag_uuid` 列になり、ラベルとセグメントが LeRobot のタスクとエピソードになります。これが無いと、Transformer や Exporter は動きません。

このページは `meta.json` の書き方のリファレンスです。パイプライン側の設定は[設定](configuration_ja.md)へ。

## 置き場所

ROS bag と同じディレクトリに、`meta.json` という名前で置きます。

```text
recordings/episode_0001/
├── data.mcap
└── meta.json
```

Ingestor は既定でこれを読みます（`require_metadata: true` 相当）。`false` にしてよいのは、[メタデータを使うステージ](#どのステージに要るか)が無い、中身を覗くだけのパイプラインだけです。

書き出し済みの[中間フォーマット](intermediate-format_ja.md)を読み直すときは、メタデータは中に入っているので `meta.json` は要りません。

## 書き方（スキーマ v2.0）

そのままコピーして埋められる最小形です。値が無い欄は `null` と書きます。

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

各フィールドの意味:

- `uuid`: 録画ごとに一意な ID。出力フォルダ名と `rosbag_uuid` 列になります。
- `robot.type` / `robot.id`: ロボットの機種と個体。`type` はデータセットの `robot_type` に載ります。
- `files[]`: 録画ファイルの一覧。`type` が `mcap`、`rosbag2`、`rosbag` のどれかである項目が 1 つ必要です。
- `environment`: `type` は `real_world` か `simulation`。`site` は収録場所で、エピソードのメタデータに載ります。
- `runner`: 操作した主体。`type` は `operator` か `model`。
- `devices[]`: 使った機材の一覧。空でかまいません。
- `programs[]`: 収録に使ったソフトの出どころ。`role` が `interface`（または `teleoperation`）のものと `data_collection`（または `data_capture`）のものが 1 つずつ、それぞれ `source.git` 付きで必要です。
- `episode`: 録画全体のタスク。`label` が全体タスクの名前になります（空文字なら全体タスクなし）。
- `labels[]`: サブタスク名の一覧。LeRobot の `tasks.jsonl` に登録されます。
- `segments[]`: エピソードにする時間区間。`label_idx` は `labels[]` への添字、`success` は成否です。

時刻はすべて秒で、録画のメッセージと同じ時計で書きます。ROS bag のタイムスタンプが UNIX 時刻なら、ここも UNIX 時刻の秒です。0 起点で書いた区間は実際の録画と重ならず、変換が `no segments overlap` で失敗します。

## 出力への効き方

`LeRobotV21TransformerConfig` はこのファイルを次のように使います。

- `labels[]` の各項目がタスクになります。`episode.label` が空でなければ、それも録画全体のタスクとして登録されます。
- 各 `segment` が学習区間になります。区間の作り方は変換の `separate_per_primitive` 次第で、`false`（既定）なら重なったセグメント全部で 1 エピソード（境界の行に `next.done` が立つ）、`true` ならセグメントごとに 1 エピソードです。
- 同期後の時間軸と重なるセグメントだけが使われます。1 つも重ならなければ失敗です。
- `robot` / `environment.site` / `files[]` / `programs[]` は、データセットのメタデータにそのまま書き込まれます。

## どのステージに要るか

| ステージ | meta.json |
|---|---|
| Rosbag1 / Rosbag2 Ingestor | 既定で読む。`require_metadata: false` で省略可 |
| `UuidEnricherConfig` | 必須。無いと実行全体が止まります |
| `VideoEncoderConfig` / `DepthVideoConfig` | 必須（UUID が出力パスになるため） |
| `ParquetVideoExporterConfig` | 必須 |
| `LeRobotV21TransformerConfig` | 必須 |
| Synchronizer、TF / 差分 / シフトの Enricher、ImageEncoderConfig / DepthImageEncoderConfig | 不要 |

## 投入前チェック

失敗した実行を 1 回節約するための確認です。

- `meta.json` が各 ROS bag と同じディレクトリにある（この綴りで）。
- `uuid` が録画ごとに一意。
- `files[]` に `type` が `mcap` / `rosbag2` / `rosbag` の項目がある。
- `programs[]` に `interface` 系と `data_collection` 系が 1 つずつ、`source.git` 付きである。
- すべての `segments[].label_idx` が `labels[]` の範囲内。
- セグメントの秒が録画と同じ時計で、録画の範囲に収まっている。

## うまくいかないとき

| エラー文に含まれる断片 | 原因 | 直し方 |
|---|---|---|
| `failed to read meta.json` | ROS bag の隣に `meta.json` が無い | 同じディレクトリに置く（サブフォルダ不可） |
| `missing required data: airoa_metadata` | メタデータ必須のステージが、読み込まずに動いた | `require_metadata: true` に戻すか、そのステージを外す |
| `skipped:` で実行が止まる | `UuidEnricherConfig` がメタデータなしで走った | `meta.json` を置くか、このステージを外す |
| `no segments overlap` | セグメントの時刻が録画と重ならない | 秒の時計を確認。Synchronizer が入っているかも確認 |
| program や git のフィールド名を含む missing 系 | `programs[]` の 2 役が揃っていない | `interface` と `data_collection` を `source.git` 付きで書く |

ステージ順序起因のエラーは[設定の表](configuration_ja.md#うまくいかないとき)へ。
