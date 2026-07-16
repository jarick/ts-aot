#![allow(
    dead_code,
    unused_variables,
    clippy::missing_const_for_fn,
    clippy::needless_pass_by_value,
    clippy::must_use_candidate,
    clippy::cast_possible_wrap,
    clippy::ptr_arg,
    clippy::implicit_hasher,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc
)]

use std::collections::HashMap;
use std::rc::Rc;

pub fn __ts_aot_host_console_log(s: &str) {
    println!("{s}");
}

pub fn __ts_aot_math_sqrt(x: f64) -> f64 {
    x.sqrt()
}

pub fn __ts_aot_string_concat(a: &str, b: &str) -> String {
    let mut out = String::with_capacity(a.len() + b.len());
    out.push_str(a);
    out.push_str(b);
    out
}

pub fn __ts_aot_string_equals(a: &str, b: &str) -> bool {
    a == b
}

pub fn __ts_aot_string_len(s: &str) -> i64 {
    s.len() as i64
}

pub fn __ts_aot_array_create<T>() -> Vec<T> {
    Vec::new()
}

pub fn __ts_aot_array_get<T: Clone>(arr: &[T], idx: i64) -> Option<T> {
    let i = usize::try_from(idx).ok()?;
    arr.get(i).cloned()
}

pub fn __ts_aot_array_set<T>(arr: &mut Vec<T>, idx: i64, value: T) -> bool {
    let Ok(i) = usize::try_from(idx) else {
        return false;
    };
    if i < arr.len() {
        arr[i] = value;
        true
    } else {
        false
    }
}

pub fn __ts_aot_array_len<T>(arr: &[T]) -> i64 {
    arr.len() as i64
}

pub fn __ts_aot_map_create() -> HashMap<String, String> {
    HashMap::new()
}

pub fn __ts_aot_map_get(map: &HashMap<String, String>, key: &str) -> Option<String> {
    map.get(key).cloned()
}

pub fn __ts_aot_map_set(map: &mut HashMap<String, String>, key: String, value: String) {
    map.insert(key, value);
}

pub fn __ts_aot_typeof<T: 'static>(value: &T) -> &'static str {
    use std::any::TypeId;
    let id = TypeId::of::<T>();
    if id == TypeId::of::<i64>()
        || id == TypeId::of::<i32>()
        || id == TypeId::of::<i128>()
        || id == TypeId::of::<u32>()
        || id == TypeId::of::<u64>()
        || id == TypeId::of::<f32>()
        || id == TypeId::of::<f64>()
    {
        "number"
    } else if id == TypeId::of::<bool>() {
        "boolean"
    } else if id == TypeId::of::<String>() || id == TypeId::of::<&str>() {
        "string"
    } else {
        "object"
    }
}

pub fn __ts_aot_typeof_unit() -> &'static str {
    "undefined"
}

pub fn __ts_aot_typeof_null() -> &'static str {
    "object"
}

pub fn __ts_aot_op_delete<T>(_target: &T) -> bool {
    true
}

pub fn __ts_aot_op_in<L: 'static, R: 'static>(value: &L, object: &R) -> bool {
    if let Some(arr) = (object as &dyn std::any::Any).downcast_ref::<Vec<i64>>() {
        if let Some(idx) = (value as &dyn std::any::Any).downcast_ref::<i64>() {
            let Ok(i) = usize::try_from(*idx) else {
                return false;
            };
            return i < arr.len();
        }
        return false;
    }
    if let Some(arr) = (object as &dyn std::any::Any).downcast_ref::<Vec<String>>() {
        if let Some(needle) = (value as &dyn std::any::Any).downcast_ref::<String>() {
            return arr.iter().any(|s| s == needle);
        }
        return false;
    }
    if let Some(map) =
        (object as &dyn std::any::Any).downcast_ref::<std::collections::HashMap<String, i64>>()
    {
        if let Some(key) = (value as &dyn std::any::Any).downcast_ref::<String>() {
            return map.contains_key(key);
        }
        return false;
    }
    false
}

pub trait TsClassId {
    fn class_id() -> u32;
}

const PRIMITIVE_CLASS_ID_BASE: u32 = 0xFFFF_FF00;
pub const STRUCT_ID_DYNAMIC: u32 = 0xFFFF_FFFE;

impl TsClassId for i8 {
    fn class_id() -> u32 {
        PRIMITIVE_CLASS_ID_BASE
    }
}
impl TsClassId for i16 {
    fn class_id() -> u32 {
        PRIMITIVE_CLASS_ID_BASE + 1
    }
}
impl TsClassId for i32 {
    fn class_id() -> u32 {
        PRIMITIVE_CLASS_ID_BASE + 2
    }
}
impl TsClassId for i64 {
    fn class_id() -> u32 {
        PRIMITIVE_CLASS_ID_BASE + 3
    }
}
impl TsClassId for i128 {
    fn class_id() -> u32 {
        PRIMITIVE_CLASS_ID_BASE + 4
    }
}
impl TsClassId for u8 {
    fn class_id() -> u32 {
        PRIMITIVE_CLASS_ID_BASE + 5
    }
}
impl TsClassId for u16 {
    fn class_id() -> u32 {
        PRIMITIVE_CLASS_ID_BASE + 6
    }
}
impl TsClassId for u32 {
    fn class_id() -> u32 {
        PRIMITIVE_CLASS_ID_BASE + 7
    }
}
impl TsClassId for u64 {
    fn class_id() -> u32 {
        PRIMITIVE_CLASS_ID_BASE + 8
    }
}
impl TsClassId for u128 {
    fn class_id() -> u32 {
        PRIMITIVE_CLASS_ID_BASE + 9
    }
}
impl TsClassId for f32 {
    fn class_id() -> u32 {
        PRIMITIVE_CLASS_ID_BASE + 10
    }
}
impl TsClassId for f64 {
    fn class_id() -> u32 {
        PRIMITIVE_CLASS_ID_BASE + 11
    }
}
impl TsClassId for bool {
    fn class_id() -> u32 {
        PRIMITIVE_CLASS_ID_BASE + 12
    }
}
impl TsClassId for char {
    fn class_id() -> u32 {
        PRIMITIVE_CLASS_ID_BASE + 13
    }
}
impl TsClassId for String {
    fn class_id() -> u32 {
        PRIMITIVE_CLASS_ID_BASE + 14
    }
}
impl TsClassId for &str {
    fn class_id() -> u32 {
        PRIMITIVE_CLASS_ID_BASE + 15
    }
}
impl TsClassId for () {
    fn class_id() -> u32 {
        PRIMITIVE_CLASS_ID_BASE + 16
    }
}
impl<T> TsClassId for Vec<T> {
    fn class_id() -> u32 {
        PRIMITIVE_CLASS_ID_BASE + 17
    }
}
impl<K, V> TsClassId for HashMap<K, V> {
    fn class_id() -> u32 {
        PRIMITIVE_CLASS_ID_BASE + 18
    }
}
impl TsClassId for Dynamic {
    fn class_id() -> u32 {
        STRUCT_ID_DYNAMIC
    }
}
impl TsClassId for DynamicValue {
    fn class_id() -> u32 {
        STRUCT_ID_DYNAMIC
    }
}
impl<T> TsClassId for Option<T> {
    fn class_id() -> u32 {
        PRIMITIVE_CLASS_ID_BASE + 19
    }
}
impl<T, E> TsClassId for Result<T, E> {
    fn class_id() -> u32 {
        PRIMITIVE_CLASS_ID_BASE + 20
    }
}
impl<T> TsClassId for Box<T> {
    fn class_id() -> u32 {
        PRIMITIVE_CLASS_ID_BASE + 21
    }
}
impl<T> TsClassId for Rc<T> {
    fn class_id() -> u32 {
        PRIMITIVE_CLASS_ID_BASE + 22
    }
}

impl<T> TsClassId for (T,) {
    fn class_id() -> u32 {
        PRIMITIVE_CLASS_ID_BASE + 23
    }
}
impl<T1, T2> TsClassId for (T1, T2) {
    fn class_id() -> u32 {
        PRIMITIVE_CLASS_ID_BASE + 24
    }
}
impl<T1, T2, T3> TsClassId for (T1, T2, T3) {
    fn class_id() -> u32 {
        PRIMITIVE_CLASS_ID_BASE + 25
    }
}
impl<T1, T2, T3, T4> TsClassId for (T1, T2, T3, T4) {
    fn class_id() -> u32 {
        PRIMITIVE_CLASS_ID_BASE + 26
    }
}
impl<T1, T2, T3, T4, T5> TsClassId for (T1, T2, T3, T4, T5) {
    fn class_id() -> u32 {
        PRIMITIVE_CLASS_ID_BASE + 27
    }
}
impl<T1, T2, T3, T4, T5, T6> TsClassId for (T1, T2, T3, T4, T5, T6) {
    fn class_id() -> u32 {
        PRIMITIVE_CLASS_ID_BASE + 28
    }
}
impl<T1, T2, T3, T4, T5, T6, T7> TsClassId for (T1, T2, T3, T4, T5, T6, T7) {
    fn class_id() -> u32 {
        PRIMITIVE_CLASS_ID_BASE + 29
    }
}
impl<T1, T2, T3, T4, T5, T6, T7, T8> TsClassId for (T1, T2, T3, T4, T5, T6, T7, T8) {
    fn class_id() -> u32 {
        PRIMITIVE_CLASS_ID_BASE + 30
    }
}

pub fn __ts_aot_op_instanceof<T: TsClassId>(_value: &T, target_type_id: u32) -> bool {
    T::class_id() == target_type_id
}

#[derive(Clone, Debug)]
pub enum DynamicValue {
    Undefined,
    Null,
    Bool(bool),
    Number(f64),
    Integer(i64),
    String(String),
    Object(Dynamic),
}

#[derive(Clone, Debug)]
pub struct Dynamic {
    pub fields: Rc<std::cell::RefCell<HashMap<String, DynamicValue>>>,
}

impl Dynamic {
    #[must_use]
    pub fn new() -> Self {
        Dynamic {
            fields: Rc::new(std::cell::RefCell::new(HashMap::new())),
        }
    }
}

impl Default for Dynamic {
    fn default() -> Self {
        Self::new()
    }
}

impl PartialEq for DynamicValue {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (DynamicValue::Undefined, DynamicValue::Undefined)
            | (DynamicValue::Null, DynamicValue::Null) => true,
            (DynamicValue::Bool(a), DynamicValue::Bool(b)) => a == b,
            (DynamicValue::Number(a), DynamicValue::Number(b)) => {
                a == b || (a.is_nan() && b.is_nan())
            }
            (DynamicValue::Integer(a), DynamicValue::Integer(b)) => a == b,
            (DynamicValue::String(a), DynamicValue::String(b)) => a == b,
            (DynamicValue::Object(a), DynamicValue::Object(b)) => Rc::ptr_eq(&a.fields, &b.fields),
            _ => false,
        }
    }
}

impl PartialEq for Dynamic {
    fn eq(&self, other: &Self) -> bool {
        *self.fields.borrow() == *other.fields.borrow()
    }
}

pub fn __ts_aot_dynamic_get(value: &DynamicValue, field_name: &str) -> DynamicValue {
    match value {
        DynamicValue::Object(dyn_obj) => dyn_obj
            .fields
            .borrow()
            .get(field_name)
            .cloned()
            .unwrap_or(DynamicValue::Undefined),
        _ => DynamicValue::Undefined,
    }
}

pub fn __ts_aot_dynamic_unwrap(value: Option<DynamicValue>) -> DynamicValue {
    value.unwrap_or(DynamicValue::Undefined)
}

pub fn __ts_aot_dynamic_set(target: &mut DynamicValue, field_name: &str, value: DynamicValue) {
    if !matches!(target, DynamicValue::Object(_)) {
        *target = DynamicValue::Object(Dynamic::new());
    }
    if let DynamicValue::Object(dyn_obj) = target {
        dyn_obj
            .fields
            .borrow_mut()
            .insert(field_name.to_owned(), value);
    }
}

pub fn __ts_aot_dynamic_key(s: &str) -> DynamicValue {
    DynamicValue::String(s.to_owned())
}

impl From<i64> for DynamicValue {
    fn from(v: i64) -> Self {
        DynamicValue::Integer(v)
    }
}

impl From<f64> for DynamicValue {
    fn from(v: f64) -> Self {
        DynamicValue::Number(v)
    }
}

impl From<bool> for DynamicValue {
    fn from(v: bool) -> Self {
        DynamicValue::Bool(v)
    }
}

impl From<String> for DynamicValue {
    fn from(v: String) -> Self {
        DynamicValue::String(v)
    }
}

impl From<&str> for DynamicValue {
    fn from(v: &str) -> Self {
        DynamicValue::String(v.to_owned())
    }
}

pub fn __ts_aot_dynamic_has(value: &DynamicValue, key: &DynamicValue) -> bool {
    let DynamicValue::String(field_name) = key else {
        return false;
    };
    match value {
        DynamicValue::Object(dyn_obj) => dyn_obj.fields.borrow().contains_key(field_name),
        _ => false,
    }
}

pub fn __ts_aot_dynamic_delete(target: &mut DynamicValue, field_name: &str) {
    if let DynamicValue::Object(dyn_obj) = target {
        dyn_obj.fields.borrow_mut().remove(field_name);
    }
}

pub const DYNAMIC_OP_ADD: u8 = 0;
pub const DYNAMIC_OP_SUB: u8 = 1;
pub const DYNAMIC_OP_MUL: u8 = 2;
pub const DYNAMIC_OP_DIV: u8 = 3;
pub const DYNAMIC_OP_MOD: u8 = 4;

#[allow(clippy::cast_precision_loss)]
pub fn __ts_aot_dynamic_op(op: u8, left: &DynamicValue, right: &DynamicValue) -> DynamicValue {
    let numeric = |a: &DynamicValue, b: &DynamicValue| -> Option<DynamicValue> {
        match (a, b) {
            (DynamicValue::Integer(x), DynamicValue::Integer(y)) => {
                Some(DynamicValue::Integer(x.wrapping_add(*y)))
            }
            (DynamicValue::Number(x), DynamicValue::Number(y)) => Some(DynamicValue::Number(x + y)),
            (DynamicValue::Integer(x), DynamicValue::Number(y)) => {
                Some(DynamicValue::Number(*x as f64 + y))
            }
            (DynamicValue::Number(x), DynamicValue::Integer(y)) => {
                Some(DynamicValue::Number(x + *y as f64))
            }
            _ => None,
        }
    };
    let numeric_sub = |a: &DynamicValue, b: &DynamicValue| -> Option<DynamicValue> {
        match (a, b) {
            (DynamicValue::Integer(x), DynamicValue::Integer(y)) => {
                Some(DynamicValue::Integer(x.wrapping_sub(*y)))
            }
            (DynamicValue::Number(x), DynamicValue::Number(y)) => Some(DynamicValue::Number(x - y)),
            (DynamicValue::Integer(x), DynamicValue::Number(y)) => {
                Some(DynamicValue::Number(*x as f64 - y))
            }
            (DynamicValue::Number(x), DynamicValue::Integer(y)) => {
                Some(DynamicValue::Number(x - *y as f64))
            }
            _ => None,
        }
    };
    let numeric_mul = |a: &DynamicValue, b: &DynamicValue| -> Option<DynamicValue> {
        match (a, b) {
            (DynamicValue::Integer(x), DynamicValue::Integer(y)) => {
                Some(DynamicValue::Integer(x.wrapping_mul(*y)))
            }
            (DynamicValue::Number(x), DynamicValue::Number(y)) => Some(DynamicValue::Number(x * y)),
            (DynamicValue::Integer(x), DynamicValue::Number(y)) => {
                Some(DynamicValue::Number(*x as f64 * y))
            }
            (DynamicValue::Number(x), DynamicValue::Integer(y)) => {
                Some(DynamicValue::Number(x * *y as f64))
            }
            _ => None,
        }
    };
    let numeric_div = |a: &DynamicValue, b: &DynamicValue| -> Option<DynamicValue> {
        let to_f64 = |v: &DynamicValue| -> Option<f64> {
            match v {
                DynamicValue::Integer(x) => Some(*x as f64),
                DynamicValue::Number(x) => Some(*x),
                _ => None,
            }
        };
        match (to_f64(a), to_f64(b)) {
            (Some(x), Some(y)) if y != 0.0 => Some(DynamicValue::Number(x / y)),
            (Some(x), Some(0.0)) => Some(DynamicValue::Number(x / 0.0)),
            _ => None,
        }
    };
    let numeric_mod = |a: &DynamicValue, b: &DynamicValue| -> Option<DynamicValue> {
        match (a, b) {
            (DynamicValue::Integer(x), DynamicValue::Integer(y)) if *y != 0 => {
                Some(DynamicValue::Integer(x.wrapping_rem(*y)))
            }
            (DynamicValue::Integer(x), DynamicValue::Number(y)) => {
                Some(DynamicValue::Number(*x as f64 % *y))
            }
            (DynamicValue::Number(x), DynamicValue::Integer(y)) => {
                Some(DynamicValue::Number(*x % *y as f64))
            }
            (DynamicValue::Number(x), DynamicValue::Number(y)) => {
                Some(DynamicValue::Number(*x % *y))
            }
            _ => None,
        }
    };
    match op {
        DYNAMIC_OP_ADD => {
            if let (DynamicValue::String(a), DynamicValue::String(b)) = (left, right) {
                let mut s = a.clone();
                s.push_str(b);
                return DynamicValue::String(s);
            }
            numeric(left, right).unwrap_or(DynamicValue::Undefined)
        }
        DYNAMIC_OP_SUB => numeric_sub(left, right).unwrap_or(DynamicValue::Undefined),
        DYNAMIC_OP_MUL => numeric_mul(left, right).unwrap_or(DynamicValue::Undefined),
        DYNAMIC_OP_DIV => numeric_div(left, right).unwrap_or(DynamicValue::Undefined),
        DYNAMIC_OP_MOD => numeric_mod(left, right).unwrap_or(DynamicValue::Undefined),
        _ => DynamicValue::Undefined,
    }
}
