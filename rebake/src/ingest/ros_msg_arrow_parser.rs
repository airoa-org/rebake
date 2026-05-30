use image;
use std::collections::HashMap;

use arrow::array::{
    ArrayBuilder, BooleanBuilder, FixedSizeListBuilder, Float32Builder, Float64Builder,
    Int8Builder, Int16Builder, Int32Builder, Int64Builder, ListBuilder, StringBuilder,
    StructBuilder, TimestampNanosecondBuilder, UInt8Builder, UInt16Builder, UInt32Builder,
    UInt64Builder,
};
use arrow::datatypes::{DataType, Field, Fields, TimeUnit};

use crate::common::extract_short_type_name;
use crate::core::error::{StageError, StageResult};
use crate::ros::msg_deserializer::{RosGeneration, RosMsgDeserializer};
use crate::ros::types::{BaseType, FieldDefinition, FieldType, MessageDefinition, Primitive};

#[macro_export]
/// A macro to generate helper functions for downcasting a `dyn ArrayBuilder` to a concrete typed builder.
///
/// # Example
///
/// ```rust
/// mod docs {
///     use arrow::array::{ArrayBuilder, BooleanBuilder};
///     use rebake::impl_downcast_builder_typed;
///
///     impl_downcast_builder_typed! {
///         bool => BooleanBuilder,
///     }
///     // Generated (simplified):
///     // pub fn downcast_bool_builder(builder: &mut dyn ArrayBuilder) -> &mut BooleanBuilder {
///     //     builder.as_any_mut().downcast_mut::<BooleanBuilder>().unwrap()
///     // }
///
///     pub fn run() {
///         let mut builder = BooleanBuilder::new();
///         let array_builder: &mut dyn ArrayBuilder = &mut builder;
///         let bool_builder = downcast_bool_builder(array_builder);
///         bool_builder.append_value(true);
///         assert_eq!(builder.len(), 1);
///     }
/// }
///
/// docs::run();
/// ```
/// A macro to generate helper functions for downcasting a `dyn ArrayBuilder` to a concrete typed builder.
///
/// # Contract
///
/// The caller must ensure that the builder was created with the correct concrete type.
/// These functions are used internally by the parser where schema guarantees type correctness.
macro_rules! impl_downcast_builder_typed {
    ($($short_name:ident => $builder_type:ident),* $(,)?) => {
        $(
            paste::paste! {
                pub fn [<downcast_ $short_name _builder>](builder: &mut dyn ArrayBuilder) -> &mut $builder_type {
                    builder.as_any_mut().downcast_mut::<$builder_type>()
                        .expect(concat!("builder must be ", stringify!($builder_type), " - schema mismatch"))
                }
            }
        )*
    };
}

impl_downcast_builder_typed! {
    bool => BooleanBuilder,
    f32 => Float32Builder,
    f64 => Float64Builder,
    i8 => Int8Builder,
    u8 => UInt8Builder,
    i16 => Int16Builder,
    i32 => Int32Builder,
    u32 => UInt32Builder,
    i64 => Int64Builder,
    u64 => UInt64Builder,
    string => StringBuilder,
    timestamp => UInt64Builder,
    struct => StructBuilder,
}

/// Downcasts a `dyn ArrayBuilder` to a mutable `ListBuilder`.
///
/// # Contract
///
/// The caller must ensure that the builder was created as a `ListBuilder<B>`.
/// This function is used internally by the parser where schema guarantees type correctness.
pub fn downcast_list_builder<B>(builder: &mut dyn ArrayBuilder) -> &mut ListBuilder<B>
where
    B: ArrayBuilder,
{
    debug_assert!(builder.as_any_mut().is::<ListBuilder<B>>());
    // CONTRACT: schema guarantees builder type matches expected downcast target
    #[allow(clippy::expect_used)]
    builder
        .as_any_mut()
        .downcast_mut::<ListBuilder<B>>()
        .expect("builder must be ListBuilder<B> - schema mismatch")
}

/// Downcasts a `dyn ArrayBuilder` to a mutable `FixedSizeListBuilder`.
///
/// # Contract
///
/// The caller must ensure that the builder was created as a `FixedSizeListBuilder<B>`.
/// This function is used internally by the parser where schema guarantees type correctness.
pub fn downcast_fixed_size_list_builder<B>(
    builder: &mut dyn ArrayBuilder,
) -> &mut FixedSizeListBuilder<B>
where
    B: ArrayBuilder,
{
    debug_assert!(builder.as_any_mut().is::<FixedSizeListBuilder<B>>());
    // CONTRACT: schema guarantees builder type matches expected downcast target
    #[allow(clippy::expect_used)]
    builder
        .as_any_mut()
        .downcast_mut::<FixedSizeListBuilder<B>>()
        .expect("builder must be FixedSizeListBuilder<B> - schema mismatch")
}

#[macro_export]
/// A macro to generate functions that parse a sequence of primitive types from a ROS message
/// and append them to a `ListBuilder`.
///
/// # Example
///
/// ```rust
/// mod docs {
///     use arrow::array::{Array, ArrayBuilder, BooleanBuilder, ListBuilder};
///     use rebake::ingest::ros_msg_arrow_parser::{cast_value, downcast_list_builder};
///     use rebake::ros::msg_deserializer::{RosGeneration, RosMsgDeserializer};
///
///     pub struct DemoParser<'a> {
///         ros_msg_deserializer: RosMsgDeserializer<'a>,
///     }
///
///     impl<'a> DemoParser<'a> {
///         rebake::impl_parse_sequence_typed! {
///             bool => BooleanBuilder => bool,
///         }
///         // Generated (simplified):
///         // fn parse_sequence_bool(&mut self, builder: &mut dyn ArrayBuilder) {
///         //     let length = self.ros_msg_deserializer.read_sequence_length();
///         //     let mut values = Vec::<bool>::with_capacity(length as usize);
///         //     for _ in 0..length as usize {
///         //         let raw = self.ros_msg_deserializer.deserialize_bool();
///         //         values.push(cast_value::<_, bool>(raw));
///         //     }
///         //     let list_builder = downcast_list_builder::<BooleanBuilder>(builder);
///         //     list_builder.values().append_slice(&values);
///         //     list_builder.append(true);
///         // }
///     }
///
///     pub fn run() {
///         let payload = [1u8, 0, 0, 0, 1];
///         let mut parser = DemoParser { ros_msg_deserializer: RosMsgDeserializer::new(&payload, RosGeneration::ROS1) };
///         let mut builder = ListBuilder::new(BooleanBuilder::new());
///         let builder_dyn: &mut dyn ArrayBuilder = &mut builder;
///         parser.parse_sequence_bool(builder_dyn);
///         let array = builder.finish();
///         assert_eq!(array.len(), 1);
///     }
/// }
///
/// docs::run();
/// ```
macro_rules! impl_parse_sequence_typed {
    ($($short_name:ident => $builder_type:ident => $value_type:ty),* $(,)?) => {
        $(
            paste::paste! {
                fn [<parse_sequence_ $short_name>](&mut self, builder: &mut dyn ArrayBuilder) {
                    let length = self.ros_msg_deserializer.read_sequence_length();

                    let mut values = Vec::<$value_type>::with_capacity(length as usize);
                    for _ in 0..length as usize {
                        let raw = self.ros_msg_deserializer.[<deserialize_$short_name>]();
                        values.push(cast_value::<_, $value_type>(raw));
                    }

                    let list_builder = downcast_list_builder::<$builder_type>(builder);
                    list_builder.values().append_slice(&values);
                    list_builder.append(true);
                }
            }
        )*
    };
}

#[macro_export]
/// A macro to generate functions that parse a fixed-size array of primitive types from a ROS message
/// and append them to a `FixedSizeListBuilder`.
///
/// # Example
///
/// ```rust
/// mod docs {
///     use arrow::array::{Array, ArrayBuilder, BooleanBuilder, FixedSizeListBuilder};
///     use rebake::ingest::ros_msg_arrow_parser::{
///         cast_value, downcast_fixed_size_list_builder,
///     };
///     use rebake::ros::msg_deserializer::{RosGeneration, RosMsgDeserializer};
///
///     pub struct DemoParser<'a> {
///         ros_msg_deserializer: RosMsgDeserializer<'a>,
///     }
///
///     impl<'a> DemoParser<'a> {
///         rebake::impl_parse_array_typed! {
///             bool => BooleanBuilder => bool,
///         }
///         // Generated (simplified):
///         // fn parse_array_bool(&mut self, builder: &mut dyn ArrayBuilder, length: &u32) {
///         //     let mut values = Vec::<bool>::with_capacity(*length as usize);
///         //     for _ in 0..*length as usize {
///         //         let raw = self.ros_msg_deserializer.deserialize_bool();
///         //         values.push(cast_value::<_, bool>(raw));
///         //     }
///         //     let array_builder = builder
///         //         .as_any_mut()
///         //         .downcast_mut::<FixedSizeListBuilder<BooleanBuilder>>()
///         //         .unwrap();
///         //     array_builder.values().append_slice(&values);
///         //     array_builder.append(true);
///         // }
///     }
///
///     pub fn run() {
///         let payload = [1u8, 0u8];
///         let mut parser = DemoParser { ros_msg_deserializer: RosMsgDeserializer::new(&payload, RosGeneration::ROS1) };
///         let mut builder = FixedSizeListBuilder::new(BooleanBuilder::new(), 2);
///         let builder_dyn: &mut dyn ArrayBuilder = &mut builder;
///         let length = 2u32;
///         parser.parse_array_bool(builder_dyn, &length);
///         let array = builder.finish();
///         assert_eq!(array.len(), 1);
///     }
/// }
///
/// docs::run();
/// ```
/// # Contract
///
/// The caller must ensure that the builder was created as a `FixedSizeListBuilder<$builder_type>`.
/// This macro is used internally by the parser where schema guarantees type correctness.
macro_rules! impl_parse_array_typed {
    ($($short_name:ident => $builder_type:ident => $value_type:ty),* $(,)?) => {
        $(
            paste::paste! {
                fn [<parse_array_ $short_name>](&mut self, builder: &mut dyn ArrayBuilder, length: &u32) {
                    let mut values = Vec::<$value_type>::with_capacity(*length as usize);
                    for _ in 0..*length as usize {
                        let raw = self.ros_msg_deserializer.[<deserialize_ $short_name>]();
                        values.push(cast_value::<_, $value_type>(raw));
                    }

                    let array_builder = builder
                        .as_any_mut()
                        .downcast_mut::<FixedSizeListBuilder<$builder_type>>()
                        .expect(concat!("builder must be FixedSizeListBuilder<", stringify!($builder_type), "> - schema mismatch"));
                    array_builder.values().append_slice(&values);
                    array_builder.append(true);
                }
            }
        )*
    };
}

#[macro_export]
/// A macro to generate functions that parse a single primitive value from a ROS message
/// and append it to a typed `ArrayBuilder`.
///
/// # Example
///
/// ```rust
/// mod docs {
///     use arrow::array::{Array, ArrayBuilder, BooleanBuilder};
///     use rebake::ingest::ros_msg_arrow_parser::cast_value;
///     use rebake::ros::msg_deserializer::{RosGeneration, RosMsgDeserializer};
///
///     pub struct DemoParser<'a> {
///         ros_msg_deserializer: RosMsgDeserializer<'a>,
///     }
///
///     impl<'a> DemoParser<'a> {
///         rebake::impl_parse_primitive_typed! {
///             bool => BooleanBuilder => bool,
///         }
///         // Generated (simplified):
///         // fn parse_bool(&mut self, builder: &mut dyn ArrayBuilder) {
///         //     let typed_builder = builder
///         //         .as_any_mut()
///         //         .downcast_mut::<BooleanBuilder>()
///         //         .unwrap();
///         //     let raw = self.ros_msg_deserializer.deserialize_bool();
///         //     let value = cast_value::<_, bool>(raw);
///         //     typed_builder.append_value(value);
///         // }
///     }
///
///     pub fn run() {
///         let payload = [1u8];
///         let mut parser = DemoParser { ros_msg_deserializer: RosMsgDeserializer::new(&payload, RosGeneration::ROS1) };
///         let mut builder = BooleanBuilder::new();
///         let builder_dyn: &mut dyn ArrayBuilder = &mut builder;
///         parser.parse_bool(builder_dyn);
///         assert_eq!(builder.finish().len(), 1);
///     }
/// }
///
/// docs::run();
/// ```
/// # Contract
///
/// The caller must ensure that the builder was created as a `$builder_type`.
/// This macro is used internally by the parser where schema guarantees type correctness.
macro_rules! impl_parse_primitive_typed {
    ($($short_name:ident => $builder_type:ident => $value_type:ty),* $(,)?) => {
        $(
            paste::paste! {
                fn [<parse_ $short_name>](&mut self, builder: &mut dyn ArrayBuilder) {
                    let typed_builder = builder
                        .as_any_mut()
                        .downcast_mut::<$builder_type>()
                        .expect(concat!("builder must be ", stringify!($builder_type), " - schema mismatch"));
                    let raw = self.ros_msg_deserializer.[<deserialize_ $short_name>]();
                    let value = cast_value::<_, $value_type>(raw);
                    typed_builder.append_value(value);
                }
            }
        )*
    };
}

pub fn cast_value<U, T>(value: U) -> T
where
    U: Into<T>,
{
    value.into()
}

const TIMESTAMP_IDX: usize = 0;
const PUBLISH_TIMESTAMP_IDX: usize = 1;
const MESSAGE_FIELD_START_IDX: usize = 2;

fn append_record_timestamps(
    array_builders: &mut [Box<dyn ArrayBuilder>],
    timestamp_ns: u64,
    publish_timestamp_ns: Option<u64>,
) {
    downcast_timestamp_builder(&mut array_builders[TIMESTAMP_IDX]).append_value(timestamp_ns);

    let publish_builder = downcast_timestamp_builder(&mut array_builders[PUBLISH_TIMESTAMP_IDX]);
    if let Some(value) = publish_timestamp_ns {
        publish_builder.append_value(value);
    } else {
        publish_builder.append_null();
    }
}

/// Deserializes ROS message data and populates Arrow `ArrayBuilder`s with the values.
pub struct RosMsgArrowParser<'a> {
    /// A table mapping ROS message type names to their `MessageDefinition`.
    msg_definition_table: &'a HashMap<String, MessageDefinition>,
    /// A deserializer for reading primitive types from a ROS message byte stream.
    ros_msg_deserializer: RosMsgDeserializer<'a>,
    /// The short type name of the ROS message being deserialized (e.g., "JointState").
    type_name: &'a str,
    /// The timestamp of the message in nanoseconds.
    timestamp_ns: u64,
    /// The source publish timestamp of the message in nanoseconds, if available.
    publish_timestamp_ns: Option<u64>,
}

impl<'a> RosMsgArrowParser<'a> {
    pub fn new(
        ros_generation: RosGeneration,
        msg_definition_table: &'a HashMap<String, MessageDefinition>,
        type_name: &'a str,
        data: &'a [u8],
        timestamp_ns: u64,
        publish_timestamp_ns: Option<u64>,
    ) -> Self {
        Self {
            msg_definition_table,
            ros_msg_deserializer: RosMsgDeserializer::new(data, ros_generation),
            type_name: extract_short_type_name(type_name),
            timestamp_ns,
            publish_timestamp_ns,
        }
    }

    impl_parse_sequence_typed! {
        bool => BooleanBuilder => bool,
        f32 => Float32Builder => f32,
        f64 => Float64Builder => f64,
        i8 => Int8Builder => i8,
        u8 => UInt8Builder => u8,
        i16 => Int16Builder => i16,
        u16 => UInt32Builder => u32,
        i32 => Int32Builder => i32,
        u32 => UInt32Builder => u32,
        i64 => Int64Builder => i64,
        u64 => UInt64Builder => u64,
    }

    impl_parse_array_typed! {
        bool => BooleanBuilder => bool,
        f32 => Float32Builder => f32,
        f64 => Float64Builder => f64,
        i8 => Int8Builder => i8,
        u8 => UInt8Builder => u8,
        i16 => Int16Builder => i16,
        u16 => UInt32Builder => u32,
        i32 => Int32Builder => i32,
        u32 => UInt32Builder => u32,
        i64 => Int64Builder => i64,
        u64 => UInt64Builder => u64,
    }

    impl_parse_primitive_typed! {
        bool => BooleanBuilder => bool,
        f32 => Float32Builder => f32,
        f64 => Float64Builder => f64,
        i8 => Int8Builder => i8,
        u8 => UInt8Builder => u8,
        i16 => Int16Builder => i16,
        u16 => UInt32Builder => u32,
        i32 => Int32Builder => i32,
        u32 => UInt32Builder => u32,
        i64 => Int64Builder => i64,
        u64 => UInt64Builder => u64,
    }

    /// Parses a `string` primitive field.
    ///
    /// # Contract
    ///
    /// The caller must ensure that the builder was created as a `StringBuilder`.
    /// This is guaranteed by schema-driven builder creation.
    ///
    /// # Errors
    ///
    /// Returns `StageError::InvalidData` if the string field contains invalid UTF-8.
    fn parse_string(&mut self, builder: &mut dyn ArrayBuilder) -> StageResult<()> {
        // CONTRACT: schema guarantees builder type matches expected downcast target
        #[allow(clippy::expect_used)]
        let typed_builder = builder
            .as_any_mut()
            .downcast_mut::<StringBuilder>()
            .expect("builder must be StringBuilder - schema mismatch");
        let value = self.ros_msg_deserializer.deserialize_string()?;
        typed_builder.append_value(value);
        Ok(())
    }

    /// Parses the ROS message data and populates the provided `ArrayBuilder`s.
    ///
    /// # Errors
    ///
    /// Returns `StageError::MissingData` if the message type is not defined.
    /// Returns `StageError::InvalidData` if string fields contain invalid UTF-8.
    pub fn parse(&mut self, array_builders: &'a mut [Box<dyn ArrayBuilder>]) -> StageResult<()> {
        append_record_timestamps(array_builders, self.timestamp_ns, self.publish_timestamp_ns);

        let msg_definition = self
            .msg_definition_table
            .get(self.type_name)
            .ok_or_else(|| {
                StageError::missing(format!(
                    "message type '{}' in definition table",
                    self.type_name
                ))
            })?;
        for (array_builder, field) in array_builders
            .iter_mut()
            .skip(MESSAGE_FIELD_START_IDX)
            .zip(msg_definition.fields.iter())
        {
            self.parse_field(field, array_builder)?;
        }
        Ok(())
    }

    /// Parses a single field based on its `FieldDefinition`.
    ///
    /// # Errors
    ///
    /// Returns `StageError::InvalidData` if string fields contain invalid UTF-8.
    /// Returns `StageError::MissingData` if a nested struct type is not defined.
    fn parse_field(
        &mut self,
        field: &FieldDefinition,
        array_builder: &mut dyn ArrayBuilder,
    ) -> StageResult<()> {
        match &field.data_type {
            FieldType::Array { data_type, length } => {
                self.parse_array(data_type, length, array_builder)
            }
            FieldType::Sequence(base_type) => self.parse_sequence(base_type, array_builder),
            FieldType::Base(base_type) => self.parse_base_type(base_type, array_builder),
        }
    }

    /// Parses a fixed-size array field.
    ///
    /// # Errors
    ///
    /// Returns `StageError::InvalidData` if string elements contain invalid UTF-8.
    /// Returns `StageError::MissingData` if a nested struct type is not defined.
    fn parse_array(
        &mut self,
        data_type: &BaseType,
        length: &u32,
        array_builder: &mut dyn ArrayBuilder,
    ) -> StageResult<()> {
        match data_type {
            BaseType::Primitive(primitive) => match primitive {
                Primitive::Bool => self.parse_array_bool(array_builder, length),
                Primitive::Byte => self.parse_array_u8(array_builder, length),
                Primitive::Char => self.parse_array_char(array_builder, length),
                Primitive::Float32 => self.parse_array_f32(array_builder, length),
                Primitive::Float64 => self.parse_array_f64(array_builder, length),
                Primitive::Int8 => self.parse_array_i8(array_builder, length),
                Primitive::UInt8 => self.parse_array_u8(array_builder, length),
                Primitive::Int16 => self.parse_array_i16(array_builder, length),
                Primitive::UInt16 => self.parse_array_u16(array_builder, length),
                Primitive::Int32 => self.parse_array_i32(array_builder, length),
                Primitive::UInt32 => self.parse_array_u32(array_builder, length),
                Primitive::Int64 => self.parse_array_i64(array_builder, length),
                Primitive::UInt64 => self.parse_array_u64(array_builder, length),
                Primitive::String => return self.parse_array_string(array_builder, length),
            },
            BaseType::Struct(name) => return self.parse_array_struct(array_builder, name, length),
        }
        Ok(())
    }

    /// Parses a `char` array field.
    fn parse_array_char(&mut self, array_builder: &mut dyn ArrayBuilder, length: &u32) {
        let string_builder = downcast_fixed_size_list_builder::<StringBuilder>(array_builder);
        for _ in 0..*length as usize {
            string_builder
                .values()
                .append_value(self.ros_msg_deserializer.deserialize_char().to_string());
        }
        string_builder.append(true);
    }

    /// Parses a string array field.
    ///
    /// # Errors
    ///
    /// Returns `StageError::InvalidData` if any string element contains invalid UTF-8.
    fn parse_array_string(
        &mut self,
        array_builder: &mut dyn ArrayBuilder,
        length: &u32,
    ) -> StageResult<()> {
        let string_builder = downcast_fixed_size_list_builder::<StringBuilder>(array_builder);
        for _ in 0..*length as usize {
            let value = self.ros_msg_deserializer.deserialize_string()?;
            string_builder.values().append_value(value);
        }
        string_builder.append(true);
        Ok(())
    }

    /// Parses a struct array field.
    ///
    /// # Errors
    ///
    /// Returns `StageError::MissingData` if the struct type is not defined.
    /// Returns `StageError::InvalidData` if nested string fields contain invalid UTF-8.
    fn parse_array_struct(
        &mut self,
        array_builder: &mut dyn ArrayBuilder,
        name: &str,
        length: &u32,
    ) -> StageResult<()> {
        let substruct_builder = downcast_fixed_size_list_builder::<StructBuilder>(array_builder);

        for _ in 0..*length as usize {
            self.parse_struct(name, substruct_builder)?;
        }
        substruct_builder.append(true);
        Ok(())
    }

    /// Parses a sequence field (a dynamically-sized array).
    ///
    /// # Errors
    ///
    /// Returns `StageError::InvalidData` if string elements contain invalid UTF-8.
    /// Returns `StageError::MissingData` if a nested struct type is not defined.
    fn parse_sequence(
        &mut self,
        data_type: &BaseType,
        array_builder: &mut dyn ArrayBuilder,
    ) -> StageResult<()> {
        match data_type {
            BaseType::Primitive(primitive) => match primitive {
                Primitive::Bool => self.parse_sequence_bool(array_builder),
                Primitive::Byte => self.parse_sequence_u8(array_builder),
                Primitive::Char => self.parse_sequence_char(array_builder),
                Primitive::Float32 => self.parse_sequence_f32(array_builder),
                Primitive::Float64 => self.parse_sequence_f64(array_builder),
                Primitive::Int8 => self.parse_sequence_i8(array_builder),
                Primitive::UInt8 => self.parse_sequence_u8(array_builder),
                Primitive::Int16 => self.parse_sequence_i16(array_builder),
                Primitive::UInt16 => self.parse_sequence_u16(array_builder),
                Primitive::Int32 => self.parse_sequence_i32(array_builder),
                Primitive::UInt32 => self.parse_sequence_u32(array_builder),
                Primitive::Int64 => self.parse_sequence_i64(array_builder),
                Primitive::UInt64 => self.parse_sequence_u64(array_builder),
                Primitive::String => return self.parse_sequence_string(array_builder),
            },
            BaseType::Struct(name) => return self.parse_sequence_struct(array_builder, name),
        }
        Ok(())
    }

    /// Parses a `char` sequence field.
    fn parse_sequence_char(&mut self, array_builder: &mut dyn ArrayBuilder) {
        let length = self.ros_msg_deserializer.read_sequence_length();

        let string_builder = downcast_list_builder::<StringBuilder>(array_builder);
        for _ in 0..length as usize {
            string_builder
                .values()
                .append_value(self.ros_msg_deserializer.deserialize_char().to_string());
        }
        string_builder.append(true);
    }

    /// Parses a string sequence field.
    ///
    /// # Errors
    ///
    /// Returns `StageError::InvalidData` if any string element contains invalid UTF-8.
    fn parse_sequence_string(&mut self, array_builder: &mut dyn ArrayBuilder) -> StageResult<()> {
        let length = self.ros_msg_deserializer.read_sequence_length();

        let string_builder = downcast_list_builder::<StringBuilder>(array_builder);
        for _ in 0..length as usize {
            let value = self.ros_msg_deserializer.deserialize_string()?;
            string_builder.values().append_value(value);
        }
        string_builder.append(true);
        Ok(())
    }

    /// Parses a struct sequence field.
    ///
    /// # Contract
    ///
    /// The `list_builder.values()` must be a `StructBuilder`.
    /// This is guaranteed by the schema-driven builder creation.
    ///
    /// # Errors
    ///
    /// Returns `StageError::MissingData` if the struct type is not defined.
    /// Returns `StageError::InvalidData` if nested string fields contain invalid UTF-8.
    fn parse_sequence_struct(
        &mut self,
        array_builder: &mut dyn ArrayBuilder,
        name: &str,
    ) -> StageResult<()> {
        let length = self.ros_msg_deserializer.read_sequence_length();

        let list_builder = downcast_list_builder::<StructBuilder>(array_builder);
        // CONTRACT: ListBuilder<StructBuilder> inner values are StructBuilder
        #[allow(clippy::expect_used)]
        let substruct_builder = list_builder
            .values()
            .as_any_mut()
            .downcast_mut::<StructBuilder>()
            .expect("list values must be StructBuilder - schema contract");

        for _ in 0..length as usize {
            self.parse_struct(name, substruct_builder)?;
        }
        list_builder.append(true);
        Ok(())
    }

    /// Parses a base type field (either a primitive or a struct).
    ///
    /// # Errors
    ///
    /// Returns `StageError::InvalidData` if string fields contain invalid UTF-8.
    /// Returns `StageError::MissingData` if a nested struct type is not defined.
    fn parse_base_type(
        &mut self,
        base_type: &BaseType,
        array_builder: &mut dyn ArrayBuilder,
    ) -> StageResult<()> {
        match base_type {
            BaseType::Primitive(primitive) => self.parse_primitive(primitive, array_builder),
            BaseType::Struct(name) => self.parse_struct(name, array_builder),
        }
    }

    /// Parses a struct field.
    ///
    /// # Errors
    ///
    /// Returns `StageError::MissingData` if the struct type is not defined in the message table.
    /// Returns `StageError::InvalidData` if nested string fields contain invalid UTF-8.
    fn parse_struct(
        &mut self,
        name: &str,
        array_builder: &mut dyn ArrayBuilder,
    ) -> StageResult<()> {
        let msg_definition = self.msg_definition_table.get(name).ok_or_else(|| {
            StageError::missing(format!("struct type '{}' in message table", name))
        })?;
        let struct_builder = downcast_struct_builder(array_builder);

        for (i, field_builder) in struct_builder.field_builders_mut().iter_mut().enumerate() {
            let field = &msg_definition.fields[i];
            match &field.data_type {
                FieldType::Array { data_type, length } => {
                    self.parse_array(data_type, length, field_builder)?
                }
                FieldType::Sequence(base_type) => self.parse_sequence(base_type, field_builder)?,
                FieldType::Base(base_type) => self.parse_base_type(base_type, field_builder)?,
            }
        }

        struct_builder.append(true);
        Ok(())
    }

    /// Parses a primitive type field.
    ///
    /// # Errors
    ///
    /// Returns `StageError::InvalidData` if the string field contains invalid UTF-8.
    fn parse_primitive(
        &mut self,
        primitive: &Primitive,
        array_builder: &mut dyn ArrayBuilder,
    ) -> StageResult<()> {
        match primitive {
            Primitive::Bool => self.parse_bool(array_builder),
            Primitive::Byte => self.parse_u8(array_builder),
            Primitive::Char => self.parse_char(array_builder),
            Primitive::Float32 => self.parse_f32(array_builder),
            Primitive::Float64 => self.parse_f64(array_builder),
            Primitive::Int8 => self.parse_i8(array_builder),
            Primitive::UInt8 => self.parse_u8(array_builder),
            Primitive::Int16 => self.parse_i16(array_builder),
            Primitive::UInt16 => self.parse_u16(array_builder),
            Primitive::Int32 => self.parse_i32(array_builder),
            Primitive::UInt32 => self.parse_u32(array_builder),
            Primitive::Int64 => self.parse_i64(array_builder),
            Primitive::UInt64 => self.parse_u64(array_builder),
            Primitive::String => return self.parse_string(array_builder),
        }
        Ok(())
    }

    /// Parses a `char` field.
    fn parse_char(&mut self, array_builder: &mut dyn ArrayBuilder) {
        let byte_builder = downcast_fixed_size_list_builder::<StringBuilder>(array_builder);
        byte_builder
            .values()
            .append_value(self.ros_msg_deserializer.deserialize_char().to_string());
    }
}

/// Decodes a compressed image message and populates the corresponding `ArrayBuilder`s.
///
/// This function handles both `sensor_msgs/CompressedImage` and `sensor_msgs/CompressedDepth`.
/// It deserializes the message header and format, appends them to the builders, and returns
/// the raw image data payload as a `Vec<u8>` along with the format string from the ROS message.
///
/// # Returns
///
/// A tuple of (image_data, format) where format is the ROS CompressedImage format field
/// (e.g., "jpeg", "png", "rgb8; jpeg compressed").
///
/// # Errors
///
/// Returns `StageError::InvalidData` if the string fields contain invalid UTF-8.
pub fn decode_compressed_image(
    array_builders: &mut [Box<dyn ArrayBuilder>],
    data: &[u8],
    timestamp_ns: u64,
    publish_timestamp_ns: Option<u64>,
    index: u32,
    ros_generation: RosGeneration,
) -> StageResult<(Vec<u8>, String)> {
    match ros_generation {
        RosGeneration::ROS1 => decode_compressed_image_ros1(
            array_builders,
            data,
            timestamp_ns,
            publish_timestamp_ns,
            index,
        ),
        RosGeneration::ROS2 => decode_compressed_image_ros2(
            array_builders,
            data,
            timestamp_ns,
            publish_timestamp_ns,
            index,
        ),
    }
}

fn decode_compressed_image_ros1(
    array_builders: &mut [Box<dyn ArrayBuilder>],
    data: &[u8],
    timestamp_ns: u64,
    publish_timestamp_ns: Option<u64>,
    index: u32,
) -> StageResult<(Vec<u8>, String)> {
    const HEADER_IDX: usize = 2;
    const FORMAT_IDX: usize = 3;
    const INDEX_IDX: usize = 4;

    let mut ros_msg_deserializer = RosMsgDeserializer::new(data, RosGeneration::ROS1);

    let seq = ros_msg_deserializer.deserialize_u32();
    let sec = ros_msg_deserializer.deserialize_i32();
    let nanosec = ros_msg_deserializer.deserialize_u32();
    let frame_id = ros_msg_deserializer.deserialize_string()?;

    let format = ros_msg_deserializer.deserialize_string()?;
    let data_length = ros_msg_deserializer.read_sequence_length();
    let data = ros_msg_deserializer
        .next_bytes(data_length as usize)
        .to_vec();

    append_record_timestamps(array_builders, timestamp_ns, publish_timestamp_ns);

    {
        let header_builder = downcast_struct_builder(&mut array_builders[HEADER_IDX]);
        downcast_u32_builder(&mut header_builder.field_builders_mut()[0]).append_value(seq);

        {
            let time_builder = downcast_struct_builder(&mut header_builder.field_builders_mut()[1]);
            downcast_i32_builder(&mut time_builder.field_builders_mut()[0]).append_value(sec);
            downcast_u32_builder(&mut time_builder.field_builders_mut()[1]).append_value(nanosec);
            time_builder.append(true);
        }

        downcast_string_builder(&mut header_builder.field_builders_mut()[2]).append_value(frame_id);
        header_builder.append(true);
    }

    downcast_string_builder(&mut array_builders[FORMAT_IDX]).append_value(&format);
    downcast_u32_builder(&mut array_builders[INDEX_IDX]).append_value(index);

    Ok((data, format))
}

fn decode_compressed_image_ros2(
    array_builders: &mut [Box<dyn ArrayBuilder>],
    data: &[u8],
    timestamp_ns: u64,
    publish_timestamp_ns: Option<u64>,
    index: u32,
) -> StageResult<(Vec<u8>, String)> {
    const HEADER_IDX: usize = 2;
    const FORMAT_IDX: usize = 3;
    const INDEX_IDX: usize = 4;

    let mut ros_msg_deserializer = RosMsgDeserializer::new(data, RosGeneration::ROS2);

    let sec = ros_msg_deserializer.deserialize_i32();
    let nanosec = ros_msg_deserializer.deserialize_u32();
    let frame_id = ros_msg_deserializer.deserialize_string()?;

    let format = ros_msg_deserializer.deserialize_string()?;
    let data_length = ros_msg_deserializer.read_sequence_length();
    let data = ros_msg_deserializer
        .next_bytes(data_length as usize)
        .to_vec();

    append_record_timestamps(array_builders, timestamp_ns, publish_timestamp_ns);

    {
        let header_builder = downcast_struct_builder(&mut array_builders[HEADER_IDX]);

        {
            let time_builder = downcast_struct_builder(&mut header_builder.field_builders_mut()[0]);
            downcast_i32_builder(&mut time_builder.field_builders_mut()[0]).append_value(sec);
            downcast_u32_builder(&mut time_builder.field_builders_mut()[1]).append_value(nanosec);
            time_builder.append(true);
        }

        downcast_string_builder(&mut header_builder.field_builders_mut()[1]).append_value(frame_id);
        header_builder.append(true);
    }

    downcast_string_builder(&mut array_builders[FORMAT_IDX]).append_value(&format);
    downcast_u32_builder(&mut array_builders[INDEX_IDX]).append_value(index);

    Ok((data, format))
}

/// Decodes a PointCloud2 message and populates the corresponding `ArrayBuilder`s.
///
/// # Errors
///
/// Returns `StageError::InvalidData` if the string fields contain invalid UTF-8.
pub fn decode_point_cloud2(
    array_builders: &mut [Box<dyn ArrayBuilder>],
    data: &[u8],
    timestamp_ns: u64,
    publish_timestamp_ns: Option<u64>,
    index: u32,
    ros_generation: RosGeneration,
) -> StageResult<Vec<u8>> {
    match ros_generation {
        RosGeneration::ROS1 => decode_point_cloud2_ros1(
            array_builders,
            data,
            timestamp_ns,
            publish_timestamp_ns,
            index,
        ),
        RosGeneration::ROS2 => decode_point_cloud2_ros2(
            array_builders,
            data,
            timestamp_ns,
            publish_timestamp_ns,
            index,
        ),
    }
}

fn decode_point_cloud2_ros1(
    array_builders: &mut [Box<dyn ArrayBuilder>],
    data: &[u8],
    timestamp_ns: u64,
    publish_timestamp_ns: Option<u64>,
    index: u32,
) -> StageResult<Vec<u8>> {
    const HEADER_IDX: usize = 2;
    const HEIGHT_IDX: usize = 3;
    const WIDTH_IDX: usize = 4;
    const FIELDS_IDX: usize = 5;
    const BIG_ENDIAN_IDX: usize = 6;
    const POINT_STEP_IDX: usize = 7;
    const ROW_STEP_IDX: usize = 8;
    const INDEX_IDX: usize = 9;
    const IS_DENSE_IDX: usize = 10;

    let mut ros_msg_deserializer = RosMsgDeserializer::new(data, RosGeneration::ROS1);

    let seq = ros_msg_deserializer.deserialize_u32();
    let sec = ros_msg_deserializer.deserialize_i32();
    let nanosec = ros_msg_deserializer.deserialize_u32();
    let frame_id = ros_msg_deserializer.deserialize_string()?;

    let height = ros_msg_deserializer.deserialize_u32();
    let width = ros_msg_deserializer.deserialize_u32();

    let field_count = ros_msg_deserializer.read_sequence_length();
    append_point_fields(
        &mut array_builders[FIELDS_IDX],
        field_count,
        &mut ros_msg_deserializer,
    )?;

    let is_bigendian = ros_msg_deserializer.deserialize_bool();
    let point_step = ros_msg_deserializer.deserialize_u32();
    let row_step = ros_msg_deserializer.deserialize_u32();
    let data_length = ros_msg_deserializer.read_sequence_length();
    let data = ros_msg_deserializer
        .next_bytes(data_length as usize)
        .to_vec();
    let is_dense = ros_msg_deserializer.deserialize_bool();

    append_record_timestamps(array_builders, timestamp_ns, publish_timestamp_ns);
    append_ros1_header(&mut array_builders[HEADER_IDX], seq, sec, nanosec, frame_id);
    downcast_u32_builder(&mut array_builders[HEIGHT_IDX]).append_value(height);
    downcast_u32_builder(&mut array_builders[WIDTH_IDX]).append_value(width);
    downcast_bool_builder(&mut array_builders[BIG_ENDIAN_IDX]).append_value(is_bigendian);
    downcast_u32_builder(&mut array_builders[POINT_STEP_IDX]).append_value(point_step);
    downcast_u32_builder(&mut array_builders[ROW_STEP_IDX]).append_value(row_step);
    downcast_u32_builder(&mut array_builders[INDEX_IDX]).append_value(index);
    downcast_bool_builder(&mut array_builders[IS_DENSE_IDX]).append_value(is_dense);

    Ok(data)
}

fn decode_point_cloud2_ros2(
    array_builders: &mut [Box<dyn ArrayBuilder>],
    data: &[u8],
    timestamp_ns: u64,
    publish_timestamp_ns: Option<u64>,
    index: u32,
) -> StageResult<Vec<u8>> {
    const HEADER_IDX: usize = 2;
    const HEIGHT_IDX: usize = 3;
    const WIDTH_IDX: usize = 4;
    const FIELDS_IDX: usize = 5;
    const BIG_ENDIAN_IDX: usize = 6;
    const POINT_STEP_IDX: usize = 7;
    const ROW_STEP_IDX: usize = 8;
    const INDEX_IDX: usize = 9;
    const IS_DENSE_IDX: usize = 10;

    let mut ros_msg_deserializer = RosMsgDeserializer::new(data, RosGeneration::ROS2);

    let sec = ros_msg_deserializer.deserialize_i32();
    let nanosec = ros_msg_deserializer.deserialize_u32();
    let frame_id = ros_msg_deserializer.deserialize_string()?;

    let height = ros_msg_deserializer.deserialize_u32();
    let width = ros_msg_deserializer.deserialize_u32();

    let field_count = ros_msg_deserializer.read_sequence_length();
    append_point_fields(
        &mut array_builders[FIELDS_IDX],
        field_count,
        &mut ros_msg_deserializer,
    )?;

    let is_bigendian = ros_msg_deserializer.deserialize_bool();
    let point_step = ros_msg_deserializer.deserialize_u32();
    let row_step = ros_msg_deserializer.deserialize_u32();
    let data_length = ros_msg_deserializer.read_sequence_length();
    let data = ros_msg_deserializer
        .next_bytes(data_length as usize)
        .to_vec();
    let is_dense = ros_msg_deserializer.deserialize_bool();

    append_record_timestamps(array_builders, timestamp_ns, publish_timestamp_ns);
    append_ros2_header(&mut array_builders[HEADER_IDX], sec, nanosec, frame_id);
    downcast_u32_builder(&mut array_builders[HEIGHT_IDX]).append_value(height);
    downcast_u32_builder(&mut array_builders[WIDTH_IDX]).append_value(width);
    downcast_bool_builder(&mut array_builders[BIG_ENDIAN_IDX]).append_value(is_bigendian);
    downcast_u32_builder(&mut array_builders[POINT_STEP_IDX]).append_value(point_step);
    downcast_u32_builder(&mut array_builders[ROW_STEP_IDX]).append_value(row_step);
    downcast_u32_builder(&mut array_builders[INDEX_IDX]).append_value(index);
    downcast_bool_builder(&mut array_builders[IS_DENSE_IDX]).append_value(is_dense);

    Ok(data)
}

/// Decodes a ROS Image message and returns the encoded JPEG bytes.
///
/// # Errors
///
/// Returns `StageError::InvalidData` if the image encoding is unsupported.
/// Supported encodings: rgb8, bgr8, mono8.
pub fn decode_image(
    array_builders: &mut [Box<dyn ArrayBuilder>],
    data: &[u8],
    timestamp_ns: u64,
    publish_timestamp_ns: Option<u64>,
    index: u32,
    ros_generation: RosGeneration,
    topic: &str,
) -> Result<Vec<u8>, StageError> {
    match ros_generation {
        RosGeneration::ROS1 => decode_image_ros1(
            array_builders,
            data,
            timestamp_ns,
            publish_timestamp_ns,
            index,
            topic,
        ),
        RosGeneration::ROS2 => decode_image_ros2(
            array_builders,
            data,
            timestamp_ns,
            publish_timestamp_ns,
            index,
            topic,
        ),
    }
}

/// Decodes a ROS Image message with ROS1-style Header (seq, stamp, frame_id).
/// Note: Uses ROS2 deserializer since MCAP files typically use CDR serialization
/// without the ROS1 alignment rules.
fn decode_image_ros1(
    array_builders: &mut [Box<dyn ArrayBuilder>],
    data: &[u8],
    timestamp_ns: u64,
    publish_timestamp_ns: Option<u64>,
    index: u32,
    topic: &str,
) -> Result<Vec<u8>, StageError> {
    const HEADER_IDX: usize = 2;
    const HEIGHT_IDX: usize = 3;
    const WIDTH_IDX: usize = 4;
    const ENCODING_IDX: usize = 5;
    const BIG_ENDIAN_IDX: usize = 6;
    const STEP_IDX: usize = 7;
    const INDEX_IDX: usize = 8;

    let mut ros_msg_deserializer = RosMsgDeserializer::new(data, RosGeneration::ROS2);

    // Header (ROS1-style schema has seq field)
    let seq = ros_msg_deserializer.deserialize_u32();
    let sec = ros_msg_deserializer.deserialize_i32();
    let nanosec = ros_msg_deserializer.deserialize_u32();
    let frame_id = ros_msg_deserializer.deserialize_string()?;

    let height = ros_msg_deserializer.deserialize_u32();
    let width = ros_msg_deserializer.deserialize_u32();
    let encoding = ros_msg_deserializer.deserialize_string()?;
    let is_bigendian = ros_msg_deserializer.deserialize_u8();
    let step = ros_msg_deserializer.deserialize_u32();

    let data_length = ros_msg_deserializer.read_sequence_length();
    let raw_data = ros_msg_deserializer.next_bytes(data_length as usize);

    // Encode to JPEG
    let mut jpeg_data = Vec::new();
    let color_type = match encoding.as_str() {
        "rgb8" => image::ExtendedColorType::Rgb8,
        "bgr8" => image::ExtendedColorType::Bgr8,
        "mono8" => image::ExtendedColorType::L8,
        unsupported => {
            return Err(StageError::invalid(format!(
                "Unsupported image encoding '{}' for topic '{}'. Supported: rgb8, bgr8, mono8",
                unsupported, topic
            )));
        }
    };

    let img_buffer = raw_data.to_vec();
    let mut encoder = image::codecs::jpeg::JpegEncoder::new(&mut jpeg_data);
    encoder
        .encode(&img_buffer, width, height, color_type)
        .map_err(|e| {
            StageError::external(format!("Failed to encode JPEG for topic '{}'", topic), e)
        })?;

    append_record_timestamps(array_builders, timestamp_ns, publish_timestamp_ns);

    // Header (ROS1 format with seq)
    {
        let header_builder = downcast_struct_builder(&mut array_builders[HEADER_IDX]);
        downcast_u32_builder(&mut header_builder.field_builders_mut()[0]).append_value(seq);
        {
            let time_builder = downcast_struct_builder(&mut header_builder.field_builders_mut()[1]);
            downcast_i32_builder(&mut time_builder.field_builders_mut()[0]).append_value(sec);
            downcast_u32_builder(&mut time_builder.field_builders_mut()[1]).append_value(nanosec);
            time_builder.append(true);
        }
        downcast_string_builder(&mut header_builder.field_builders_mut()[2]).append_value(frame_id);
        header_builder.append(true);
    }

    downcast_u32_builder(&mut array_builders[HEIGHT_IDX]).append_value(height);
    downcast_u32_builder(&mut array_builders[WIDTH_IDX]).append_value(width);
    downcast_string_builder(&mut array_builders[ENCODING_IDX]).append_value(encoding);
    downcast_u8_builder(&mut array_builders[BIG_ENDIAN_IDX]).append_value(is_bigendian);
    downcast_u32_builder(&mut array_builders[STEP_IDX]).append_value(step);
    downcast_u32_builder(&mut array_builders[INDEX_IDX]).append_value(index);

    Ok(jpeg_data)
}

fn decode_image_ros2(
    array_builders: &mut [Box<dyn ArrayBuilder>],
    data: &[u8],
    timestamp_ns: u64,
    publish_timestamp_ns: Option<u64>,
    index: u32,
    topic: &str,
) -> Result<Vec<u8>, StageError> {
    const HEADER_IDX: usize = 2;
    const HEIGHT_IDX: usize = 3;
    const WIDTH_IDX: usize = 4;
    const ENCODING_IDX: usize = 5;
    const BIG_ENDIAN_IDX: usize = 6;
    const STEP_IDX: usize = 7;
    const INDEX_IDX: usize = 8;

    let mut ros_msg_deserializer = RosMsgDeserializer::new(data, RosGeneration::ROS2);

    // Header
    let sec = ros_msg_deserializer.deserialize_i32();
    let nanosec = ros_msg_deserializer.deserialize_u32();
    let frame_id = ros_msg_deserializer.deserialize_string()?;

    let height = ros_msg_deserializer.deserialize_u32();
    let width = ros_msg_deserializer.deserialize_u32();
    let encoding = ros_msg_deserializer.deserialize_string()?;
    let is_bigendian = ros_msg_deserializer.deserialize_u8();
    let step = ros_msg_deserializer.deserialize_u32();

    let data_length = ros_msg_deserializer.read_sequence_length();
    let raw_data = ros_msg_deserializer.next_bytes(data_length as usize);

    // Encode to JPEG
    let mut jpeg_data = Vec::new();
    let color_type = match encoding.as_str() {
        "rgb8" => image::ExtendedColorType::Rgb8,
        "bgr8" => image::ExtendedColorType::Bgr8,
        "mono8" => image::ExtendedColorType::L8,
        unsupported => {
            return Err(StageError::invalid(format!(
                "Unsupported image encoding '{}' for topic '{}'. Supported: rgb8, bgr8, mono8",
                unsupported, topic
            )));
        }
    };

    let img_buffer = raw_data.to_vec();
    let mut encoder = image::codecs::jpeg::JpegEncoder::new(&mut jpeg_data);
    encoder
        .encode(&img_buffer, width, height, color_type)
        .map_err(|e| {
            StageError::external(format!("Failed to encode JPEG for topic '{}'", topic), e)
        })?;

    append_record_timestamps(array_builders, timestamp_ns, publish_timestamp_ns);

    // Header
    {
        let header_builder = downcast_struct_builder(&mut array_builders[HEADER_IDX]);
        {
            let time_builder = downcast_struct_builder(&mut header_builder.field_builders_mut()[0]);
            downcast_i32_builder(&mut time_builder.field_builders_mut()[0]).append_value(sec);
            downcast_u32_builder(&mut time_builder.field_builders_mut()[1]).append_value(nanosec);
            time_builder.append(true);
        }
        downcast_string_builder(&mut header_builder.field_builders_mut()[1]).append_value(frame_id);
        header_builder.append(true);
    }

    downcast_u32_builder(&mut array_builders[HEIGHT_IDX]).append_value(height);
    downcast_u32_builder(&mut array_builders[WIDTH_IDX]).append_value(width);
    downcast_string_builder(&mut array_builders[ENCODING_IDX]).append_value(encoding);
    downcast_u8_builder(&mut array_builders[BIG_ENDIAN_IDX]).append_value(is_bigendian);
    downcast_u32_builder(&mut array_builders[STEP_IDX]).append_value(step);
    downcast_u32_builder(&mut array_builders[INDEX_IDX]).append_value(index);

    Ok(jpeg_data)
}

/// Appends PointCloud2 field metadata to the builder.
///
/// # Contract
///
/// The `list_builder.values()` must be a `StructBuilder` for PointField.
/// This is guaranteed by the PointCloud2 schema builder creation.
///
/// # Errors
///
/// Returns `StageError::InvalidData` if the field name contains invalid UTF-8.
fn append_point_fields(
    array_builder: &mut dyn ArrayBuilder,
    field_count: u32,
    ros_msg_deserializer: &mut RosMsgDeserializer,
) -> StageResult<()> {
    let list_builder = downcast_list_builder::<StructBuilder>(array_builder);
    {
        // CONTRACT: ListBuilder<StructBuilder> inner values are StructBuilder
        #[allow(clippy::expect_used)]
        let struct_builder = list_builder
            .values()
            .as_any_mut()
            .downcast_mut::<StructBuilder>()
            .expect("list values must be StructBuilder - PointCloud2 schema contract");
        for _ in 0..field_count as usize {
            let name = ros_msg_deserializer.deserialize_string()?;
            let offset = ros_msg_deserializer.deserialize_u32();
            let datatype = ros_msg_deserializer.deserialize_u8();
            let count = ros_msg_deserializer.deserialize_u32();

            {
                let field_builders = struct_builder.field_builders_mut();
                downcast_string_builder(&mut field_builders[0]).append_value(name);
                downcast_u32_builder(&mut field_builders[1]).append_value(offset);
                downcast_u8_builder(&mut field_builders[2]).append_value(datatype);
                downcast_u32_builder(&mut field_builders[3]).append_value(count);
            }

            struct_builder.append(true);
        }
    }
    list_builder.append(true);
    Ok(())
}

fn append_ros1_header(
    array_builder: &mut dyn ArrayBuilder,
    seq: u32,
    sec: i32,
    nanosec: u32,
    frame_id: String,
) {
    let header_builder = downcast_struct_builder(array_builder);
    downcast_u32_builder(&mut header_builder.field_builders_mut()[0]).append_value(seq);

    {
        let time_builder = downcast_struct_builder(&mut header_builder.field_builders_mut()[1]);
        downcast_i32_builder(&mut time_builder.field_builders_mut()[0]).append_value(sec);
        downcast_u32_builder(&mut time_builder.field_builders_mut()[1]).append_value(nanosec);
        time_builder.append(true);
    }

    downcast_string_builder(&mut header_builder.field_builders_mut()[2]).append_value(frame_id);
    header_builder.append(true);
}

fn append_ros2_header(
    array_builder: &mut dyn ArrayBuilder,
    sec: i32,
    nanosec: u32,
    frame_id: String,
) {
    let header_builder = downcast_struct_builder(array_builder);

    {
        let time_builder = downcast_struct_builder(&mut header_builder.field_builders_mut()[0]);
        downcast_i32_builder(&mut time_builder.field_builders_mut()[0]).append_value(sec);
        downcast_u32_builder(&mut time_builder.field_builders_mut()[1]).append_value(nanosec);
        time_builder.append(true);
    }

    downcast_string_builder(&mut header_builder.field_builders_mut()[1]).append_value(frame_id);
    header_builder.append(true);
}

/// Creates a `Box<dyn ArrayBuilder>` for a `FixedSizeList` based on the given field and length.
///
/// # Errors
///
/// Returns `StageError::InvalidData` if the inner field's data type is unsupported.
pub fn create_fixed_size_list_builder(
    field: &Field,
    length: i32,
    parent_field_name: &str,
) -> Result<Box<dyn ArrayBuilder>, StageError> {
    match field.data_type() {
        DataType::Boolean => Ok(Box::new(FixedSizeListBuilder::new(
            BooleanBuilder::new(),
            length,
        ))),
        DataType::UInt8 => Ok(Box::new(FixedSizeListBuilder::new(
            UInt8Builder::new(),
            length,
        ))),
        DataType::UInt16 => Ok(Box::new(FixedSizeListBuilder::new(
            UInt16Builder::new(),
            length,
        ))),
        DataType::UInt32 => Ok(Box::new(FixedSizeListBuilder::new(
            UInt32Builder::new(),
            length,
        ))),
        DataType::UInt64 => Ok(Box::new(FixedSizeListBuilder::new(
            UInt64Builder::new(),
            length,
        ))),
        DataType::Int8 => Ok(Box::new(FixedSizeListBuilder::new(
            Int8Builder::new(),
            length,
        ))),
        DataType::Int16 => Ok(Box::new(FixedSizeListBuilder::new(
            Int16Builder::new(),
            length,
        ))),
        DataType::Int32 => Ok(Box::new(FixedSizeListBuilder::new(
            Int32Builder::new(),
            length,
        ))),
        DataType::Int64 => Ok(Box::new(FixedSizeListBuilder::new(
            Int64Builder::new(),
            length,
        ))),
        DataType::Float32 => Ok(Box::new(FixedSizeListBuilder::new(
            Float32Builder::new(),
            length,
        ))),
        DataType::Float64 => Ok(Box::new(FixedSizeListBuilder::new(
            Float64Builder::new(),
            length,
        ))),
        DataType::Utf8 => Ok(Box::new(FixedSizeListBuilder::new(
            StringBuilder::new(),
            length,
        ))),
        DataType::Struct(sub_fields) => {
            let struct_builder = create_struct_builder(sub_fields, parent_field_name)?;
            Ok(Box::new(FixedSizeListBuilder::new(struct_builder, length)))
        }
        unsupported => Err(StageError::invalid(format!(
            "Unsupported Arrow DataType {:?} in FixedSizeList for field '{}'",
            unsupported, parent_field_name
        ))),
    }
}

/// Creates a `Box<dyn ArrayBuilder>` for a `List` based on the given field.
///
/// # Errors
///
/// Returns `StageError::InvalidData` if the inner field's data type is unsupported.
pub fn create_list_builder(
    field: &Field,
    parent_field_name: &str,
) -> Result<Box<dyn ArrayBuilder>, StageError> {
    match field.data_type() {
        DataType::Boolean => Ok(Box::new(ListBuilder::new(BooleanBuilder::new()))),
        DataType::UInt8 => Ok(Box::new(ListBuilder::new(UInt8Builder::new()))),
        DataType::UInt16 => Ok(Box::new(ListBuilder::new(UInt16Builder::new()))),
        DataType::UInt32 => Ok(Box::new(ListBuilder::new(UInt32Builder::new()))),
        DataType::UInt64 => Ok(Box::new(ListBuilder::new(UInt64Builder::new()))),
        DataType::Int8 => Ok(Box::new(ListBuilder::new(Int8Builder::new()))),
        DataType::Int16 => Ok(Box::new(ListBuilder::new(Int16Builder::new()))),
        DataType::Int32 => Ok(Box::new(ListBuilder::new(Int32Builder::new()))),
        DataType::Int64 => Ok(Box::new(ListBuilder::new(Int64Builder::new()))),
        DataType::Float32 => Ok(Box::new(ListBuilder::new(Float32Builder::new()))),
        DataType::Float64 => Ok(Box::new(ListBuilder::new(Float64Builder::new()))),
        DataType::Utf8 => Ok(Box::new(ListBuilder::new(StringBuilder::new()))),
        DataType::Struct(sub_fields) => {
            let struct_builder = create_struct_builder(sub_fields, parent_field_name)?;
            Ok(Box::new(ListBuilder::new(struct_builder)))
        }
        unsupported => Err(StageError::invalid(format!(
            "Unsupported Arrow DataType {:?} in List for field '{}'",
            unsupported, parent_field_name
        ))),
    }
}

/// Creates a `StructBuilder` from a set of `Fields`.
///
/// # Errors
///
/// Returns `StageError::InvalidData` if any field's data type is unsupported.
pub fn create_struct_builder(
    fields: &Fields,
    parent_field_name: &str,
) -> Result<StructBuilder, StageError> {
    let field_builders: Result<Vec<_>, _> = fields
        .iter()
        .map(|field| create_array_builder(field, parent_field_name))
        .collect();
    Ok(StructBuilder::new(fields.clone(), field_builders?))
}

/// Creates a `Box<dyn ArrayBuilder>` for a given `Field`.
///
/// This function serves as a factory for all supported `ArrayBuilder` types.
///
/// # Errors
///
/// Returns `StageError::InvalidData` if the field's data type is unsupported.
pub fn create_array_builder(
    field: &Field,
    parent_field_name: &str,
) -> Result<Box<dyn ArrayBuilder>, StageError> {
    let field_name = if parent_field_name.is_empty() {
        field.name().to_string()
    } else {
        format!("{}.{}", parent_field_name, field.name())
    };

    match field.data_type() {
        DataType::Timestamp(TimeUnit::Nanosecond, _) => {
            Ok(Box::new(TimestampNanosecondBuilder::new()))
        }
        DataType::Boolean => Ok(Box::new(BooleanBuilder::new())),
        DataType::UInt8 => Ok(Box::new(UInt8Builder::new())),
        DataType::UInt16 => Ok(Box::new(UInt16Builder::new())),
        DataType::UInt32 => Ok(Box::new(UInt32Builder::new())),
        DataType::UInt64 => Ok(Box::new(UInt64Builder::new())),
        DataType::Int8 => Ok(Box::new(Int8Builder::new())),
        DataType::Int16 => Ok(Box::new(Int16Builder::new())),
        DataType::Int32 => Ok(Box::new(Int32Builder::new())),
        DataType::Int64 => Ok(Box::new(Int64Builder::new())),
        DataType::Float32 => Ok(Box::new(Float32Builder::new())),
        DataType::Float64 => Ok(Box::new(Float64Builder::new())),
        DataType::Utf8 => Ok(Box::new(StringBuilder::new())),
        DataType::Struct(sub_fields) => {
            Ok(Box::new(create_struct_builder(sub_fields, &field_name)?))
        }
        DataType::FixedSizeList(inner_field, length) => {
            create_fixed_size_list_builder(inner_field, *length, &field_name)
        }
        DataType::List(inner_field) => create_list_builder(inner_field, &field_name),
        unsupported => Err(StageError::invalid(format!(
            "Unsupported Arrow DataType {:?} for field '{}'",
            unsupported, field_name
        ))),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::arrow::arrow_schema_builder::ArrowSchemaBuilder;
    use crate::arrow::utils::create_builtin_message_definition_table;
    use crate::ros::schema_type_parser::parse_schema_text_to_message_definition_table;
    use arrow::array::{Array, BooleanArray, StringArray, UInt32Array, UInt64Array};
    use arrow::datatypes::{DataType, Field};

    #[test]
    fn test_parse_appends_publish_timestamp_when_present() {
        let mut message_definition_table = create_builtin_message_definition_table();
        parse_schema_text_to_message_definition_table(
            "std_msgs/Bool",
            "bool data\n",
            &mut message_definition_table,
        )
        .unwrap();
        let schema = ArrowSchemaBuilder::new(&message_definition_table)
            .build("Bool")
            .unwrap();
        let mut builders: Vec<Box<dyn ArrayBuilder>> = schema
            .fields()
            .iter()
            .map(|field| create_array_builder(field, ""))
            .collect::<Result<_, _>>()
            .unwrap();

        let mut parser = RosMsgArrowParser::new(
            RosGeneration::ROS1,
            &message_definition_table,
            "std_msgs/Bool",
            &[1],
            123,
            Some(456),
        );
        parser.parse(&mut builders).unwrap();

        let timestamp_array = builders[0]
            .finish()
            .as_any()
            .downcast_ref::<UInt64Array>()
            .unwrap()
            .clone();
        let publish_array = builders[1]
            .finish()
            .as_any()
            .downcast_ref::<UInt64Array>()
            .unwrap()
            .clone();
        let data_array = builders[2]
            .finish()
            .as_any()
            .downcast_ref::<BooleanArray>()
            .unwrap()
            .clone();

        assert_eq!(timestamp_array.value(0), 123);
        assert_eq!(publish_array.value(0), 456);
        assert!(data_array.value(0));
    }

    #[test]
    fn test_parse_appends_null_publish_timestamp_when_absent() {
        let mut message_definition_table = create_builtin_message_definition_table();
        parse_schema_text_to_message_definition_table(
            "std_msgs/Bool",
            "bool data\n",
            &mut message_definition_table,
        )
        .unwrap();
        let schema = ArrowSchemaBuilder::new(&message_definition_table)
            .build("Bool")
            .unwrap();
        let mut builders: Vec<Box<dyn ArrayBuilder>> = schema
            .fields()
            .iter()
            .map(|field| create_array_builder(field, ""))
            .collect::<Result<_, _>>()
            .unwrap();

        let mut parser = RosMsgArrowParser::new(
            RosGeneration::ROS1,
            &message_definition_table,
            "std_msgs/Bool",
            &[1],
            123,
            None,
        );
        parser.parse(&mut builders).unwrap();

        let publish_array = builders[1]
            .finish()
            .as_any()
            .downcast_ref::<UInt64Array>()
            .unwrap()
            .clone();

        assert!(publish_array.is_null(0));
    }

    #[test]
    fn test_decode_image_ros2() {
        // ... (setup code remains same until verification) ...

        // Construct a mock ROS2 Image message (CDR format)
        // Header: sec(i32), nanosec(u32), frame_id(string)
        // height(u32), width(u32), encoding(string), is_bigendian(u8), step(u32), data(uint8[])

        let mut data = Vec::new();

        // CDR Header (4 bytes)
        // Byte 1: 0x01 for Little Endian
        data.extend_from_slice(&[0x00, 0x01, 0x00, 0x00]);

        // Header
        data.extend_from_slice(&123i32.to_le_bytes()); // sec
        data.extend_from_slice(&456u32.to_le_bytes()); // nanosec

        let frame_id = "camera_frame";
        data.extend_from_slice(&(frame_id.len() as u32 + 1).to_le_bytes()); // length including null
        data.extend_from_slice(frame_id.as_bytes());
        data.push(0); // null terminator

        // height (u32) - aligned to 4.
        while data.len() % 4 != 0 {
            data.push(0);
        }
        data.extend_from_slice(&10u32.to_le_bytes()); // height

        // width (u32)
        data.extend_from_slice(&10u32.to_le_bytes()); // width

        // encoding (string)
        let encoding = "rgb8";
        data.extend_from_slice(&(encoding.len() as u32 + 1).to_le_bytes());
        data.extend_from_slice(encoding.as_bytes());
        data.push(0);

        // is_bigendian (u8)
        data.push(0);

        // step (u32) - aligned to 4.
        while data.len() % 4 != 0 {
            data.push(0);
        }
        data.extend_from_slice(&(10 * 3_u32).to_le_bytes()); // step (width * 3)

        // data (uint8[]) - sequence
        // Sequence length (u32)
        let image_data = vec![0u8; 10 * 10 * 3]; // 10x10 RGB black image
        data.extend_from_slice(&(image_data.len() as u32).to_le_bytes());
        data.extend_from_slice(&image_data);

        // Prepare builders
        let mut builders: Vec<Box<dyn ArrayBuilder>> = vec![
            Box::new(UInt64Builder::new()),
            Box::new(UInt64Builder::new()),
            // ... (rest of builders)
            Box::new(StructBuilder::new(
                Fields::from(vec![
                    Field::new(
                        "stamp",
                        DataType::Struct(Fields::from(vec![
                            Field::new("sec", DataType::Int32, false),
                            Field::new("nanosec", DataType::UInt32, false),
                        ])),
                        false,
                    ),
                    Field::new("frame_id", DataType::Utf8, false),
                ]),
                vec![
                    Box::new(StructBuilder::new(
                        Fields::from(vec![
                            Field::new("sec", DataType::Int32, false),
                            Field::new("nanosec", DataType::UInt32, false),
                        ]),
                        vec![
                            Box::new(Int32Builder::new()),
                            Box::new(UInt32Builder::new()),
                        ],
                    )),
                    Box::new(StringBuilder::new()),
                ],
            )),
            Box::new(UInt32Builder::new()),
            Box::new(UInt32Builder::new()),
            Box::new(StringBuilder::new()),
            Box::new(UInt8Builder::new()),
            Box::new(UInt32Builder::new()),
            Box::new(UInt32Builder::new()),
        ];

        let jpeg_bytes = decode_image(
            &mut builders,
            &data,
            1000,
            Some(1100),
            0,
            RosGeneration::ROS2,
            "/test/image",
        )
        .expect("decode_image should succeed for valid rgb8 image");

        // Verify JPEG magic bytes
        assert!(!jpeg_bytes.is_empty());
        assert_eq!(jpeg_bytes[0], 0xFF);
        assert_eq!(jpeg_bytes[1], 0xD8);

        // Verify builders
        let height_array = builders[3].finish();
        let width_array = builders[4].finish();
        let encoding_array = builders[5].finish();

        let height_array = height_array.as_any().downcast_ref::<UInt32Array>().unwrap();
        let width_array = width_array.as_any().downcast_ref::<UInt32Array>().unwrap();
        let encoding_array = encoding_array
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();

        assert_eq!(height_array.value(0), 10);
        assert_eq!(width_array.value(0), 10);
        assert_eq!(encoding_array.value(0), "rgb8");
    }

    #[test]
    fn test_decode_image_unsupported_encoding_returns_error() {
        // Based on test_decode_image_ros2, but with unsupported "16UC1" encoding
        // This verifies that decode_image returns StageError::InvalidData for unsupported encodings

        let mut data = Vec::new();

        // CDR Header (4 bytes) - Little Endian
        data.extend_from_slice(&[0x00, 0x01, 0x00, 0x00]);

        // Header
        data.extend_from_slice(&123i32.to_le_bytes()); // sec
        data.extend_from_slice(&456u32.to_le_bytes()); // nanosec

        let frame_id = "camera_frame";
        data.extend_from_slice(&(frame_id.len() as u32 + 1).to_le_bytes());
        data.extend_from_slice(frame_id.as_bytes());
        data.push(0); // null terminator

        // height (u32) - aligned to 4
        while data.len() % 4 != 0 {
            data.push(0);
        }
        data.extend_from_slice(&10u32.to_le_bytes());

        // width (u32)
        data.extend_from_slice(&10u32.to_le_bytes());

        // encoding: "16UC1" (unsupported depth encoding)
        let encoding = "16UC1";
        data.extend_from_slice(&(encoding.len() as u32 + 1).to_le_bytes());
        data.extend_from_slice(encoding.as_bytes());
        data.push(0);

        // is_bigendian (u8)
        data.push(0);

        // step (u32) - aligned to 4
        while data.len() % 4 != 0 {
            data.push(0);
        }
        data.extend_from_slice(&(10 * 2_u32).to_le_bytes()); // 16-bit = 2 bytes per pixel

        // data (uint8[])
        let image_data = vec![0u8; 10 * 10 * 2];
        data.extend_from_slice(&(image_data.len() as u32).to_le_bytes());
        data.extend_from_slice(&image_data);

        // Builders - same structure as test_decode_image_ros2
        let mut builders: Vec<Box<dyn ArrayBuilder>> = vec![
            Box::new(UInt64Builder::new()),
            Box::new(UInt64Builder::new()),
            Box::new(StructBuilder::new(
                Fields::from(vec![
                    Field::new(
                        "stamp",
                        DataType::Struct(Fields::from(vec![
                            Field::new("sec", DataType::Int32, false),
                            Field::new("nanosec", DataType::UInt32, false),
                        ])),
                        false,
                    ),
                    Field::new("frame_id", DataType::Utf8, false),
                ]),
                vec![
                    Box::new(StructBuilder::new(
                        Fields::from(vec![
                            Field::new("sec", DataType::Int32, false),
                            Field::new("nanosec", DataType::UInt32, false),
                        ]),
                        vec![
                            Box::new(Int32Builder::new()),
                            Box::new(UInt32Builder::new()),
                        ],
                    )),
                    Box::new(StringBuilder::new()),
                ],
            )),
            Box::new(UInt32Builder::new()),
            Box::new(UInt32Builder::new()),
            Box::new(StringBuilder::new()),
            Box::new(UInt8Builder::new()),
            Box::new(UInt32Builder::new()),
            Box::new(UInt32Builder::new()),
        ];

        let result = decode_image(
            &mut builders,
            &data,
            1000,
            Some(1100),
            0,
            RosGeneration::ROS2,
            "/test/depth_image",
        );

        assert!(
            result.is_err(),
            "decode_image should fail for unsupported 16UC1 encoding"
        );
        let err = result.unwrap_err();
        assert!(
            matches!(err, StageError::InvalidData { .. }),
            "Expected StageError::InvalidData, got {:?}",
            err
        );
    }
}
