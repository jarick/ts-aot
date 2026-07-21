use std::any::Any;
use std::any::TypeId;
use std::collections::HashMap;
use std::hash::BuildHasher;
use std::ops::Index;
use std::panic::panic_any;
use std::rc::Rc;

pub fn __ts_aot_host_console_log(s: &str) {
    println!("{s}");
}

pub fn __ts_aot_throw<T: Any + Send + 'static>(value: T) -> ! {
    panic_any(value)
}

#[must_use]
pub fn __ts_aot_math_sqrt(x: f64) -> f64 {
    x.sqrt()
}

#[derive(Debug, Clone)]
pub struct RegExpHandle {
    #[allow(dead_code)]
    regex: regex::Regex,
    source: String,
}

impl RegExpHandle {
    pub fn new(pattern: &str, flags: &str) -> Result<Self, regex::Error> {
        let compiled = compile_js_pattern(pattern, flags);
        let regex = regex::Regex::new(&compiled)?;
        Ok(Self {
            regex,
            source: pattern.to_owned(),
        })
    }

    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }
}

fn compile_js_pattern(pattern: &str, flags: &str) -> String {
    let mut recognized = String::new();
    for ch in flags.chars() {
        if matches!(ch, 'i' | 's' | 'm') && !recognized.contains(ch) {
            recognized.push(ch);
        }
    }
    if recognized.is_empty() {
        pattern.to_owned()
    } else {
        format!("(?{recognized}){pattern}")
    }
}

#[must_use]
pub fn __ts_aot_regex_new(pattern: &str, flags: &str) -> RegExpHandle {
    RegExpHandle::new(pattern, flags).unwrap_or_else(|_| RegExpHandle {
        regex: regex::Regex::new("^$").expect("^$ is a valid no-op regex"),
        source: pattern.to_owned(),
    })
}

#[must_use]
pub fn __ts_aot_string_concat(a: &str, b: &str) -> String {
    let mut out = String::with_capacity(a.len() + b.len());
    out.push_str(a);
    out.push_str(b);
    out
}

#[must_use]
pub fn __ts_aot_string_equals(a: &str, b: &str) -> bool {
    a == b
}

#[must_use]
pub fn __ts_aot_string_len(s: &str) -> i64 {
    i64::try_from(s.encode_utf16().count()).unwrap_or(0)
}

#[must_use]
pub fn __ts_aot_array_create<T>() -> Vec<T> {
    Vec::new()
}

#[must_use]
pub fn __ts_aot_array_get<T: Clone>(arr: &[T], idx: i64) -> Option<T> {
    let i = usize::try_from(idx).ok()?;
    arr.get(i).cloned()
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

#[must_use]
pub fn __ts_aot_map_create<S: BuildHasher + Default>() -> HashMap<String, String, S> {
    HashMap::default()
}

#[must_use]
pub fn __ts_aot_map_get<S: BuildHasher>(
    map: &HashMap<String, String, S>,
    key: &str,
) -> Option<String> {
    map.get(key).cloned()
}

pub fn __ts_aot_map_set<S: BuildHasher>(
    map: &mut HashMap<String, String, S>,
    key: String,
    value: String,
) {
    map.insert(key, value);
}

#[must_use]
pub fn __ts_aot_typeof<T: 'static>(value: &T) -> &'static str {
    if let Some(dv) = (value as &dyn std::any::Any).downcast_ref::<DynamicValue>() {
        return match dv {
            DynamicValue::Undefined => "undefined",
            DynamicValue::Null | DynamicValue::Object(_) => "object",
            DynamicValue::Bool(_) => "boolean",
            DynamicValue::Number(_) | DynamicValue::Integer(_) => "number",
            DynamicValue::String(_) => "string",
        };
    }
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
    if let Some(arr) = (object as &dyn std::any::Any).downcast_ref::<Vec<i64>>()
        && let Some(idx) = (value as &dyn std::any::Any).downcast_ref::<i64>()
    {
        let Ok(i) = usize::try_from(*idx) else {
            return false;
        };
        return i < arr.len();
    }
    if let Some(arr) = (object as &dyn std::any::Any).downcast_ref::<Vec<String>>()
        && let Some(needle) = (value as &dyn std::any::Any).downcast_ref::<String>()
    {
        return arr.iter().any(|s| s == needle);
    }
    if let Some(map) =
        (object as &dyn std::any::Any).downcast_ref::<std::collections::HashMap<String, String>>()
        && let Some(key) = (value as &dyn std::any::Any).downcast_ref::<String>()
    {
        return map.contains_key(key);
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
impl<K, V, S: BuildHasher> TsClassId for HashMap<K, V, S> {
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

#[must_use]
pub fn __ts_aot_op_instanceof<T: TsClassId + 'static>(value: &T, target_type_id: u32) -> bool {
    if let Some(dv) = (value as &dyn std::any::Any).downcast_ref::<DynamicValue>() {
        return matches!(dv, DynamicValue::Object(_)) && target_type_id == STRUCT_ID_DYNAMIC;
    }
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
    pub proto: Rc<std::cell::RefCell<Option<Box<DynamicValue>>>>,
    pub field_order: Rc<std::cell::RefCell<Vec<String>>>,
}

impl Dynamic {
    #[must_use]
    pub fn new() -> Self {
        Dynamic {
            fields: Rc::new(std::cell::RefCell::new(HashMap::new())),
            proto: Rc::new(std::cell::RefCell::new(None)),
            field_order: Rc::new(std::cell::RefCell::new(Vec::new())),
        }
    }
}

impl Default for Dynamic {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Debug)]
pub struct TemplateStringsArray {
    pub cooked: Vec<String>,
    pub raw: Vec<String>,
}

impl TemplateStringsArray {
    #[must_use]
    pub fn new(cooked: Vec<String>, raw: Vec<String>) -> Self {
        debug_assert_eq!(cooked.len(), raw.len());
        Self { cooked, raw }
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.cooked.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.cooked.is_empty()
    }
}

impl Index<usize> for TemplateStringsArray {
    type Output = str;
    fn index(&self, idx: usize) -> &str {
        &self.cooked[idx]
    }
}

impl PartialEq for DynamicValue {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (DynamicValue::Undefined, DynamicValue::Undefined)
            | (DynamicValue::Null, DynamicValue::Null) => true,
            (DynamicValue::Bool(a), DynamicValue::Bool(b)) => a == b,
            (DynamicValue::Number(a), DynamicValue::Number(b)) => a == b,
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

#[must_use]
pub fn __ts_aot_object_new() -> DynamicValue {
    DynamicValue::Object(Dynamic::new())
}

#[must_use]
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

#[must_use]
pub fn __ts_aot_dynamic_unwrap(value: Option<DynamicValue>) -> DynamicValue {
    value.unwrap_or(DynamicValue::Undefined)
}

pub fn __ts_aot_dynamic_set(target: &mut DynamicValue, field_name: &str, value: DynamicValue) {
    if !matches!(target, DynamicValue::Object(_)) {
        *target = DynamicValue::Object(Dynamic::new());
    }
    if let DynamicValue::Object(dyn_obj) = target {
        let mut fields = dyn_obj.fields.borrow_mut();
        let is_new = !fields.contains_key(field_name);
        fields.insert(field_name.to_owned(), value);
        drop(fields);
        if is_new {
            dyn_obj.field_order.borrow_mut().push(field_name.to_owned());
        }
    }
}

#[must_use]
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

#[must_use]
pub fn __ts_aot_dyn_vec_new() -> Vec<DynamicValue> {
    Vec::new()
}

pub fn __ts_aot_dyn_vec_append(vec: &mut Vec<DynamicValue>, value: DynamicValue) {
    vec.push(value);
}

#[must_use]
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
    if let DynamicValue::Object(dyn_obj) = target
        && dyn_obj.fields.borrow_mut().remove(field_name).is_some()
    {
        dyn_obj.field_order.borrow_mut().retain(|k| k != field_name);
    }
}

pub const DYNAMIC_OP_ADD: u8 = 0;
pub const DYNAMIC_OP_SUB: u8 = 1;
pub const DYNAMIC_OP_MUL: u8 = 2;
pub const DYNAMIC_OP_DIV: u8 = 3;
pub const DYNAMIC_OP_MOD: u8 = 4;

#[must_use]
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
            (Some(x), Some(y)) => Some(DynamicValue::Number(x / y)),
            _ => None,
        }
    };
    let numeric_mod = |a: &DynamicValue, b: &DynamicValue| -> Option<DynamicValue> {
        match (a, b) {
            (DynamicValue::Integer(x), DynamicValue::Integer(y)) if *y != 0 => {
                Some(DynamicValue::Integer(x.wrapping_rem(*y)))
            }
            (DynamicValue::Integer(_), DynamicValue::Integer(0)) => {
                Some(DynamicValue::Number(f64::NAN))
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

#[must_use]
pub fn __ts_aot_object_proto_get(obj: &DynamicValue) -> Option<DynamicValue> {
    if let DynamicValue::Object(d) = obj {
        d.proto.borrow().as_ref().map(|b| (**b).clone())
    } else {
        None
    }
}

pub fn __ts_aot_object_proto_set(obj: &DynamicValue, proto: Option<DynamicValue>) -> DynamicValue {
    if let DynamicValue::Object(d) = obj {
        let valid = matches!(
            &proto,
            None | Some(DynamicValue::Object(_) | DynamicValue::Null)
        );
        if valid {
            *d.proto.borrow_mut() = proto.map(Box::new);
        }
        obj.clone()
    } else {
        DynamicValue::Undefined
    }
}

#[must_use]
pub fn __ts_aot_object_set_prototype_of(obj: &DynamicValue, proto: DynamicValue) -> DynamicValue {
    let DynamicValue::Object(d) = obj else {
        __ts_aot_throw("Object.setPrototypeOf called on non-object");
    };
    if !matches!(proto, DynamicValue::Object(_) | DynamicValue::Null) {
        __ts_aot_throw("Object.setPrototypeOf: prototype must be an Object or null");
    }
    *d.proto.borrow_mut() = if matches!(proto, DynamicValue::Null) {
        None
    } else {
        Some(Box::new(proto))
    };
    obj.clone()
}

#[must_use]
pub fn __ts_aot_object_keys(obj: &DynamicValue) -> Vec<String> {
    if let DynamicValue::Object(d) = obj {
        d.field_order.borrow().clone()
    } else {
        Vec::new()
    }
}

#[cfg(test)]
mod tests;
