use ts_aot_core::MAX_DENSE_ARRAY_LEN;

use crate::host::__ts_aot_throw;
use crate::string::JsString;

fn validate_dense_len(len: i64, fn_name: &str) {
    if len < 0 || len >= i64::from(MAX_DENSE_ARRAY_LEN) {
        __ts_aot_throw(format!(
            "{fn_name}: out-of-range length {len} \
             (expected 0 <= len < {MAX_DENSE_ARRAY_LEN}, the AOT dense-Vec cap; \
             lengths above this would attempt to allocate billions of bytes and OOM)"
        ));
    }
}

#[must_use]
pub fn __ts_aot_array_create<T>() -> Vec<T> {
    Vec::new()
}

#[must_use]
pub fn __ts_aot_array_create_with_len<T: Default + Clone>(len: i64) -> Vec<T> {
    validate_dense_len(len, "__ts_aot_array_create_with_len");
    vec![T::default(); usize::try_from(len).expect("checked above")]
}

pub fn __ts_aot_array_push<T>(arr: &mut Vec<T>, item: T) {
    arr.push(item);
}

#[must_use]
pub fn __ts_aot_array_from<T: Clone>(arr: &[T]) -> Vec<T> {
    arr.to_owned()
}

#[must_use]
pub fn __ts_aot_array_from_string(s: &JsString) -> Vec<JsString> {
    char::decode_utf16(s.units_iter())
        .map(|result| match result {
            Ok(c) => JsString::Valid(c.to_string()),
            Err(e) => JsString::from_units(vec![e.unpaired_surrogate()]),
        })
        .collect()
}

#[must_use]
pub fn __ts_aot_array_from_mapped<T, R, F>(arr: &[T], mut mapfn: F) -> Vec<R>
where
    T: Clone,
    F: FnMut(T, i64) -> R,
{
    arr.iter()
        .enumerate()
        .map(|(i, x)| mapfn(x.clone(), i64::try_from(i).unwrap_or(0)))
        .collect()
}

#[must_use]
pub fn __ts_aot_array_from_length_mapped<T, R, F>(len: i64, mut mapfn: F) -> Vec<R>
where
    T: Default,
    F: FnMut(T, i64) -> R,
{
    validate_dense_len(len, "__ts_aot_array_from_length_mapped");
    let n = usize::try_from(len).expect("checked above");
    (0..n)
        .map(|i| mapfn(T::default(), i64::try_from(i).unwrap_or(0)))
        .collect()
}

#[must_use]
pub fn __ts_aot_array_get<T: Clone>(arr: &[T], idx: i64) -> Option<T> {
    let i = usize::try_from(idx).ok()?;
    arr.get(i).cloned()
}

#[must_use]
pub fn __ts_aot_array_get_or_default<T: Clone + Default>(arr: &[T], idx: i64) -> T {
    let Ok(i) = usize::try_from(idx) else {
        return T::default();
    };
    arr.get(i).cloned().unwrap_or_default()
}

#[must_use]
pub fn __ts_aot_array_set<T>(arr: &mut [T], idx: i64, value: T) -> bool {
    let Ok(i) = usize::try_from(idx) else {
        return false;
    };
    if let Some(slot) = arr.get_mut(i) {
        *slot = value;
        true
    } else {
        false
    }
}

#[must_use]
pub fn __ts_aot_array_len<T>(arr: &[T]) -> i64 {
    i64::try_from(arr.len()).unwrap_or(0)
}

pub struct TsArrayMarker;

pub trait IsArray {
    fn is_array(&self) -> bool;
}

impl<T> IsArray for Vec<T> {
    fn is_array(&self) -> bool {
        true
    }
}

impl IsArray for TsArrayMarker {
    fn is_array(&self) -> bool {
        true
    }
}

#[must_use]
pub fn __ts_aot_array_is_array<T: IsArray + ?Sized>(value: &T) -> bool {
    value.is_array()
}

#[must_use]
pub fn __ts_aot_array_is_array_false() -> bool {
    false
}
