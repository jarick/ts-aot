use super::*;
use ts_aot_core::{Atom, FunctionId, LocalId, TypeId};
use ts_aot_ir_mir::{FunctionEffects, FunctionKind, MirBlock, MirDecl, MirFunctionDecl, MirParam};

fn empty_function(id: u32, throws: Option<TypeId>) -> MirFunctionDecl {
    MirFunctionDecl {
        id: FunctionId::from_raw(id),
        name: Atom::from(format!("fn{}", id)),
        export_name: None,
        params: Vec::<MirParam>::new(),
        ret: TypeId::from_raw(0),
        throws,
        body: ts_aot_ir_mir::MirBody::default(),
        kind: FunctionKind::Plain,
        effects: FunctionEffects::default(),
    }
}

fn throw_stmt() -> MirStmt {
    MirStmt::Throw {
        error: MirExpr::Int {
            value: 7,
            ty: TypeId::from_raw(0),
        },
        error_ty: TypeId::from_raw(0),
    }
}

fn throw_err_ty() -> TypeId {
    TypeId::from_raw(42)
}

#[test]
fn function_without_throws_is_left_alone() {
    let mut f = empty_function(0, None);
    f.body.block = MirBlock::with(throw_stmt());
    let mut program = MirProgram::new(ts_aot_core::ModuleId::from_raw(0));
    program.push_decl(MirDecl::Function(f));

    lower_result(&mut program);

    let MirDecl::Function(after) = &program.declarations[0] else {
        panic!("expected function");
    };
    match &after.body.block.stmts[0] {
        MirStmt::Throw { error_ty, .. } => assert_eq!(*error_ty, TypeId::from_raw(0)),
        other => panic!("expected Throw, got {other:?}"),
    }
}

#[test]
fn throw_in_throwing_function_becomes_return_result_err() {
    let mut f = empty_function(0, Some(throw_err_ty()));
    f.body.block = MirBlock::with(throw_stmt());
    let mut program = MirProgram::new(ts_aot_core::ModuleId::from_raw(0));
    program.push_decl(MirDecl::Function(f));

    lower_result(&mut program);

    let MirDecl::Function(after) = &program.declarations[0] else {
        panic!("expected function");
    };
    match &after.body.block.stmts[0] {
        MirStmt::ReturnResultErr { err_ty, .. } => assert_eq!(*err_ty, throw_err_ty()),
        other => panic!("expected ReturnResultErr, got {other:?}"),
    }
}

#[test]
fn throw_inside_if_branch_is_rewritten() {
    let mut f = empty_function(0, Some(throw_err_ty()));
    let cond = MirExpr::Bool(true);
    f.body.block = MirBlock::with(MirStmt::If {
        cond,
        then_block: MirBlock::with(throw_stmt()),
        else_block: None,
    });
    let mut program = MirProgram::new(ts_aot_core::ModuleId::from_raw(0));
    program.push_decl(MirDecl::Function(f));

    lower_result(&mut program);

    let MirDecl::Function(after) = &program.declarations[0] else {
        panic!("expected function");
    };
    let MirStmt::If { then_block, .. } = &after.body.block.stmts[0] else {
        panic!("expected If");
    };
    assert!(matches!(
        then_block.stmts[0],
        MirStmt::ReturnResultErr { .. }
    ));
}

#[test]
fn throw_inside_while_body_is_rewritten() {
    let mut f = empty_function(0, Some(throw_err_ty()));
    f.body.block = MirBlock::with(MirStmt::While {
        cond: MirExpr::Bool(true),
        body: MirBlock::with(throw_stmt()),
    });
    let mut program = MirProgram::new(ts_aot_core::ModuleId::from_raw(0));
    program.push_decl(MirDecl::Function(f));

    lower_result(&mut program);

    let MirDecl::Function(after) = &program.declarations[0] else {
        panic!("expected function");
    };
    let MirStmt::While { body, .. } = &after.body.block.stmts[0] else {
        panic!("expected While");
    };
    assert!(matches!(body.stmts[0], MirStmt::ReturnResultErr { .. }));
}

#[test]
fn throw_in_for_of_body_is_rewritten() {
    let mut f = empty_function(0, Some(throw_err_ty()));
    f.body.block = MirBlock::with(MirStmt::ForOf {
        item: LocalId::from_raw(0),
        iterable: MirExpr::Local(LocalId::from_raw(1)),
        body: MirBlock::with(throw_stmt()),
    });
    let mut program = MirProgram::new(ts_aot_core::ModuleId::from_raw(0));
    program.push_decl(MirDecl::Function(f));

    lower_result(&mut program);

    let MirDecl::Function(after) = &program.declarations[0] else {
        panic!("expected function");
    };
    let MirStmt::ForOf { body, .. } = &after.body.block.stmts[0] else {
        panic!("expected ForOf");
    };
    assert!(matches!(body.stmts[0], MirStmt::ReturnResultErr { .. }));
}

#[test]
fn multiple_decls_are_processed_independently() {
    let mut throwing = empty_function(0, Some(throw_err_ty()));
    throwing.body.block = MirBlock::with(throw_stmt());

    let mut plain = empty_function(1, None);
    plain.body.block = MirBlock::with(throw_stmt());

    let mut program = MirProgram::new(ts_aot_core::ModuleId::from_raw(0));
    program.push_decl(MirDecl::Function(throwing));
    program.push_decl(MirDecl::Function(plain));

    lower_result(&mut program);

    let MirDecl::Function(t) = &program.declarations[0] else {
        panic!()
    };
    let MirDecl::Function(p) = &program.declarations[1] else {
        panic!()
    };
    assert!(matches!(
        t.body.block.stmts[0],
        MirStmt::ReturnResultErr { .. }
    ));
    assert!(matches!(p.body.block.stmts[0], MirStmt::Throw { .. }));
}

#[test]
fn non_throwing_function_body_is_unchanged_when_no_throws_present() {
    let mut f = empty_function(0, Some(throw_err_ty()));
    f.body.block = MirBlock::with(MirStmt::Return(Some(MirExpr::Unit)));
    let mut program = MirProgram::new(ts_aot_core::ModuleId::from_raw(0));
    program.push_decl(MirDecl::Function(f));

    lower_result(&mut program);

    let MirDecl::Function(after) = &program.declarations[0] else {
        panic!("expected function");
    };
    assert!(matches!(after.body.block.stmts[0], MirStmt::Return(_)));
}

#[test]
fn throw_inside_if_else_both_branches_are_rewritten() {
    let mut f = empty_function(0, Some(throw_err_ty()));
    f.body.block = MirBlock::with(MirStmt::If {
        cond: MirExpr::Bool(true),
        then_block: MirBlock::with(throw_stmt()),
        else_block: Some(MirBlock::with(throw_stmt())),
    });
    let mut program = MirProgram::new(ts_aot_core::ModuleId::from_raw(0));
    program.push_decl(MirDecl::Function(f));

    lower_result(&mut program);

    let MirDecl::Function(after) = &program.declarations[0] else {
        panic!("expected function");
    };
    let MirStmt::If {
        then_block,
        else_block,
        ..
    } = &after.body.block.stmts[0]
    else {
        panic!("expected If");
    };
    assert!(matches!(
        then_block.stmts[0],
        MirStmt::ReturnResultErr { .. }
    ));
    let Some(else_block) = else_block else {
        panic!("expected Some(else_block)");
    };
    assert!(matches!(
        else_block.stmts[0],
        MirStmt::ReturnResultErr { .. }
    ));
}

#[test]
fn empty_program_is_a_noop() {
    let mut program = MirProgram::new(ts_aot_core::ModuleId::from_raw(0));
    lower_result(&mut program);
    assert_eq!(program.decl_count(), 0);
}

#[test]
fn struct_decl_is_skipped() {
    use ts_aot_core::{FieldId, StructId, Visibility};
    use ts_aot_ir_mir::{MirFieldDecl, MirStructDecl};

    let s = MirStructDecl {
        id: StructId::from_raw(0),
        name: Atom::new_inline("1"),
        fields: vec![MirFieldDecl {
            id: FieldId::from_raw(0),
            name: Atom::new_inline("10"),
            ty: TypeId::from_raw(0),
            mutable: false,
            visibility: Visibility::Public,
        }],
        methods: Vec::new(),
    };
    let mut program = MirProgram::new(ts_aot_core::ModuleId::from_raw(0));
    program.push_decl(MirDecl::Struct(s));

    lower_result(&mut program);

    assert_eq!(program.decl_count(), 1);
    assert!(program.structs().next().is_some());
}

#[test]
fn throw_error_expression_is_preserved() {
    let mut f = empty_function(0, Some(throw_err_ty()));
    let payload = MirExpr::String {
        id: Atom::new_inline("9"),
        ty: TypeId::from_raw(0),
    };
    f.body.block = MirBlock::with(MirStmt::Throw {
        error: payload.clone(),
        error_ty: TypeId::from_raw(0),
    });
    let mut program = MirProgram::new(ts_aot_core::ModuleId::from_raw(0));
    program.push_decl(MirDecl::Function(f));

    lower_result(&mut program);

    let MirDecl::Function(after) = &program.declarations[0] else {
        panic!("expected function");
    };
    match &after.body.block.stmts[0] {
        MirStmt::ReturnResultErr { error, .. } => {
            assert!(matches!(
                error,
                MirExpr::String { id, .. } if *id == Atom::new_inline("9")
            ));
        }
        other => panic!("expected ReturnResultErr, got {other:?}"),
    }
}
