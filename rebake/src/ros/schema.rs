use crate::core::error::{StageError, StageResult};

static SCHEMA_SECTION_DELIMITER: &str =
    "================================================================================";

/// Represents a single section of a ROS message definition file.
///
/// ROS message definition files can contain multiple message types, separated by
/// `================================================================================`.
/// This struct holds the content of one such section.
///
/// # Example
///
/// A `.msg` file for `sensor_msgs/JointState` contains definitions for both
/// `JointState` and the nested `std_msgs/Header`. This content is split into
/// two `SchemaSection` objects.
///
/// ```text
/// Header header
/// string[] name
/// float64[] position
/// float64[] velocity
/// float64[] effort
///
/// ================================================================================
/// MSG: std_msgs/Header
/// # Standard metadata for higher-level stamped data types.
/// # This is generally used to communicate timestamped data
/// # in a particular coordinate frame.
/// #
/// # sequence ID: consecutively increasing ID
/// uint32 seq
/// #Two-integer timestamp that is expressed as:
/// # * stamp.sec: seconds (stamp_secs) since epoch (in Python the variable is called 'secs')
/// # * stamp.nsec: nanoseconds since stamp_secs (in Python the variable is called 'nsecs')
/// # time-handling sugar is provided by the client library
/// time stamp
/// #Frame this data is associated with
/// string frame_id
/// ```
///
/// **Section 1:**
/// ```text
/// SchemaSection {
///     type_name: "JointState",
///     content:
///         """
///         Header header
///         string[] name
///         float64[] position
///         float64[] velocity
///         float64[] effort
///         """
/// }
/// ```
///
/// **Section 2:**
/// ```text
/// SchemaSection {
///     type_name: "Header",
///     content:
///         """
///         # Standard metadata for higher-level stamped data types.
///         # This is generally used to communicate timestamped data
///         # in a particular coordinate frame.
///         #
///         # sequence ID: consecutively increasing ID
///         uint32 seq
///         #Two-integer timestamp that is expressed as:
///         # * stamp.sec: seconds (stamp_secs) since epoch (in Python the variable is called 'secs')
///         # * stamp.nsec: nanoseconds since stamp_secs (in Python the variable is called 'nsecs')
///         # time-handling sugar is provided by the client library
///         time stamp
///         #Frame this data is associated with
///         string frame_id
///         """
/// }
/// ```
#[derive(Debug, Clone)]
pub struct SchemaSection {
    pub type_name: String,
    pub content: String,
}

impl SchemaSection {
    /// Creates a new `SchemaSection`.
    pub fn new(type_name: String, content: String) -> SchemaSection {
        SchemaSection { type_name, content }
    }
}

/// Splits a full ROS message definition string into multiple `SchemaSection`s.
///
/// Each section is delimited by a line of `===` characters.
/// See [`SchemaSection`] for more details.
///
/// # Errors
///
/// Returns `StageError::InvalidData` if a nested type section is malformed:
/// - Missing type name after "MSG:" marker
/// - Missing newline after type declaration
pub fn split_schema_text_to_sections(
    schema_name: &str,
    schema_text: &str,
) -> StageResult<Vec<SchemaSection>> {
    let mut sections = Vec::new();

    // Split the message definition text by the delimiter line.
    let raw_sections: Vec<&str> = schema_text
        .split(SCHEMA_SECTION_DELIMITER)
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();

    for (index, raw_section) in raw_sections.iter().enumerate() {
        let (type_name, content) = if index == 0 {
            // The first section is the primary message type.
            (schema_name, *raw_section)
        } else {
            // Subsequent sections are for nested types and start with "MSG: <type_name>".
            // The format is "MSG: type_name\n<content>", so we expect at least 2 tokens.
            let type_name = raw_section.split_whitespace().nth(1).ok_or_else(|| {
                StageError::invalid(format!(
                    "MSG section must have format 'MSG: type_name', got: '{}'",
                    raw_section.lines().next().unwrap_or("")
                ))
            })?;
            let newline_pos = raw_section.find('\n').ok_or_else(|| {
                StageError::invalid(format!(
                    "MSG section must contain a newline after type declaration for '{}'",
                    type_name
                ))
            })?;
            let content = &raw_section[newline_pos + 1..];
            (type_name, content)
        };

        let schema_section = SchemaSection::new(type_name.to_string(), content.to_string());
        sections.push(schema_section);
    }

    Ok(sections)
}
