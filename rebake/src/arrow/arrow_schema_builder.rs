use arrow::datatypes::{DataType, Field, Schema};
use std::collections::HashMap;
use std::sync::Arc;

use crate::common::extract_short_type_name;
use crate::core::error::{StageError, StageResult};
use crate::ros::types::{BaseType, FieldDefinition, FieldType, MessageDefinition, Primitive};

/// Builds an Arrow `Schema` from a ROS `MessageDefinition`.
pub struct ArrowSchemaBuilder<'a> {
    message_definition_table: &'a HashMap<String, MessageDefinition>,
}

impl<'a> ArrowSchemaBuilder<'a> {
    pub fn new(message_definition_table: &'a HashMap<String, MessageDefinition>) -> Self {
        Self {
            message_definition_table,
        }
    }

    /// Builds an Arrow `Schema` for a given ROS message type name.
    ///
    /// # Errors
    ///
    /// Returns `StageError::InvalidData` if:
    /// - The `type_name` is not found in `message_definition_table`
    /// - A nested struct type referenced by the message is not defined
    pub fn build(&self, type_name: &str) -> StageResult<Arc<Schema>> {
        let short_type_name = extract_short_type_name(type_name);
        let message_definition = self
            .message_definition_table
            .get(short_type_name)
            .ok_or_else(|| {
                StageError::invalid(format!(
                    "type '{}' not found in message_definition_table",
                    type_name
                ))
            })?;

        // All schemas include transport-level timestamps before message fields.
        let mut fields = Vec::new();
        fields.push(Field::new(
            "timestamp_ns".to_string(),
            DataType::UInt64,
            true,
        ));
        fields.push(Field::new(
            "publish_timestamp_ns".to_string(),
            DataType::UInt64,
            true,
        ));

        // Convert each field in the ROS message definition to an Arrow `Field`.
        let data_fields: Result<Vec<Field>, _> = message_definition
            .fields
            .iter()
            .map(|field| self.ros_field_to_arrow_field(field))
            .collect();
        fields.extend(data_fields?);

        Ok(Arc::new(Schema::new(fields)))
    }

    // Converts a ROS `FieldDefinition` to an Arrow `Field`.
    fn ros_field_to_arrow_field(&self, field: &FieldDefinition) -> StageResult<Field> {
        let arrow_type = match &field.data_type {
            FieldType::Base(base_type) => self.ros_base_type_to_arrow_data_type(base_type)?,
            FieldType::Array { data_type, length } => {
                self.ros_array_type_to_arrow_data_type(data_type, *length)?
            }
            FieldType::Sequence(data_type) => {
                self.ros_sequence_type_to_arrow_data_type(data_type)?
            }
        };
        Ok(Field::new(field.name.clone(), arrow_type, true))
    }

    // Converts a ROS fixed-size array type to an Arrow `DataType::FixedSizeList`.
    fn ros_array_type_to_arrow_data_type(
        &self,
        data_type: &BaseType,
        length: u32,
    ) -> StageResult<DataType> {
        let base_arrow_type = self.ros_base_type_to_arrow_data_type(data_type)?;

        Ok(DataType::FixedSizeList(
            Arc::new(Field::new("item", base_arrow_type, true)),
            length as i32,
        ))
    }

    // Converts a ROS sequence type (dynamic array) to an Arrow `DataType::List`.
    fn ros_sequence_type_to_arrow_data_type(&self, data_type: &BaseType) -> StageResult<DataType> {
        let base_arrow_type = self.ros_base_type_to_arrow_data_type(data_type)?;

        Ok(DataType::List(Arc::new(Field::new(
            "item",
            base_arrow_type,
            true,
        ))))
    }

    // Converts a ROS `BaseType` (primitive or struct) to an Arrow `DataType`.
    fn ros_base_type_to_arrow_data_type(&self, base_type: &BaseType) -> StageResult<DataType> {
        match base_type {
            BaseType::Primitive(primitive) => Ok(self.ros_primitive_to_arrow_data_type(primitive)),
            BaseType::Struct(name) => {
                let message_definition =
                    self.message_definition_table.get(name).ok_or_else(|| {
                        StageError::invalid(format!(
                            "nested struct type '{}' not found in message_definition_table",
                            name
                        ))
                    })?;
                let fields: Result<Vec<Field>, _> = message_definition
                    .fields
                    .iter()
                    .map(|field| self.ros_field_to_arrow_field(field))
                    .collect();
                Ok(DataType::Struct(fields?.into()))
            }
        }
    }

    // Converts a ROS `Primitive` type to an Arrow `DataType`.
    fn ros_primitive_to_arrow_data_type(&self, primitive: &Primitive) -> DataType {
        match primitive {
            Primitive::Bool => DataType::Boolean,
            Primitive::Byte => DataType::UInt8,
            Primitive::Char => DataType::UInt8,
            Primitive::Float32 => DataType::Float32,
            Primitive::Float64 => DataType::Float64,
            Primitive::Int8 => DataType::Int8,
            Primitive::UInt8 => DataType::UInt8,
            Primitive::Int16 => DataType::Int16,
            // Polars has limitations around UInt16 columns, so encode ROS `uint16`
            // fields as Arrow UInt32 to keep the data pipeline compatible with Polars.
            Primitive::UInt16 => DataType::UInt32,
            Primitive::Int32 => DataType::Int32,
            Primitive::UInt32 => DataType::UInt32,
            Primitive::Int64 => DataType::Int64,
            Primitive::UInt64 => DataType::UInt64,
            Primitive::String => DataType::Utf8,
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    use arrow::datatypes::Fields;

    use crate::arrow::utils::create_builtin_message_definition_table;
    use crate::ros::schema_type_parser::parse_schema_text_to_message_definition_table;

    #[test]
    fn test_parse_joint_state() {
        let schema_name = "sensor_msgs/JointState";
        let schema_text = include_str!("../../testdata/msgs/JointState.msg");
        let mut message_definition_table = create_builtin_message_definition_table();
        parse_schema_text_to_message_definition_table(
            schema_name,
            schema_text,
            &mut message_definition_table,
        )
        .unwrap();

        let arrow_schema_builder = ArrowSchemaBuilder::new(&message_definition_table);
        let schema = arrow_schema_builder.build("JointState").unwrap();

        let expected_schema = Arc::new(Schema::new(vec![
            Field::new("timestamp_ns", DataType::UInt64, true),
            Field::new("publish_timestamp_ns", DataType::UInt64, true),
            Field::new(
                "header",
                DataType::Struct(Fields::from(vec![
                    Field::new("seq", DataType::UInt32, true),
                    Field::new(
                        "stamp",
                        DataType::Struct(Fields::from(vec![
                            Field::new("sec", DataType::Int32, true),
                            Field::new("nanosec", DataType::UInt32, true),
                        ])),
                        true,
                    ),
                    Field::new("frame_id", DataType::Utf8, true),
                ])),
                true,
            ),
            Field::new(
                "name",
                DataType::List(Arc::new(Field::new("item", DataType::Utf8, true))),
                true,
            ),
            Field::new(
                "position",
                DataType::List(Arc::new(Field::new("item", DataType::Float64, true))),
                true,
            ),
            Field::new(
                "velocity",
                DataType::List(Arc::new(Field::new("item", DataType::Float64, true))),
                true,
            ),
            Field::new(
                "effort",
                DataType::List(Arc::new(Field::new("item", DataType::Float64, true))),
                true,
            ),
        ]));
        assert_eq!(schema, expected_schema);
    }

    #[test]
    fn test_parse_point_cloud2() {
        let schema_name = "sensor_msgs/PointCloud2";
        let schema_text = include_str!("../../testdata/msgs/PointCloud2.msg");
        let mut message_definition_table = create_builtin_message_definition_table();
        parse_schema_text_to_message_definition_table(
            schema_name,
            schema_text,
            &mut message_definition_table,
        )
        .unwrap();

        let arrow_schema_builder = ArrowSchemaBuilder::new(&message_definition_table);
        let schema = arrow_schema_builder.build("PointCloud2").unwrap();

        let expected_schema = Arc::new(Schema::new(vec![
            Field::new("timestamp_ns", DataType::UInt64, true),
            Field::new("publish_timestamp_ns", DataType::UInt64, true),
            Field::new(
                "header",
                DataType::Struct(Fields::from(vec![
                    Field::new(
                        "stamp",
                        DataType::Struct(Fields::from(vec![
                            Field::new("sec", DataType::Int32, true),
                            Field::new("nanosec", DataType::UInt32, true),
                        ])),
                        true,
                    ),
                    Field::new("frame_id", DataType::Utf8, true),
                ])),
                true,
            ),
            Field::new("height", DataType::UInt32, true),
            Field::new("width", DataType::UInt32, true),
            Field::new(
                "fields",
                DataType::List(Arc::new(Field::new(
                    "item",
                    DataType::Struct(Fields::from(vec![
                        Field::new("name", DataType::Utf8, true),
                        Field::new("offset", DataType::UInt32, true),
                        Field::new("datatype", DataType::UInt8, true),
                        Field::new("count", DataType::UInt32, true),
                    ])),
                    true,
                ))),
                true,
            ),
            Field::new("is_bigendian", DataType::Boolean, true),
            Field::new("point_step", DataType::UInt32, true),
            Field::new("row_step", DataType::UInt32, true),
            Field::new(
                "data",
                DataType::List(Arc::new(Field::new("item", DataType::UInt8, true))),
                true,
            ),
            Field::new("is_dense", DataType::Boolean, true),
        ]));
        assert_eq!(schema, expected_schema);
    }

    #[test]
    fn test_parse_image_ros2() {
        // ROS2 Image schema (no seq in Header)
        let schema_name = "sensor_msgs/Image";
        let schema_text = r#"Header header
uint32 height
uint32 width
string encoding
uint8 is_bigendian
uint32 step
uint32 index

================================================================================
MSG: std_msgs/Header
time stamp
string frame_id
"#;
        let mut message_definition_table = create_builtin_message_definition_table();
        parse_schema_text_to_message_definition_table(
            schema_name,
            schema_text,
            &mut message_definition_table,
        )
        .unwrap();

        let arrow_schema_builder = ArrowSchemaBuilder::new(&message_definition_table);
        let schema = arrow_schema_builder.build("Image").unwrap();

        // Verify header field is a Struct
        let header_field = schema.field(2);
        assert_eq!(header_field.name(), "header");
        match header_field.data_type() {
            DataType::Struct(fields) => {
                assert_eq!(fields.len(), 2);
                assert_eq!(fields[0].name(), "stamp");
                assert_eq!(fields[1].name(), "frame_id");
            }
            other => panic!("expected Struct for header, got {other:?}"),
        }
    }

    #[test]
    fn uint16_fields_are_upcast_to_uint32() {
        let schema_name = "custom_msgs/ServoState";
        let schema_text = include_str!("../../testdata/msgs/ServoState.msg");
        let mut message_definition_table = create_builtin_message_definition_table();
        parse_schema_text_to_message_definition_table(
            schema_name,
            schema_text,
            &mut message_definition_table,
        )
        .unwrap();

        let arrow_schema_builder = ArrowSchemaBuilder::new(&message_definition_table);
        let schema = arrow_schema_builder.build("ServoState").unwrap();

        let error_status = schema.field_with_name("error_status").unwrap();
        let list_field = match error_status.data_type() {
            DataType::List(field) => field,
            other => panic!("expected List for error_status, got {other:?}"),
        };

        assert_eq!(list_field.data_type(), &DataType::UInt32);
    }
}
