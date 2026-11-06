use std::hash::BuildHasher;

use bumpalo::collections::Vec as BumpVec;
use indexmap::IndexMap;
use ts_aot_core::canonical_integer_index;

use crate::arena::Ctx;
use crate::string::JsString;

#[must_use]
pub fn __ts_aot_map_create<'a, S: BuildHasher + Default>() -> IndexMap<JsString<'a>, JsString<'a>, S>
{
    IndexMap::default()
}

#[must_use]
pub fn __ts_aot_map_get<'a, S: BuildHasher>(
    map: &IndexMap<JsString<'a>, JsString<'a>, S>,
    key: &JsString<'_>,
) -> Option<JsString<'a>> {
    map.get(key).copied()
}

pub fn __ts_aot_map_set<'a, S: BuildHasher>(
    map: &mut IndexMap<JsString<'a>, JsString<'a>, S>,
    key: JsString<'a>,
    value: JsString<'a>,
) {
    map.insert(key, value);
}

#[must_use]
pub fn __ts_aot_object_keys<'a, S: BuildHasher>(
    ctx: &Ctx<'a>,
    map: &IndexMap<JsString<'a>, JsString<'a>, S>,
) -> BumpVec<'a, JsString<'a>> {
    let mut int_indices: Vec<(u64, JsString<'a>)> = Vec::new();
    let mut string_keys: Vec<JsString<'a>> = Vec::new();
    for key in map.keys() {
        let as_string: String = match key {
            JsString::Valid(s) => (*s).to_owned(),
            JsString::Raw(units) => String::from_utf16_lossy(units),
        };
        if let Some(n) = canonical_integer_index(&as_string) {
            int_indices.push((n, *key));
        } else {
            string_keys.push(*key);
        }
    }
    int_indices.sort_by_key(|(n, _)| *n);
    BumpVec::from_iter_in(
        int_indices.into_iter().map(|(_, s)| s).chain(string_keys),
        ctx.arena,
    )
}
