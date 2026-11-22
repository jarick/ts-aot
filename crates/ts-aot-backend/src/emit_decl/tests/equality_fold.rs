use super::super::emit_function;
use super::{empty_func, norm, opt_eq_body, optional_binary_func};
use ts_aot_core::{LocalId, Type, TypeTable};
use ts_aot_ir_mir::{BinaryOp, MirExpr};

#[test]
fn loose_eq_of_two_nullish_literals_folds_to_true() {
    let mut types = TypeTable::new();
    let f64_ty = types.intern(&Type::F64);
    let opt_ty = types.intern(&Type::Optional {
        inner: f64_ty,
        nullish: ts_aot_core::NullishKinds::BOTH,
    });
    let bool_ty = types.intern(&Type::Bool);
    let mut f = empty_func("bothNull");
    f.ret = bool_ty;
    f.body = opt_eq_body(
        BinaryOp::Eq,
        MirExpr::Null { ty: opt_ty },
        MirExpr::Unit,
        bool_ty,
    );
    let s = emit_function(&f, &types)
        .expect("function should emit")
        .to_string();
    assert!(!norm(&s).contains("is_none"), "got: {s}");
    assert!(norm(&s).contains("returntrue"), "got: {s}");
    assert!(!norm(&s).contains("==()"), "got: {s}");
}

#[test]
fn strict_eq_of_two_nullish_literals_folds_to_false() {
    let mut types = TypeTable::new();
    let f64_ty = types.intern(&Type::F64);
    let opt_ty = types.intern(&Type::Optional {
        inner: f64_ty,
        nullish: ts_aot_core::NullishKinds::BOTH,
    });
    let bool_ty = types.intern(&Type::Bool);
    let mut f = empty_func("bothNullStrict");
    f.ret = bool_ty;
    f.body = opt_eq_body(
        BinaryOp::StrictEq,
        MirExpr::Null { ty: opt_ty },
        MirExpr::Unit,
        bool_ty,
    );
    let s = emit_function(&f, &types)
        .expect("function should emit")
        .to_string();
    assert!(norm(&s).contains("returnfalse"), "got: {s}");
    assert!(!norm(&s).contains("==()"), "got: {s}");
}

#[test]
fn eq_undefined_with_non_optional_number_folds_to_false() {
    let mut types = TypeTable::new();
    let f64_ty = types.intern(&Type::F64);
    let bool_ty = types.intern(&Type::Bool);
    let mut f = optional_binary_func("plainUndef", f64_ty, bool_ty);
    f.body = opt_eq_body(
        BinaryOp::Eq,
        MirExpr::Local(LocalId::from_raw(0)),
        MirExpr::Unit,
        bool_ty,
    );
    let s = emit_function(&f, &types)
        .expect("function should emit")
        .to_string();
    assert!(norm(&s).contains("returnfalse"), "got: {s}");
    assert!(!norm(&s).contains("==()"), "got: {s}");
    assert!(!norm(&s).contains("is_none"), "got: {s}");
}

#[test]
fn eq_null_with_non_optional_number_folds_to_false() {
    let mut types = TypeTable::new();
    let f64_ty = types.intern(&Type::F64);
    let bool_ty = types.intern(&Type::Bool);
    let mut f = optional_binary_func("plain", f64_ty, bool_ty);
    f.body = opt_eq_body(
        BinaryOp::Eq,
        MirExpr::Local(LocalId::from_raw(0)),
        MirExpr::Null { ty: f64_ty },
        bool_ty,
    );
    let s = emit_function(&f, &types)
        .expect("function should emit")
        .to_string();
    assert!(norm(&s).contains("returnfalse"), "got: {s}");
    assert!(!norm(&s).contains("==()"), "got: {s}");
    assert!(!norm(&s).contains("is_none"), "got: {s}");
}

#[test]
fn ne_null_with_non_optional_number_folds_to_true() {
    let mut types = TypeTable::new();
    let f64_ty = types.intern(&Type::F64);
    let bool_ty = types.intern(&Type::Bool);
    let mut f = optional_binary_func("plainNe", f64_ty, bool_ty);
    f.body = opt_eq_body(
        BinaryOp::Ne,
        MirExpr::Local(LocalId::from_raw(0)),
        MirExpr::Null { ty: f64_ty },
        bool_ty,
    );
    let s = emit_function(&f, &types)
        .expect("function should emit")
        .to_string();
    assert!(norm(&s).contains("returntrue"), "got: {s}");
    assert!(!norm(&s).contains("!=()"), "got: {s}");
}

#[test]
fn eq_null_with_non_optional_string_folds_to_false() {
    let mut types = TypeTable::new();
    let string_ty = types.intern(&Type::String);
    let bool_ty = types.intern(&Type::Bool);
    let mut f = optional_binary_func("plainStr", string_ty, bool_ty);
    f.body = opt_eq_body(
        BinaryOp::Eq,
        MirExpr::Local(LocalId::from_raw(0)),
        MirExpr::Null { ty: string_ty },
        bool_ty,
    );
    let s = emit_function(&f, &types)
        .expect("function should emit")
        .to_string();
    assert!(norm(&s).contains("returnfalse"), "got: {s}");
    assert!(!norm(&s).contains("==()"), "got: {s}");
}

#[test]
fn eq_null_with_null_typed_operand_folds_to_true() {
    let mut types = TypeTable::new();
    let null_ty = types.intern(&Type::Null);
    let bool_ty = types.intern(&Type::Bool);
    let mut f = optional_binary_func("nullOp", null_ty, bool_ty);
    f.body = opt_eq_body(
        BinaryOp::Eq,
        MirExpr::Local(LocalId::from_raw(0)),
        MirExpr::Null { ty: null_ty },
        bool_ty,
    );
    let s = emit_function(&f, &types)
        .expect("function should emit")
        .to_string();
    assert!(norm(&s).contains("returntrue"), "got: {s}");
    assert!(!norm(&s).contains("==()"), "got: {s}");
}

#[test]
fn loose_eq_undefined_with_null_typed_operand_folds_to_true() {
    let mut types = TypeTable::new();
    let null_ty = types.intern(&Type::Null);
    let bool_ty = types.intern(&Type::Bool);
    let mut f = optional_binary_func("nullUndefOp", null_ty, bool_ty);
    f.body = opt_eq_body(
        BinaryOp::Eq,
        MirExpr::Local(LocalId::from_raw(0)),
        MirExpr::Unit,
        bool_ty,
    );
    let s = emit_function(&f, &types)
        .expect("function should emit")
        .to_string();
    assert!(norm(&s).contains("returntrue"), "got: {s}");
}

#[test]
fn strict_eq_undefined_with_null_typed_operand_folds_to_false() {
    let mut types = TypeTable::new();
    let null_ty = types.intern(&Type::Null);
    let bool_ty = types.intern(&Type::Bool);
    let mut f = optional_binary_func("nullUndefOpStrict", null_ty, bool_ty);
    f.body = opt_eq_body(
        BinaryOp::StrictEq,
        MirExpr::Local(LocalId::from_raw(0)),
        MirExpr::Unit,
        bool_ty,
    );
    let s = emit_function(&f, &types)
        .expect("function should emit")
        .to_string();
    assert!(norm(&s).contains("returnfalse"), "got: {s}");
}

#[test]
fn loose_eq_null_with_undefined_typed_operand_folds_to_true() {
    let mut types = TypeTable::new();
    let void_ty = types.intern(&Type::Void);
    let bool_ty = types.intern(&Type::Bool);
    let mut f = optional_binary_func("voidOp", void_ty, bool_ty);
    f.body = opt_eq_body(
        BinaryOp::Eq,
        MirExpr::Local(LocalId::from_raw(0)),
        MirExpr::Null { ty: void_ty },
        bool_ty,
    );
    let s = emit_function(&f, &types)
        .expect("function should emit")
        .to_string();
    assert!(norm(&s).contains("returntrue"), "got: {s}");
    assert!(!norm(&s).contains("==()"), "got: {s}");
}

#[test]
fn strict_eq_null_with_undefined_typed_operand_folds_to_false() {
    let mut types = TypeTable::new();
    let void_ty = types.intern(&Type::Void);
    let bool_ty = types.intern(&Type::Bool);
    let mut f = optional_binary_func("voidOpStrict", void_ty, bool_ty);
    f.body = opt_eq_body(
        BinaryOp::StrictEq,
        MirExpr::Local(LocalId::from_raw(0)),
        MirExpr::Null { ty: void_ty },
        bool_ty,
    );
    let s = emit_function(&f, &types)
        .expect("function should emit")
        .to_string();
    assert!(norm(&s).contains("returnfalse"), "got: {s}");
    assert!(!norm(&s).contains("==()"), "got: {s}");
}

#[test]
fn strict_eq_null_with_non_optional_number_folds_to_false() {
    let mut types = TypeTable::new();
    let f64_ty = types.intern(&Type::F64);
    let bool_ty = types.intern(&Type::Bool);
    let mut f = optional_binary_func("plainStrict", f64_ty, bool_ty);
    f.body = opt_eq_body(
        BinaryOp::StrictEq,
        MirExpr::Local(LocalId::from_raw(0)),
        MirExpr::Null { ty: f64_ty },
        bool_ty,
    );
    let s = emit_function(&f, &types)
        .expect("function should emit")
        .to_string();
    assert!(norm(&s).contains("returnfalse"), "got: {s}");
    assert!(!norm(&s).contains("==()"), "got: {s}");
}

#[test]
fn strict_ne_null_with_non_optional_number_folds_to_true() {
    let mut types = TypeTable::new();
    let f64_ty = types.intern(&Type::F64);
    let bool_ty = types.intern(&Type::Bool);
    let mut f = optional_binary_func("plainStrictNe", f64_ty, bool_ty);
    f.body = opt_eq_body(
        BinaryOp::StrictNe,
        MirExpr::Local(LocalId::from_raw(0)),
        MirExpr::Null { ty: f64_ty },
        bool_ty,
    );
    let s = emit_function(&f, &types)
        .expect("function should emit")
        .to_string();
    assert!(norm(&s).contains("returntrue"), "got: {s}");
    assert!(!norm(&s).contains("!=()"), "got: {s}");
}

#[test]
fn eq_undefined_with_undefined_typed_operand_folds_to_true() {
    let mut types = TypeTable::new();
    let void_ty = types.intern(&Type::Void);
    let bool_ty = types.intern(&Type::Bool);
    let mut f = optional_binary_func("voidUndefOp", void_ty, bool_ty);
    f.body = opt_eq_body(
        BinaryOp::Eq,
        MirExpr::Local(LocalId::from_raw(0)),
        MirExpr::Unit,
        bool_ty,
    );
    let s = emit_function(&f, &types)
        .expect("function should emit")
        .to_string();
    assert!(norm(&s).contains("returntrue"), "got: {s}");
}
