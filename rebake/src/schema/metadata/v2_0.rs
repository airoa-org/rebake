use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::v1_3::{self, MetadataV1_3};

const V2_0_SCHEMA_URL: &str = "https://raw.githubusercontent.com/airoa-org/airoa-metadata/main/airoa_metadata/schemas/v2_0.json";

#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema, Deserialize)]
pub struct MetadataV2_0 {
    #[serde(rename = "$schema")]
    #[schemars(url)]
    #[schemars(description = "JSON Schema URI that defines this metadata document.")]
    pub schema: String,
    #[schemars(regex(pattern = "^\\d+\\.\\d+$"))]
    #[schemars(description = "Schema version using major.minor notation (e.g. 2.0).")]
    pub schema_version: String,
    #[schemars(description = "Stable UUID that uniquely identifies this run.")]
    pub uuid: String,
    #[schemars(description = "Robot identity (URI, type, id) plus optional checksum.")]
    pub robot: Robot,
    #[schemars(length(min = 1))]
    #[schemars(description = "Input files or artifacts consumed by this run.")]
    pub files: Vec<File>,
    #[schemars(
        description = "Environment context (site, optional location, and type such as real_world or simulation)."
    )]
    pub environment: Environment,
    #[schemars(description = "Runner identity including type (operator or model) and name.")]
    pub runner: Runner,
    #[schemars(description = "Teleoperation devices referenced by this run.")]
    pub devices: Vec<Device>,
    #[schemars(length(min = 1))]
    #[schemars(
        description = "Programs or services (teleoperation/data logging) active in this run."
    )]
    pub programs: Vec<Program>,
    #[schemars(description = "Episode metadata: start/end timestamps, success flag, and label.")]
    pub episode: Episode,
    #[schemars(description = "Ordered list of high-level instructions executed in the run.")]
    pub labels: Vec<String>,
    #[schemars(description = "Execution segments aligned with the `labels` order.")]
    pub segments: Vec<Segment>,
}

#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema, Deserialize)]
pub struct Robot {
    #[schemars(url)]
    pub uri: Option<String>,
    #[serde(rename = "type")]
    #[schemars(description = "Type of robot (e.g. hsr).")]
    pub robot_type: String,
    #[schemars(description = "Robot identifier (e.g. hsr001).")]
    pub id: String,
    #[schemars(description = "Optional checksum used to verify the robot image or config.")]
    pub checksum: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema, Deserialize)]
pub struct File {
    #[serde(rename = "type")]
    #[schemars(description = "File type (e.g. rosbag or mcap).")]
    pub file_type: String,
    #[schemars(description = "Filename (e.g. data.bag).")]
    pub name: String,
    #[schemars(description = "Optional checksum to ensure file integrity.")]
    pub checksum: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema, Deserialize)]
pub struct Environment {
    #[serde(rename = "type")]
    #[schemars(description = "Environment type (e.g. real_world or simulation).")]
    pub env_type: EnvType,
    #[schemars(description = "Site or facility of the environment (e.g. TRC).")]
    pub site: String,
    #[schemars(description = "Optional location detail within the site (e.g. room_A).")]
    pub location: Option<String>,
}

#[derive(Debug, Clone, Serialize, JsonSchema, Deserialize, PartialEq, Eq)]
pub enum EnvType {
    #[serde(rename = "real_world")]
    RealWorld,
    #[serde(rename = "simulation")]
    Simulation,
}

#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema, Deserialize)]
pub struct Runner {
    #[serde(rename = "type")]
    #[schemars(description = "Runner type (e.g. operator or model).")]
    pub runner_type: RunnerType,
    #[schemars(description = "Organization of the runner (e.g. airoa). Empty if unavailable.")]
    pub organization: String,
    #[schemars(description = "Name of the runner (e.g. TarouTanaka).")]
    pub name: String,
}

#[derive(Debug, Clone, Serialize, JsonSchema, Deserialize, PartialEq, Eq)]
pub enum RunnerType {
    #[serde(rename = "operator")]
    Operator,
    #[serde(rename = "model")]
    Model,
}

#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema, Deserialize)]
pub struct Device {
    #[schemars(description = "Role of the device (e.g. controller).")]
    pub role: String,
    #[serde(rename = "type")]
    #[schemars(description = "Device type (e.g. joystick).")]
    pub device_type: String,
    #[schemars(description = "Device identifier (e.g. joystick001).")]
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema, Deserialize)]
pub struct Program {
    #[schemars(description = "Program role (e.g. teleoperation or data_collection).")]
    pub role: String,
    #[schemars(description = "Program name (e.g. hsr_leader_teleop).")]
    pub name: String,
    pub source: Source,
}

#[derive(Debug, Clone, Serialize, JsonSchema, Deserialize, PartialEq)]
pub struct Source {
    #[schemars(description = "Git repository snapshot for this artifact.")]
    pub git: Option<GitSource>,
}

#[derive(Debug, Clone, Serialize, JsonSchema, Deserialize, PartialEq)]
pub struct GitSource {
    #[schemars(url)]
    #[schemars(description = "Git repository URI.")]
    pub uri: String,
    #[schemars(description = "Commit hash.")]
    pub hash: String,
    #[schemars(description = "Branch name.")]
    pub branch: String,
    #[schemars(description = "Git tag.")]
    pub tag: Option<String>,
}

#[derive(Debug, Clone, Serialize, JsonSchema, Deserialize, PartialEq)]
pub struct Episode {
    #[schemars(range(min = 0))]
    #[schemars(description = "UNIX timestamp (seconds since epoch) when this episode began.")]
    pub start_time: f64,
    #[schemars(range(min = 0))]
    #[schemars(description = "UNIX timestamp (seconds since epoch) when this episode ended.")]
    pub end_time: f64,
    #[schemars(description = "True if the full episode succeeded.")]
    pub success: bool,
    #[schemars(description = "Human-readable label describing the episode task.")]
    pub label: String,
}

#[derive(Debug, Clone, Serialize, JsonSchema, Deserialize, PartialEq)]
pub struct Segment {
    #[schemars(range(min = 0))]
    #[schemars(description = "UNIX timestamp (seconds since epoch) when this segment began.")]
    pub start_time: f64,
    #[schemars(range(min = 0))]
    #[schemars(description = "UNIX timestamp (seconds since epoch) when this segment ended.")]
    pub end_time: f64,
    #[schemars(range(min = 0))]
    #[schemars(description = "Index within `labels` associated with this segment.")]
    pub label_idx: usize,
    #[schemars(description = "True if the segment objective succeeded.")]
    pub success: bool,
}

// =============================================================================
// Default impls — sensible defaults for partial construction from Python.
// Required-by-schema fields with no good default (e.g. Episode.label, File.name)
// have no Default; their parent must supply them explicitly.
// =============================================================================

impl Default for MetadataV2_0 {
    fn default() -> Self {
        Self {
            schema: V2_0_SCHEMA_URL.to_string(),
            schema_version: "2.0".to_string(),
            uuid: uuid::Uuid::new_v4().to_string(),
            robot: Robot::default(),
            files: Vec::new(),
            environment: Environment::default(),
            runner: Runner::default(),
            devices: Vec::new(),
            programs: vec![Program::default()],
            episode: Episode::default(),
            labels: Vec::new(),
            segments: Vec::new(),
        }
    }
}

impl Default for Robot {
    fn default() -> Self {
        Self {
            uri: None,
            robot_type: "unknown".to_string(),
            // Empty string honestly signals "no ID known"; auto-generating a
            // fresh uuid would falsely claim a specific robot identity.
            id: String::new(),
            checksum: None,
        }
    }
}

impl Default for Environment {
    fn default() -> Self {
        Self {
            env_type: EnvType::RealWorld,
            site: "unknown".to_string(),
            location: None,
        }
    }
}

impl Default for EnvType {
    fn default() -> Self {
        EnvType::RealWorld
    }
}

impl Default for Runner {
    fn default() -> Self {
        Self {
            runner_type: RunnerType::Operator,
            organization: String::new(),
            name: String::new(),
        }
    }
}

impl Default for RunnerType {
    fn default() -> Self {
        RunnerType::Operator
    }
}

impl Default for Program {
    fn default() -> Self {
        Self {
            role: "interface".to_string(),
            name: "unknown".to_string(),
            source: Source::default(),
        }
    }
}

impl Default for Source {
    fn default() -> Self {
        Self { git: None }
    }
}

impl Default for Episode {
    fn default() -> Self {
        Self {
            start_time: 0.0,
            end_time: 0.0,
            success: true,
            label: String::new(),
        }
    }
}

// =============================================================================
// Conversion from V1.3 to V2.0
// =============================================================================

use crate::core::StageError;

impl TryFrom<MetadataV1_3> for MetadataV2_0 {
    type Error = StageError;

    fn try_from(v1: MetadataV1_3) -> Result<Self, Self::Error> {
        let uuid = v1.uuid.clone();

        // Extract robot from entities
        let robot = extract_robot_from_entities(&v1.context.entities);

        // Extract environment from entities
        let environment = extract_environment_from_entities(&v1.context.entities);

        // Extract runner from entities
        let runner = extract_runner_from_entities(&v1.context.entities);

        // Convert files
        let files = v1
            .files
            .iter()
            .map(|f| File {
                file_type: f.datatype.clone(),
                name: f.name.clone(),
                checksum: None,
            })
            .collect();

        // Extract devices from entities (e.g., role="controller")
        let devices = extract_devices_from_entities(&v1.context.entities);

        // Convert components to programs
        let programs = v1
            .context
            .components
            .iter()
            .map(|c| Program {
                role: c.role.clone(),
                name: c.name.clone(),
                source: Source {
                    git: c.source.git.as_ref().map(|g| GitSource {
                        uri: g.uri.clone(),
                        hash: g.hash.clone(),
                        branch: g.branch.clone(),
                        tag: None,
                    }),
                },
            })
            .collect();

        // Partition segments: is_composite=true becomes episode, others stay as segments
        let (composite_segments, non_composite_segments): (Vec<_>, Vec<_>) =
            v1.run.segments.iter().partition(|s| s.is_composite);
        // Allow only the known legacy bug where the composite instruction reuses
        // the last non-composite instruction idx.
        let allowed_duplicate_instruction_idx = allowed_composite_duplicate_instruction_idx(
            &v1.run.instructions,
            &composite_segments,
            &non_composite_segments,
        );

        // Build episode from composite segment (use first one, or create default)
        let episode = if let Some(composite) = composite_segments.first() {
            let instruction =
                if Some(composite.instruction_idx) == allowed_duplicate_instruction_idx {
                    v1.run
                        .instructions
                        .iter()
                        .rev()
                        .find(|i| i.idx == composite.instruction_idx)
                } else {
                    v1.run
                        .instructions
                        .iter()
                        .find(|i| i.idx == composite.instruction_idx)
                }
                .ok_or_else(|| {
                    StageError::invalid(format!(
                        "composite segment references instruction_idx {} which does not exist",
                        composite.instruction_idx
                    ))
                })?;
            let label = instruction.text.first().cloned().ok_or_else(|| {
                StageError::invalid(format!(
                    "instruction {} has empty text array",
                    composite.instruction_idx
                ))
            })?;
            Episode {
                start_time: composite.start_time,
                end_time: composite.end_time,
                success: composite.success,
                label,
            }
        } else {
            // No composite segment - derive from all segments
            let start_time = non_composite_segments
                .first()
                .map(|s| s.start_time)
                .unwrap_or(0.0);
            let end_time = non_composite_segments
                .last()
                .map(|s| s.end_time)
                .unwrap_or(0.0);
            let success = if non_composite_segments.is_empty() {
                false
            } else {
                non_composite_segments.iter().all(|s| s.success)
            };
            Episode {
                start_time,
                end_time,
                success,
                label: v1.run.episode_label.clone(),
            }
        };

        // Build labels from instructions and keep a stable instruction_idx -> label_idx map.
        // This preserves instruction assignment across versions.
        let mut labels: Vec<String> = Vec::with_capacity(v1.run.instructions.len());
        let mut instruction_to_label_idx: HashMap<usize, usize> =
            HashMap::with_capacity(v1.run.instructions.len());
        for (label_idx, instruction) in v1.run.instructions.iter().enumerate() {
            if instruction_to_label_idx.contains_key(&instruction.idx)
                && Some(instruction.idx) != allowed_duplicate_instruction_idx
            {
                return Err(StageError::invalid(format!(
                    "duplicate instruction idx {} in V1.3 instructions",
                    instruction.idx
                )));
            }
            instruction_to_label_idx
                .entry(instruction.idx)
                .or_insert(label_idx);
            let label_text = instruction.text.first().cloned().ok_or_else(|| {
                StageError::invalid(format!(
                    "instruction {} has empty text array",
                    instruction.idx
                ))
            })?;
            labels.push(label_text);
        }

        // Convert non-composite segments. label_idx is derived from the instruction mapping,
        // not from the segment's position.
        let mut segments: Vec<Segment> = Vec::with_capacity(non_composite_segments.len());

        for (segment_idx, s) in non_composite_segments.iter().enumerate() {
            let label_idx = instruction_to_label_idx
                .get(&s.instruction_idx)
                .copied()
                .ok_or_else(|| {
                    StageError::invalid(format!(
                        "segment {} references instruction_idx {} which does not exist",
                        segment_idx, s.instruction_idx
                    ))
                })?;

            segments.push(Segment {
                start_time: s.start_time,
                end_time: s.end_time,
                label_idx,
                success: s.success,
            });
        }

        // Normalize to the current canonical schema URL for V2.0 output.
        let schema = V2_0_SCHEMA_URL.to_string();

        Ok(MetadataV2_0 {
            schema,
            schema_version: "2.0".to_string(),
            uuid,
            robot,
            files,
            environment,
            runner,
            devices,
            programs,
            episode,
            labels,
            segments,
        })
    }
}

/// Extract Robot from V1.3 entities.
///
fn extract_robot_from_entities(entities: &[v1_3::Entity]) -> Robot {
    let id = entities
        .iter()
        .find(|e| e.role == "robot")
        .and_then(|e| e.id.clone())
        .unwrap_or_default();

    Robot {
        uri: None,
        robot_type: "HSR".to_string(),
        id,
        checksum: None,
    }
}

/// Extract Environment from V1.3 entities.
fn extract_environment_from_entities(entities: &[v1_3::Entity]) -> Environment {
    let site = entities
        .iter()
        .find(|e| e.role == "location")
        .and_then(|e| e.name.clone())
        .unwrap_or_default();

    Environment {
        env_type: EnvType::RealWorld, // Default to real_world
        site,
        location: Some(String::new()),
    }
}

/// Extract Runner from V1.3 entities.
///
/// The runner name is determined from operator entity name when available.
/// If operator information is missing, name is set to an empty string.
/// Organization is taken from organization entity name when available.
fn extract_runner_from_entities(entities: &[v1_3::Entity]) -> Runner {
    let organization = entities
        .iter()
        .find(|e| e.role == "organization")
        .and_then(|e| e.name.clone())
        .unwrap_or_default();

    // Try to find operator entity first
    if let Some(operator) = entities.iter().find(|e| e.role == "operator") {
        let name = operator.name.clone().unwrap_or_default();

        return Runner {
            runner_type: RunnerType::Operator,
            organization,
            name,
        };
    }

    Runner {
        runner_type: RunnerType::Operator,
        organization,
        name: String::new(),
    }
}

/// Extract Devices from V1.3 entities (controller, joystick, etc.).
fn extract_devices_from_entities(entities: &[v1_3::Entity]) -> Vec<Device> {
    entities
        .iter()
        .filter(|e| matches!(e.role.as_str(), "controller" | "joystick" | "device"))
        .map(|e| Device {
            role: e.role.clone(),
            device_type: e.name.clone().unwrap_or_else(|| e.role.clone()),
            id: e.id.clone().unwrap_or_default(),
        })
        .collect()
}

/// Return the duplicated instruction idx only for the known V1.3 compatibility case.
///
/// Some legacy metadata writes the composite instruction with the same `idx` as the
/// last non-composite instruction. When that exact pattern appears, we keep using the
/// first instruction for normal segments and let the composite episode label use the
/// later instruction text. Any other duplicate pattern must still be rejected.
fn allowed_composite_duplicate_instruction_idx(
    instructions: &[v1_3::Instruction],
    composite_segments: &[&v1_3::Segment],
    non_composite_segments: &[&v1_3::Segment],
) -> Option<usize> {
    // The known bug only makes sense when there is both a composite segment and
    // at least one non-composite segment to collide with.
    let composite_instruction_idx = composite_segments.first()?.instruction_idx;
    let last_non_composite_instruction_idx = non_composite_segments.last()?.instruction_idx;
    if composite_instruction_idx != last_non_composite_instruction_idx {
        return None;
    }

    let mut instruction_counts: HashMap<usize, usize> = HashMap::new();
    let mut duplicated_idx: Option<usize> = None;

    // Accept at most one duplicated idx, and only if it appears exactly twice.
    for instruction in instructions {
        let count = instruction_counts.entry(instruction.idx).or_insert(0);
        *count += 1;

        if *count == 2 {
            if duplicated_idx.replace(instruction.idx).is_some() {
                return None;
            }
        } else if *count > 2 {
            return None;
        }
    }

    // The duplicated idx must be the same one referenced by the composite segment.
    let duplicated_idx = duplicated_idx?;
    if duplicated_idx != composite_instruction_idx {
        return None;
    }

    // The workaround is intentionally narrow: the duplicated composite instruction
    // must also be the final instruction entry in the V1.3 list.
    if instructions.last().map(|instruction| instruction.idx) != Some(duplicated_idx) {
        return None;
    }

    Some(duplicated_idx)
}
