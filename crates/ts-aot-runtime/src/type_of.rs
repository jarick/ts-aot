use std::any::TypeId;
use std::rc::Rc;

use indexmap::IndexMap;
use ts_aot_core::canonical_integer_index;

use crate::host::__ts_aot_throw;
use crate::string::JsString;

#[must_use]
pub fn __ts_aot_typeof<T: 'static>(value: &T) -> &'static str {
    let _ = value;
    let id = TypeId::of::<T>();
    if id == TypeId::of::<i8>()
        || id == TypeId::of::<i16>()
        || id == TypeId::of::<i32>()
        || id == TypeId::of::<i64>()
        || id == TypeId::of::<i128>()
        || id == TypeId::of::<u8>()
        || id == TypeId::of::<u16>()
        || id == TypeId::of::<u32>()
        || id == TypeId::of::<u64>()
        || id == TypeId::of::<u128>()
        || id == TypeId::of::<usize>()
        || id == TypeId::of::<isize>()
        || id == TypeId::of::<f32>()
        || id == TypeId::of::<f64>()
    {
        "number"
    } else if id == TypeId::of::<bool>() {
        "boolean"
    } else if id == TypeId::of::<JsString>() {
        "string"
    } else {
        "object"
    }
}

#[must_use]
pub fn __ts_aot_typeof_unit() -> &'static str {
    "undefined"
}

#[must_use]
pub fn __ts_aot_typeof_null() -> &'static str {
    "object"
}

#[must_use]
pub fn __ts_aot_op_in<L: 'static, R: 'static>(value: &L, object: &R) -> bool {
    let any_value = value as &dyn std::any::Any;
    if let Some(arr) = (object as &dyn std::any::Any).downcast_ref::<Vec<i64>>() {
        let Some(idx) = any_value.downcast_ref::<i64>() else {
            __ts_aot_throw(
                "__ts_aot_op_in: Vec<i64> requires i64 key, got unsupported key type".to_owned(),
            );
        };
        let Ok(i) = usize::try_from(*idx) else {
            return false;
        };
        return i < arr.len();
    }
    if let Some(arr) = (object as &dyn std::any::Any).downcast_ref::<Vec<JsString>>() {
        let Some(needle) = any_value.downcast_ref::<JsString>() else {
            __ts_aot_throw(
                "__ts_aot_op_in: Vec<JsString> requires JsString key, got unsupported key type"
                    .to_owned(),
            );
        };
        let Some(idx) = canonical_integer_index(&needle.to_string_lossy()) else {
            return false;
        };
        return usize::try_from(idx).is_ok_and(|i| i < arr.len());
    }
    if let Some(map) = (object as &dyn std::any::Any).downcast_ref::<IndexMap<JsString, JsString>>()
    {
        if let Some(key) = any_value.downcast_ref::<JsString>() {
            return map.contains_key(key);
        }
        if let Some(key) = any_value.downcast_ref::<String>() {
            return map.contains_key(&JsString::from(key.as_str()));
        }
        __ts_aot_throw(
            "__ts_aot_op_in: IndexMap<JsString, JsString> requires JsString or String key, \
             got unsupported key type"
                .to_owned(),
        );
    }
    __ts_aot_throw(
        "__ts_aot_op_in: unsupported container type (codegen must emit a recognized type: \
         Vec<i64>, Vec<JsString>, or IndexMap<JsString, JsString>)"
            .to_owned(),
    );
}

pub trait TsClassId {
    fn class_id() -> u32;
}

const PRIMITIVE_CLASS_ID_BASE: u32 = 0xFFFF_FF00;

macro_rules! impl_ts_class_id {
    ($t:ty => $id:expr) => {
        impl TsClassId for $t {
            fn class_id() -> u32 {
                $id
            }
        }
    };
    ($t:ty, $($g:ident),+ => $id:expr) => {
        impl<$($g),*> TsClassId for $t {
            fn class_id() -> u32 {
                $id
            }
        }
    };
}

impl_ts_class_id!(i8 => PRIMITIVE_CLASS_ID_BASE);
impl_ts_class_id!(i16 => PRIMITIVE_CLASS_ID_BASE + 1);
impl_ts_class_id!(i32 => PRIMITIVE_CLASS_ID_BASE + 2);
impl_ts_class_id!(i64 => PRIMITIVE_CLASS_ID_BASE + 3);
impl_ts_class_id!(i128 => PRIMITIVE_CLASS_ID_BASE + 4);
impl_ts_class_id!(u8 => PRIMITIVE_CLASS_ID_BASE + 5);
impl_ts_class_id!(u16 => PRIMITIVE_CLASS_ID_BASE + 6);
impl_ts_class_id!(u32 => PRIMITIVE_CLASS_ID_BASE + 7);
impl_ts_class_id!(u64 => PRIMITIVE_CLASS_ID_BASE + 8);
impl_ts_class_id!(u128 => PRIMITIVE_CLASS_ID_BASE + 9);
impl_ts_class_id!(f32 => PRIMITIVE_CLASS_ID_BASE + 10);
impl_ts_class_id!(f64 => PRIMITIVE_CLASS_ID_BASE + 11);
impl_ts_class_id!(bool => PRIMITIVE_CLASS_ID_BASE + 12);
impl_ts_class_id!(char => PRIMITIVE_CLASS_ID_BASE + 13);
impl_ts_class_id!(String => PRIMITIVE_CLASS_ID_BASE + 14);
impl_ts_class_id!(&str => PRIMITIVE_CLASS_ID_BASE + 15);
impl_ts_class_id!(() => PRIMITIVE_CLASS_ID_BASE + 16);
impl_ts_class_id!(Vec<T>, T => PRIMITIVE_CLASS_ID_BASE + 17);
impl_ts_class_id!(IndexMap<K, V>, K, V => PRIMITIVE_CLASS_ID_BASE + 18);
impl_ts_class_id!(Option<T>, T => PRIMITIVE_CLASS_ID_BASE + 19);
impl_ts_class_id!(Result<T, E>, T, E => PRIMITIVE_CLASS_ID_BASE + 20);
impl<T: TsClassId> TsClassId for Box<T> {
    fn class_id() -> u32 {
        T::class_id()
    }
}
impl<T: TsClassId> TsClassId for Rc<T> {
    fn class_id() -> u32 {
        T::class_id()
    }
}
impl_ts_class_id!((T,), T => PRIMITIVE_CLASS_ID_BASE + 23);
impl_ts_class_id!((T1, T2), T1, T2 => PRIMITIVE_CLASS_ID_BASE + 24);
impl_ts_class_id!((T1, T2, T3), T1, T2, T3 => PRIMITIVE_CLASS_ID_BASE + 25);
impl_ts_class_id!((T1, T2, T3, T4), T1, T2, T3, T4 => PRIMITIVE_CLASS_ID_BASE + 26);
impl_ts_class_id!((T1, T2, T3, T4, T5), T1, T2, T3, T4, T5 => PRIMITIVE_CLASS_ID_BASE + 27);
impl_ts_class_id!((T1, T2, T3, T4, T5, T6), T1, T2, T3, T4, T5, T6 => PRIMITIVE_CLASS_ID_BASE + 28);
impl_ts_class_id!((T1, T2, T3, T4, T5, T6, T7), T1, T2, T3, T4, T5, T6, T7 => PRIMITIVE_CLASS_ID_BASE + 29);
impl_ts_class_id!((T1, T2, T3, T4, T5, T6, T7, T8), T1, T2, T3, T4, T5, T6, T7, T8 => PRIMITIVE_CLASS_ID_BASE + 30);

#[must_use]
pub fn __ts_aot_op_instanceof<T: TsClassId + 'static>(value: &T, target_type_id: u32) -> bool {
    let _ = value;
    T::class_id() == target_type_id
}
