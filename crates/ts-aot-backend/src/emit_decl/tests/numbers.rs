use super::super::emit_decls;
use super::empty_func;
use ts_aot_core::{Atom, LocalId, ModuleId, Type, TypeTable};
use ts_aot_ir_mir::{
    MirBlock, MirBody, MirDecl, MirExpr, MirLocalDecl, MirParam, MirProgram, MirStmt, RuntimeOp,
};

fn emit_fixed(optional_digits: bool) -> String {
    let mut types = TypeTable::new();
    let f64_ty = types.intern(&Type::F64);
    let string_ty = types.intern(&Type::String);
    let i64_ty = types.intern(&Type::I64);
    let digits_ty = if optional_digits {
        types.intern(&Type::Optional {
            inner: i64_ty,
            nullish: ts_aot_core::NullishKinds::BOTH,
        })
    } else {
        i64_ty
    };
    let value = LocalId::from_raw(0);
    let digits = LocalId::from_raw(1);
    let dest = LocalId::from_raw(2);
    let mut f = empty_func("fixed");
    f.params = vec![
        MirParam {
            id: value,
            name: Atom::from("n"),
            ty: f64_ty,
        },
        MirParam {
            id: digits,
            name: Atom::from("d"),
            ty: digits_ty,
        },
    ];
    f.ret = string_ty;
    f.body = MirBody {
        locals: vec![MirLocalDecl {
            id: dest,
            name: Atom::from("s"),
            ty: string_ty,
            mutable: false,
            narrowed: false,
        }],
        block: MirBlock {
            stmts: vec![MirStmt::Runtime {
                op: RuntimeOp::NumberToFixed,
                args: vec![MirExpr::Local(value), MirExpr::Local(digits)],
                dest: Some(dest),
                ty: string_ty,
                target_ty: None,
            }],
        },
    };
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(f));
    emit_decls(&prog, &types)
        .expect("number formatting must emit")
        .to_string()
}

#[test]
fn number_to_fixed_plain_integer_digits_use_a_range_preserving_conversion() {
    let s = emit_fixed(false);
    let norm: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        norm.contains("i32::try_from(d).unwrap_or(i32::MAX)"),
        "a plain integer precision must convert with `i32::try_from`, so an out-of-range value \
         stays out of range instead of truncating to a valid precision; got: {s}"
    );
    assert!(
        !norm.contains("(d)asi32"),
        "a plain integer precision must not truncate with `as i32`; got: {s}"
    );
}

#[test]
fn number_to_fixed_optional_integer_digits_use_a_range_preserving_conversion() {
    let s = emit_fixed(true);
    let norm: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        norm.contains("i32::try_from(__ts_digits).unwrap_or(i32::MAX)"),
        "a present optional integer precision must convert with `i32::try_from`, matching the \
         plain path; got: {s}"
    );
    assert!(
        !norm.contains("(__ts_digits)asi32"),
        "a present optional integer precision must not truncate with `as i32`; got: {s}"
    );
}

#[test]
fn number_to_fixed_optional_float_digits_keep_the_numeric_cast() {
    let mut types = TypeTable::new();
    let f64_ty = types.intern(&Type::F64);
    let string_ty = types.intern(&Type::String);
    let digits_ty = types.intern(&Type::Optional {
        inner: f64_ty,
        nullish: ts_aot_core::NullishKinds::BOTH,
    });
    let value = LocalId::from_raw(0);
    let digits = LocalId::from_raw(1);
    let dest = LocalId::from_raw(2);
    let mut f = empty_func("fixed");
    f.params = vec![
        MirParam {
            id: value,
            name: Atom::from("n"),
            ty: f64_ty,
        },
        MirParam {
            id: digits,
            name: Atom::from("d"),
            ty: digits_ty,
        },
    ];
    f.ret = string_ty;
    f.body = MirBody {
        locals: vec![MirLocalDecl {
            id: dest,
            name: Atom::from("s"),
            ty: string_ty,
            mutable: false,
            narrowed: false,
        }],
        block: MirBlock {
            stmts: vec![MirStmt::Runtime {
                op: RuntimeOp::NumberToFixed,
                args: vec![MirExpr::Local(value), MirExpr::Local(digits)],
                dest: Some(dest),
                ty: string_ty,
                target_ty: None,
            }],
        },
    };
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(f));
    let s = emit_decls(&prog, &types)
        .expect("number formatting must emit")
        .to_string();
    let norm: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        norm.contains("(__ts_digits)asi32"),
        "a non-integer numeric precision keeps the plain numeric cast; got: {s}"
    );
}
