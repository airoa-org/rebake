use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(transparent)]
pub struct TopicFeatureMap {
    pub map: Vec<TopicFeatureMapEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(tag = "type")]
pub enum TopicFeatureMapEntry {
    Parquet {
        /// ROS topic name
        topic: String,
        /// JSON Pointer (RFC 6901) identifying the target field within the ROS topic message structure.
        /// Example: `/points/0/positions`.
        field: String,
        /// LeRobot feature name
        feature: String,
        /// List of names for the feature dimensions
        #[serde(skip_serializing_if = "Option::is_none")]
        names: Option<Vec<String>>,
        /// Description of the feature
        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<String>,
    },
    Video {
        /// ROS topic name
        topic: String,
        /// LeRobot feature name
        feature: String,
        /// List of names for the feature dimensions
        #[serde(skip_serializing_if = "Option::is_none")]
        names: Option<Vec<String>>,
        /// Description of the feature
        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<String>,
    },
    Image {
        /// ROS topic name
        topic: String,
        /// LeRobot feature name
        feature: String,
        /// List of names for the feature dimensions
        #[serde(skip_serializing_if = "Option::is_none")]
        names: Option<Vec<String>>,
        /// Description of the feature
        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<String>,
    },
}

impl TopicFeatureMapEntry {
    pub fn topic(&self) -> &str {
        match self {
            TopicFeatureMapEntry::Parquet { topic, .. }
            | TopicFeatureMapEntry::Video { topic, .. }
            | TopicFeatureMapEntry::Image { topic, .. } => topic,
        }
    }

    pub fn feature(&self) -> &str {
        match self {
            TopicFeatureMapEntry::Parquet { feature, .. }
            | TopicFeatureMapEntry::Video { feature, .. }
            | TopicFeatureMapEntry::Image { feature, .. } => feature,
        }
    }

    pub fn names(&self) -> Option<&Vec<String>> {
        match self {
            TopicFeatureMapEntry::Parquet { names, .. }
            | TopicFeatureMapEntry::Video { names, .. }
            | TopicFeatureMapEntry::Image { names, .. } => names.as_ref(),
        }
    }

    pub fn description(&self) -> Option<&String> {
        match self {
            TopicFeatureMapEntry::Parquet { description, .. }
            | TopicFeatureMapEntry::Video { description, .. }
            | TopicFeatureMapEntry::Image { description, .. } => description.as_ref(),
        }
    }
}

/// Source of the robot model configuration.
///
/// Robot model defines the mapping from ROS topics to LeRobot features.
/// It can be provided as either:
/// - An inline `TopicFeatureMap` (for programmatic/API usage)
/// - A file path string pointing to a YAML file (for CLI/YAML config usage)
///
/// # Serialization
///
/// Uses `#[serde(untagged)]` so that:
/// - A JSON/YAML **array** deserializes as `Inline(TopicFeatureMap)`
/// - A JSON/YAML **string** deserializes as `Path(String)`
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RobotModelSource {
    /// Inline robot model data (TopicFeatureMap provided directly).
    Inline(TopicFeatureMap),
    /// Path to a YAML file containing the robot model.
    Path(String),
}

impl RobotModelSource {
    /// Resolve to a `TopicFeatureMap`.
    ///
    /// For `Inline`, returns the contained map directly (no I/O).
    /// For `Path`, reads and parses the YAML file from disk.
    pub fn resolve(&self) -> Result<TopicFeatureMap, std::io::Error> {
        match self {
            RobotModelSource::Inline(map) => Ok(map.clone()),
            RobotModelSource::Path(path) => {
                let content = std::fs::read_to_string(path)?;
                serde_yaml::from_str(&content)
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn test_json_array_deserializes_as_inline() {
        let json =
            r#"[{"type": "Parquet", "topic": "/joint", "field": "/pos", "feature": "obs.state"}]"#;
        let source: RobotModelSource = serde_json::from_str(json).unwrap();
        match &source {
            RobotModelSource::Inline(map) => assert_eq!(map.map[0].topic(), "/joint"),
            RobotModelSource::Path(_) => panic!("expected Inline"),
        }
    }

    #[test]
    fn test_json_string_deserializes_as_path() {
        let json = r#""./config/robot_model/hsr2.yaml""#;
        let source: RobotModelSource = serde_json::from_str(json).unwrap();
        assert_eq!(
            source,
            RobotModelSource::Path("./config/robot_model/hsr2.yaml".to_string())
        );
    }

    #[test]
    fn test_inline_json_roundtrip() {
        let original = RobotModelSource::Inline(TopicFeatureMap {
            map: vec![TopicFeatureMapEntry::Parquet {
                topic: "/joint".to_string(),
                field: "/pos".to_string(),
                feature: "obs.state".to_string(),
                names: Some(vec!["a".to_string()]),
                description: None,
            }],
        });
        let json = serde_json::to_string(&original).unwrap();
        let restored: RobotModelSource = serde_json::from_str(&json).unwrap();
        assert_eq!(original, restored);
    }
}
