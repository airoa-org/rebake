# Guide: create a dataset for a new robot

This guide takes you from ROS bags for a robot that rebake does not ship a config for to a LeRobot v2.1 dataset. You write two YAML files: a pipeline config and a robot model. If your robot already has a shipped config (YUBI / HSR / G2), you do not need this page. Pass that config directly to [`run`](cli.md#run).

The prerequisites are simple: `rebake-cli` is built as described in the [README](../README.md), and you have a directory of recordings. The example uses a fictional robot named `my_robot`: a 3-joint arm with one camera.

## 1. Add meta.json to each recording

rebake reads the recording identity, task labels, and episode time ranges (segments) from `meta.json` next to the ROS bag. Without it, conversion will not run.

```text
recordings/
├── episode_0001/
│   ├── data.mcap
│   └── meta.json
└── episode_0002/
    └── ...
```

For a copy-and-fill template and field meanings, see [metadata](metadata.md). The common failure is time: segment seconds must use the same clock as the messages in the recording. If they do not overlap, no episode is produced and the run fails.

## 2. Inspect the ROS bag contents

To write a robot model, you need topic names and field names. The fastest way is to export once to the intermediate format and inspect the Parquet files directly.

```bash
rebake-cli export ./recordings -o ./intermediate -j 8
duckdb -c "DESCRIBE SELECT * FROM './intermediate/*/parquet/joint_states.parquet'"
```

One table is one topic, and columns are message fields. Camera and depth pixels are stored as videos instead of table bytes. See [intermediate format](intermediate-format.md) for the output layout.

## 3. Write the robot model

A robot model declares which topic field becomes which dataset feature. Features are the column or video names read by training code, usually following the LeRobot `observation.*` and `action.*` convention.

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
  topic: /joint_states/action        # created by ShiftEnricherConfig in step 4
  field: /position
  feature: action.joint_position
  names: [shoulder, elbow, wrist]
```

`Parquet` maps a field to a column. `Video` maps a camera topic to video. Writing `names` lets rebake check the feature width against the data, which catches shape drift early. For entry types and field path syntax, see [configuration: robot model](configuration.md#robot-model).

## 4. Write the pipeline

A pipeline is an ordered list of stages. The minimal shape has three stages: read, synchronize to one timeline, write.

```yaml
# config/pipeline/my_robot.yaml
work_dir: "./orchestrator_work"
stage_configs:
  - Rosbag2IngestorConfig: {}              # use Rosbag1IngestorConfig for .bag
  - ZeroOrderHoldTimeSynchronizerConfig:
      fps: 30
  - LeRobotV21TransformerConfig:
      outdir: "./lerobot_my_robot"
      robot_model: "./config/robot_model/my_robot.yaml"
      video_config:
        fps: 30                            # must match the synchronizer fps
```

There is one important number: `video_config.fps` must match the synchronizer `fps`. A mismatch is not an error, but it silently creates a dataset whose videos and table rows do not line up.

For the `action.joint_position` feature from step 3, add one stage after synchronization and before transformation:

```yaml
  - ShiftEnricherConfig:
      source_topic: /joint_states
      output_topic: /joint_states/action
      shift_steps: 1
```

For adding end-effector poses from TF, keeping depth cameras, and other stage placement rules, see [configuration: stage order](configuration.md#stage-order). The shipped `config/pipeline/yubi.yaml` is a good complete example.

## 5. Run it

```bash
rebake-cli run ./recordings -c config/pipeline/my_robot.yaml -j 8
```

Each recording creates a dataset under `./lerobot_my_robot/<uuid>/`. To combine several recordings into one training dataset, use [`merge`](cli.md#merge) at the end.

## 6. Check the result

A run finishing is not the same thing as the dataset being right. Check three things.

Open `meta/info.json`. Confirm `fps` is the synchronizer rate and `total_episodes` is what you expect. By default one recording becomes one episode. If you set `separate_per_primitive: true`, each segment becomes one episode. If the count is lower than the segments you wrote, the segment times are outside the recording range.

Under `videos/chunk-000/`, confirm there is a folder with the same name as each `Video` feature in the robot model.

Finally, inspect columns:

```bash
duckdb -c "SELECT * FROM './lerobot_my_robot/*/data/chunk-000/episode_000000.parquet' LIMIT 5"
```

If the feature columns are present, the dataset is ready to load with the `lerobot` library and use for training.
