use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetadataV1_3 {
    pub uuid: String,
    pub version: String,
    pub files: Vec<File>,
    pub context: Context,
    pub run: Run,
    #[serde(rename = "$schema", default)]
    pub schema: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct File {
    #[serde(rename = "type")]
    pub datatype: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Context {
    pub entities: Vec<Entity>,
    pub components: Vec<Component>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Entity {
    pub role: String,
    pub id: Option<String>,
    pub name: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Component {
    pub role: String,
    pub name: String,
    pub source: Source,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Source {
    pub git: Option<GitSource>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GitSource {
    pub uri: String,
    pub hash: String,
    pub branch: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Run {
    pub total_time_s: f64,
    pub instructions: Vec<Instruction>,
    pub segments: Vec<Segment>,
    #[serde(default)]
    pub episode_label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Instruction {
    pub idx: usize,
    pub text: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Segment {
    pub start_time: f64,
    pub end_time: f64,
    pub instruction_idx: usize,
    pub success: bool,
    pub controlled_by: String,
    #[serde(default)]
    pub is_composite: bool,
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn test_metadata_deserialize() {
        let metadata = r#"
        {
            "uuid": "1fe37622-18bb-4402-a12e-0d7c3d94e518",
            "version": "1.3",
            "files": [
                {
                "type": "rosbag",
                "name": "data.bag"
                }
            ],
            "context": {
                "entities": [
                {
                    "role": "robot",
                    "id": "robot002"
                },
                {
                    "role": "location",
                    "name": "location001"
                }
                ],
                "components": [
                {
                    "role": "interface",
                    "name": "hsr_leader_teleop",
                    "source": {
                    "git": {
                        "uri": "https://github.com/airoa-org/hsr_leader_teleop.git",
                        "hash": "v7.0.0",
                        "branch": "HEAD"
                    }
                    }
                },
                {
                    "role": "data_collection",
                    "name": "rosbag_manager",
                    "source": {
                    "git": {
                        "uri": "https://github.com/airoa-org/hsr_data_collection.git",
                        "hash": "v4.0.0",
                        "branch": "develop"
                    }
                    }
                }
                ]
            },
            "run": {
                "total_time_s": 132.19864225387573,
                "instructions": [
                {
                    "idx": 0,
                    "text": [
                    "open the oven toaster"
                    ]
                },
                {
                    "idx": 1,
                    "text": [
                    "pick up a slice of bread on the plate"
                    ]
                },
                {
                    "idx": 2,
                    "text": [
                    "place a slice of bread into the oven toaster"
                    ]
                },
                {
                    "idx": 3,
                    "text": [
                    "close the oven toaster"
                    ]
                },
                {
                    "idx": 4,
                    "text": [
                    "open the oven toaster"
                    ]
                },
                {
                    "idx": 5,
                    "text": [
                    "take a slice of bread out of the oven toaster"
                    ]
                },
                {
                    "idx": 6,
                    "text": [
                    "place a slice of bread on the plate"
                    ]
                },
                {
                    "idx": 7,
                    "text": [
                    "close the oven toaster"
                    ]
                },
                {
                    "idx": 8,
                    "text": [
                    "Bake a toast"
                    ]
                }
                ],
                "segments": [
                {
                    "start_time": 1750037087.8824904,
                    "end_time": 1750037108.5811408,
                    "instruction_idx": 0,
                    "success": true,
                    "controlled_by": "operator"
                },
                {
                    "start_time": 1750037117.0811522,
                    "end_time": 1750037132.2811499,
                    "instruction_idx": 1,
                    "success": true,
                    "controlled_by": "operator"
                },
                {
                    "start_time": 1750037132.681155,
                    "end_time": 1750037146.5811129,
                    "instruction_idx": 2,
                    "success": true,
                    "controlled_by": "operator"
                },
                {
                    "start_time": 1750037146.8814409,
                    "end_time": 1750037158.681084,
                    "instruction_idx": 3,
                    "success": true,
                    "controlled_by": "operator"
                },
                {
                    "start_time": 1750037158.9811552,
                    "end_time": 1750037173.6812286,
                    "instruction_idx": 4,
                    "success": true,
                    "controlled_by": "operator"
                },
                {
                    "start_time": 1750037173.9812186,
                    "end_time": 1750037187.2810903,
                    "instruction_idx": 5,
                    "success": true,
                    "controlled_by": "operator"
                },
                {
                    "start_time": 1750037197.4811676,
                    "end_time": 1750037197.6811655,
                    "instruction_idx": 6,
                    "success": true,
                    "controlled_by": "operator"
                },
                {
                    "start_time": 1750037219.7811415,
                    "end_time": 1750037220.0811327,
                    "instruction_idx": 7,
                    "success": true,
                    "controlled_by": "operator"
                },
                {
                    "start_time": 1750037087.8824904,
                    "end_time": 1750037220.0811327,
                    "instruction_idx": 8,
                    "success": true,
                    "controlled_by": "operator",
                    "is_composite": true
                }
                ],
                "episode_label": "Operator003"
            },
            "$schema": "https://raw.githubusercontent.com/airoa-org/airoa-metadata/refs/tags/v1.3/airoa_metadata/schemas/v1_3.json"
            }
        "#;
        let metadata: MetadataV1_3 = serde_json::from_str(metadata).unwrap();
        println!("{:?}", metadata);
    }

    #[test]
    fn test_metadata_deserialize_without_optional_fields() {
        // Test JSON without $schema and episode_label fields
        let metadata = r#"
        {
        "uuid": "03500db5-b165-4183-8ba8-470179b4158b",
        "version": "1.3",
        "files": [
            {
            "type": "rosbag2",
            "name": "data.bag_0.mcap"
            }
        ],
        "context": {
            "entities": [
            {
                "role": "robot",
                "id": "hsrf002"
            },
            {
                "role": "location",
                "name": "trc"
            },
            {
                "role": "operator",
                "id": "weblab-admin"
            },
            {
                "role": "task-template",
                "id": "template-1",
                "name": "Pick-and-Place Demo",
                "description": "Episode for object picking routine"
            }
            ],
            "components": [
            {
                "role": "interface",
                "name": "hsr_leader_teleop",
                "source": {
                "git": {
                    "uri": "unknown",
                    "hash": "unknown",
                    "branch": "unknown"
                }
                }
            },
            {
                "role": "data_capture",
                "name": "record_manager",
                "source": {
                "git": {
                    "uri": "https://github.com/airoa-org/hsrf_data_collection",
                    "hash": "",
                    "branch": ""
                }
                }
            }
            ]
        },
        "run": {
            "total_time_s": 1.5995078086853027,
            "instructions": [
            {
                "idx": 0,
                "text": [
                "Approach"
                ]
            },
            {
                "idx": 1,
                "text": [
                "Pick"
                ]
            },
            {
                "idx": 2,
                "text": [
                "Place"
                ]
            },
            {
                "idx": 3,
                "text": [
                "Pick-and-Place Demo"
                ]
            }
            ],
            "segments": [
            {
                "start_time": 1764656041.0943003,
                "end_time": 1764656041.5942738,
                "instruction_idx": 0,
                "controlled_by": "operator",
                "success": true,
                "is_composite": false
            },
            {
                "start_time": 1764656041.7943227,
                "end_time": 1764656042.0941849,
                "instruction_idx": 1,
                "controlled_by": "operator",
                "success": true,
                "is_composite": false
            },
            {
                "start_time": 1764656042.3945506,
                "end_time": 1764656043.1942227,
                "instruction_idx": 2,
                "controlled_by": "operator",
                "success": true,
                "is_composite": false
            },
            {
                "start_time": 1764656041.0943003,
                "end_time": 1764656043.1942227,
                "instruction_idx": 3,
                "controlled_by": "operator",
                "success": true,
                "is_composite": true
            }
            ]
        }
        }
        "#;
        let metadata: MetadataV1_3 = serde_json::from_str(metadata).unwrap();
        assert_eq!(metadata.uuid, "03500db5-b165-4183-8ba8-470179b4158b");
        assert_eq!(metadata.schema, ""); // default empty string
        assert_eq!(metadata.run.episode_label, ""); // default empty string
        assert_eq!(metadata.context.entities.len(), 4);
        assert_eq!(
            metadata.context.entities[3].description,
            Some("Episode for object picking routine".to_string())
        );
    }
}
