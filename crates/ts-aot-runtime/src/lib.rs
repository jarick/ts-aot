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
    fn runtime_struct_id_dynamic_matches_compiler_constant() {
        use crate::runtime_source::STRUCT_ID_DYNAMIC;
        assert_eq!(
            STRUCT_ID_DYNAMIC, 0xFFFF_FFFE,
            "runtime STRUCT_ID_DYNAMIC must match ts_aot_core::STRUCT_ID_DYNAMIC \
             (0xFFFF_FFFE). If you change one, change the other — this assertion is a \
             loud sync check so the compiler-emitted instanceof target and the \
             runtime-emitted class_id cannot drift apart"
        );
    }

    #[test]
    fn runtime_dynamic_impls_ts_class_id() {
        use crate::runtime_source::{
            __ts_aot_op_instanceof, Dynamic, DynamicValue, STRUCT_ID_DYNAMIC, TsClassId,
        };
        let dyn_val = Dynamic::new();
        let id = Dynamic::class_id();
        assert_eq!(
            id, STRUCT_ID_DYNAMIC,
            "Dynamic::class_id() must equal STRUCT_ID_DYNAMIC (0xFFFF_FFFE) so that \
             compiler-emitted `x instanceof Dynamic` resolves to the same id at runtime; \
             otherwise the instanceof check returns false even when the value is a Dynamic"
        );
        assert!(
            __ts_aot_op_instanceof(&dyn_val, STRUCT_ID_DYNAMIC),
            "Dynamic must implement TsClassId so __ts_aot_op_instanceof(&Dynamic, \
             STRUCT_ID_DYNAMIC) compiles and returns true when value is a Dynamic"
        );
        let dyn_value: DynamicValue = DynamicValue::Object(Dynamic::new());
        assert!(
            __ts_aot_op_instanceof(&dyn_value, STRUCT_ID_DYNAMIC),
            "DynamicValue::Object(Dynamic) must also pass instanceof against \
             STRUCT_ID_DYNAMIC, since the user-facing value is DynamicValue, not Dynamic"
        );
        assert_ne!(
            id,
            i64::class_id(),
            "Dynamic class_id must be distinct from primitives to avoid false matches"
        );
    }

    #[test]
    fn runtime_dynamic_value_integer_preserves_precision() {
        use crate::runtime_source::{
            __ts_aot_dynamic_get, __ts_aot_dynamic_set, Dynamic, DynamicValue,
        };
        let mut val = DynamicValue::Object(Dynamic::new());
        let big: i64 = 9_007_199_254_740_993;
        __ts_aot_dynamic_set(&mut val, "big", DynamicValue::Integer(big));
        match __ts_aot_dynamic_get(&val, "big") {
            DynamicValue::Integer(n) => assert_eq!(
                n, big,
                "Integer variant must preserve full i64 precision (Number(f64) would lose bits > 2^53)"
            ),
            other => panic!("expected Integer({big}), got {other:?}"),
        }
    }

    #[test]
    fn runtime_dynamic_value_undefined_distinct_from_null() {
        use crate::runtime_source::{
            __ts_aot_dynamic_get, __ts_aot_dynamic_set, Dynamic, DynamicValue,
        };
        let mut val = DynamicValue::Object(Dynamic::new());
        __ts_aot_dynamic_set(&mut val, "u", DynamicValue::Undefined);
        __ts_aot_dynamic_set(&mut val, "n", DynamicValue::Null);
        assert!(matches!(
            __ts_aot_dynamic_get(&val, "u"),
            DynamicValue::Undefined
        ));
        assert!(matches!(
            __ts_aot_dynamic_get(&val, "n"),
            DynamicValue::Null
        ));
        assert_ne!(
            DynamicValue::Undefined,
            DynamicValue::Null,
            "PartialEq must keep Undefined and Null distinct (strict equality); \
             JS has loose `undefined == null` but TS-typed code compares them as distinct \
             sentinels — 'missing field' (Undefined) is not the same as 'explicit null' (Null)"
        );
        assert_eq!(
            DynamicValue::Undefined,
            DynamicValue::Undefined,
            "Undefined == Undefined (reflexive)"
        );
        assert_eq!(
            DynamicValue::Null,
            DynamicValue::Null,
            "Null == Null (reflexive)"
        );
    }

    #[test]
    fn runtime_dynamic_value_from_i64() {
        use crate::runtime_source::DynamicValue;
        let v: DynamicValue = DynamicValue::from(42_i64);
        assert!(matches!(v, DynamicValue::Integer(42)));
    }

    #[test]
    fn runtime_dynamic_value_from_f64() {
        use crate::runtime_source::DynamicValue;
        let v: DynamicValue = DynamicValue::from(3.5_f64);
        assert!(matches!(v, DynamicValue::Number(n) if (n - 3.5).abs() < f64::EPSILON));
    }

    #[test]
    fn runtime_dynamic_value_from_bool() {
        use crate::runtime_source::DynamicValue;
        assert!(matches!(DynamicValue::from(true), DynamicValue::Bool(true)));
        assert!(matches!(
            DynamicValue::from(false),
            DynamicValue::Bool(false)
        ));
    }

    #[test]
    fn runtime_dynamic_value_from_string_and_str() {
        use crate::runtime_source::DynamicValue;
        let owned: DynamicValue = DynamicValue::from(String::from("hi"));
        let borrowed: DynamicValue = DynamicValue::from("hi");
        assert!(matches!(owned, DynamicValue::String(s) if s == "hi"));
        assert!(matches!(borrowed, DynamicValue::String(s) if s == "hi"));
    }

    #[test]
    fn runtime_dynamic_op_add_integers() {
        use crate::runtime_source::{__ts_aot_dynamic_op, DYNAMIC_OP_ADD, DynamicValue};
        let l = DynamicValue::Integer(40);
        let r = DynamicValue::Integer(2);
        assert!(matches!(
            __ts_aot_dynamic_op(DYNAMIC_OP_ADD, &l, &r),
            DynamicValue::Integer(42)
        ));
    }

    #[test]
    fn runtime_dynamic_op_add_numbers_promotes_int_to_float() {
        use crate::runtime_source::{__ts_aot_dynamic_op, DYNAMIC_OP_ADD, DynamicValue};
        let l = DynamicValue::Integer(40);
        let r = DynamicValue::Number(2.5);
        match __ts_aot_dynamic_op(DYNAMIC_OP_ADD, &l, &r) {
            DynamicValue::Number(n) => assert!((n - 42.5).abs() < f64::EPSILON),
            other => panic!("expected Number(42.5), got {other:?}"),
        }
    }

    #[test]
    fn runtime_dynamic_op_add_strings_concatenates() {
        use crate::runtime_source::{__ts_aot_dynamic_op, DYNAMIC_OP_ADD, DynamicValue};
        let l = DynamicValue::String("foo".to_owned());
        let r = DynamicValue::String("bar".to_owned());
        assert!(matches!(
            __ts_aot_dynamic_op(DYNAMIC_OP_ADD, &l, &r),
            DynamicValue::String(s) if s == "foobar"
        ));
    }

    #[test]
    fn runtime_dynamic_op_sub_mul_div_mod() {
        use crate::runtime_source::{
            __ts_aot_dynamic_op, DYNAMIC_OP_DIV, DYNAMIC_OP_MOD, DYNAMIC_OP_MUL, DYNAMIC_OP_SUB,
            DynamicValue,
        };
        let a = DynamicValue::Integer(10);
        let b = DynamicValue::Integer(3);
        assert!(matches!(
            __ts_aot_dynamic_op(DYNAMIC_OP_SUB, &a, &b),
            DynamicValue::Integer(7)
        ));
        assert!(matches!(
            __ts_aot_dynamic_op(DYNAMIC_OP_MUL, &a, &b),
            DynamicValue::Integer(30)
        ));
        assert!(matches!(
            __ts_aot_dynamic_op(DYNAMIC_OP_DIV, &a, &b),
            DynamicValue::Number(n) if (n - 10.0 / 3.0).abs() < 1e-9
        ));
        assert!(matches!(
            __ts_aot_dynamic_op(DYNAMIC_OP_MOD, &a, &b),
            DynamicValue::Integer(1)
        ));
    }

    #[test]
    fn runtime_dynamic_op_mod_for_floats() {
        use crate::runtime_source::{__ts_aot_dynamic_op, DYNAMIC_OP_MOD, DynamicValue};
        let a = DynamicValue::Number(5.5);
        let b = DynamicValue::Number(2.0);
        match __ts_aot_dynamic_op(DYNAMIC_OP_MOD, &a, &b) {
            DynamicValue::Number(n) => assert!(
                (n - 1.5).abs() < 1e-9,
                "5.5 % 2.0 must be 1.5 (JS fidelity), got {n}"
            ),
            other => panic!("expected Number(1.5), got {other:?}"),
        }
        let neg = DynamicValue::Number(-5.0);
        match __ts_aot_dynamic_op(DYNAMIC_OP_MOD, &neg, &b) {
            DynamicValue::Number(n) => assert!(
                (n - (-1.0)).abs() < 1e-9,
                "-5.0 % 2.0 must be -1.0 (sign of dividend, JS fidelity), got {n}"
            ),
            other => panic!("expected Number(-1.0), got {other:?}"),
        }
        let int_a = DynamicValue::Integer(7);
        let float_b = DynamicValue::Number(2.5);
        match __ts_aot_dynamic_op(DYNAMIC_OP_MOD, &int_a, &float_b) {
            DynamicValue::Number(n) => assert!(
                (n - 2.0).abs() < 1e-9,
                "7 % 2.5 must be 2.0 (mixed Integer/Number), got {n}"
            ),
            other => panic!("expected Number(2.0), got {other:?}"),
        }
    }

    #[test]
    fn runtime_dynamic_op_mod_operand_order_preserved() {
        use crate::runtime_source::{__ts_aot_dynamic_op, DYNAMIC_OP_MOD, DynamicValue};
        match __ts_aot_dynamic_op(
            DYNAMIC_OP_MOD,
            &DynamicValue::Number(7.0),
            &DynamicValue::Integer(2),
        ) {
            DynamicValue::Number(n) => assert!(
                (n - 1.0).abs() < 1e-9,
                "7.0 % 2 (Number % Integer) must be 1.0 with operand order preserved, got {n}"
            ),
            other => panic!("expected Number(1.0), got {other:?}"),
        }
        match __ts_aot_dynamic_op(
            DYNAMIC_OP_MOD,
            &DynamicValue::Number(1.0),
            &DynamicValue::Integer(2),
        ) {
            DynamicValue::Number(n) => assert!(
                (n - 1.0).abs() < 1e-9,
                "1.0 % 2 (Number % Integer) must be 1.0, got {n}"
            ),
            other => panic!("expected Number(1.0), got {other:?}"),
        }
        match __ts_aot_dynamic_op(
            DYNAMIC_OP_MOD,
            &DynamicValue::Integer(7),
            &DynamicValue::Number(2.0),
        ) {
            DynamicValue::Number(n) => assert!(
                (n - 1.0).abs() < 1e-9,
                "7 % 2.0 (Integer % Number) must be 1.0, got {n}"
            ),
            other => panic!("expected Number(1.0), got {other:?}"),
        }
    }

    #[test]
    fn runtime_dynamic_op_non_numeric_returns_undefined() {
        use crate::runtime_source::{__ts_aot_dynamic_op, DYNAMIC_OP_ADD, DynamicValue};
        let l = DynamicValue::Bool(true);
        let r = DynamicValue::Integer(1);
        assert!(matches!(
            __ts_aot_dynamic_op(DYNAMIC_OP_ADD, &l, &r),
            DynamicValue::Undefined
        ));
    }

    #[test]
    fn runtime_dynamic_op_div_by_zero_yields_ieee754_specials() {
        use crate::runtime_source::{__ts_aot_dynamic_op, DYNAMIC_OP_DIV, DynamicValue};
        let zero = DynamicValue::Number(0.0);
        let pos = DynamicValue::Number(1.0);
        let neg = DynamicValue::Number(-1.0);
        match __ts_aot_dynamic_op(DYNAMIC_OP_DIV, &zero, &zero) {
            DynamicValue::Number(n) => {
                assert!(n.is_nan(), "0.0 / 0.0 must be NaN (JS fidelity), got {n}");
            }
            other => panic!("expected Number(NaN), got {other:?}"),
        }
        match __ts_aot_dynamic_op(DYNAMIC_OP_DIV, &pos, &zero) {
            DynamicValue::Number(n) => assert!(
                n.is_infinite() && n.is_sign_positive(),
                "1.0 / 0.0 must be +Infinity (JS fidelity), got {n}"
            ),
            other => panic!("expected Number(+Infinity), got {other:?}"),
        }
        match __ts_aot_dynamic_op(DYNAMIC_OP_DIV, &neg, &zero) {
            DynamicValue::Number(n) => assert!(
                n.is_infinite() && n.is_sign_negative(),
                "-1.0 / 0.0 must be -Infinity (JS fidelity), got {n}"
            ),
            other => panic!("expected Number(-Infinity), got {other:?}"),
        }
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

    #[test]
    fn runtime_dynamic_get_set_returns_value() {
        use crate::runtime_source::{
            __ts_aot_dynamic_get, __ts_aot_dynamic_set, Dynamic, DynamicValue,
        };
        let mut val = DynamicValue::Object(Dynamic::new());
        __ts_aot_dynamic_set(&mut val, "x", DynamicValue::Number(42.0));
        match __ts_aot_dynamic_get(&val, "x") {
            DynamicValue::Number(n) => {
                assert!((n - 42.0).abs() < f64::EPSILON, "expected ~42.0, got {n}");
            }
            other => panic!("expected Number(42.0), got {other:?}"),
        }
    }

    #[test]
    fn runtime_dynamic_get_missing_field_returns_undefined() {
        use crate::runtime_source::{__ts_aot_dynamic_get, Dynamic, DynamicValue};
        let val = DynamicValue::Object(Dynamic::new());
        assert_eq!(
            __ts_aot_dynamic_get(&val, "missing"),
            DynamicValue::Undefined,
            "get on missing field must return Undefined (JS fidelity)"
        );
    }

    #[test]
    fn runtime_dynamic_get_on_non_object_returns_undefined() {
        use crate::runtime_source::{__ts_aot_dynamic_get, DynamicValue};
        assert_eq!(
            __ts_aot_dynamic_get(&DynamicValue::Number(42.0), "x"),
            DynamicValue::Undefined,
            "get on non-Object value must return Undefined (no-op field access)"
        );
        assert_eq!(
            __ts_aot_dynamic_get(&DynamicValue::Null, "x"),
            DynamicValue::Undefined,
        );
    }

    #[test]
    fn runtime_dynamic_has_returns_true_for_set_field() {
        use crate::runtime_source::{
            __ts_aot_dynamic_has, __ts_aot_dynamic_key, __ts_aot_dynamic_set, Dynamic, DynamicValue,
        };
        let mut val = DynamicValue::Object(Dynamic::new());
        assert!(
            !__ts_aot_dynamic_has(&val, &__ts_aot_dynamic_key("x")),
            "fresh Object value must not have field"
        );
        __ts_aot_dynamic_set(&mut val, "x", DynamicValue::Bool(true));
        assert!(
            __ts_aot_dynamic_has(&val, &__ts_aot_dynamic_key("x")),
            "after set, has must return true"
        );
    }

    #[test]
    fn runtime_dynamic_has_on_non_object_returns_false() {
        use crate::runtime_source::{__ts_aot_dynamic_has, __ts_aot_dynamic_key, DynamicValue};
        assert!(!__ts_aot_dynamic_has(
            &DynamicValue::Null,
            &__ts_aot_dynamic_key("x")
        ));
        assert!(!__ts_aot_dynamic_has(
            &DynamicValue::Number(42.0),
            &__ts_aot_dynamic_key("x")
        ));
    }

    #[test]
    fn runtime_dynamic_set_overwrites_existing_field() {
        use crate::runtime_source::{
            __ts_aot_dynamic_get, __ts_aot_dynamic_set, Dynamic, DynamicValue,
        };
        let mut val = DynamicValue::Object(Dynamic::new());
        __ts_aot_dynamic_set(&mut val, "x", DynamicValue::Number(1.0));
        __ts_aot_dynamic_set(&mut val, "x", DynamicValue::Number(2.0));
        match __ts_aot_dynamic_get(&val, "x") {
            DynamicValue::Number(n) => {
                assert!(
                    (n - 2.0).abs() < f64::EPSILON,
                    "second set must overwrite first, got {n}"
                );
            }
            other => panic!("expected Number(2.0), got {other:?}"),
        }
    }

    #[test]
    fn runtime_dynamic_set_on_non_object_promotes_to_object() {
        use crate::runtime_source::{__ts_aot_dynamic_get, __ts_aot_dynamic_set, DynamicValue};
        let mut val = DynamicValue::Number(42.0);
        __ts_aot_dynamic_set(&mut val, "x", DynamicValue::Bool(true));
        match __ts_aot_dynamic_get(&val, "x") {
            DynamicValue::Bool(b) => assert!(b),
            other => panic!("expected Bool(true), got {other:?}"),
        }
    }

    #[test]
    fn runtime_dynamic_stores_nested_object() {
        use crate::runtime_source::{
            __ts_aot_dynamic_get, __ts_aot_dynamic_set, Dynamic, DynamicValue,
        };
        let mut outer = DynamicValue::Object(Dynamic::new());
        let mut inner = DynamicValue::Object(Dynamic::new());
        __ts_aot_dynamic_set(&mut inner, "y", DynamicValue::String(String::from("hi")));
        __ts_aot_dynamic_set(&mut outer, "nested", inner);
        let mut nested = __ts_aot_dynamic_get(&outer, "nested");
        match __ts_aot_dynamic_get(&nested, "y") {
            DynamicValue::String(s) => assert_eq!(s, "hi"),
            other => panic!("expected String(hi) before mutation, got {other:?}"),
        }
        __ts_aot_dynamic_set(
            &mut nested,
            "y",
            DynamicValue::String(String::from("modified")),
        );
        let nested_again = __ts_aot_dynamic_get(&outer, "nested");
        match __ts_aot_dynamic_get(&nested_again, "y") {
            DynamicValue::String(s) => assert_eq!(
                s, "modified",
                "mutation through retrieved nested must be visible through outer \
                 (shared object identity, not a clone)"
            ),
            other => panic!("expected String(modified) after mutation, got {other:?}"),
        }
    }

    #[test]
    fn runtime_dynamic_delete_removes_field() {
        use crate::runtime_source::{
            __ts_aot_dynamic_delete, __ts_aot_dynamic_get, __ts_aot_dynamic_has,
            __ts_aot_dynamic_key, __ts_aot_dynamic_set, Dynamic, DynamicValue,
        };
        let mut val = DynamicValue::Object(Dynamic::new());
        __ts_aot_dynamic_set(&mut val, "x", DynamicValue::Number(42.0));
        assert!(__ts_aot_dynamic_has(&val, &__ts_aot_dynamic_key("x")));
        __ts_aot_dynamic_delete(&mut val, "x");
        assert!(
            !__ts_aot_dynamic_has(&val, &__ts_aot_dynamic_key("x")),
            "after delete, has must return false"
        );
        assert_eq!(
            __ts_aot_dynamic_get(&val, "x"),
            DynamicValue::Undefined,
            "deleted field must return Undefined on get"
        );
    }

    #[test]
    fn runtime_dynamic_delete_on_non_object_is_noop() {
        use crate::runtime_source::{
            __ts_aot_dynamic_delete, __ts_aot_dynamic_has, __ts_aot_dynamic_key, DynamicValue,
        };
        let mut val = DynamicValue::Number(42.0);
        __ts_aot_dynamic_delete(&mut val, "x");
        assert!(!__ts_aot_dynamic_has(&val, &__ts_aot_dynamic_key("x")));
    }

    #[test]
    fn runtime_dynamic_object_equality_uses_rc_identity_not_structural() {
        use crate::runtime_source::{
            __ts_aot_dynamic_get, __ts_aot_dynamic_set, Dynamic, DynamicValue,
        };
        let shared_inner = DynamicValue::Object(Dynamic::new());
        let mut outer = DynamicValue::Object(Dynamic::new());
        __ts_aot_dynamic_set(&mut outer, "nested", shared_inner.clone());
        let retrieved = __ts_aot_dynamic_get(&outer, "nested");
        assert_eq!(
            retrieved, shared_inner,
            "retrieved nested must equal the original (shared Rc identity via __ts_aot_dynamic_get clone)"
        );
        let mut separate = DynamicValue::Object(Dynamic::new());
        __ts_aot_dynamic_set(&mut separate, "x", DynamicValue::Integer(42));
        __ts_aot_dynamic_set(&mut separate, "y", DynamicValue::String("hi".to_owned()));
        let mut clone_structurally = DynamicValue::Object(Dynamic::new());
        __ts_aot_dynamic_set(&mut clone_structurally, "x", DynamicValue::Integer(42));
        __ts_aot_dynamic_set(
            &mut clone_structurally,
            "y",
            DynamicValue::String("hi".to_owned()),
        );
        assert_ne!(
            separate, clone_structurally,
            "two DynamicValue::Object with identical fields but different Rc must NOT be equal \
             (Rc::ptr_eq identity, not structural HashMap comparison — consistent with JS reference \
             equality for objects)"
        );
    }

    #[test]
    fn runtime_throw_helper_panics_with_value_payload() {
        use crate::runtime_source::__ts_aot_throw;
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            __ts_aot_throw(42_i64);
        }));
        let err = result.expect_err("__ts_aot_throw must panic");
        let recovered = err.downcast_ref::<i64>().copied().expect(
            "panic payload must downcast to i64 so generated `let err: i64 = __e` \
                    bindings can recover the thrown value",
        );
        assert_eq!(recovered, 42, "payload must round-trip through panic");
    }

    #[test]
    fn runtime_throw_helper_captures_string_payload() {
        use crate::runtime_source::__ts_aot_throw;
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            __ts_aot_throw(String::from("oops"));
        }));
        let err = result.expect_err("string payload must panic");
        let recovered = err
            .downcast_ref::<String>()
            .cloned()
            .expect("string payload must downcast to String");
        assert_eq!(recovered, "oops", "String payload must round-trip");
    }

    #[test]
    fn runtime_throw_helper_panics_with_prefix_message() {
        use crate::runtime_source::__ts_aot_throw;
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            __ts_aot_throw(42_i64);
        }));
        let err = result.expect_err("must panic");
        let msg = err
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| err.downcast_ref::<i64>().map(|_| "i64"))
            .unwrap_or("unknown");
        assert!(
            !msg.is_empty(),
            "panic payload must carry a recoverable representation of the thrown value"
        );
    }
}
