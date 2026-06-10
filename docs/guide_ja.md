# ガイド: 新しいロボットのデータセットを作る

rebake が設定を同梱していないロボットの ROS bag から、LeRobot v2.1 データセットを作るまでを通します。書くのは YAML 2 つ、パイプライン設定とロボットモデルだけです。同梱設定のあるロボット（YUBI / HSR / G2）なら、このページは要りません。[CLI](cli_ja.md#run) の `run` にそのまま渡してください。

前提は 2 つ。[README](../README_ja.md) の手順で `rebake-cli` がビルド済みであること、手元に録画のディレクトリがあることです。例として、3 関節アーム + カメラ 1 台の架空のロボット my_robot を最後まで使います。

## 1. 各録画に meta.json を置く

rebake は録画の素性、つまり ID、タスクのラベル、エピソードにする時間区間（セグメント）を、ROS bag の隣に置いた `meta.json` から読みます。これが無いと変換は動きません。

```text
recordings/
├── episode_0001/
│   ├── data.mcap
│   └── meta.json
└── episode_0002/
    └── ...
```

コピペして埋められる最小形とフィールドの意味は[メタデータ](metadata_ja.md)へ。いちばん事故が多いのは時刻です。セグメントの秒は録画のメッセージと同じ時計で書きます。ずれていると、エピソードがひとつもできずに失敗します。

## 2. ROS bag の中身を見る

ロボットモデルを書くには、トピック名とフィールド名が要ります。一度中間フォーマットに変換して、Parquet を直接覗くのが早道です。

```bash
rebake-cli export ./recordings -o ./intermediate -j 8
duckdb -c "DESCRIBE SELECT * FROM './intermediate/*/parquet/joint_states.parquet'"
```

テーブル 1 つがトピック 1 つ、列がメッセージのフィールドです。カメラと深度の画素はテーブルではなく動画になります。出来上がったディレクトリの中身は[中間フォーマット](intermediate-format_ja.md)に書いてあります。

## 3. ロボットモデルを書く

ロボットモデルは「どのトピックのどのフィールドを、データセットのどの feature にするか」の宣言です。feature は完成したデータセットで学習コードが読む列・動画の名前で、`observation.*` と `action.*` という LeRobot の流儀で付けます。

```yaml
# config/robot_model/my_robot.yaml
- type: Parquet
  topic: /joint_states
  field: /position
  feature: observation.state
  names: [shoulder, elbow, wrist]

- type: Video
  topic: /camera/image_raw/compressed
  feature: observation.image.head
  names: [height, width, channel]

- type: Parquet
  topic: /joint_states/action        # 手順 4 の ShiftEnricherConfig が作るトピック
  field: /position
  feature: action.joint_position
  names: [shoulder, elbow, wrist]
```

`Parquet` はフィールドを列に、`Video` はカメラトピックを動画にします。`names` を書いておくと次元数が実データと照合されるので、形のずれに早く気づけます。entry の種類と `field` の書き方は[設定: ロボットモデル](configuration_ja.md#ロボットモデル)へ。

## 4. パイプラインを書く

パイプラインはステージの順序付きリストです。最小はこの 3 段。読み込んで、1 本の時間軸に揃えて、書き出します。

```yaml
# config/pipeline/my_robot.yaml
work_dir: "./orchestrator_work"
stage_configs:
  - Rosbag2IngestorConfig: {}              # .bag なら Rosbag1IngestorConfig
  - ZeroOrderHoldTimeSynchronizerConfig:
      fps: 30
  - LeRobotV21TransformerConfig:
      outdir: "./lerobot_my_robot"
      robot_model: "./config/robot_model/my_robot.yaml"
      video_config:
        fps: 30                            # 同期の fps と必ず同じ値に
```

守る数字はひとつだけです。`video_config.fps` は同期の `fps` と同じ値にします。違っていてもエラーにはならず、動画と行がずれたデータセットが黙ってできあがります。

あとは必要な分だけステージを足します。手順 3 の `action.joint_position` のためには、同期の後ろ（変換の前）に 1 段:

```yaml
  - ShiftEnricherConfig:
      source_topic: /joint_states
      output_topic: /joint_states/action
      shift_steps: 1
```

TF から手先姿勢を出す、深度カメラを残す、といった足し方は[設定: ステージの並べ方](configuration_ja.md#ステージの並べ方)へ。全部入りの実例としては、同梱の `config/pipeline/yubi.yaml` が読みやすいです。

## 5. 実行する

```bash
rebake-cli run ./recordings -c config/pipeline/my_robot.yaml -j 8
```

データセットは録画ごとに `./lerobot_my_robot/<uuid>/` にできます。複数の録画を 1 つの学習用データセットにまとめるのは、最後に [merge](cli_ja.md#merge) の仕事です。

## 6. 出来上がりを確かめる

走り切ったことと、正しくできたことは別です。見るのは 3 点だけです。

`meta/info.json` を開きます。`fps` が同期に指定した値になっているか。`total_episodes` が意図どおりか。既定では録画全体で 1 エピソードなので 1、セグメントごとに分けた（`separate_per_primitive: true`）ならセグメント数です。書いたはずのセグメントより少なければ、時刻が録画の外にはみ出しています（手順 1 の時計を確認）。

`videos/chunk-000/` の下に、ロボットモデルの `Video` feature と同じ名前のフォルダがあるか確かめます。

最後に列を見ます。

```bash
duckdb -c "SELECT * FROM './lerobot_my_robot/*/data/chunk-000/episode_000000.parquet' LIMIT 5"
```

feature の列が揃っていれば完成です。`lerobot` ライブラリで読み込んで、学習に進んでください。
