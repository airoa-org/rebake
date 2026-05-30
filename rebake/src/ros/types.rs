/// Represents a parsed ROS message definition.
///
/// This struct provides a strongly-typed representation of a `.msg` file's content,
/// including its fields and nested types. It serves as an intermediate representation
/// before being converted into an Arrow `Schema`.
///
/// # Example
///
/// The definition for `sensor_msgs/JointState` would be parsed into a `MessageDefinition`
/// for `JointState` and another for the nested `std_msgs/Header`.
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
/// **Section 1 (JointState):**
///
/// ```text
/// MessageDefinition {  
///     name: "JointState"  
///     fields: [  
///         FieldDefinition {  
///             name: "header",  
///             data_type: Base::Struct("Header")  
///         },  
///         FieldDefinition {  
///             name: "name",  
///             data_type: Sequence(Base::Primitive::String)  
///         },
///         FieldDefinition {  
///             name: "position",  
///             data_type: Sequence(Base::Primitive::Float64)  
///         },
///         FieldDefinition {  
///             name: "velocity",  
///             data_type: Sequence(Base::Primitive::Float64)
///         },
///         FieldDefinition {  
///             name: "effort",  
///             data_type: Sequence(Base::Primitive::Float64)
///         },
///     ],
/// }
/// ```
///
/// **Section 2 (Header):**
///
/// ```text
/// MessageDefinition {
///     name: "Header"
///     fields: [
///         FieldDefinition {
///             name: "seq",
///             data_type: Base::Primitive::UInt32,
///         },
///         FieldDefinition {
///             name: "stamp",
///             data_type: Base::Struct("name"),
///         },
///         FieldDefinition {
///             name: "frame_id",
///             data_type: Base::Primitive::String
///         },
///     ],
/// }
/// ```
///
/// See: [ROS.org: msg](https://wiki.ros.org/msg)
#[derive(Clone, Debug, PartialEq)]
pub struct MessageDefinition {
    pub name: String,
    pub fields: Vec<FieldDefinition>,
}

impl MessageDefinition {
    /// Creates a new `MessageDefinition`.
    pub fn new(name: String, fields: Vec<FieldDefinition>) -> MessageDefinition {
        MessageDefinition { name, fields }
    }
}

/// Represents a single field within a ROS message definition.
///
/// See: "2.1 Fields" section of [ROS.org: msg](https://wiki.ros.org/msg)
#[derive(Clone, Debug, PartialEq)]
pub struct FieldDefinition {
    pub name: String,
    pub data_type: FieldType,
}

impl FieldDefinition {
    /// Creates a new `FieldDefinition`.
    pub fn new(name: String, data_type: FieldType) -> FieldDefinition {
        FieldDefinition { name, data_type }
    }

    /// Creates a new primitive `FieldDefinition`.
    pub fn new_primitive(name: String, data_type: Primitive) -> FieldDefinition {
        FieldDefinition::new(name, FieldType::Base(BaseType::Primitive(data_type)))
    }

    /// Creates a new struct `FieldDefinition`.
    pub fn new_struct(name: String, data_type: String) -> FieldDefinition {
        FieldDefinition::new(name, FieldType::Base(BaseType::Struct(data_type)))
    }

    /// Creates a new fixed-size array `FieldDefinition`.
    pub fn new_array(name: String, data_type: BaseType, length: u32) -> FieldDefinition {
        FieldDefinition::new(name, FieldType::Array { data_type, length })
    }

    /// Creates a new sequence (dynamic array) `FieldDefinition`.
    pub fn new_sequence(name: String, data_type: BaseType) -> FieldDefinition {
        FieldDefinition::new(name, FieldType::Sequence(data_type))
    }
}

/// An enum representing the data type of a field in a ROS message.
#[derive(Clone, Debug, PartialEq)]
pub enum FieldType {
    /// A base type, which can be either a primitive or a nested struct.
    Base(BaseType),
    /// A fixed-size array.
    Array { data_type: BaseType, length: u32 },
    /// A variable-size array (sequence).
    Sequence(BaseType),
}

/// Represents a base data type, which is either a primitive or a struct.
#[derive(Clone, Debug, PartialEq)]
pub enum BaseType {
    /// A ROS primitive type (e.g., `int32`, `string`, `bool`).
    Primitive(Primitive),
    /// A nested message type, identified by its name.
    ///
    /// The full definition for this struct is found by looking up the name
    /// in the `Pipeline.message_definition_table`.
    Struct(String),
}

/// An enum for all ROS primitive types.
#[derive(Clone, Debug, PartialEq)]
pub enum Primitive {
    Bool,
    Byte,
    Char,
    Float32,
    Float64,
    Int8,
    UInt8,
    Int16,
    UInt16,
    Int32,
    UInt32,
    Int64,
    UInt64,
    String,
}
