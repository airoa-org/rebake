use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Custom deserializer for bool that accepts string ("true"/"false")
fn deserialize_bool_from_string<'de, D>(deserializer: D) -> Result<bool, D::Error>
where
    D: Deserializer<'de>,
{
    use serde::de::Error;

    let s = String::deserialize(deserializer)?;
    match s.to_lowercase().as_str() {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(Error::custom(format!("Invalid boolean string: {}", s))),
    }
}

/// Custom serializer for bool that outputs string ("true"/"false")
fn serialize_bool_as_string<S>(value: &bool, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    serializer.serialize_str(if *value { "true" } else { "false" })
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone, Default)]
pub struct Feature {
    #[serde(default)]
    pub dtype: DType,
    #[serde(default)]
    pub shape: Vec<usize>,
    #[serde(default)]
    pub names: Option<Vec<String>>,
    #[serde(rename = "info", skip_serializing_if = "Option::is_none", default)]
    pub video_info: Option<VideoInfo>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub description: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone, Default)]
pub enum DType {
    #[default]
    #[serde(rename = "bool")]
    Bool,
    #[serde(rename = "int8")]
    Int8,
    #[serde(rename = "int16")]
    Int16,
    #[serde(rename = "int32")]
    Int32,
    #[serde(rename = "int64")]
    Int64,
    #[serde(rename = "uint8")]
    UInt8,
    #[serde(rename = "uint16")]
    UInt16,
    #[serde(rename = "uint32")]
    UInt32,
    #[serde(rename = "uint64")]
    UInt64,
    #[serde(rename = "float32")]
    Float32,
    #[serde(rename = "float64")]
    Float64,
    #[serde(rename = "string")]
    String,
    #[serde(rename = "video")]
    Video,
    #[serde(rename = "image")]
    Image,
}

impl From<&polars::prelude::DataType> for DType {
    fn from(dtype: &polars::prelude::DataType) -> Self {
        match dtype {
            polars::prelude::DataType::Boolean => DType::Bool,
            polars::prelude::DataType::Int8 => DType::Int8,
            polars::prelude::DataType::Int16 => DType::Int16,
            polars::prelude::DataType::Int32 => DType::Int32,
            polars::prelude::DataType::Int64 => DType::Int64,
            polars::prelude::DataType::UInt8 => DType::UInt8,
            polars::prelude::DataType::UInt16 => DType::UInt16,
            polars::prelude::DataType::UInt32 => DType::UInt32,
            polars::prelude::DataType::UInt64 => DType::UInt64,
            polars::prelude::DataType::Float32 => DType::Float32,
            polars::prelude::DataType::Float64 => DType::Float64,
            polars::prelude::DataType::String => DType::String,
            other => panic!(
                "Unsupported Polars DataType: {:?}. This is a bug in rebake-rs. \
                 Supported types: Boolean, Int8, Int16, Int32, Int64, UInt8, UInt16, \
                 UInt32, UInt64, Float32, Float64, String",
                other
            ),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct VideoInfo {
    #[serde(rename = "video.fps")]
    pub fps: usize,
    #[serde(rename = "video.codec")]
    pub codec: String,
    #[serde(rename = "video.pix_fmt")]
    pub pix_fmt: String,
    #[serde(
        rename = "video.is_depth_map",
        serialize_with = "serialize_bool_as_string",
        deserialize_with = "deserialize_bool_from_string"
    )]
    pub is_depth_map: bool,
    #[serde(
        rename = "has_audio",
        serialize_with = "serialize_bool_as_string",
        deserialize_with = "deserialize_bool_from_string"
    )]
    pub has_audio: bool,
}
