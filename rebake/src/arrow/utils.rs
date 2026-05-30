use std::collections::HashMap;

use crate::ros::types::{FieldDefinition, MessageDefinition, Primitive};

pub fn create_builtin_message_definition_table() -> HashMap<String, MessageDefinition> {
    let mut message_definition_table = HashMap::new();
    message_definition_table.insert("time".to_string(), create_time_message_definition());
    message_definition_table.insert("duration".to_string(), create_duration_message_definition());
    message_definition_table
}

pub fn create_time_message_definition() -> MessageDefinition {
    MessageDefinition::new(
        "time".to_string(),
        vec![
            FieldDefinition::new_primitive("sec".to_string(), Primitive::Int32),
            FieldDefinition::new_primitive("nanosec".to_string(), Primitive::UInt32),
        ],
    )
}

pub fn create_duration_message_definition() -> MessageDefinition {
    MessageDefinition::new(
        "duration".to_string(),
        vec![
            FieldDefinition::new_primitive("sec".to_string(), Primitive::Int32),
            FieldDefinition::new_primitive("nanosec".to_string(), Primitive::UInt32),
        ],
    )
}
