use indexmap::IndexMap;
use ts_aot_runtime::{__ts_aot_object_keys, Ctx, Host, JsString, run_in_arena};

fn with_ctx<R>(f: impl for<'a> FnOnce(&Ctx<'a>) -> R) -> R {
    struct H;
    impl Host for H {}
    run_in_arena(&H, f)
}

fn js(s: &str) -> JsString<'_> {
    JsString::from(s)
}

fn keys(map: &IndexMap<JsString<'_>, JsString<'_>>) -> Vec<String> {
    with_ctx(|ctx| {
        __ts_aot_object_keys(ctx, map)
            .iter()
            .map(JsString::to_string_lossy)
            .collect()
    })
}

fn owned(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| (*s).to_owned()).collect()
}

#[test]
fn object_keys_returns_insertion_order_for_string_keys() {
    let mut map: IndexMap<JsString, JsString> = IndexMap::new();
    map.insert(js("z"), js("1"));
    map.insert(js("a"), js("2"));
    map.insert(js("m"), js("3"));
    assert_eq!(
        keys(&map),
        owned(&["z", "a", "m"]),
        "Object.keys() must preserve string-key insertion order (JS semantic), not alphabetic"
    );
}

#[test]
fn object_keys_emits_integer_indices_first_in_ascending_numeric_order() {
    let three = [u16::from(b'3')];
    let mut map: IndexMap<JsString, JsString> = IndexMap::new();
    map.insert(js("2"), js("x"));
    map.insert(js("1"), js("y"));
    map.insert(JsString::Raw(&three), js("z"));
    assert_eq!(
        keys(&map),
        owned(&["1", "2", "3"]),
        "Object.keys() must emit canonical integer indices in ascending numeric order, \
         regardless of insertion order (ECMAScript OwnPropertyKeys) — both Valid and Raw \
         JsString integer keys must be classified and ordered together"
    );
}

#[test]
fn object_keys_recognises_integer_index_via_raw_jsstring() {
    let seven = [u16::from(b'7')];
    let mut map: IndexMap<JsString, JsString> = IndexMap::new();
    let raw_index = JsString::Raw(&seven);
    map.insert(raw_index, js("v"));
    map.insert(js("z"), js("w"));
    assert_eq!(
        keys(&map),
        owned(&["7", "z"]),
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
    assert_eq!(
        keys(&map),
        owned(&["1", "2", "z", "a", "m"]),
        "Object.keys() must emit integer indices first (ascending), then string keys in insertion order"
    );
}

#[test]
fn object_keys_returns_empty_vec_for_empty_map() {
    let map: IndexMap<JsString, JsString> = IndexMap::new();
    assert_eq!(keys(&map), [] as [String; 0]);
}
