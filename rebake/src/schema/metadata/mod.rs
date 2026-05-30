//! Airoa metadata parsing and conversion.
//!
//! Handles parsing of Airoa metadata files (V1.3 and V2.0 formats)
//! and provides conversion between versions.
//!
//! # Supported Formats
//!
//! - V1.3: Legacy format with `version` field
//! - V2.0: Current format with `schema_version` field and extended segment info
//!
//! # Responsibilities
//!
//! - Owns: Metadata parsing, version detection, V1.3→V2.0 conversion, Arrow serialization (via [`arrow`] submodule)
//! - Does not own: Arrow schema construction from ROS types (see [`crate::arrow`] module)

pub mod arrow;
pub mod v1_3;
pub mod v2_0;

pub use v1_3::MetadataV1_3;
pub use v2_0::MetadataV2_0;

use crate::core::StageError;

/// Airoa metadata that can hold either V1.3 or V2.0 format.
///
/// This enum allows storing metadata in its original format without immediate conversion.
/// Conversion to V2.0 can be deferred to later stages (e.g., Transform stage).
#[derive(Debug, Clone, serde::Serialize)]
#[serde(untagged)]
// Note: V2_0 is larger than V1_3 by design (more comprehensive metadata).
// Boxing would add indirection and API complexity without significant benefit.
#[allow(clippy::large_enum_variant)]
pub enum AiroaMetadata {
    V1_3(MetadataV1_3),
    V2_0(MetadataV2_0),
}

impl AiroaMetadata {
    /// Convert to V2.0 format, performing conversion if necessary.
    ///
    /// - If already V2.0, returns a clone of the metadata
    /// - If V1.3, performs conversion to V2.0
    ///
    /// # Errors
    /// Returns an error if V1.3 to V2.0 conversion fails (e.g., missing instruction references).
    pub fn into_v2_0(self) -> Result<MetadataV2_0, StageError> {
        match self {
            AiroaMetadata::V1_3(v1_3) => MetadataV2_0::try_from(v1_3),
            AiroaMetadata::V2_0(v2_0) => Ok(v2_0),
        }
    }

    /// Get the version of this metadata.
    pub fn version(&self) -> MetadataVersion {
        match self {
            AiroaMetadata::V1_3(_) => MetadataVersion::V1_3,
            AiroaMetadata::V2_0(_) => MetadataVersion::V2_0,
        }
    }

    /// Get UUID as string (works for both versions).
    pub fn uuid_string(&self) -> String {
        match self {
            AiroaMetadata::V1_3(v1_3) => v1_3.uuid.clone(),
            AiroaMetadata::V2_0(v2_0) => v2_0.uuid.clone(),
        }
    }
}

impl From<MetadataV1_3> for AiroaMetadata {
    fn from(v1_3: MetadataV1_3) -> Self {
        AiroaMetadata::V1_3(v1_3)
    }
}

impl From<MetadataV2_0> for AiroaMetadata {
    fn from(v2_0: MetadataV2_0) -> Self {
        AiroaMetadata::V2_0(v2_0)
    }
}

/// Metadata version detected from JSON content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetadataVersion {
    V1_3,
    V2_0,
}

/// Detect the metadata version from JSON content.
///
/// This function examines the JSON structure to determine the version:
/// - V2_0: Has `schema_version` field with value "2.0"
/// - V1_3: Has `version` field with value "1.3" (or any other version pattern)
pub fn detect_version(json: &str) -> Result<MetadataVersion, StageError> {
    let value: serde_json::Value = serde_json::from_str(json)
        .map_err(|e| StageError::invalid_with("failed to parse JSON for version detection", e))?;

    if let Some(schema_version) = value.get("schema_version").and_then(|v| v.as_str())
        && schema_version.starts_with("2.")
    {
        return Ok(MetadataVersion::V2_0);
    }

    if let Some(version) = value.get("version").and_then(|v| v.as_str())
        && version.starts_with("1.")
    {
        return Ok(MetadataVersion::V1_3);
    }

    Err(StageError::invalid(
        "could not detect metadata version: missing 'version' or 'schema_version' field",
    ))
}

/// Parse metadata JSON and return it in its original format (V1.3 or V2.0).
///
/// This function detects the version and parses without conversion.
/// Use `AiroaMetadata::into_v2_0()` to convert later if needed.
pub fn parse_metadata(json: &str) -> Result<AiroaMetadata, StageError> {
    let version = detect_version(json)?;
    match version {
        MetadataVersion::V2_0 => {
            let v2_0: MetadataV2_0 = serde_json::from_str(json)
                .map_err(|e| StageError::invalid_with("failed to parse V2.0 metadata", e))?;
            Ok(AiroaMetadata::V2_0(v2_0))
        }
        MetadataVersion::V1_3 => {
            let v1_3: MetadataV1_3 = serde_json::from_str(json)
                .map_err(|e| StageError::invalid_with("failed to parse V1.3 metadata", e))?;
            Ok(AiroaMetadata::V1_3(v1_3))
        }
    }
}

/// Parse metadata JSON and return V2_0, converting from V1_3 if necessary.
///
/// This function automatically detects the version and converts to V2_0:
/// - If the JSON is V2_0 format, it's parsed directly
/// - If the JSON is V1_3 format, it's parsed and converted to V2_0
///
/// Note: Consider using `parse_metadata()` instead and deferring conversion
/// to later stages for better separation of concerns.
pub fn parse_metadata_as_v2_0(json: &str) -> Result<MetadataV2_0, StageError> {
    parse_metadata(json)?.into_v2_0()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    /// Test conversion from V1.3 to V2.0 using actual testdata files.
    ///
    /// This test reads the V1.3 metadata testdata, converts it to V2.0,
    /// and compares the result against the expected V2.0 testdata file.
    /// The comparison uses PartialEq on MetadataV2_0, ensuring all fields match.
    ///
    /// This single test covers:
    /// - V1.3 JSON parsing
    /// - V2.0 JSON parsing
    /// - Version detection
    /// - V1.3 → V2.0 conversion logic
    /// - All field equality via PartialEq
    #[test]
    fn test_convert_v1_3_to_v2_0_from_testdata() {
        use std::fs;

        // Read V1.3 metadata from testdata
        let v1_3_json = fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/testdata/metadata/v1.3/meta.json"
        ))
        .expect("failed to read V1.3 testdata file");

        // Read expected V2.0 metadata from testdata
        let expected_json = fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/testdata/metadata/v2.0/meta.json"
        ))
        .expect("failed to read V2.0 testdata file");

        // Parse V1.3 and convert to V2.0
        let converted = parse_metadata_as_v2_0(&v1_3_json).expect("failed to convert V1.3 to V2.0");

        // Parse expected V2.0
        let expected: MetadataV2_0 =
            serde_json::from_str(&expected_json).expect("failed to parse expected V2.0 metadata");

        // Compare entire structs using PartialEq
        assert_eq!(converted, expected);
    }

    /// Test Arrow RecordBatch conversion with testdata.
    #[test]
    fn test_arrow_conversion_with_testdata() {
        use super::arrow::airoa_metadata_to_record_batch;
        use std::fs;

        // Test V2.0 → Arrow
        let v2_0_path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/testdata/metadata/v2.0/meta.json"
        );
        let v2_0_json = fs::read_to_string(v2_0_path).expect("failed to read V2.0 testdata file");

        let metadata = parse_metadata(&v2_0_json).expect("failed to parse V2.0 metadata");

        let batch = airoa_metadata_to_record_batch(&metadata)
            .expect("failed to convert to Arrow RecordBatch");

        assert_eq!(batch.num_rows(), 1);
        assert!(batch.num_columns() > 0);

        // Verify expected columns exist
        let schema = batch.schema();
        let column_names: Vec<&str> = schema.fields().iter().map(|f| f.name().as_str()).collect();
        assert!(column_names.contains(&"uuid"));
        assert!(column_names.contains(&"schema_version"));
        assert!(column_names.contains(&"robot"));
        assert!(column_names.contains(&"environment"));
        assert!(column_names.contains(&"segments"));
    }

    /// Test Arrow conversion with V1.3 metadata (via AiroaMetadata enum).
    #[test]
    fn test_arrow_conversion_with_v1_3_testdata() {
        use super::arrow::airoa_metadata_to_record_batch;
        use std::fs;

        let v1_3_path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/testdata/metadata/v1.3/meta.json"
        );
        let v1_3_json = fs::read_to_string(v1_3_path).expect("failed to read V1.3 testdata file");

        let metadata = parse_metadata(&v1_3_json).expect("failed to parse V1.3 metadata");

        // V1.3 metadata should serialize to Arrow with V1.3 schema
        let batch = airoa_metadata_to_record_batch(&metadata)
            .expect("failed to convert V1.3 to Arrow RecordBatch");

        assert_eq!(batch.num_rows(), 1);
        assert!(batch.num_columns() > 0);

        // V1.3 has different column names
        let schema = batch.schema();
        let column_names: Vec<&str> = schema.fields().iter().map(|f| f.name().as_str()).collect();
        assert!(column_names.contains(&"uuid"));
        assert!(column_names.contains(&"version")); // V1.3 uses "version" not "schema_version"
    }

    /// Test parsing V2.0 with robot URI field.
    #[test]
    fn test_parse_v2_0_with_robot_uri() {
        let json = r#"
        {
            "$schema": "https://raw.githubusercontent.com/airoa-org/airoa-metadata/main/airoa_metadata/schemas/v2_0.json",
            "schema_version": "2.0",
            "uuid": "123e4567-e89b-12d3-a456-426614174000",
            "robot": {
                "type": "HSR",
                "id": "robot-001",
                "uri": "https://example.com/robot.yaml"
            },
            "files": [],
            "environment": {
                "type": "real_world",
                "site": "test-site"
            },
            "runner": {
                "type": "operator",
                "organization": "",
                "name": "op-001"
            },
            "devices": [],
            "programs": [],
            "episode": {
                "start_time": 0.0,
                "end_time": 1.0,
                "success": true,
                "label": "test"
            },
            "labels": ["test"],
            "segments": [
                {
                    "start_time": 0.0,
                    "end_time": 1.0,
                    "label_idx": 0,
                    "success": true
                }
            ]
        }
        "#;

        let metadata = parse_metadata_as_v2_0(json).expect("failed to parse V2.0 with robot uri");
        assert_eq!(
            metadata.robot.uri.as_deref(),
            Some("https://example.com/robot.yaml")
        );
    }

    /// Test that composite segment label must come from a valid instruction.
    #[test]
    fn test_convert_v1_3_fails_when_composite_instruction_is_missing() {
        let v1_3_json = r#"
        {
            "uuid": "123e4567-e89b-12d3-a456-426614174000",
            "version": "1.3",
            "files": [
                {"type": "rosbag", "name": "data.bag"}
            ],
            "context": {
                "entities": [
                    {"role": "robot", "id": "robot-001"},
                    {"role": "location", "name": "test-site"}
                ],
                "components": []
            },
            "run": {
                "total_time_s": 1.0,
                "instructions": [
                    {"idx": 0, "text": ["single instruction"]}
                ],
                "segments": [
                    {
                        "start_time": 0.0,
                        "end_time": 1.0,
                        "instruction_idx": 999,
                        "success": true,
                        "controlled_by": "operator",
                        "is_composite": true
                    }
                ],
                "episode_label": "legacy label"
            }
        }
        "#;

        let err = parse_metadata_as_v2_0(v1_3_json).expect_err("expected conversion to fail");
        assert!(
            err.to_string()
                .contains("composite segment references instruction_idx"),
            "unexpected error: {err}"
        );
    }

    /// Allow the known V1.3 bug where the composite instruction reuses the
    /// last non-composite instruction idx.
    #[test]
    fn test_convert_v1_3_allows_duplicate_idx_for_composite_instruction() {
        let v1_3_json = r#"
        {
            "uuid": "123e4567-e89b-12d3-a456-426614174000",
            "version": "1.3",
            "files": [
                {"type": "rosbag", "name": "data.bag"}
            ],
            "context": {
                "entities": [
                    {"role": "robot", "id": "robot-001"},
                    {"role": "location", "name": "test-site"}
                ],
                "components": []
            },
            "run": {
                "total_time_s": 3.0,
                "instructions": [
                    {"idx": 0, "text": ["first task"]},
                    {"idx": 1, "text": ["last non-composite task"]},
                    {"idx": 1, "text": ["composite task"]}
                ],
                "segments": [
                    {
                        "start_time": 0.0,
                        "end_time": 1.0,
                        "instruction_idx": 0,
                        "success": true,
                        "controlled_by": "operator"
                    },
                    {
                        "start_time": 1.0,
                        "end_time": 2.0,
                        "instruction_idx": 1,
                        "success": true,
                        "controlled_by": "operator"
                    },
                    {
                        "start_time": 0.0,
                        "end_time": 2.0,
                        "instruction_idx": 1,
                        "success": true,
                        "controlled_by": "operator",
                        "is_composite": true
                    }
                ],
                "episode_label": "legacy label"
            }
        }
        "#;

        let metadata =
            parse_metadata_as_v2_0(v1_3_json).expect("expected conversion with known duplicate");

        assert_eq!(metadata.episode.label, "composite task");
        assert_eq!(
            metadata.labels[metadata.segments[1].label_idx],
            "last non-composite task"
        );
    }

    /// Keep rejecting duplicate instruction idx values outside the known
    /// composite-instruction compatibility case.
    #[test]
    fn test_convert_v1_3_rejects_other_duplicate_instruction_idx_values() {
        let v1_3_json = r#"
        {
            "uuid": "123e4567-e89b-12d3-a456-426614174000",
            "version": "1.3",
            "files": [
                {"type": "rosbag", "name": "data.bag"}
            ],
            "context": {
                "entities": [
                    {"role": "robot", "id": "robot-001"},
                    {"role": "location", "name": "test-site"}
                ],
                "components": []
            },
            "run": {
                "total_time_s": 2.0,
                "instructions": [
                    {"idx": 0, "text": ["first task"]},
                    {"idx": 0, "text": ["unexpected duplicate"]}
                ],
                "segments": [
                    {
                        "start_time": 0.0,
                        "end_time": 1.0,
                        "instruction_idx": 0,
                        "success": true,
                        "controlled_by": "operator"
                    }
                ],
                "episode_label": "legacy label"
            }
        }
        "#;

        let err = parse_metadata_as_v2_0(v1_3_json).expect_err("expected duplicate idx failure");
        assert!(
            err.to_string()
                .contains("duplicate instruction idx 0 in V1.3 instructions"),
            "unexpected error: {err}"
        );
    }

    /// Test conversion defaults when robot/location entities are absent.
    #[test]
    fn test_convert_v1_3_without_robot_or_location_uses_empty_defaults() {
        let v1_3_json = r#"
        {
            "uuid": "123e4567-e89b-12d3-a456-426614174000",
            "version": "1.3",
            "files": [
                {"type": "rosbag", "name": "data.bag"}
            ],
            "context": {
                "entities": [],
                "components": []
            },
            "run": {
                "total_time_s": 1.0,
                "instructions": [
                    {"idx": 0, "text": ["single instruction"]}
                ],
                "segments": [],
                "episode_label": "legacy label"
            }
        }
        "#;

        let metadata =
            parse_metadata_as_v2_0(v1_3_json).expect("expected conversion with empty defaults");
        assert_eq!(metadata.robot.id, "");
        assert_eq!(metadata.environment.site, "");
        assert_eq!(metadata.episode.label, "legacy label");
        assert!(!metadata.episode.success);
    }

    /// Test that organization entity name is mapped to runner.organization.
    #[test]
    fn test_convert_v1_3_uses_organization_entity_name_for_runner() {
        let v1_3_json = r#"
        {
            "uuid": "123e4567-e89b-12d3-a456-426614174000",
            "version": "1.3",
            "files": [
                {"type": "rosbag", "name": "data.bag"}
            ],
            "context": {
                "entities": [
                    {"role": "robot", "id": "robot-001"},
                    {"role": "location", "name": "test-site"},
                    {"role": "organization", "name": "airoa"},
                    {"role": "operator", "name": "op-name"}
                ],
                "components": []
            },
            "run": {
                "total_time_s": 1.0,
                "instructions": [
                    {"idx": 0, "text": ["single instruction"]}
                ],
                "segments": [],
                "episode_label": "legacy label"
            }
        }
        "#;

        let metadata =
            parse_metadata_as_v2_0(v1_3_json).expect("expected conversion with organization");
        assert_eq!(metadata.runner.organization, "airoa");
        assert_eq!(metadata.runner.name, "op-name");
    }

    /// Test that invalid legacy UUID values are preserved during V1.3 -> V2.0 conversion.
    #[test]
    fn test_convert_v1_3_with_invalid_uuid_keeps_value() {
        let v1_3_json = r#"
        {
            "uuid": "legacy_uuid_without_rfc4122_format",
            "version": "1.3",
            "files": [
                {"type": "rosbag", "name": "data.bag"}
            ],
            "context": {
                "entities": [],
                "components": []
            },
            "run": {
                "total_time_s": 1.0,
                "instructions": [
                    {"idx": 0, "text": ["single instruction"]}
                ],
                "segments": [
                    {
                        "start_time": 0.0,
                        "end_time": 1.0,
                        "instruction_idx": 0,
                        "success": true,
                        "controlled_by": "operator"
                    }
                ],
                "episode_label": "legacy label"
            }
        }
        "#;

        let metadata =
            parse_metadata_as_v2_0(v1_3_json).expect("expected conversion with legacy uuid");
        assert_eq!(metadata.uuid, "legacy_uuid_without_rfc4122_format");
    }
}
