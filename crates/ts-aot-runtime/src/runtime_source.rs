#![allow(dead_code, clippy::missing_const_for_fn, clippy::needless_pass_by_value)]

use std::collections::HashMap;

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

pub fn __ts_aot_call_indirect(
    callee: &str,
    args: &[u64],
    table: &[(&str, fn(&[u64]) -> u64)],
) -> u64 {
    for (name, f) in table {
        if *name == callee {
            return f(args);
        }
    }
    panic!("__ts_aot_call_indirect: unknown callee {callee:?}")
}
