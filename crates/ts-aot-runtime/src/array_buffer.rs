use std::rc::Rc;

use crate::host::__ts_aot_throw;

#[derive(Clone)]
pub struct ArrayBufferHandle {
    bytes: Rc<Vec<u8>>,
    byte_length: usize,
}

impl ArrayBufferHandle {
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    #[must_use]
    pub fn byte_length(&self) -> usize {
        self.byte_length
    }

    #[must_use]
    pub fn shared_bytes(&self) -> Rc<Vec<u8>> {
        Rc::clone(&self.bytes)
    }

    #[must_use]
    pub fn from_slice(data: &[u8]) -> Self {
        let mut bytes: Vec<u8> = Vec::new();
        if bytes.try_reserve_exact(data.len()).is_err() {
            __ts_aot_throw(format!(
                "RangeError: ArrayBuffer allocation failed (out of memory) for {} bytes",
                data.len()
            ));
        }
        bytes.extend_from_slice(data);
        ArrayBufferHandle {
            bytes: Rc::new(bytes),
            byte_length: data.len(),
        }
    }
}

#[must_use]
pub fn __ts_aot_array_buffer_new(byte_length: i64) -> ArrayBufferHandle {
    if byte_length < 0 {
        __ts_aot_throw(format!(
            "RangeError: ArrayBuffer byteLength {byte_length} is negative"
        ));
    }
    let Ok(len) = usize::try_from(byte_length) else {
        __ts_aot_throw(format!(
            "RangeError: ArrayBuffer byteLength {byte_length} exceeds platform usize"
        ));
    };
    let mut bytes: Vec<u8> = Vec::new();
    if bytes.try_reserve_exact(len).is_err() {
        __ts_aot_throw(format!(
            "RangeError: ArrayBuffer byteLength {len} allocation failed (out of memory)"
        ));
    }
    bytes.resize(len, 0u8);
    ArrayBufferHandle {
        bytes: Rc::new(bytes),
        byte_length: len,
    }
}

#[must_use]
pub fn __ts_aot_array_buffer_slice(
    buf: &ArrayBufferHandle,
    begin: i64,
    end: i64,
) -> ArrayBufferHandle {
    let len = buf.byte_length();
    let resolve_bound = |raw: i64| -> usize {
        if raw < 0 {
            let abs_raw = raw
                .checked_abs()
                .and_then(|v| usize::try_from(v).ok())
                .unwrap_or(len);
            len.saturating_sub(abs_raw)
        } else {
            usize::try_from(raw).unwrap_or(len).min(len)
        }
    };
    let lo = resolve_bound(begin);
    let hi = resolve_bound(end);
    let new_len = hi.saturating_sub(lo);
    ArrayBufferHandle::from_slice(&buf.bytes[lo..lo + new_len])
}
