use super::super::emit_decls;
use super::{empty_func, for_header_fragment, signature_fragment};
use ts_aot_core::{Atom, LocalId, ModuleId, Type, TypeId, TypeTable};
use ts_aot_ir_mir::{
    MirBlock, MirBody, MirDecl, MirExpr, MirLocalDecl, MirParam, MirProgram, MirStmt, RuntimeOp,
};

#[test]
fn for_of_item_consumed_by_array_push_marks_item_mut() {
    let mut types = TypeTable::new();
    let i32_ty = types.intern(&Type::I32);
    let arr_ty = types.intern(&Type::Array { element: i32_ty });
    let source_param = LocalId::from_raw(0);
    let item = LocalId::from_raw(1);
    let mut f = empty_func("push_each");
    f.ret = TypeId::from_raw(0);
    f.params = vec![MirParam {
        id: source_param,
        name: Atom::from("sources"),
        ty: arr_ty,
    }];
    f.body = MirBody {
        locals: vec![MirLocalDecl {
            id: item,
            name: Atom::from("arr"),
            ty: arr_ty,
            mutable: false,
            narrowed: false,
        }],
        block: MirBlock {
            stmts: vec![MirStmt::ForOf {
                item,
                iterable: MirExpr::Local(source_param),
                iter_ty: arr_ty,
                body: MirBlock {
                    stmts: vec![MirStmt::Runtime {
                        op: RuntimeOp::ArrayPush,
                        args: vec![
                            MirExpr::Local(item),
                            MirExpr::Int {
                                value: 1,
                                ty: i32_ty,
                            },
                        ],
                        dest: None,
                        ty: arr_ty,
                        target_ty: None,
                    }],
                },
            }],
        },
    };
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(f));
    let tokens = emit_decls(&prog, &types).expect("decls should emit");
    let s = tokens.to_string();
    let for_header = for_header_fragment(&s, "push_each");
    assert!(
        for_header.contains("mut arr"),
        "for-of item consumed by RuntimeOp::ArrayPush (as args[0]) must emit `mut arr` in the \
         for-of header (locals_mut must OR local.mutable with collect_written_locals so the for-of \
         binding gets a `mut` qualifier; array push needs &mut Vec). Got for-of header: \
         `{for_header}`"
    );
}

#[test]
fn for_of_item_consumed_by_array_set_marks_item_mut() {
    let mut types = TypeTable::new();
    let i32_ty = types.intern(&Type::I32);
    let arr_ty = types.intern(&Type::Array { element: i32_ty });
    let source_param = LocalId::from_raw(0);
    let item = LocalId::from_raw(1);
    let mut f = empty_func("poke_each");
    f.ret = TypeId::from_raw(0);
    f.params = vec![MirParam {
        id: source_param,
        name: Atom::from("sources"),
        ty: arr_ty,
    }];
    f.body = MirBody {
        locals: vec![MirLocalDecl {
            id: item,
            name: Atom::from("arr"),
            ty: arr_ty,
            mutable: false,
            narrowed: false,
        }],
        block: MirBlock {
            stmts: vec![MirStmt::ForOf {
                item,
                iterable: MirExpr::Local(source_param),
                iter_ty: arr_ty,
                body: MirBlock {
                    stmts: vec![MirStmt::Runtime {
                        op: RuntimeOp::ArraySet,
                        args: vec![
                            MirExpr::Local(item),
                            MirExpr::Int {
                                value: 0,
                                ty: i32_ty,
                            },
                            MirExpr::Int {
                                value: 7,
                                ty: i32_ty,
                            },
                        ],
                        dest: None,
                        ty: arr_ty,
                        target_ty: None,
                    }],
                },
            }],
        },
    };
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(f));
    let tokens = emit_decls(&prog, &types).expect("decls should emit");
    let s = tokens.to_string();
    let for_header = for_header_fragment(&s, "poke_each");
    assert!(
        for_header.contains("mut arr"),
        "for-of item consumed by RuntimeOp::ArraySet (as args[0]) must emit `mut arr` in the \
         for-of header (locals_mut must OR local.mutable with collect_written_locals so the for-of \
         binding gets a `mut` qualifier; array indexed-set needs &mut [T]). Got for-of header: \
         `{for_header}`"
    );
}

#[test]
fn for_of_item_consumed_by_map_set_marks_item_mut() {
    let mut types = TypeTable::new();
    let str_ty = types.intern(&Type::String);
    let map_ty_placeholder = types.intern(&Type::Error);
    let source_param = LocalId::from_raw(0);
    let item = LocalId::from_raw(1);
    let mut f = empty_func("seed_each");
    f.ret = TypeId::from_raw(0);
    f.params = vec![MirParam {
        id: source_param,
        name: Atom::from("sources"),
        ty: map_ty_placeholder,
    }];
    f.body = MirBody {
        locals: vec![MirLocalDecl {
            id: item,
            name: Atom::from("m"),
            ty: map_ty_placeholder,
            mutable: false,
            narrowed: false,
        }],
        block: MirBlock {
            stmts: vec![MirStmt::ForOf {
                item,
                iterable: MirExpr::Local(source_param),
                iter_ty: map_ty_placeholder,
                body: MirBlock {
                    stmts: vec![MirStmt::Runtime {
                        op: RuntimeOp::MapSet,
                        args: vec![
                            MirExpr::Local(item),
                            MirExpr::String {
                                id: Atom::from("k"),
                                ty: str_ty,
                            },
                            MirExpr::String {
                                id: Atom::from("v"),
                                ty: str_ty,
                            },
                        ],
                        dest: None,
                        ty: map_ty_placeholder,
                        target_ty: None,
                    }],
                },
            }],
        },
    };
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(f));
    let tokens = emit_decls(&prog, &types).expect("decls should emit");
    let s = tokens.to_string();
    let for_header = for_header_fragment(&s, "seed_each");
    assert!(
        for_header.contains("mut m"),
        "for-of item consumed by RuntimeOp::MapSet (as args[0]) must emit `mut m` in the \
         for-of header (locals_mut must OR local.mutable with collect_written_locals so the for-of \
         binding gets a `mut` qualifier; map set needs &mut map). Got for-of header: \
         `{for_header}`"
    );
}

#[test]
fn for_of_generator_iterable_marks_iterable_param_mut() {
    let mut types = TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let gen_ty = types.intern(&Type::Generator {
        inner: i64_ty,
        is_async: false,
    });
    let gen_param = LocalId::from_raw(0);
    let item = LocalId::from_raw(1);
    let mut f = empty_func("drain");
    f.ret = TypeId::from_raw(0);
    f.params = vec![MirParam {
        id: gen_param,
        name: Atom::from("g"),
        ty: gen_ty,
    }];
    f.body = MirBody {
        locals: vec![MirLocalDecl {
            id: item,
            name: Atom::from("x"),
            ty: i64_ty,
            mutable: false,
            narrowed: false,
        }],
        block: MirBlock {
            stmts: vec![MirStmt::ForOf {
                item,
                iterable: MirExpr::Local(gen_param),
                iter_ty: gen_ty,
                body: MirBlock {
                    stmts: vec![MirStmt::Expr(MirExpr::Local(item))],
                },
            }],
        },
    };
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(f));
    let tokens = emit_decls(&prog, &types).expect("decls should emit");
    let s = tokens.to_string();
    let sig = signature_fragment(&s, "drain");
    assert!(
        sig.contains("mut g"),
        "param that is the iterable of a `for-of` over a Generator must emit `mut g` in the \
         function signature (ForOf lowers to a mutable-borrow `for` loop). Got signature \
         fragment: `{sig}`"
    );
}
