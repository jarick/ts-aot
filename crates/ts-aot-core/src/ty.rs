use crate::ids::{Atom, GenericParamId, StructId, TypeId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MemoryKind {
    CopyValue,
    ValueStruct,
    GcRef,
    NullableGcRef,
    LinearMemoryValue,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NullishKinds {
    pub null: bool,
    pub undefined: bool,
}

impl NullishKinds {
    pub const NULL_ONLY: Self = Self {
        null: true,
        undefined: false,
    };
    pub const UNDEFINED_ONLY: Self = Self {
        null: false,
        undefined: true,
    };
    pub const BOTH: Self = Self {
        null: true,
        undefined: true,
    };
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Type {
    Void,
    Bool,
    I8,
    I16,
    I32,
    I64,
    U8,
    U16,
    U32,
    U64,
    F32,
    F64,
    String,
    Null,
    Optional {
        inner: TypeId,
        nullish: NullishKinds,
    },
    Struct {
        id: StructId,
    },
    Array {
        element: TypeId,
    },
    Fn {
        params: Vec<TypeId>,
        /// Positional "the caller may omit this argument" flags, parallel to
        /// `params`: `true` for a `?`-optional parameter and for one with a
        /// default value. The right to omit an argument is *declared* here and
        /// is never inferred from the parameter's type - a required parameter
        /// whose type happens to admit `undefined` (`x: T | undefined`, no `?`)
        /// stays required. A `optional` vector shorter than `params` (including
        /// the empty one) means "every remaining parameter is required".
        ///
        /// These flags take part in `Hash`/`Eq`, so two function types that
        /// differ only in optionality intern to two distinct `TypeId`s.
        optional: Vec<bool>,
        ret: TypeId,
        err: Option<TypeId>,
    },
    Promise {
        ok: TypeId,
        err: Option<TypeId>,
    },
    Date,
    Symbol,
    BigInt,
    ArrayBuffer,
    WeakMap {
        key: TypeId,
        value: TypeId,
    },
    Int8Array,
    Uint8Array,
    Uint8ClampedArray,
    Int16Array,
    Uint16Array,
    Int32Array,
    Uint32Array,
    Float32Array,
    Float64Array,
    Result {
        ok: TypeId,
        err: TypeId,
    },
    Union {
        variants: Vec<TypeId>,
    },
    Intersection {
        parts: Vec<TypeId>,
    },
    Tuple {
        elements: Vec<TypeId>,
    },
    Named {
        symbol: Atom,
    },
    Generator {
        inner: TypeId,
        is_async: bool,
    },
    GeneratorResult {
        inner: TypeId,
    },
    AsyncGenYield {
        inner: TypeId,
    },
    GenericParam {
        id: GenericParamId,
    },
    Error,
}

impl Type {
    /// Whether parameter `index` of this function type may be omitted by a
    /// caller. `true` only for a parameter whose `optional` flag is set; a
    /// missing or short [`Type::Fn::optional`] vector means "required", so the
    /// parameter's own type (`T | undefined`) never grants the right to omit an
    /// argument.
    #[must_use]
    pub fn fn_param_optional(&self, index: usize) -> bool {
        match self {
            Type::Fn { optional, .. } => optional.get(index).copied().unwrap_or(false),
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TypeTable;
    use crate::ids::{FunctionId, LocalId};

    #[test]
    fn primitive_variants_are_distinct() {
        assert_ne!(Type::I32, Type::I64);
        assert_ne!(Type::F32, Type::F64);
        assert_ne!(Type::String, Type::Null);
        assert_ne!(Type::Bool, Type::I32);
        assert_ne!(Type::Void, Type::Bool);
        assert_ne!(Type::Error, Type::Void);
    }

    #[test]
    fn primitive_variants_hash_consistently() {
        use std::collections::HashSet;
        let mut set = HashSet::new();
        set.insert(Type::I32);
        set.insert(Type::I32);
        set.insert(Type::I64);
        assert_eq!(set.len(), 2);
    }

    #[test]
    fn optional_equality_depends_on_inner_and_nullish() {
        let a = Type::Optional {
            inner: TypeId::from_raw(7),
            nullish: NullishKinds::NULL_ONLY,
        };
        let b = Type::Optional {
            inner: TypeId::from_raw(7),
            nullish: NullishKinds::NULL_ONLY,
        };
        let c = Type::Optional {
            inner: TypeId::from_raw(8),
            nullish: NullishKinds::NULL_ONLY,
        };
        let d = Type::Optional {
            inner: TypeId::from_raw(7),
            nullish: NullishKinds::UNDEFINED_ONLY,
        };
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_ne!(a, d);
    }

    #[test]
    fn struct_equality_depends_only_on_id() {
        let a = Type::Struct {
            id: StructId::from_raw(3),
        };
        let b = Type::Struct {
            id: StructId::from_raw(3),
        };
        let c = Type::Struct {
            id: StructId::from_raw(4),
        };
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn fn_equality_considers_all_components() {
        let p1 = TypeId::from_raw(1);
        let p2 = TypeId::from_raw(2);
        let ret = TypeId::from_raw(3);
        let err = TypeId::from_raw(4);
        let base = Type::Fn {
            params: vec![p1, p2],
            optional: vec![false, true],
            ret,
            err: None,
        };
        assert_eq!(
            base.clone(),
            Type::Fn {
                params: vec![p1, p2],
                optional: vec![false, true],
                ret,
                err: None
            }
        );
        assert_ne!(
            base.clone(),
            Type::Fn {
                params: vec![p1, p2],
                optional: vec![false, true],
                ret,
                err: Some(err)
            }
        );
        assert_ne!(
            base,
            Type::Fn {
                params: vec![p2, p1],
                optional: vec![false, true],
                ret,
                err: None
            }
        );
    }

    #[test]
    fn fn_equality_considers_optional_flags() {
        let p1 = TypeId::from_raw(1);
        let p2 = TypeId::from_raw(2);
        let ret = TypeId::from_raw(3);
        let required = Type::Fn {
            params: vec![p1, p2],
            optional: vec![false, false],
            ret,
            err: None,
        };
        let second_omittable = Type::Fn {
            params: vec![p1, p2],
            optional: vec![false, true],
            ret,
            err: None,
        };
        assert_ne!(
            required, second_omittable,
            "two function types that differ only in which parameters may be omitted must stay \
             distinct: `(a, b)` and `(a, b?)` are not the same signature"
        );
        assert_ne!(
            required,
            Type::Fn {
                params: vec![p1, p2],
                optional: vec![],
                ret,
                err: None
            },
            "an all-`false` vector is not the same value as the empty one, even though both mean \
             \"every parameter is required\""
        );
    }

    #[test]
    fn fn_param_optional_defaults_to_required() {
        let required = Type::Fn {
            params: vec![TypeId::from_raw(1), TypeId::from_raw(2)],
            optional: vec![],
            ret: TypeId::from_raw(3),
            err: None,
        };
        assert!(!required.fn_param_optional(0));
        assert!(!required.fn_param_optional(1));
        assert!(
            !required.fn_param_optional(7),
            "an index past the end must be \"required\", never a panic"
        );

        let flagged = Type::Fn {
            params: vec![TypeId::from_raw(1), TypeId::from_raw(2)],
            optional: vec![true],
            ret: TypeId::from_raw(3),
            err: None,
        };
        assert!(flagged.fn_param_optional(0));
        assert!(
            !flagged.fn_param_optional(1),
            "a short `optional` vector leaves the remaining parameters required"
        );

        assert!(
            !Type::I64.fn_param_optional(0),
            "a non-function type has no parameters"
        );
    }

    #[test]
    fn result_distinguishes_ok_from_err() {
        let ty = Type::Result {
            ok: TypeId::from_raw(1),
            err: TypeId::from_raw(2),
        };
        assert_ne!(
            ty.clone(),
            Type::Result {
                ok: TypeId::from_raw(2),
                err: TypeId::from_raw(1)
            }
        );
    }

    #[test]
    fn union_equality_depends_on_variant_order() {
        let a = Type::Union {
            variants: vec![TypeId::from_raw(1), TypeId::from_raw(2)],
        };
        let b = Type::Union {
            variants: vec![TypeId::from_raw(1), TypeId::from_raw(2)],
        };
        let c = Type::Union {
            variants: vec![TypeId::from_raw(2), TypeId::from_raw(1)],
        };
        assert_eq!(a, b);
        assert_ne!(a, c, "variant order must participate in equality");
    }

    #[test]
    fn union_equality_depends_on_variant_count() {
        let one = Type::Union {
            variants: vec![TypeId::from_raw(1)],
        };
        let two = Type::Union {
            variants: vec![TypeId::from_raw(1), TypeId::from_raw(2)],
        };
        assert_ne!(one, two);
    }

    #[test]
    fn union_distinguishes_from_other_aggregate_variants() {
        let u = Type::Union {
            variants: vec![TypeId::from_raw(1), TypeId::from_raw(2)],
        };
        let p = Type::Promise {
            ok: TypeId::from_raw(1),
            err: Some(TypeId::from_raw(2)),
        };
        let r = Type::Result {
            ok: TypeId::from_raw(1),
            err: TypeId::from_raw(2),
        };
        assert_ne!(u, p);
        assert_ne!(u, r);
    }

    #[test]
    fn intersection_type_id_intern_preserves_part_order() {
        let mut types = TypeTable::new();
        let forward = vec![TypeId::from_raw(1), TypeId::from_raw(2)];
        let reverse = vec![TypeId::from_raw(2), TypeId::from_raw(1)];
        let three_parts = vec![
            TypeId::from_raw(1),
            TypeId::from_raw(2),
            TypeId::from_raw(3),
        ];
        let id_forward = types.intern(&Type::Intersection { parts: forward });
        let id_reverse = types.intern(&Type::Intersection { parts: reverse });
        let id_three_parts = types.intern(&Type::Intersection { parts: three_parts });
        assert_ne!(
            id_forward, id_reverse,
            "TypeTable interning is order-sensitive: A & B and B & A produce distinct TypeIds unless canonicalised upstream (the resolver does this by sorting parts by TypeId::raw() before interning)"
        );
        assert_ne!(
            id_forward, id_three_parts,
            "TypeTable interning is set-sensitive: a superset of parts must produce a distinct TypeId"
        );
    }

    #[test]
    fn intersection_type_id_equality_canonicalises_against_union() {
        let mut types = TypeTable::new();
        let mut sorted = vec![TypeId::from_raw(1), TypeId::from_raw(2)];
        sorted.sort_unstable_by_key(|id| id.raw());
        let id_intersection = types.intern(&Type::Intersection { parts: sorted });
        let id_union = types.intern(&Type::Union {
            variants: vec![TypeId::from_raw(1), TypeId::from_raw(2)],
        });
        assert_ne!(
            id_intersection, id_union,
            "Intersection and Union with same canonicalised parts must remain distinct TypeIds"
        );
    }

    #[test]
    fn intersection_equality_depends_on_part_count() {
        let one = Type::Intersection {
            parts: vec![TypeId::from_raw(1)],
        };
        let two = Type::Intersection {
            parts: vec![TypeId::from_raw(1), TypeId::from_raw(2)],
        };
        assert_ne!(one, two);
    }

    #[test]
    fn empty_intersection_is_a_well_formed_distinct_type() {
        let mut types = TypeTable::new();
        let empty = Type::Intersection { parts: vec![] };
        let id_a = types.intern(&empty);
        let id_b = types.intern(&Type::Intersection { parts: vec![] });
        assert_eq!(
            id_a, id_b,
            "empty Intersection must intern to a stable TypeId (dedup via Type Hash/Eq)"
        );
        assert_ne!(
            id_a,
            types.intern(&Type::Error),
            "empty Intersection must be distinct from the unresolved (error) type"
        );
        assert_ne!(
            id_a,
            types.intern(&Type::Union { variants: vec![] }),
            "empty Intersection must be distinct from empty Union"
        );
        assert_ne!(
            id_a,
            types.intern(&Type::Intersection {
                parts: vec![TypeId::from_raw(1)]
            }),
            "empty Intersection must be distinct from a populated Intersection"
        );
    }

    #[test]
    fn intersection_distinguishes_from_union_and_other_aggregates() {
        let i = Type::Intersection {
            parts: vec![TypeId::from_raw(1), TypeId::from_raw(2)],
        };
        let u = Type::Union {
            variants: vec![TypeId::from_raw(1), TypeId::from_raw(2)],
        };
        let p = Type::Promise {
            ok: TypeId::from_raw(1),
            err: Some(TypeId::from_raw(2)),
        };
        let r = Type::Result {
            ok: TypeId::from_raw(1),
            err: TypeId::from_raw(2),
        };
        assert_ne!(
            i, u,
            "Intersection and Union with the same TypeIds must be distinguishable"
        );
        assert_ne!(i, p);
        assert_ne!(i, r);
    }

    #[test]
    fn tuple_equality_preserves_element_order() {
        let a = Type::Tuple {
            elements: vec![TypeId::from_raw(1), TypeId::from_raw(2)],
        };
        let b = Type::Tuple {
            elements: vec![TypeId::from_raw(1), TypeId::from_raw(2)],
        };
        let c = Type::Tuple {
            elements: vec![TypeId::from_raw(2), TypeId::from_raw(1)],
        };
        assert_eq!(a, b);
        assert_ne!(
            a, c,
            "Tuple element order is positional: [A, B] must not equal [B, A]"
        );
    }

    #[test]
    fn tuple_equality_depends_on_element_count() {
        let one = Type::Tuple {
            elements: vec![TypeId::from_raw(1)],
        };
        let two = Type::Tuple {
            elements: vec![TypeId::from_raw(1), TypeId::from_raw(2)],
        };
        assert_ne!(one, two);
    }

    #[test]
    fn tuple_distinguishes_from_array_and_other_aggregates() {
        let t = Type::Tuple {
            elements: vec![TypeId::from_raw(1), TypeId::from_raw(2)],
        };
        let a = Type::Array {
            element: TypeId::from_raw(1),
        };
        let u = Type::Union {
            variants: vec![TypeId::from_raw(1), TypeId::from_raw(2)],
        };
        let i = Type::Intersection {
            parts: vec![TypeId::from_raw(1), TypeId::from_raw(2)],
        };
        assert_ne!(
            t, a,
            "Tuple [A, B] is fixed-length heterogeneous; Array T is dynamic-length homogeneous"
        );
        assert_ne!(t, u);
        assert_ne!(t, i);
    }

    #[test]
    fn empty_tuple_is_distinct_from_void_by_design() {
        let mut types = TypeTable::new();
        let id_empty = types.intern(&Type::Tuple { elements: vec![] });
        let id_void = types.intern(&Type::Void);
        let id_error = types.intern(&Type::Error);
        let id_again = types.intern(&Type::Tuple { elements: vec![] });
        assert_ne!(
            id_empty, id_void,
            "TypeScript `[]` (empty tuple) is semantically a zero-length tuple, not `void` (no value) — they must remain distinct TypeIds"
        );
        assert_ne!(
            id_empty, id_error,
            "empty tuple must also be distinct from the unresolved (error) type"
        );
        assert_eq!(
            id_empty, id_again,
            "interning two empty Tuples must yield the same TypeId (dedup)"
        );
    }

    #[test]
    fn generator_equality_depends_on_inner_type() {
        let a = Type::Generator {
            inner: TypeId::from_raw(7),
            is_async: false,
        };
        let b = Type::Generator {
            inner: TypeId::from_raw(7),
            is_async: false,
        };
        let c = Type::Generator {
            inner: TypeId::from_raw(8),
            is_async: false,
        };
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn generator_result_equality_depends_on_inner_type() {
        let a = Type::GeneratorResult {
            inner: TypeId::from_raw(1),
        };
        let b = Type::GeneratorResult {
            inner: TypeId::from_raw(1),
        };
        let c = Type::GeneratorResult {
            inner: TypeId::from_raw(2),
        };
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_ne!(
            a,
            Type::Generator {
                inner: TypeId::from_raw(1),
                is_async: false
            },
            "Generator and GeneratorResult must remain distinct"
        );
    }

    #[test]
    fn named_equality_depends_only_on_symbol() {
        let a = Type::Named {
            symbol: Atom::from("Foo"),
        };
        let b = Type::Named {
            symbol: Atom::from("Foo"),
        };
        let c = Type::Named {
            symbol: Atom::from("Bar"),
        };
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn memory_kind_is_copy_and_distinct() {
        let kinds = [
            MemoryKind::CopyValue,
            MemoryKind::ValueStruct,
            MemoryKind::GcRef,
            MemoryKind::NullableGcRef,
            MemoryKind::LinearMemoryValue,
        ];
        for (i, a) in kinds.iter().enumerate() {
            for (j, b) in kinds.iter().enumerate() {
                assert_eq!(i == j, a == b);
            }
        }
    }

    #[test]
    fn type_carries_indirection_ids_without_compile_error() {
        let _ = Type::Array {
            element: TypeId::from_raw(0),
        };
        let _ = Type::Promise {
            ok: TypeId::from_raw(1),
            err: None,
        };
        let _ = Type::Optional {
            inner: TypeId::from_raw(2),
            nullish: NullishKinds::BOTH,
        };
        let _ = FunctionId::from_raw(0);
        let _ = LocalId::from_raw(0);
    }
}
