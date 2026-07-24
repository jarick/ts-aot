use std::collections::{HashMap, HashSet};

use ts_aot_core::{Atom, GenericParamId, LocalId, Span, StructId, Type, TypeId, TypeTable};
use ts_aot_ir_hir::{HirExpr, HirFunction, HirParam};

use super::infer::{
    bind_param_ty_resolved, build_mapping, format_type_args, hir_expr_ty, infer_type_args,
    type_resolved,
};

#[test]
fn format_type_args_empty_returns_empty_string() {
    assert_eq!(format_type_args(&[]), "");
}

#[test]
fn format_type_args_single_yields_t_prefixed_raw() {
    let mut types = TypeTable::new();
    let t = types.intern(&Type::I32);
    assert_eq!(format_type_args(&[t]), format!("_t{}", t.raw()));
}

#[test]
fn format_type_args_multiple_joined_with_underscores() {
    let mut types = TypeTable::new();
    let a = types.intern(&Type::I32);
    let b = types.intern(&Type::Bool);
    assert_eq!(
        format_type_args(&[a, b]),
        format!("_t{}_t{}", a.raw(), b.raw())
    );
}

fn make_fn(
    name: &str,
    params: Vec<HirParam>,
    ret: TypeId,
    type_params: Vec<GenericParamId>,
) -> HirFunction {
    HirFunction {
        name: Atom::from(name),
        params,
        ret,
        throws: None,
        body: vec![],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params,
        async_info: None,
    }
}

#[test]
fn build_mapping_empty_when_no_type_params() {
    let f = make_fn("f", vec![], TypeId::from_raw(0), vec![]);
    let mapping = build_mapping(&f, &[]);
    assert!(mapping.is_empty());
}

#[test]
fn build_mapping_pairs_each_type_param_with_type_arg() {
    let mut types = TypeTable::new();
    let a = types.intern(&Type::I32);
    let b = types.intern(&Type::Bool);
    let gp0 = GenericParamId::from_raw(0);
    let gp1 = GenericParamId::from_raw(1);
    let f = make_fn("f", vec![], a, vec![gp0, gp1]);
    let mapping = build_mapping(&f, &[a, b]);
    assert_eq!(mapping.get(&gp0), Some(&a));
    assert_eq!(mapping.get(&gp1), Some(&b));
    assert_eq!(mapping.len(), 2);
}

#[test]
fn type_resolved_true_for_leaf_types() {
    let mut types = TypeTable::new();
    for variant in [
        Type::Void,
        Type::Never,
        Type::Bool,
        Type::I32,
        Type::F64,
        Type::String,
        Type::Null,
    ] {
        let id = types.intern(&variant);
        assert!(type_resolved(id, &types), "leaf {variant:?} should resolve");
    }
}

#[test]
fn type_resolved_false_for_generic_param() {
    let mut types = TypeTable::new();
    let gp = types.intern(&Type::GenericParam {
        id: GenericParamId::from_raw(0),
    });
    assert!(!type_resolved(gp, &types));
}

#[test]
fn type_resolved_recurses_through_composites() {
    let mut types = TypeTable::new();
    let gp = types.intern(&Type::GenericParam {
        id: GenericParamId::from_raw(0),
    });
    let i32 = types.intern(&Type::I32);
    let str_ty = types.intern(&Type::String);

    let unresolved = [
        types.intern(&Type::Optional { inner: gp }),
        types.intern(&Type::Array { element: gp }),
        types.intern(&Type::Fn {
            params: vec![i32],
            ret: gp,
            err: None,
        }),
        types.intern(&Type::Promise { ok: gp, err: None }),
        types.intern(&Type::Result { ok: i32, err: gp }),
    ];
    for ty in unresolved {
        assert!(
            !type_resolved(ty, &types),
            "composite with inner GenericParam must not resolve"
        );
    }

    let resolved = [
        types.intern(&Type::Optional { inner: i32 }),
        types.intern(&Type::Array { element: i32 }),
        types.intern(&Type::Fn {
            params: vec![i32],
            ret: i32,
            err: None,
        }),
        types.intern(&Type::Promise {
            ok: i32,
            err: Some(str_ty),
        }),
        types.intern(&Type::Result {
            ok: i32,
            err: str_ty,
        }),
    ];
    for ty in resolved {
        assert!(
            type_resolved(ty, &types),
            "composite with all-resolved inner must resolve"
        );
    }
}

#[test]
fn hir_expr_ty_returns_inner_ty_for_typed_variants() {
    let mut types = TypeTable::new();
    let i32 = types.intern(&Type::I32);
    let bool_ty = types.intern(&Type::Bool);
    let i64 = types.intern(&Type::I64);
    let null_ty = types.intern(&Type::Null);

    let local = HirExpr::Local {
        id: LocalId::from_raw(0),
        ty: i32,

        span: Span::default(),
    };
    assert_eq!(hir_expr_ty(&local, &mut types), Some(i32));

    let global = HirExpr::Global {
        name: Atom::from("g"),
        ty: i32,

        span: Span::default(),
    };
    assert_eq!(hir_expr_ty(&global, &mut types), Some(i32));

    assert_eq!(
        hir_expr_ty(&HirExpr::Bool(true, Span::default()), &mut types),
        Some(bool_ty)
    );
    assert_eq!(
        hir_expr_ty(&HirExpr::Int(0, Span::default()), &mut types),
        Some(i64)
    );
    assert_eq!(
        hir_expr_ty(&HirExpr::Null(Span::default()), &mut types),
        Some(null_ty)
    );
}

#[test]
fn hir_expr_ty_returns_none_for_unit_and_undefined() {
    let mut types = TypeTable::new();
    assert_eq!(
        hir_expr_ty(&HirExpr::Unit(Span::default()), &mut types),
        None
    );
    assert_eq!(
        hir_expr_ty(&HirExpr::Undefined(Span::default()), &mut types),
        None
    );
}

#[test]
fn hir_expr_ty_returns_target_for_type_assertion() {
    let mut types = TypeTable::new();
    let i32 = types.intern(&Type::I32);
    let f64 = types.intern(&Type::F64);
    let expr = HirExpr::TypeAssertion {
        expr: Box::new(HirExpr::Local {
            id: LocalId::from_raw(0),
            ty: i32,

            span: Span::default(),
        }),
        target: f64,

        span: Span::default(),
    };
    assert_eq!(hir_expr_ty(&expr, &mut types), Some(f64));
}

#[test]
fn bind_param_ty_resolved_peels_optional() {
    let mut types = TypeTable::new();
    let gp = GenericParamId::from_raw(0);
    let i32 = types.intern(&Type::I32);
    let mut found = HashMap::new();
    let param = Type::Optional {
        inner: types.intern(&Type::GenericParam { id: gp }),
    };
    let arg = types.intern(&Type::Optional { inner: i32 });
    bind_param_ty_resolved(param, Some(arg), &mut found, &mut HashSet::new(), &types);
    assert_eq!(found.get(&gp), Some(&i32));
}

#[test]
fn bind_param_ty_resolved_peels_promise_with_both_type_params() {
    let mut types = TypeTable::new();
    let gp0 = GenericParamId::from_raw(0);
    let gp1 = GenericParamId::from_raw(1);
    let i32 = types.intern(&Type::I32);
    let str_ty = types.intern(&Type::String);
    let mut found = HashMap::new();
    let param = Type::Promise {
        ok: types.intern(&Type::GenericParam { id: gp0 }),
        err: Some(types.intern(&Type::GenericParam { id: gp1 })),
    };
    let arg = types.intern(&Type::Promise {
        ok: i32,
        err: Some(str_ty),
    });
    bind_param_ty_resolved(param, Some(arg), &mut found, &mut HashSet::new(), &types);
    assert_eq!(found.get(&gp0), Some(&i32));
    assert_eq!(found.get(&gp1), Some(&str_ty));
}

#[test]
fn bind_param_ty_resolved_peels_result_with_both_components() {
    let mut types = TypeTable::new();
    let gp0 = GenericParamId::from_raw(0);
    let gp1 = GenericParamId::from_raw(1);
    let i32 = types.intern(&Type::I32);
    let str_ty = types.intern(&Type::String);
    let mut found = HashMap::new();
    let param = Type::Result {
        ok: types.intern(&Type::GenericParam { id: gp0 }),
        err: types.intern(&Type::GenericParam { id: gp1 }),
    };
    let arg = types.intern(&Type::Result {
        ok: i32,
        err: str_ty,
    });
    bind_param_ty_resolved(param, Some(arg), &mut found, &mut HashSet::new(), &types);
    assert_eq!(found.get(&gp0), Some(&i32));
    assert_eq!(found.get(&gp1), Some(&str_ty));
}

#[test]
fn bind_param_ty_resolved_peels_fn_params_and_ret() {
    let mut types = TypeTable::new();
    let gp0 = GenericParamId::from_raw(0);
    let gp1 = GenericParamId::from_raw(1);
    let i32 = types.intern(&Type::I32);
    let bool_ty = types.intern(&Type::Bool);
    let mut found = HashMap::new();
    let param = Type::Fn {
        params: vec![types.intern(&Type::GenericParam { id: gp0 })],
        ret: types.intern(&Type::GenericParam { id: gp1 }),
        err: None,
    };
    let arg = types.intern(&Type::Fn {
        params: vec![i32],
        ret: bool_ty,
        err: None,
    });
    bind_param_ty_resolved(param, Some(arg), &mut found, &mut HashSet::new(), &types);
    assert_eq!(found.get(&gp0), Some(&i32));
    assert_eq!(found.get(&gp1), Some(&bool_ty));
}

#[test]
fn bind_param_ty_resolved_struct_param_does_not_bind() {
    let mut types = TypeTable::new();
    let i32 = types.intern(&Type::I32);
    let mut found = HashMap::new();
    bind_param_ty_resolved(
        Type::Struct {
            id: StructId::from_raw(0),
        },
        Some(i32),
        &mut found,
        &mut HashSet::new(),
        &types,
    );
    assert!(found.is_empty(), "Type::Struct param must fall through");
}

#[test]
fn bind_param_ty_resolved_conflicting_generic_param_binds_resolves_to_empty() {
    let mut types = TypeTable::new();
    let gp = GenericParamId::from_raw(0);
    let i32 = types.intern(&Type::I32);
    let str_ty = types.intern(&Type::String);
    let mut found = HashMap::new();
    let mut conflicted = HashSet::new();
    let generic_param = Type::GenericParam { id: gp };

    bind_param_ty_resolved(
        generic_param.clone(),
        Some(i32),
        &mut found,
        &mut conflicted,
        &types,
    );
    assert_eq!(found.get(&gp), Some(&i32));

    bind_param_ty_resolved(
        generic_param,
        Some(str_ty),
        &mut found,
        &mut conflicted,
        &types,
    );
    assert!(
        !found.contains_key(&gp),
        "conflicting binding must drop the entry so the final mapping stays unresolved"
    );
}

#[test]
fn bind_param_ty_resolved_identical_generic_param_binds_are_idempotent() {
    let mut types = TypeTable::new();
    let gp = GenericParamId::from_raw(0);
    let i32 = types.intern(&Type::I32);
    let mut found = HashMap::new();
    let mut conflicted = HashSet::new();
    let generic_param = Type::GenericParam { id: gp };

    bind_param_ty_resolved(
        generic_param.clone(),
        Some(i32),
        &mut found,
        &mut conflicted,
        &types,
    );
    bind_param_ty_resolved(
        generic_param,
        Some(i32),
        &mut found,
        &mut conflicted,
        &types,
    );
    assert_eq!(
        found.get(&gp),
        Some(&i32),
        "identical rebind must not evict"
    );
}

#[test]
fn infer_type_args_with_multiple_type_params() {
    let mut types = TypeTable::new();
    let ret_ty = TypeId::from_raw(0);
    let gp0 = GenericParamId::from_raw(0);
    let gp1 = GenericParamId::from_raw(1);
    let f = make_fn(
        "f",
        vec![
            HirParam {
                name: Atom::from("a"),
                ty: types.intern(&Type::GenericParam { id: gp0 }),
            },
            HirParam {
                name: Atom::from("b"),
                ty: types.intern(&Type::GenericParam { id: gp1 }),
            },
        ],
        ret_ty,
        vec![gp0, gp1],
    );
    let args = vec![
        HirExpr::Int(1, Span::default()),
        HirExpr::Bool(true, Span::default()),
    ];
    let inferred = infer_type_args(&f, &args, &mut types);
    assert_eq!(inferred.len(), 2);
    let expected_i64 = types.intern(&Type::I64);
    let expected_bool = types.intern(&Type::Bool);
    assert_eq!(
        inferred[0], expected_i64,
        "first param binds to I64 from Int arg"
    );
    assert_eq!(
        inferred[1], expected_bool,
        "second param binds to Bool from Bool arg"
    );
}

#[test]
fn bind_param_ty_resolved_conflict_does_not_resurrect_on_later_match() {
    let mut types = TypeTable::new();
    let gp = GenericParamId::from_raw(0);
    let i32 = types.intern(&Type::I32);
    let str_ty = types.intern(&Type::String);
    let mut found = HashMap::new();
    let mut conflicted = HashSet::new();
    let generic_param = Type::GenericParam { id: gp };

    bind_param_ty_resolved(
        generic_param.clone(),
        Some(i32),
        &mut found,
        &mut conflicted,
        &types,
    );
    bind_param_ty_resolved(
        generic_param.clone(),
        Some(str_ty),
        &mut found,
        &mut conflicted,
        &types,
    );
    assert!(!found.contains_key(&gp), "second bind must drop the entry");

    bind_param_ty_resolved(
        generic_param,
        Some(i32),
        &mut found,
        &mut conflicted,
        &types,
    );
    assert!(
        !found.contains_key(&gp),
        "after conflict, a later matching bind must NOT resurrect the binding"
    );
}

#[test]
fn infer_type_args_conflict_blocks_single_param_arg_fallback() {
    let mut types = TypeTable::new();
    let gp = GenericParamId::from_raw(0);
    let i32 = types.intern(&Type::I32);
    let f = make_fn(
        "f",
        vec![
            HirParam {
                name: Atom::from("a"),
                ty: types.intern(&Type::GenericParam { id: gp }),
            },
            HirParam {
                name: Atom::from("b"),
                ty: types.intern(&Type::GenericParam { id: gp }),
            },
        ],
        i32,
        vec![gp],
    );
    let args = vec![
        HirExpr::Int(1, Span::default()),
        HirExpr::String(Atom::from("x"), Span::default()),
    ];
    let inferred = infer_type_args(&f, &args, &mut types);
    assert_eq!(inferred.len(), 1);
    let expected_gp = types.intern(&Type::GenericParam { id: gp });
    assert_eq!(
        inferred[0], expected_gp,
        "single-type-param conflict must skip the args[0] fallback and stay unresolved"
    );
}
