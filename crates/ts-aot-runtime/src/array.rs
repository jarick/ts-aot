use bumpalo::collections::Vec as BumpVec;
use ts_aot_core::MAX_DENSE_ARRAY_LEN;

use crate::arena::Ctx;
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
pub fn __ts_aot_array_create<'a, T>(_ctx: &Ctx<'a>, elements: BumpVec<'a, T>) -> BumpVec<'a, T> {
    elements
}

#[must_use]
pub fn __ts_aot_array_create_with_len<'a, T: Default + Clone>(
    ctx: &Ctx<'a>,
    len: i64,
) -> BumpVec<'a, T> {
    validate_dense_len(len, "__ts_aot_array_create_with_len");
    let n = usize::try_from(len).expect("checked above");
    let mut out = BumpVec::with_capacity_in(n, ctx.arena);
    out.resize(n, T::default());
    out
}

#[must_use]
pub fn __ts_aot_array_concat<'a, T: Clone>(
    ctx: &Ctx<'a>,
    parts: BumpVec<'a, BumpVec<'a, T>>,
) -> BumpVec<'a, T> {
    let total: usize = parts.iter().map(BumpVec::len).sum();
    let cap = usize::try_from(MAX_DENSE_ARRAY_LEN).unwrap_or(usize::MAX);
    if total >= cap {
        __ts_aot_throw(format!(
            "RangeError: __ts_aot_array_concat allocation failed (out of memory) for {total} elements"
        ));
    }
    let mut out = BumpVec::with_capacity_in(total, ctx.arena);
    for part in parts {
        out.extend_from_slice(&part);
    }
    out
}

#[must_use]
pub fn __ts_aot_array_hole<'a, T: Default>(ctx: &Ctx<'a>) -> BumpVec<'a, T> {
    let mut out = BumpVec::with_capacity_in(1, ctx.arena);
    out.push(T::default());
    out
}

pub fn __ts_aot_array_push<T>(arr: &mut BumpVec<'_, T>, item: T) -> i64 {
    arr.push(item);
    i64::try_from(arr.len()).unwrap_or(0)
}

#[must_use]
pub fn __ts_aot_array_from<'a, T: Clone>(ctx: &Ctx<'a>, arr: &[T]) -> BumpVec<'a, T> {
    BumpVec::from_iter_in(arr.iter().cloned(), ctx.arena)
}

#[must_use]
pub fn __ts_aot_array_from_string<'a>(
    ctx: &Ctx<'a>,
    s: &JsString<'_>,
) -> BumpVec<'a, JsString<'a>> {
    BumpVec::from_iter_in(
        char::decode_utf16(s.units_iter()).map(|result| match result {
            Ok(c) => JsString::Valid(ctx.arena.alloc_str(&c.to_string())),
            Err(e) => JsString::Raw(ctx.arena.alloc_slice_copy(&[e.unpaired_surrogate()])),
        }),
        ctx.arena,
    )
}

#[must_use]
pub fn __ts_aot_array_from_mapped<'a, T, R, F>(
    ctx: &Ctx<'a>,
    arr: &[T],
    mut mapfn: F,
) -> BumpVec<'a, R>
where
    T: Clone,
    F: FnMut(T, i64) -> R,
{
    BumpVec::from_iter_in(
        arr.iter()
            .enumerate()
            .map(|(i, x)| mapfn(x.clone(), i64::try_from(i).unwrap_or(0))),
        ctx.arena,
    )
}

#[must_use]
pub fn __ts_aot_array_from_length_mapped<'a, T, R, F>(
    ctx: &Ctx<'a>,
    len: i64,
    mut mapfn: F,
) -> BumpVec<'a, R>
where
    T: Default,
    F: FnMut(T, i64) -> R,
{
    validate_dense_len(len, "__ts_aot_array_from_length_mapped");
    let n = usize::try_from(len).expect("checked above");
    BumpVec::from_iter_in(
        (0..n).map(|i| mapfn(T::default(), i64::try_from(i).unwrap_or(0))),
        ctx.arena,
    )
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
pub fn __ts_aot_array_len<T>(arr: &[T]) -> f64 {
    f64::from(u32::try_from(arr.len()).unwrap_or(u32::MAX))
}

#[must_use]
pub fn __ts_aot_array_own_keys<'a, T>(
    ctx: &Ctx<'a>,
    arr: &[T],
    include_length: bool,
) -> BumpVec<'a, JsString<'a>> {
    let mut keys: BumpVec<'a, JsString<'a>> = BumpVec::from_iter_in(
        (0..arr.len()).map(|i| JsString::Valid(ctx.arena.alloc_str(&i.to_string()))),
        ctx.arena,
    );
    if include_length {
        keys.push(JsString::Valid("length"));
    }
    keys
}

pub struct TsArrayMarker;

pub trait IsArray {
    fn is_array(&self) -> bool;
}

impl<T> IsArray for BumpVec<'_, T> {
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
