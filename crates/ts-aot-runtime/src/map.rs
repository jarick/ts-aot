use std::hash::BuildHasher;

use indexmap::IndexMap;
use ts_aot_core::canonical_integer_index;

use crate::string::JsString;

#[must_use]
pub fn __ts_aot_map_create<S: BuildHasher + Default>() -> IndexMap<JsString, JsString, S> {
    IndexMap::default()
}

#[must_use]
pub fn __ts_aot_map_get<S: BuildHasher>(
    map: &IndexMap<JsString, JsString, S>,
    key: &JsString,
) -> Option<JsString> {
    map.get(key).cloned()
}

pub fn __ts_aot_map_set<S: BuildHasher>(
    map: &mut IndexMap<JsString, JsString, S>,
    key: JsString,
    value: JsString,
) {
    map.insert(key, value);
}

#[must_use]
pub fn __ts_aot_object_keys<S: BuildHasher>(
    map: &IndexMap<JsString, JsString, S>,
) -> Vec<JsString> {
    let mut int_indices: Vec<(u64, JsString)> = Vec::new();
    let mut string_keys: Vec<JsString> = Vec::new();
    for key in map.keys() {
        let as_string: String = match key {
            JsString::Valid(s) => s.clone(),
            JsString::Raw(units) => String::from_utf16_lossy(units),
        };
        if let Some(n) = canonical_integer_index(&as_string) {
            int_indices.push((n, key.clone()));
        } else {
            string_keys.push(key.clone());
        }
    }
    int_indices.sort_by_key(|(n, _)| *n);
    int_indices
        .into_iter()
        .map(|(_, s)| s)
        .chain(string_keys)
        .collect()
}
