use indexmap::IndexMap;
use ts_aot_runtime::{__ts_aot_object_get_prototype_of, __ts_aot_object_keys};

#[test]
fn object_keys_returns_insertion_order_for_string_keys() {
    let mut map: IndexMap<String, String> = IndexMap::new();
    map.insert("z".to_owned(), "1".to_owned());
    map.insert("a".to_owned(), "2".to_owned());
    map.insert("m".to_owned(), "3".to_owned());
    let keys = __ts_aot_object_keys(&map);
    assert_eq!(
        keys,
        vec!["z".to_owned(), "a".to_owned(), "m".to_owned()],
        "Object.keys() must preserve string-key insertion order (JS semantic), not alphabetic"
    );
}

#[test]
fn object_keys_emits_integer_indices_first_in_ascending_numeric_order() {
    let mut map: IndexMap<String, String> = IndexMap::new();
    map.insert("2".to_owned(), "x".to_owned());
    map.insert("1".to_owned(), "y".to_owned());
    let keys = __ts_aot_object_keys(&map);
    assert_eq!(
        keys,
        vec!["1".to_owned(), "2".to_owned()],
        "Object.keys() must emit canonical integer indices in ascending numeric order, \
         regardless of insertion order (ECMAScript OwnPropertyKeys)"
    );
}

#[test]
fn object_keys_integer_indices_first_then_string_keys_in_insertion_order() {
    let mut map: IndexMap<String, String> = IndexMap::new();
    map.insert("z".to_owned(), "1".to_owned());
    map.insert("1".to_owned(), "a".to_owned());
    map.insert("a".to_owned(), "2".to_owned());
    map.insert("2".to_owned(), "b".to_owned());
    map.insert("m".to_owned(), "3".to_owned());
    let keys = __ts_aot_object_keys(&map);
    assert_eq!(
        keys,
        vec![
            "1".to_owned(),
            "2".to_owned(),
            "z".to_owned(),
            "a".to_owned(),
            "m".to_owned()
        ],
        "Object.keys() must emit integer indices first (ascending), then string keys in insertion order"
    );
}

#[test]
fn object_keys_returns_empty_vec_for_empty_map() {
    let map: IndexMap<String, String> = IndexMap::new();
    let keys = __ts_aot_object_keys(&map);
    assert!(keys.is_empty());
}

#[test]
fn object_get_prototype_of_returns_zero_sentinel() {
    assert_eq!(__ts_aot_object_get_prototype_of(0), 0);
}
