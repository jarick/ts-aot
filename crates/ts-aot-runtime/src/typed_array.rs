use std::rc::Rc;

use crate::array_buffer::{__ts_aot_array_buffer_new, ArrayBufferHandle};
use crate::host::__ts_aot_throw;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TypedArrayKind {
    Int8,
    Uint8,
    Uint8Clamped,
    Int16,
    Uint16,
    Int32,
    Uint32,
    Float32,
    Float64,
}

impl TypedArrayKind {
    #[must_use]
    pub fn bytes_per_element(self) -> usize {
        match self {
            Self::Int8 | Self::Uint8 | Self::Uint8Clamped => 1,
            Self::Int16 | Self::Uint16 => 2,
            Self::Int32 | Self::Uint32 | Self::Float32 => 4,
            Self::Float64 => 8,
        }
    }

    #[must_use]
    pub fn from_id(id: i64) -> Self {
        match id {
            0 => Self::Int8,
            1 => Self::Uint8,
            2 => Self::Uint8Clamped,
            3 => Self::Int16,
            4 => Self::Uint16,
            5 => Self::Int32,
            6 => Self::Uint32,
            7 => Self::Float32,
            8 => Self::Float64,
            _ => __ts_aot_throw(format!(
                "RangeError: unknown TypedArray kind id {id} (valid ids 0..=8)"
            )),
        }
    }
}

#[derive(Clone)]
pub struct TypedArrayHandle {
    buffer: Rc<ArrayBufferHandle>,
    byte_offset: usize,
    byte_length: usize,
    length: usize,
    kind: TypedArrayKind,
}

impl TypedArrayHandle {
    #[must_use]
    pub fn buffer(&self) -> Rc<ArrayBufferHandle> {
        Rc::clone(&self.buffer)
    }

    #[must_use]
    pub fn byte_offset(&self) -> i64 {
        i64::try_from(self.byte_offset).unwrap_or(i64::MAX)
    }

    #[must_use]
    pub fn byte_length(&self) -> i64 {
        i64::try_from(self.byte_length).unwrap_or(i64::MAX)
    }

    #[must_use]
    pub fn length(&self) -> i64 {
        i64::try_from(self.length).unwrap_or(i64::MAX)
    }

    #[must_use]
    pub fn kind(&self) -> TypedArrayKind {
        self.kind
    }

    #[must_use]
    pub fn raw_buffer(&self) -> &ArrayBufferHandle {
        &self.buffer
    }

    #[must_use]
    pub fn raw_byte_offset(&self) -> usize {
        self.byte_offset
    }

    #[must_use]
    pub fn raw_length(&self) -> usize {
        self.length
    }
}

#[must_use]
pub fn __ts_aot_typed_array_new(length: i64, kind_id: i64) -> TypedArrayHandle {
    if length < 0 {
        __ts_aot_throw(format!(
            "RangeError: TypedArray length {length} is negative"
        ));
    }
    let Ok(elem_count) = usize::try_from(length) else {
        __ts_aot_throw(format!(
            "RangeError: TypedArray length {length} exceeds platform usize"
        ));
    };
    let kind = TypedArrayKind::from_id(kind_id);
    let bytes_per_elem = kind.bytes_per_element();
    let Some(total_bytes) = elem_count.checked_mul(bytes_per_elem) else {
        __ts_aot_throw(format!(
            "RangeError: TypedArray byte length overflow ({elem_count} elements * {bytes_per_elem} bytes)"
        ));
    };
    let buffer = __ts_aot_array_buffer_new(i64::try_from(total_bytes).unwrap_or(i64::MAX));
    TypedArrayHandle {
        buffer: Rc::new(buffer),
        byte_offset: 0,
        byte_length: total_bytes,
        length: elem_count,
        kind,
    }
}
