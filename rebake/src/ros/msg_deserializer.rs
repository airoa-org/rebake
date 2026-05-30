use byteorder::{BigEndian, ByteOrder, LittleEndian};

use crate::core::error::{StageError, StageResult};

const ROS2_CDR_HEADER_SIZE: usize = 4;

#[derive(Copy, Clone, PartialEq, Debug)]
pub enum RosGeneration {
    ROS1,
    ROS2,
}

#[derive(Copy, Clone, PartialEq, Debug, Default)]
pub enum Endianness {
    #[default]
    BigEndian,
    LittleEndian,
}

impl Endianness {
    fn from_cdr_header(data: &[u8]) -> Self {
        match data[1] {
            0x00 => Self::BigEndian,
            _ => Self::LittleEndian,
        }
    }
}

/// Deserializes primitive types from a ROS message byte stream.
///
/// This deserializer follows the standard ROS serialization format, reading data
/// in Little Endian byte order.
///
/// See: [roscpp_serialization/include/ros/serialization.h](https://github.com/ros/roscpp_core/blob/noetic-devel/roscpp_serialization/include/ros/serialization.h)
#[derive(Default, Debug)]
pub struct RosMsgDeserializer<'a> {
    /// The byte slice of the ROS message data.
    data: &'a [u8],
    /// The current position of the cursor within the byte slice.
    position: usize,
    /// The byte order of the ROS message data.
    byte_order: Endianness,
    /// Whether to enable alignment of the cursor.
    enable_alignment: bool,
    enable_split_string_last: bool,
}

impl<'a> RosMsgDeserializer<'a> {
    pub fn new(data: &'a [u8], generation: RosGeneration) -> Self {
        match generation {
            RosGeneration::ROS1 => Self::new_ros1(data),
            RosGeneration::ROS2 => Self::new_ros2(data),
        }
    }

    pub fn new_ros1(data: &'a [u8]) -> Self {
        Self {
            data,
            position: 0,
            byte_order: Endianness::LittleEndian,
            enable_alignment: false,
            enable_split_string_last: false,
        }
    }

    pub fn new_ros2(data: &'a [u8]) -> Self {
        let byte_order = Endianness::from_cdr_header(data);

        Self {
            data,
            position: ROS2_CDR_HEADER_SIZE,
            byte_order,
            enable_alignment: true,
            enable_split_string_last: true,
        }
    }

    #[inline]
    pub fn align_to(&mut self, count: usize) {
        let modulo = (self.position - ROS2_CDR_HEADER_SIZE) % count;
        if modulo != 0 {
            self.position += count - modulo;
        }
    }

    #[inline]
    pub fn next_bytes(&mut self, count: usize) -> &'a [u8] {
        self.position += count;
        &self.data[self.position - count..self.position]
    }

    #[inline]
    pub fn read_sequence_length(&mut self) -> u32 {
        if self.enable_alignment {
            self.align_to(4);
        }
        let header = self.next_bytes(4);

        match self.byte_order {
            Endianness::BigEndian => BigEndian::read_u32(header),
            Endianness::LittleEndian => LittleEndian::read_u32(header),
        }
    }

    pub fn deserialize_f64(&mut self) -> f64 {
        if self.enable_alignment {
            self.align_to(8);
        }
        let bytes = self.next_bytes(8);

        match self.byte_order {
            Endianness::BigEndian => BigEndian::read_f64(bytes),
            Endianness::LittleEndian => LittleEndian::read_f64(bytes),
        }
    }

    pub fn deserialize_f32(&mut self) -> f32 {
        if self.enable_alignment {
            self.align_to(4);
        }
        let bytes = self.next_bytes(4);

        match self.byte_order {
            Endianness::BigEndian => BigEndian::read_f32(bytes),
            Endianness::LittleEndian => LittleEndian::read_f32(bytes),
        }
    }

    pub fn deserialize_bool(&mut self) -> bool {
        let bytes = self.next_bytes(1);

        bytes[0] == 0x01
    }

    pub fn deserialize_i8(&mut self) -> i8 {
        let bytes = self.next_bytes(1);

        bytes[0] as i8
    }

    pub fn deserialize_u8(&mut self) -> u8 {
        let bytes = self.next_bytes(1);

        bytes[0]
    }

    pub fn deserialize_char(&mut self) -> char {
        let byte = self.next_bytes(1)[0];

        byte as char
    }

    pub fn deserialize_i16(&mut self) -> i16 {
        if self.enable_alignment {
            self.align_to(2);
        }
        let bytes = self.next_bytes(2);

        match self.byte_order {
            Endianness::BigEndian => BigEndian::read_i16(bytes),
            Endianness::LittleEndian => LittleEndian::read_i16(bytes),
        }
    }

    pub fn deserialize_u16(&mut self) -> u16 {
        if self.enable_alignment {
            self.align_to(2);
        }
        let bytes = self.next_bytes(2);

        match self.byte_order {
            Endianness::BigEndian => BigEndian::read_u16(bytes),
            Endianness::LittleEndian => LittleEndian::read_u16(bytes),
        }
    }

    pub fn deserialize_i32(&mut self) -> i32 {
        if self.enable_alignment {
            self.align_to(4);
        }
        let bytes = self.next_bytes(4);

        match self.byte_order {
            Endianness::BigEndian => BigEndian::read_i32(bytes),
            Endianness::LittleEndian => LittleEndian::read_i32(bytes),
        }
    }

    pub fn deserialize_u32(&mut self) -> u32 {
        if self.enable_alignment {
            self.align_to(4);
        }
        let bytes = self.next_bytes(4);

        match self.byte_order {
            Endianness::BigEndian => BigEndian::read_u32(bytes),
            Endianness::LittleEndian => LittleEndian::read_u32(bytes),
        }
    }

    pub fn deserialize_i64(&mut self) -> i64 {
        if self.enable_alignment {
            self.align_to(8);
        }
        let bytes = self.next_bytes(8);

        match self.byte_order {
            Endianness::BigEndian => BigEndian::read_i64(bytes),
            Endianness::LittleEndian => LittleEndian::read_i64(bytes),
        }
    }

    pub fn deserialize_u64(&mut self) -> u64 {
        if self.enable_alignment {
            self.align_to(8);
        }
        let bytes = self.next_bytes(8);

        match self.byte_order {
            Endianness::BigEndian => BigEndian::read_u64(bytes),
            Endianness::LittleEndian => LittleEndian::read_u64(bytes),
        }
    }

    /// Deserializes a UTF-8 string from the ROS message byte stream.
    ///
    /// # Errors
    ///
    /// Returns `StageError::InvalidData` if the byte sequence is not valid UTF-8.
    pub fn deserialize_string(&mut self) -> StageResult<String> {
        if self.enable_alignment {
            self.align_to(4);
        }
        let header = self.next_bytes(4);
        let byte_length = match self.byte_order {
            Endianness::BigEndian => BigEndian::read_u32(header),
            Endianness::LittleEndian => LittleEndian::read_u32(header),
        };
        let mut bytes = self.next_bytes(byte_length as usize);

        if self.enable_split_string_last {
            bytes = match bytes.split_last() {
                None => bytes,
                Some((_null_char, contents)) => contents,
            };
        }
        std::str::from_utf8(bytes)
            .map(|s| s.to_string())
            .map_err(|e| StageError::invalid_with("invalid UTF-8 in ROS string field", e))
    }
}
