use super::super::{emit_decls, emit_function};
use super::{empty_func, norm, opt_eq_body, optional_binary_func};
use ts_aot_core::{Atom, FunctionId, LocalId, ModuleId, Type, TypeTable};
use ts_aot_ir_mir::{BinaryOp, MirDecl, MirExpr, MirParam, MirProgram};

#[test]
fn eq_null_on_optional_number_emits_is_none() {
    let mut types = TypeTable::new();
    let f64_ty = types.intern(&Type::F64);
    let opt_ty = types.intern(&Type::Optional {
        inner: f64_ty,
        nullish: ts_aot_core::NullishKinds::BOTH,
    });
    let bool_ty = types.intern(&Type::Bool);
    let mut f = optional_binary_func("isNull", opt_ty, bool_ty);
    f.body = opt_eq_body(
        BinaryOp::Eq,
        MirExpr::Local(LocalId::from_raw(0)),
        MirExpr::Null { ty: opt_ty },
        bool_ty,
    );
    let s = emit_function(&f, &types)
        .expect("function should emit")
        .to_string();
    assert!(norm(&s).contains("(x).is_none()"), "got: {s}");
    assert!(!norm(&s).contains("==()"), "got: {s}");
}

#[test]
fn ne_null_on_optional_number_emits_is_some() {
    let mut types = TypeTable::new();
    let f64_ty = types.intern(&Type::F64);
    let opt_ty = types.intern(&Type::Optional {
        inner: f64_ty,
        nullish: ts_aot_core::NullishKinds::BOTH,
    });
    let bool_ty = types.intern(&Type::Bool);
    let mut f = optional_binary_func("isNotNull", opt_ty, bool_ty);
    f.body = opt_eq_body(
        BinaryOp::Ne,
        MirExpr::Local(LocalId::from_raw(0)),
        MirExpr::Null { ty: opt_ty },
        bool_ty,
    );
    let s = emit_function(&f, &types)
        .expect("function should emit")
        .to_string();
    assert!(norm(&s).contains("(x).is_some()"), "got: {s}");
    assert!(!norm(&s).contains("!=()"), "got: {s}");
}

#[test]
fn eq_null_on_optional_string_emits_is_none() {
    let mut types = TypeTable::new();
    let string_ty = types.intern(&Type::String);
    let opt_ty = types.intern(&Type::Optional {
        inner: string_ty,
        nullish: ts_aot_core::NullishKinds::BOTH,
    });
    let bool_ty = types.intern(&Type::Bool);
    let mut f = optional_binary_func("isNullStr", opt_ty, bool_ty);
    f.body = opt_eq_body(
        BinaryOp::Eq,
        MirExpr::Local(LocalId::from_raw(0)),
        MirExpr::Null { ty: opt_ty },
        bool_ty,
    );
    let s = emit_function(&f, &types)
        .expect("function should emit")
        .to_string();
    assert!(norm(&s).contains("(x).is_none()"), "got: {s}");
}

#[test]
fn null_literal_on_the_left_emits_is_none() {
    let mut types = TypeTable::new();
    let f64_ty = types.intern(&Type::F64);
    let opt_ty = types.intern(&Type::Optional {
        inner: f64_ty,
        nullish: ts_aot_core::NullishKinds::BOTH,
    });
    let bool_ty = types.intern(&Type::Bool);
    let mut f = optional_binary_func("rev", opt_ty, bool_ty);
    f.body = opt_eq_body(
        BinaryOp::Eq,
        MirExpr::Null { ty: opt_ty },
        MirExpr::Local(LocalId::from_raw(0)),
        bool_ty,
    );
    let s = emit_function(&f, &types)
        .expect("function should emit")
        .to_string();
    assert!(norm(&s).contains("(x).is_none()"), "got: {s}");
}

#[test]
fn eq_undefined_on_optional_number_emits_is_none_like_null() {
    let mut types = TypeTable::new();
    let f64_ty = types.intern(&Type::F64);
    let opt_ty = types.intern(&Type::Optional {
        inner: f64_ty,
        nullish: ts_aot_core::NullishKinds::BOTH,
    });
    let bool_ty = types.intern(&Type::Bool);
    let mut f = optional_binary_func("isUndef", opt_ty, bool_ty);
    f.body = opt_eq_body(
        BinaryOp::Eq,
        MirExpr::Local(LocalId::from_raw(0)),
        MirExpr::Unit,
        bool_ty,
    );
    let s = emit_function(&f, &types)
        .expect("function should emit")
        .to_string();
    assert!(norm(&s).contains("(x).is_none()"), "got: {s}");
}

#[test]
fn eq_of_two_optionals_without_a_literal_is_unchanged() {
    let mut types = TypeTable::new();
    let f64_ty = types.intern(&Type::F64);
    let opt_ty = types.intern(&Type::Optional {
        inner: f64_ty,
        nullish: ts_aot_core::NullishKinds::BOTH,
    });
    let bool_ty = types.intern(&Type::Bool);
    let mut f = optional_binary_func("bothOpt", opt_ty, bool_ty);
    f.params.push(MirParam {
        id: LocalId::from_raw(1),
        name: Atom::from("y"),
        ty: opt_ty,
    });
    f.body = opt_eq_body(
        BinaryOp::Eq,
        MirExpr::Local(LocalId::from_raw(0)),
        MirExpr::Local(LocalId::from_raw(1)),
        bool_ty,
    );
    let s = emit_function(&f, &types)
        .expect("function should emit")
        .to_string();
    assert!(!norm(&s).contains("is_none"), "got: {s}");
    assert!(norm(&s).contains("x==y"), "got: {s}");
}

#[test]
fn eq_null_with_generic_operand_is_unchanged() {
    let mut types = TypeTable::new();
    let generic_ty = types.intern(&Type::GenericParam {
        id: ts_aot_core::GenericParamId::from_raw(0),
    });
    let bool_ty = types.intern(&Type::Bool);
    let mut f = optional_binary_func("genericOp", generic_ty, bool_ty);
    f.body = opt_eq_body(
        BinaryOp::Eq,
        MirExpr::Local(LocalId::from_raw(0)),
        MirExpr::Null { ty: generic_ty },
        bool_ty,
    );
    let s = emit_function(&f, &types)
        .expect("function should emit")
        .to_string();
    assert!(
        norm(&s).contains("returnx==()"),
        "a generic operand has no dedicated nullish representation, so `x == null` must stay a \
         comparison against the nullish operand inside the return expression instead of folding to \
         a constant; got: {s}"
    );
    assert!(!norm(&s).contains("is_none"), "got: {s}");
}

#[test]
fn eq_null_with_call_operand_folds_but_keeps_the_call() {
    let mut types = TypeTable::new();
    let f64_ty = types.intern(&Type::F64);
    let bool_ty = types.intern(&Type::Bool);
    let mut side_effect = empty_func("side_effect");
    side_effect.id = FunctionId::from_raw(5);
    side_effect.ret = f64_ty;
    let mut f = empty_func("callOp");
    f.ret = bool_ty;
    f.body = opt_eq_body(
        BinaryOp::Eq,
        MirExpr::Call {
            callee: FunctionId::from_raw(5),
            args: Vec::new(),
            ty: f64_ty,
        },
        MirExpr::Null { ty: f64_ty },
        bool_ty,
    );
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(side_effect));
    prog.push_decl(MirDecl::Function(f));
    let s = emit_decls(&prog, &types)
        .expect("function should emit")
        .to_string();
    let body = norm(
        s.split("fn callOp")
            .nth(1)
            .unwrap_or_else(|| panic!("`callOp` must be emitted, got: {s}")),
    );
    let return_at = body
        .find("return")
        .unwrap_or_else(|| panic!("the folded comparison must still be returned, got: {body}"));
    let call_at = body
        .find("side_effect(__ctx)")
        .unwrap_or_else(|| panic!("the call operand must still be evaluated, got: {body}"));
    let Some(false_at) = body[call_at..].find("false").map(|offset| call_at + offset) else {
        panic!("the folded comparison must yield `false`, got: {body}");
    };
    assert!(
        return_at < call_at && call_at < false_at,
        "the returned expression must evaluate the call operand before yielding the folded \
         `false`; got: {body}"
    );
    assert_eq!(
        body.matches("side_effect(__ctx)").count(),
        1,
        "the call operand must be evaluated exactly once; got: {body}"
    );
    assert!(!body.contains("==()"), "got: {body}");
}
