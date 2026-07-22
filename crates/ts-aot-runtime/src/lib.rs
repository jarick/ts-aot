use std::any::Any;
use std::any::TypeId;
use std::cell::RefCell;
use std::collections::HashMap;
use std::hash::BuildHasher;
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

#[derive(Debug, Clone)]
pub struct BigIntHandle {
    value: String,
}

impl BigIntHandle {
    #[must_use]
    pub fn new(value: &str) -> Self {
        Self {
            value: value.to_owned(),
        }
    }

    #[must_use]
    pub fn value(&self) -> &str {
        &self.value
    }
}

#[must_use]
pub fn __ts_aot_bigint_new(value: &str) -> BigIntHandle {
    BigIntHandle::new(value)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PromiseState {
    Pending,
    Fulfilled,
    Rejected,
}

pub struct Promise<T> {
    inner: Rc<RefCell<PromiseInner<T>>>,
}

struct PromiseInner<T> {
    state: PromiseState,
    value: Option<T>,
    error: Option<String>,
    callbacks: Vec<PromiseCallback<T>>,
}

type PromiseCallback<T> = Box<dyn FnOnce(Result<T, String>)>;

#[must_use]
pub fn __ts_aot_promise_create<T>() -> Promise<T> {
    Promise {
        inner: Rc::new(RefCell::new(PromiseInner {
            state: PromiseState::Pending,
            value: None,
            error: None,
            callbacks: Vec::new(),
        })),
    }
}

pub fn __ts_aot_promise_resolve<T: Clone + 'static>(promise: &Promise<T>, value: T) {
    let to_fire: Vec<PromiseCallback<T>> = {
        let mut inner = promise.inner.borrow_mut();
        if inner.state != PromiseState::Pending {
            return;
        }
        inner.state = PromiseState::Fulfilled;
        inner.value = Some(value.clone());
        std::mem::take(&mut inner.callbacks)
    };
    for cb in to_fire {
        cb(Ok(value.clone()));
    }
}

#[allow(clippy::needless_pass_by_value)]
pub fn __ts_aot_promise_reject<T: 'static>(promise: &Promise<T>, reason: String) {
    let to_fire: Vec<PromiseCallback<T>> = {
        let mut inner = promise.inner.borrow_mut();
        if inner.state != PromiseState::Pending {
            return;
        }
        inner.state = PromiseState::Rejected;
        inner.error = Some(reason.clone());
        std::mem::take(&mut inner.callbacks)
    };
    for cb in to_fire {
        cb(Err(reason.clone()));
    }
}

pub fn __ts_aot_promise_then<T: Clone + 'static>(
    promise: &Promise<T>,
    callback: Box<dyn FnOnce(Result<T, String>)>,
) {
    let inner = promise.inner.borrow();
    if matches!(
        inner.state,
        PromiseState::Fulfilled | PromiseState::Rejected
    ) {
        let result = if inner.state == PromiseState::Fulfilled {
            Ok(inner
                .value
                .clone()
                .expect("fulfilled promise must have a value"))
        } else {
            Err(inner
                .error
                .clone()
                .expect("rejected promise must have an error"))
        };
        drop(inner);
        callback(result);
    } else {
        drop(inner);
        promise.inner.borrow_mut().callbacks.push(callback);
    }
}

#[must_use]
pub fn __ts_aot_await<T: Clone>(promise: &Promise<T>) -> T {
    let inner = promise.inner.borrow();
    match &inner.state {
        PromiseState::Fulfilled => inner
            .value
            .clone()
            .expect("fulfilled promise must have a value"),
        PromiseState::Rejected => {
            panic!(
                "await on a rejected promise: {}",
                inner.error.as_deref().unwrap_or("unknown error")
            );
        }
        PromiseState::Pending => {
            panic!("__ts_aot_await only works on settled promises; this one is pending")
        }
    }
}

pub trait ModuleNamespace: Sized + Clone + 'static {}
impl<T: Sized + Clone + 'static> ModuleNamespace for T {}

type AnyNamespace = Box<dyn Any>;

fn with_module_registry<R>(f: impl FnOnce(&mut HashMap<String, AnyNamespace>) -> R) -> R {
    thread_local! {
        static REGISTRY: RefCell<HashMap<String, AnyNamespace>> = RefCell::new(HashMap::new());
    }
    REGISTRY.with(|cell| f(&mut cell.borrow_mut()))
}

pub fn __ts_aot_module_register<T: ModuleNamespace>(specifier: &str, namespace: T) {
    with_module_registry(|reg| {
        reg.insert(specifier.to_owned(), Box::new(namespace));
    });
}

#[must_use]
pub fn __ts_aot_dynamic_import<T: ModuleNamespace>(specifier: &str) -> Promise<T> {
    let promise = __ts_aot_promise_create();
    let result: Result<T, String> = with_module_registry(|reg| {
        let Some(boxed) = reg.get(specifier) else {
            return Err(format!("module '{specifier}' is not registered"));
        };
        boxed.downcast_ref::<T>().cloned().ok_or_else(|| {
            format!("module '{specifier}' is registered but the requested type does not match the registered namespace")
        })
    });
    match result {
        Ok(value) => __ts_aot_promise_resolve(&promise, value),
        Err(reason) => __ts_aot_promise_reject(&promise, reason),
    }
    promise
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
    let _ = value;
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
    let _ = value;
    T::class_id() == target_type_id
}
