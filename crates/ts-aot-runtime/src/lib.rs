pub mod runtime_source;

pub const RUNTIME_SOURCE: &str = include_str!("runtime_source.rs");

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn runtime_source_is_non_empty() {
        assert!(!RUNTIME_SOURCE.is_empty());
    }

    #[test]
    fn runtime_source_contains_host_console_log() {
        assert!(
            RUNTIME_SOURCE.contains("__ts_aot_host_console_log"),
            "runtime must define __ts_aot_host_console_log"
        );
    }

    #[test]
    fn runtime_source_contains_math_sqrt() {
        assert!(
            RUNTIME_SOURCE.contains("__ts_aot_math_sqrt"),
            "runtime must define __ts_aot_math_sqrt"
        );
    }

    #[test]
    fn runtime_source_contains_string_helpers() {
        assert!(RUNTIME_SOURCE.contains("__ts_aot_string_concat"));
        assert!(RUNTIME_SOURCE.contains("__ts_aot_string_equals"));
        assert!(RUNTIME_SOURCE.contains("__ts_aot_string_len"));
    }

    #[test]
    fn runtime_source_contains_array_helpers() {
        assert!(RUNTIME_SOURCE.contains("__ts_aot_array_create"));
        assert!(RUNTIME_SOURCE.contains("__ts_aot_array_get"));
        assert!(RUNTIME_SOURCE.contains("__ts_aot_array_set"));
        assert!(RUNTIME_SOURCE.contains("__ts_aot_array_len"));
    }

    #[test]
    fn runtime_source_contains_map_helpers() {
        assert!(RUNTIME_SOURCE.contains("__ts_aot_map_create"));
        assert!(RUNTIME_SOURCE.contains("__ts_aot_map_get"));
        assert!(RUNTIME_SOURCE.contains("__ts_aot_map_set"));
    }

    #[test]
    fn runtime_source_contains_ts_class_id_trait() {
        assert!(
            RUNTIME_SOURCE.contains("pub trait TsClassId"),
            "runtime must define `pub trait TsClassId` for safe instanceof dispatch"
        );
        assert!(
            RUNTIME_SOURCE.contains("fn class_id() -> u32"),
            "TsClassId trait must require `fn class_id() -> u32`"
        );
        assert!(
            RUNTIME_SOURCE.contains("impl TsClassId for i64"),
            "runtime must impl TsClassId for i64 (primitive used in tests + e2e)"
        );
        assert!(
            RUNTIME_SOURCE.contains("impl<T> TsClassId for Vec<T>"),
            "runtime must impl TsClassId for Vec<T> (used for arrays)"
        );
    }

    #[test]
    fn runtime_op_in_array_index_in_range_returns_true() {
        use crate::runtime_source::__ts_aot_op_in;
        let arr: Vec<i64> = vec![10, 20, 30, 40];
        let index: i64 = 2;
        assert!(
            __ts_aot_op_in(&index, &arr),
            "positive case: index 2 is a valid index in arr of len 4, must return true"
        );
    }

    #[test]
    fn runtime_op_in_array_index_out_of_range_returns_false() {
        use crate::runtime_source::__ts_aot_op_in;
        let arr: Vec<i64> = vec![10, 20, 30, 40];
        let index: i64 = 10;
        assert!(
            !__ts_aot_op_in(&index, &arr),
            "negative case: index 10 is out of range in arr of len 4, must return false"
        );
    }

    #[test]
    fn runtime_op_in_string_in_string_vec_member_returns_true() {
        use crate::runtime_source::__ts_aot_op_in;
        let arr: Vec<String> = vec!["a".to_owned(), "b".to_owned(), "c".to_owned()];
        let needle = "b".to_owned();
        assert!(
            __ts_aot_op_in(&needle, &arr),
            "positive case: 'b' is a member of the string vec, must return true"
        );
    }

    #[test]
    fn runtime_op_in_string_in_string_vec_non_member_returns_false() {
        use crate::runtime_source::__ts_aot_op_in;
        let arr: Vec<String> = vec!["a".to_owned(), "b".to_owned(), "c".to_owned()];
        let needle = "z".to_owned();
        assert!(
            !__ts_aot_op_in(&needle, &arr),
            "negative case: 'z' is not a member, must return false"
        );
    }

    #[test]
    fn runtime_op_in_non_container_returns_false() {
        use crate::runtime_source::__ts_aot_op_in;
        let arr: Vec<i64> = vec![1, 2, 3];
        let string_needle = "foo".to_owned();
        assert!(
            !__ts_aot_op_in(&string_needle, &arr),
            "negative case: String needle against Vec<i64> is type mismatch, must return false"
        );
    }

    #[test]
    fn runtime_op_in_hashmap_key_present_returns_true() {
        use crate::runtime_source::__ts_aot_op_in;
        use std::collections::HashMap;
        let mut map: HashMap<String, i64> = HashMap::new();
        map.insert("alpha".to_owned(), 1);
        map.insert("beta".to_owned(), 2);
        let key = "alpha".to_owned();
        assert!(
            __ts_aot_op_in(&key, &map),
            "positive case: 'alpha' is a key in the map, must return true"
        );
    }

    #[test]
    fn runtime_op_in_hashmap_key_absent_returns_false() {
        use crate::runtime_source::__ts_aot_op_in;
        use std::collections::HashMap;
        let mut map: HashMap<String, i64> = HashMap::new();
        map.insert("alpha".to_owned(), 1);
        let key = "missing".to_owned();
        assert!(
            !__ts_aot_op_in(&key, &map),
            "negative case: 'missing' is not a key, must return false"
        );
    }

    #[test]
    fn runtime_op_delete_non_property_returns_true() {
        use crate::runtime_source::__ts_aot_op_delete;
        let value: i64 = 42;
        let local: i64 = 7;
        assert!(
            __ts_aot_op_delete(&value),
            "real JS semantic: delete <non-property> returns true (no-op)"
        );
        assert!(
            __ts_aot_op_delete(&local),
            "real JS semantic: delete <local> returns true (no-op for non-properties)"
        );
    }

    #[test]
    fn runtime_op_delete_field_index_returns_true() {
        use crate::runtime_source::__ts_aot_op_delete;
        #[repr(C)]
        struct TestStruct {
            __type_id: u32,
            x: i64,
        }
        let foo = TestStruct { __type_id: 1, x: 5 };
        assert!(
            __ts_aot_op_delete(&foo),
            "real JS semantic: delete on struct value (convert's OpDelete runtime) \
             returns true (configurable property in our model); the actual property \
             removal is the responsibility of the caller (convert) which captures the \
             side effect in `out`"
        );
    }

    #[test]
    fn runtime_op_instanceof_matching_class_id_returns_true() {
        use crate::runtime_source::{__ts_aot_op_instanceof, TsClassId};
        struct Foo {
            _x: i64,
        }
        impl TsClassId for Foo {
            fn class_id() -> u32 {
                7
            }
        }
        let foo = Foo { _x: 5 };
        assert!(
            __ts_aot_op_instanceof(&foo, 7u32),
            "positive case: value's class_id == target_type_id must return true"
        );
    }

    #[test]
    fn runtime_op_instanceof_different_class_id_returns_false() {
        use crate::runtime_source::{__ts_aot_op_instanceof, TsClassId};
        struct Foo {
            _x: i64,
        }
        impl TsClassId for Foo {
            fn class_id() -> u32 {
                7
            }
        }
        struct Bar {
            _x: i64,
        }
        impl TsClassId for Bar {
            fn class_id() -> u32 {
                9
            }
        }
        let foo = Foo { _x: 5 };
        assert!(
            !__ts_aot_op_instanceof(&foo, 9u32),
            "negative case: value's class_id (7) != target_type_id (9) must return false"
        );
        let bar = Bar { _x: 5 };
        assert!(
            !__ts_aot_op_instanceof(&bar, 7u32),
            "negative case: value's class_id (9) != target_type_id (7) must return false"
        );
    }

    #[test]
    fn runtime_op_instanceof_primitive_value_never_matches_struct_id() {
        use crate::runtime_source::__ts_aot_op_instanceof;
        let x: i64 = 42;
        assert!(
            !__ts_aot_op_instanceof(&x, 0u32),
            "primitive value must never match struct class id 0"
        );
        assert!(
            !__ts_aot_op_instanceof(&x, 42u32),
            "primitive value must never match struct class id 42"
        );
        assert!(
            !__ts_aot_op_instanceof(&x, u32::MAX),
            "primitive value must never match u32::MAX"
        );
        let b: bool = true;
        assert!(
            !__ts_aot_op_instanceof(&b, 0u32),
            "bool primitive must never match struct class id 0"
        );
    }

    #[test]
    fn runtime_op_instanceof_primitives_have_distinct_class_ids() {
        use crate::runtime_source::TsClassId;
        assert_ne!(i64::class_id(), bool::class_id());
        assert_ne!(i64::class_id(), String::class_id());
        assert_ne!(i64::class_id(), <()>::class_id());
        assert_ne!(bool::class_id(), String::class_id());
    }

    #[test]
    fn runtime_op_instanceof_compound_types_never_match_struct_id() {
        use crate::runtime_source::__ts_aot_op_instanceof;
        let opt: Option<i64> = Some(42);
        assert!(!__ts_aot_op_instanceof(&opt, 0u32));
        let res: Result<i64, String> = Ok(42);
        assert!(!__ts_aot_op_instanceof(&res, 0u32));
        let boxed: Box<i64> = Box::new(42);
        assert!(!__ts_aot_op_instanceof(&boxed, 0u32));
        let vec: Vec<i64> = vec![1, 2, 3];
        assert!(!__ts_aot_op_instanceof(&vec, 0u32));
        let map: std::collections::HashMap<String, i64> = HashMap::new();
        assert!(!__ts_aot_op_instanceof(&map, 0u32));
        let tuple: (i64, String) = (1, String::from("a"));
        assert!(!__ts_aot_op_instanceof(&tuple, 0u32));
        let tuple3: (i64, String, bool) = (1, String::from("a"), true);
        assert!(!__ts_aot_op_instanceof(&tuple3, 0u32));
    }

    #[test]
    fn runtime_typeof_for_int_returns_number() {
        use crate::runtime_source::__ts_aot_typeof;
        let v: i64 = 42;
        assert_eq!(__ts_aot_typeof(&v), "number");
    }

    #[test]
    fn runtime_typeof_for_float_returns_number() {
        use crate::runtime_source::__ts_aot_typeof;
        let f: f32 = 1.5;
        assert_eq!(__ts_aot_typeof(&f), "number");
        let d: f64 = 1.5;
        assert_eq!(__ts_aot_typeof(&d), "number");
    }

    #[test]
    fn runtime_typeof_for_bool_returns_boolean() {
        use crate::runtime_source::__ts_aot_typeof;
        let v: bool = true;
        assert_eq!(__ts_aot_typeof(&v), "boolean");
    }

    #[test]
    fn runtime_typeof_for_string_returns_string() {
        use crate::runtime_source::__ts_aot_typeof;
        let v = String::from("hi");
        assert_eq!(__ts_aot_typeof(&v), "string");
    }

    #[test]
    fn runtime_typeof_unit_returns_undefined() {
        use crate::runtime_source::__ts_aot_typeof_unit;
        assert_eq!(__ts_aot_typeof_unit(), "undefined");
    }

    #[test]
    fn runtime_typeof_null_returns_object() {
        use crate::runtime_source::__ts_aot_typeof_null;
        assert_eq!(__ts_aot_typeof_null(), "object");
    }

    #[test]
    fn runtime_op_instanceof_accepts_arbitrary_target_id() {
        use crate::runtime_source::{__ts_aot_op_instanceof, TsClassId};
        struct Foo {
            _x: i64,
        }
        impl TsClassId for Foo {
            fn class_id() -> u32 {
                0
            }
        }
        let foo = Foo { _x: 0 };
        assert!(__ts_aot_op_instanceof(&foo, 0u32));
        assert!(!__ts_aot_op_instanceof(&foo, u32::MAX));
        assert!(!__ts_aot_op_instanceof(&foo, 42u32));
    }
}
