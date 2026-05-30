use std::collections::HashMap;

use nom::{
    IResult, Parser,
    branch::alt,
    bytes::complete::tag,
    character::complete::{alpha1, alphanumeric1},
    combinator::{map, recognize},
    multi::many0,
    sequence::pair,
};

use super::schema::{SchemaSection, split_schema_text_to_sections};
use super::types::{BaseType, FieldDefinition, FieldType, MessageDefinition, Primitive};
use crate::common::extract_short_type_name;
use crate::core::error::{StageError, StageResult};

/// Parses a ROS message definition string into `MessageDefinition` objects and stores them
/// in the provided `message_definition_table`.
///
/// This function handles message definitions that may contain nested types.
/// The `message_definition_table` uses short type names (e.g., "Header") as keys.
///
/// See: [`MessageDefinition`]
///
/// # Errors
///
/// Returns `StageError::InvalidData` if the schema text is malformed.
pub fn parse_schema_text_to_message_definition_table(
    schema_name: &str,
    schema_text: &str,
    message_definition_table: &mut HashMap<String, MessageDefinition>,
) -> StageResult<()> {
    let sections = split_schema_text_to_sections(schema_name, schema_text)?;
    parse_schema_sections_to_message_definition_table(&sections, message_definition_table)
}

/// Parses pre-split `SchemaSection`s into `MessageDefinition` objects.
///
/// This function iterates through `SchemaSection`s and adds any new message definitions
/// to the `message_definition_table`.
fn parse_schema_sections_to_message_definition_table(
    schema_sections: &[SchemaSection],
    message_definition_table: &mut HashMap<String, MessageDefinition>,
) -> StageResult<()> {
    for schema_section in schema_sections {
        let short_type_name = extract_short_type_name(schema_section.type_name.as_str());

        if message_definition_table.contains_key(short_type_name) {
            continue;
        }

        let mut fields = Vec::new();
        for line in schema_section.content.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with("#") || is_constant_line(trimmed) {
                continue;
            }

            let tokens = line.split_whitespace().collect::<Vec<&str>>();
            let short_type_name = extract_short_type_name(tokens[0]);
            let data_type = parse_ros_data_type(short_type_name)?;
            let name = tokens[1];
            let field = FieldDefinition::new(name.to_string(), data_type);
            fields.push(field);
        }

        let msg_definition = MessageDefinition::new(short_type_name.to_string(), fields);
        message_definition_table.insert(short_type_name.to_string(), msg_definition);
    }
    Ok(())
}

fn is_constant_line(line: &str) -> bool {
    line.contains("=") && !line.contains("[")
}

fn parse_ros_data_type(input: &str) -> StageResult<FieldType> {
    // Check for sequence type (e.g., "string[]").
    if input.ends_with("[]") {
        return parse_sequence_type_text(input);
    }

    // Check for fixed-size array type (e.g., "string[3]").
    if input.ends_with("]") {
        return parse_array_type_text(input);
    }

    // Otherwise, it's a base type (e.g., "string" or "std_msgs/Header").
    let data_type = parse_base_type_text(input)?;
    Ok(FieldType::Base(data_type))
}

/// Parses a sequence type string into a `FieldType::Sequence`.
///
/// # Example
/// `string[]` -> `FieldType::Sequence { data_type: BaseType::Primitive(Primitive::String) }`
///
/// # Errors
///
/// Returns `StageError::InvalidData` if the base type cannot be parsed.
fn parse_sequence_type_text(input: &str) -> StageResult<FieldType> {
    let data_type = parse_base_type_text(input.split_at(input.len() - 2).0)?;
    Ok(FieldType::Sequence(data_type))
}

/// Parses a fixed-size array type string into a `FieldType::Array`.
///
/// # Example
/// `string[3]` -> `FieldType::Array { data_type: BaseType::Primitive(Primitive::String), length: 3 }`
///
/// # Errors
///
/// Returns `StageError::InvalidData` if:
/// - The base type cannot be parsed
/// - The array length cannot be parsed as u32
fn parse_array_type_text(input: &str) -> StageResult<FieldType> {
    let data_type_and_length = input
        .split_at(input.len() - 1)
        .0
        .split('[')
        .collect::<Vec<&str>>();
    let data_type = parse_base_type_text(data_type_and_length[0])?;
    let length = data_type_and_length[1].parse::<u32>().map_err(|e| {
        StageError::invalid_with(
            format!(
                "array length '{}' is not a valid u32 in type '{}'",
                data_type_and_length[1], input
            ),
            e,
        )
    })?;

    Ok(FieldType::Array { data_type, length })
}

/// Parses a base type string into a `BaseType` (either a primitive or a struct).
///
/// # Example
/// `string` -> `BaseType::Primitive(Primitive::String)`
/// `Header` -> `BaseType::Struct("Header")`
///
/// # Errors
///
/// Returns `StageError::InvalidData` if the input is neither a valid primitive type
/// nor a valid struct identifier.
fn parse_base_type_text(input: &str) -> StageResult<BaseType> {
    if let Ok((_, prim)) = parse_primitive_type(input) {
        return Ok(BaseType::Primitive(prim));
    }

    parse_struct_type_text(input)
}

fn parse_primitive_type(input: &str) -> IResult<&str, Primitive> {
    let mut parser = alt((
        map(tag("bool"), |_| Primitive::Bool),
        map(tag("byte"), |_| Primitive::Byte),
        map(tag("char"), |_| Primitive::Char),
        map(tag("float32"), |_| Primitive::Float32),
        map(tag("float64"), |_| Primitive::Float64),
        map(tag("int8"), |_| Primitive::Int8),
        map(tag("uint8"), |_| Primitive::UInt8),
        map(tag("int16"), |_| Primitive::Int16),
        map(tag("uint16"), |_| Primitive::UInt16),
        map(tag("int32"), |_| Primitive::Int32),
        map(tag("uint32"), |_| Primitive::UInt32),
        map(tag("int64"), |_| Primitive::Int64),
        map(tag("uint64"), |_| Primitive::UInt64),
        map(tag("string"), |_| Primitive::String),
    ));
    parser.parse(input)
}

/// Parses a struct type string into a `BaseType::Struct`.
///
/// # Example
/// `Header` -> `BaseType::Struct("Header")`
/// `std_msgs/Header` -> `BaseType::Struct("std_msgs/Header")`
///
/// # Errors
///
/// Returns `StageError::InvalidData` if the input is not a valid struct identifier.
fn parse_struct_type_text(input: &str) -> StageResult<BaseType> {
    let mut parser = map(
        recognize(pair(
            parse_identifier,
            many0(pair(tag("/"), parse_identifier)),
        )),
        |full_type: &str| BaseType::Struct(full_type.to_string()),
    );
    let (_, full_type) = parser.parse(input).map_err(|e| {
        StageError::invalid(format!(
            "struct type '{}' is not a valid identifier: {:?}",
            input, e
        ))
    })?;
    Ok(full_type)
}

fn parse_identifier(input: &str) -> IResult<&str, &str> {
    let mut parser = recognize(pair(alpha1, many0(alt((alphanumeric1, tag("_"))))));
    parser.parse(input)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_camera_info() {
        let schema_name = "sensor_msgs/CameraInfo";
        let schema_text = include_str!("../../testdata/msgs/CameraInfo.msg");
        let mut message_definition_table = HashMap::new();
        parse_schema_text_to_message_definition_table(
            schema_name,
            schema_text,
            &mut message_definition_table,
        )
        .unwrap();

        let camera_info_definition = message_definition_table.get("CameraInfo").unwrap();
        let expected_camera_info_definition = MessageDefinition::new(
            "CameraInfo".to_string(),
            vec![
                FieldDefinition::new_struct("header".to_string(), "Header".to_string()),
                FieldDefinition::new_primitive("height".to_string(), Primitive::UInt32),
                FieldDefinition::new_primitive("width".to_string(), Primitive::UInt32),
                FieldDefinition::new_primitive("distortion_model".to_string(), Primitive::String),
                FieldDefinition::new_sequence(
                    "D".to_string(),
                    BaseType::Primitive(Primitive::Float64),
                ),
                FieldDefinition::new_array(
                    "K".to_string(),
                    BaseType::Primitive(Primitive::Float64),
                    9,
                ),
                FieldDefinition::new_array(
                    "R".to_string(),
                    BaseType::Primitive(Primitive::Float64),
                    9,
                ),
                FieldDefinition::new_array(
                    "P".to_string(),
                    BaseType::Primitive(Primitive::Float64),
                    12,
                ),
                FieldDefinition::new_primitive("binning_x".to_string(), Primitive::UInt32),
                FieldDefinition::new_primitive("binning_y".to_string(), Primitive::UInt32),
                FieldDefinition::new_struct("roi".to_string(), "RegionOfInterest".to_string()),
            ],
        );
        assert_eq!(*camera_info_definition, expected_camera_info_definition);

        let header_definition = message_definition_table.get("Header").unwrap();
        let expected_header_definition = MessageDefinition::new(
            "Header".to_string(),
            vec![
                FieldDefinition::new_primitive("seq".to_string(), Primitive::UInt32),
                FieldDefinition::new_struct("stamp".to_string(), "time".to_string()),
                FieldDefinition::new_primitive("frame_id".to_string(), Primitive::String),
            ],
        );
        assert_eq!(*header_definition, expected_header_definition);

        let region_of_interest_definition =
            message_definition_table.get("RegionOfInterest").unwrap();
        let expected_region_of_interest_definition = MessageDefinition::new(
            "RegionOfInterest".to_string(),
            vec![
                FieldDefinition::new_primitive("x_offset".to_string(), Primitive::UInt32),
                FieldDefinition::new_primitive("y_offset".to_string(), Primitive::UInt32),
                FieldDefinition::new_primitive("height".to_string(), Primitive::UInt32),
                FieldDefinition::new_primitive("width".to_string(), Primitive::UInt32),
                FieldDefinition::new_primitive("do_rectify".to_string(), Primitive::Bool),
            ],
        );
        assert_eq!(
            *region_of_interest_definition,
            expected_region_of_interest_definition
        );
    }

    #[test]
    fn test_parse_joint_state() {
        let schema_name = "sensor_msgs/JointState";
        let schema_text = include_str!("../../testdata/msgs/JointState.msg");
        let mut message_definition_table = HashMap::new();
        parse_schema_text_to_message_definition_table(
            schema_name,
            schema_text,
            &mut message_definition_table,
        )
        .unwrap();

        let joint_state_definition = message_definition_table.get("JointState").unwrap();
        let expected_joint_state_definition = MessageDefinition::new(
            "JointState".to_string(),
            vec![
                FieldDefinition::new_struct("header".to_string(), "Header".to_string()),
                FieldDefinition::new_sequence(
                    "name".to_string(),
                    BaseType::Primitive(Primitive::String),
                ),
                FieldDefinition::new_sequence(
                    "position".to_string(),
                    BaseType::Primitive(Primitive::Float64),
                ),
                FieldDefinition::new_sequence(
                    "velocity".to_string(),
                    BaseType::Primitive(Primitive::Float64),
                ),
                FieldDefinition::new_sequence(
                    "effort".to_string(),
                    BaseType::Primitive(Primitive::Float64),
                ),
            ],
        );
        assert_eq!(*joint_state_definition, expected_joint_state_definition);

        let header_definition = message_definition_table.get("Header").unwrap();
        let expected_header_definition = MessageDefinition::new(
            "Header".to_string(),
            vec![
                FieldDefinition::new_primitive("seq".to_string(), Primitive::UInt32),
                FieldDefinition::new_struct("stamp".to_string(), "time".to_string()),
                FieldDefinition::new_primitive("frame_id".to_string(), Primitive::String),
            ],
        );
        assert_eq!(*header_definition, expected_header_definition);
    }

    #[test]
    fn test_parse_twist() {
        let schema_name = "geometry_msgs/Twist";
        let schema_text = include_str!("../../testdata/msgs/Twist.msg");
        let mut message_definition_table = HashMap::new();
        parse_schema_text_to_message_definition_table(
            schema_name,
            schema_text,
            &mut message_definition_table,
        )
        .unwrap();

        let twist_definition = message_definition_table.get("Twist").unwrap();
        let expected_twist_definition = MessageDefinition::new(
            "Twist".to_string(),
            vec![
                FieldDefinition::new_struct("linear".to_string(), "Vector3".to_string()),
                FieldDefinition::new_struct("angular".to_string(), "Vector3".to_string()),
            ],
        );
        assert_eq!(*twist_definition, expected_twist_definition);

        let vector3_definition = message_definition_table.get("Vector3").unwrap();
        let expected_vector3_definition = MessageDefinition::new(
            "Vector3".to_string(),
            vec![
                FieldDefinition::new_primitive("x".to_string(), Primitive::Float64),
                FieldDefinition::new_primitive("y".to_string(), Primitive::Float64),
                FieldDefinition::new_primitive("z".to_string(), Primitive::Float64),
            ],
        );
        assert_eq!(*vector3_definition, expected_vector3_definition);
    }
}
