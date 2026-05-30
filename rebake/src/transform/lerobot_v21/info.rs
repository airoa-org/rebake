use std::fs::{self, File};

use camino::Utf8Path;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use crate::core::stage::StageError;

use super::Feature;

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Info {
    pub codebase_version: String,
    pub robot_type: String,
    pub total_episodes: usize,
    pub total_frames: usize,
    pub total_tasks: usize,
    pub total_videos: usize,
    pub total_chunks: usize,
    pub chunks_size: usize,
    pub fps: usize,
    pub splits: IndexMap<String, String>,
    pub data_path: String,
    pub video_path: String,
    pub features: IndexMap<String, Feature>,
}

impl Info {
    pub fn save(&self, outdir: &Utf8Path) -> Result<(), StageError> {
        let path = outdir.join("meta/info.json");
        if let Some(parent) = path.parent()
            && !parent.exists()
        {
            fs::create_dir_all(parent)?;
        }
        let mut file = File::create(path)?;
        serde_json::to_writer_pretty(&mut file, self)?;
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::transform::lerobot_v21::{DType, Feature, VideoInfo};

    #[test]
    fn test_info() {
        let info_string = r#"{
    "codebase_version": "v2.1",
    "robot_type": "hsr",
    "total_episodes": 1,
    "total_frames": 401,
    "total_tasks": 3,
    "total_videos": 2,
    "total_chunks": 1,
    "chunks_size": 1000,
    "fps": 10,
    "splits": {
        "train": "0:1"
    },
    "data_path": "data/chunk-{episode_chunk:03d}/episode_{episode_index:06d}.parquet",
    "video_path": "videos/chunk-{episode_chunk:03d}/{video_key}/episode_{episode_index:06d}.mp4",
    "features": {
        "observation.image.head": {
            "dtype": "video",
            "shape": [
                480,
                640,
                3
            ],
            "names": [
                "height",
                "width",
                "channel"
            ],
            "info": {
                "video.fps": 10,
                "video.codec": "av1",
                "video.pix_fmt": "yuv420p",
                "video.is_depth_map": "false",
                "has_audio": "false"
            }
        },
        "observation.image.hand": {
            "dtype": "video",
            "shape": [
                480,
                640,
                3
            ],
            "names": [
                "height",
                "width",
                "channel"
            ],
            "info": {
                "video.fps": 10,
                "video.codec": "av1",
                "video.pix_fmt": "yuv420p",
                "video.is_depth_map": "false",
                "has_audio": "false"
            }
        },
        "observation.state": {
            "dtype": "float32",
            "shape": [
                8
            ],
            "names": [
                "arm_lift_joint",
                "arm_flex_joint",
                "arm_roll_joint",
                "wrist_flex_joint",
                "wrist_roll_joint",
                "hand_motor_joint",
                "head_pan_joint",
                "head_tilt_joint"
            ]
        },
        "observation.wrist.wrench": {
            "dtype": "float32",
            "shape": [
                6
            ],
            "names": [
                "force_x",
                "force_y",
                "force_z",
                "torque_x",
                "torque_y",
                "torque_z"
            ],
            "description": "Wrist wrench data (force and torque) flattened"
        },
        "observation.end_effector_pose.absolute": {
            "shape": [
                6
            ],
            "dtype": "float32",
            "names": [
                "x",
                "y",
                "z",
                "roll",
                "pitch",
                "yaw"
            ]
        },
        "observation.end_effector_pose.relative": {
            "shape": [
                6
            ],
            "dtype": "float32",
            "names": [
                "x",
                "y",
                "z",
                "roll",
                "pitch",
                "yaw"
            ]
        },
        "action.absolute": {
            "dtype": "float32",
            "shape": [
                8
            ],
            "names": [
                "arm_lift_joint",
                "arm_flex_joint",
                "arm_roll_joint",
                "wrist_flex_joint",
                "wrist_roll_joint",
                "hand_motor_joint",
                "head_pan_joint",
                "head_tilt_joint"
            ],
            "description": "absolute action for all joints without hand_motor_joint(gripper)"
        },
        "action.relative": {
            "dtype": "float32",
            "shape": [
                11
            ],
            "names": [
                "arm_lift_joint",
                "arm_flex_joint",
                "arm_roll_joint",
                "wrist_flex_joint",
                "wrist_roll_joint",
                "hand_motor_joint",
                "head_pan_joint",
                "head_tilt_joint",
                "base_x",
                "base_y",
                "base_t"
            ],
            "description": "delta action for all joints and base without hand_motor_joint(gripper)"
        },
        "action.arm": {
            "dtype": "float32",
            "shape": [
                5
            ],
            "names": [
                "arm_lift_joint",
                "arm_flex_joint",
                "arm_roll_joint",
                "wrist_flex_joint",
                "wrist_roll_joint"
            ],
            "description": "absolute action for arm joints"
        },
        "action.gripper": {
            "dtype": "float32",
            "shape": [
                1
            ],
            "names": [
                "hand_motor_joint"
            ],
            "description": "absolute action for gripper"
        },
        "action.head": {
            "dtype": "float32",
            "shape": [
                2
            ],
            "names": [
                "head_pan_joint",
                "head_tilt_joint"
            ],
            "description": "absolute action for head joints"
        },
        "action.base": {
            "dtype": "float32",
            "shape": [
                3
            ],
            "names": [
                "base_x",
                "base_y",
                "base_t"
            ],
            "description": "delta action for base"
        },
        "observation.image.head.is_fresh": {
            "dtype": "bool",
            "shape": [
                3,
                1,
                1
            ],
            "names": null
        },
        "observation.image.hand.is_fresh": {
            "dtype": "bool",
            "shape": [
                3,
                1,
                1
            ],
            "names": null
        },
        "observation.state.is_fresh": {
            "dtype": "bool",
            "shape": [
                8
            ],
            "names": null
        },
        "action.absolute.is_fresh": {
            "dtype": "bool",
            "shape": [
                8
            ],
            "names": null
        },
        "action.relative.is_fresh": {
            "dtype": "bool",
            "shape": [
                11
            ],
            "names": null
        },
        "action.arm.is_fresh": {
            "dtype": "bool",
            "shape": [
                5
            ],
            "names": null
        },
        "action.gripper.is_fresh": {
            "dtype": "bool",
            "shape": [
                1
            ],
            "names": null
        },
        "action.head.is_fresh": {
            "dtype": "bool",
            "shape": [
                2
            ],
            "names": null
        },
        "action.base.is_fresh": {
            "dtype": "bool",
            "shape": [
                3
            ],
            "names": null
        },
        "episode_index": {
            "dtype": "int64",
            "shape": [
                1
            ],
            "names": null
        },
        "frame_index": {
            "dtype": "int64",
            "shape": [
                1
            ],
            "names": null
        },
        "timestamp": {
            "dtype": "float32",
            "shape": [
                1
            ],
            "names": null
        },
        "next.done": {
            "dtype": "bool",
            "shape": [
                1
            ],
            "names": null
        },
        "index": {
            "dtype": "int64",
            "shape": [
                1
            ],
            "names": null
        },
        "short_horizon_task_index": {
            "dtype": "int64",
            "shape": [
                1
            ],
            "names": null
        },
        "primitive_action_index": {
            "dtype": "int64",
            "shape": [
                1
            ],
            "names": null
        },
        "success_primitive_action": {
            "dtype": "bool",
            "shape": [
                1
            ],
            "names": null
        },
        "task_index": {
            "dtype": "int64",
            "shape": [
                1
            ],
            "names": null
        }
    }
}"#;

        let info: Info = serde_json::from_str(info_string).unwrap();

        let expected_video_info = VideoInfo {
            fps: 10,
            codec: "av1".to_string(),
            pix_fmt: "yuv420p".to_string(),
            is_depth_map: false,
            has_audio: false,
        };
        let expected_info = Info {
            codebase_version: "v2.1".to_string(),
            robot_type: "hsr".to_string(),
            total_episodes: 1,
            total_frames: 401,
            total_tasks: 3,
            total_videos: 2,
            total_chunks: 1,
            chunks_size: 1000,
            fps: 10,
            splits: IndexMap::from([("train".to_string(), "0:1".to_string())]),
            data_path: "data/chunk-{episode_chunk:03d}/episode_{episode_index:06d}.parquet"
                .to_string(),
            video_path:
                "videos/chunk-{episode_chunk:03d}/{video_key}/episode_{episode_index:06d}.mp4"
                    .to_string(),
            features: IndexMap::from([
                (
                "observation.image.head".to_string(),
                Feature {
                    dtype: DType::Video,
                    shape: vec![480, 640, 3],
                    names: Some(vec![
                        "height".to_string(),
                        "width".to_string(),
                        "channel".to_string(),
                    ]),
                        video_info: Some(expected_video_info.clone()),
                        ..Default::default()
                    },
                ),
                (
                    "observation.image.hand".to_string(),
                    Feature {
                        dtype: DType::Video,
                        shape: vec![480, 640, 3],
                        names: Some(vec![
                            "height".to_string(),
                            "width".to_string(),
                            "channel".to_string(),
                        ]),
                        video_info: Some(expected_video_info.clone()),
                        ..Default::default()
                    },
                ),
                (
                    "observation.state".to_string(),
                    Feature {
                        dtype: DType::Float32,
                        shape: vec![8],
                        names: Some(vec![
                            "arm_lift_joint".to_string(),
                            "arm_flex_joint".to_string(),
                            "arm_roll_joint".to_string(),
                            "wrist_flex_joint".to_string(),
                            "wrist_roll_joint".to_string(),
                            "hand_motor_joint".to_string(),
                            "head_pan_joint".to_string(),
                            "head_tilt_joint".to_string(),
                        ]),
                        ..Default::default()
                    },
                ),
                (
                    "observation.wrist.wrench".to_string(),
                    Feature {
                        dtype: DType::Float32,
                        shape: vec![6],
                        names: Some(vec![
                            "force_x".to_string(),
                            "force_y".to_string(),
                            "force_z".to_string(),
                            "torque_x".to_string(),
                            "torque_y".to_string(),
                            "torque_z".to_string(),
                        ]),
                        description: Some("Wrist wrench data (force and torque) flattened".to_string()),
                        ..Default::default()
                    },
                ),
                (
                    "observation.end_effector_pose.absolute".to_string(),
                    Feature {
                        dtype: DType::Float32,
                        shape: vec![6],
                        names: Some(vec![
                            "x".to_string(),
                            "y".to_string(),
                            "z".to_string(),
                            "roll".to_string(),
                            "pitch".to_string(),
                            "yaw".to_string(),
                        ]),
                        ..Default::default()
                    },
                ),
                (
                    "observation.end_effector_pose.relative".to_string(),
                    Feature {
                        dtype: DType::Float32,
                        shape: vec![6],
                        names: Some(vec![
                            "x".to_string(),
                            "y".to_string(),
                            "z".to_string(),
                            "roll".to_string(),
                            "pitch".to_string(),
                            "yaw".to_string(),
                        ]),
                        ..Default::default()
                    },
                ),
                (
                    "action.absolute".to_string(),
                    Feature {
                        dtype: DType::Float32,
                        shape: vec![8],
                        names: Some(vec![
                            "arm_lift_joint".to_string(),
                            "arm_flex_joint".to_string(),
                            "arm_roll_joint".to_string(),
                            "wrist_flex_joint".to_string(),
                            "wrist_roll_joint".to_string(),
                            "hand_motor_joint".to_string(),
                            "head_pan_joint".to_string(),
                            "head_tilt_joint".to_string(),
                        ]),
                        description: Some("absolute action for all joints without hand_motor_joint(gripper)".to_string()),
                        ..Default::default()
                    },
                ),
                (
                    "action.relative".to_string(),
                    Feature {
                        dtype: DType::Float32,
                        shape: vec![11],
                        names: Some(vec![
                            "arm_lift_joint".to_string(),
                            "arm_flex_joint".to_string(),
                            "arm_roll_joint".to_string(),
                            "wrist_flex_joint".to_string(),
                            "wrist_roll_joint".to_string(),
                            "hand_motor_joint".to_string(),
                            "head_pan_joint".to_string(),
                            "head_tilt_joint".to_string(),
                            "base_x".to_string(),
                            "base_y".to_string(),
                            "base_t".to_string(),
                        ]),
                        description: Some("delta action for all joints and base without hand_motor_joint(gripper)".to_string()),
                        ..Default::default()
                    },
                ),
                (
                    "action.arm".to_string(),
                    Feature {
                        dtype: DType::Float32,
                        shape: vec![5],
                        names: Some(vec![
                            "arm_lift_joint".to_string(),
                            "arm_flex_joint".to_string(),
                            "arm_roll_joint".to_string(),
                            "wrist_flex_joint".to_string(),
                            "wrist_roll_joint".to_string(),
                        ]),
                        description: Some("absolute action for arm joints".to_string()),
                        ..Default::default()
                    },
                ),
                (
                    "action.gripper".to_string(),
                    Feature {
                        dtype: DType::Float32,
                        shape: vec![1],
                        names: Some(vec!["hand_motor_joint".to_string()]),
                        description: Some("absolute action for gripper".to_string()),
                        ..Default::default()
                    },
                ),
                (
                    "action.head".to_string(),
                    Feature {
                        dtype: DType::Float32,
                        shape: vec![2],
                        names: Some(vec![
                            "head_pan_joint".to_string(),
                            "head_tilt_joint".to_string(),
                        ]),
                        description: Some("absolute action for head joints".to_string()),
                        ..Default::default()
                    },
                ),
                (
                    "action.base".to_string(),
                    Feature {
                        dtype: DType::Float32,
                        shape: vec![3],
                        names: Some(vec![
                            "base_x".to_string(),
                            "base_y".to_string(),
                            "base_t".to_string(),
                        ]),
                        description: Some("delta action for base".to_string()),
                        ..Default::default()
                    },
                ),
                (
                    "observation.image.head.is_fresh".to_string(),
                    Feature {
                        dtype: DType::Bool,
                        shape: vec![3, 1, 1],
                        ..Default::default()
                    },
                ),
                (
                    "observation.image.hand.is_fresh".to_string(),
                    Feature {
                        dtype: DType::Bool,
                        shape: vec![3, 1, 1],
                        ..Default::default()
                    },
                ),
                (
                    "observation.state.is_fresh".to_string(),
                    Feature {
                        dtype: DType::Bool,
                        shape: vec![8],
                        ..Default::default()
                    },
                ),
                (
                    "action.absolute.is_fresh".to_string(),
                    Feature {
                        dtype: DType::Bool,
                        shape: vec![8],
                        ..Default::default()
                    },
                ),
                (
                    "action.relative.is_fresh".to_string(),
                    Feature {
                        dtype: DType::Bool,
                        shape: vec![11],
                        ..Default::default()
                    },
                ),
                (
                    "action.arm.is_fresh".to_string(),
                    Feature {
                        dtype: DType::Bool,
                        shape: vec![5],
                        ..Default::default()
                    },
                ),
                (
                    "action.gripper.is_fresh".to_string(),
                    Feature {
                        dtype: DType::Bool,
                        shape: vec![1],
                        ..Default::default()
                    },
                ),
                (
                    "action.head.is_fresh".to_string(),
                    Feature {
                        dtype: DType::Bool,
                        shape: vec![2],
                        ..Default::default()
                    },
                ),
                (
                    "action.base.is_fresh".to_string(),
                    Feature {
                        dtype: DType::Bool,
                        shape: vec![3],
                        ..Default::default()
                    },
                ),
                (
                    "episode_index".to_string(),
                    Feature {
                        dtype: DType::Int64,
                        shape: vec![1],
                        ..Default::default()
                    },
                ),
                (
                    "frame_index".to_string(),
                    Feature {
                        dtype: DType::Int64,
                        shape: vec![1],
                        ..Default::default()
                    },
                ),
                (
                    "timestamp".to_string(),
                    Feature {
                        dtype: DType::Float32,
                        shape: vec![1],
                        ..Default::default()
                    },
                ),
                (
                    "next.done".to_string(),
                    Feature {
                        dtype: DType::Bool,
                        shape: vec![1],
                        ..Default::default()
                    },
                ),
                (
                    "index".to_string(),
                    Feature {
                        dtype: DType::Int64,
                        shape: vec![1],
                        ..Default::default()
                    },
                ),
                (
                    "short_horizon_task_index".to_string(),
                    Feature {
                        dtype: DType::Int64,
                        shape: vec![1],
                        ..Default::default()
                    },
                ),
                (
                    "primitive_action_index".to_string(),
                    Feature {
                        dtype: DType::Int64,
                        shape: vec![1],
                        ..Default::default()
                    },
                ),
                (
                    "success_primitive_action".to_string(),
                    Feature {
                        dtype: DType::Bool,
                        shape: vec![1],
                        ..Default::default()
                    },
                ),
                (
                    "task_index".to_string(),
                    Feature {
                        dtype: DType::Int64,
                        shape: vec![1],
                        ..Default::default()
                    },
                ),
            ]),
        };

        // Test deserialization
        assert_eq!(info, expected_info);

        // Test serialization as JSON value
        let serialized = serde_json::to_string_pretty(&expected_info).unwrap();
        let serialized_json: serde_json::Value = serde_json::from_str(&serialized).unwrap();
        let expected_json: serde_json::Value = serde_json::from_str(info_string).unwrap();
        assert_eq!(serialized_json, expected_json);
    }
}
