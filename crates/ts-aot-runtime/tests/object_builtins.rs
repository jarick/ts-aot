use indexmap::IndexMap;
use ts_aot_runtime::{__ts_aot_object_keys, JsString};

fn js(s: &str) -> JsString {
    JsString::from(s)
}

#[test]
fn object_keys_returns_insertion_order_for_string_keys() {
    let mut map: IndexMap<JsString, JsString> = IndexMap::new();
    map.insert(js("z"), js("1"));
    map.insert(js("a"), js("2"));
    map.insert(js("m"), js("3"));
    let keys = __ts_aot_object_keys(&map);
    assert_eq!(
        keys,
        vec![js("z"), js("a"), js("m")],
        "Object.keys() must preserve string-key insertion order (JS semantic), not alphabetic"
    );
}

#[test]
fn object_keys_emits_integer_indices_first_in_ascending_numeric_order() {
    let mut map: IndexMap<JsString, JsString> = IndexMap::new();
    map.insert(js("2"), js("x"));
    map.insert(js("1"), js("y"));
    map.insert(JsString::from_units(vec![u16::from(b'3')]), js("z"));
    let keys = __ts_aot_object_keys(&map);
    assert_eq!(
        keys,
        vec![
            js("1"),
            js("2"),
            JsString::from_units(vec![u16::from(b'3')]),
        ],
        "Object.keys() must emit canonical integer indices in ascending numeric order, \
         regardless of insertion order (ECMAScript OwnPropertyKeys) — both Valid and Raw \
         JsString integer keys must be classified and ordered together"
    );
}

#[test]
fn object_keys_recognises_integer_index_via_raw_jsstring() {
    let mut map: IndexMap<JsString, JsString> = IndexMap::new();
    let raw_index = JsString::from_units(vec![u16::from(b'7')]);
    map.insert(raw_index, js("v"));
    map.insert(js("z"), js("w"));
    let keys = __ts_aot_object_keys(&map);
    assert_eq!(
        keys,
        vec![JsString::from_units(vec![u16::from(b'7')]), js("z")],
        "Raw JsString integer-index key (7) must be classified as int index, not string key"
    );
}

#[test]
fn object_keys_integer_indices_first_then_string_keys_in_insertion_order() {
    let mut map: IndexMap<JsString, JsString> = IndexMap::new();
    map.insert(js("z"), js("1"));
    map.insert(js("1"), js("a"));
    map.insert(js("a"), js("2"));
    map.insert(js("2"), js("b"));
    map.insert(js("m"), js("3"));
    let keys = __ts_aot_object_keys(&map);
    assert_eq!(
        keys,
        vec![js("1"), js("2"), js("z"), js("a"), js("m")],
        "Object.keys() must emit integer indices first (ascending), then string keys in insertion order"
    );
}

#[test]
fn object_keys_returns_empty_vec_for_empty_map() {
    let map: IndexMap<JsString, JsString> = IndexMap::new();
    let keys = __ts_aot_object_keys(&map);
    assert!(keys.is_empty());
}
