use super::super::{emit_decls, emit_function};
use super::empty_func;
use ts_aot_core::{Atom, FieldId, LocalId, ModuleId, StructId, Type, TypeTable, Visibility};
use ts_aot_ir_mir::{
    MirBlock, MirBody, MirDecl, MirExpr, MirFieldDecl, MirLocalDecl, MirProgram, MirStmt,
    MirStructDecl,
};

#[test]
fn optional_chain_expr_emits_base_as_value() {
    let mut types = TypeTable::new();
    let int_ty = types.intern(&Type::I64);
    let local = LocalId::from_raw(0);
    let mut f = empty_func("caller");
    f.ret = int_ty;
    f.body = MirBody {
        locals: vec![MirLocalDecl {
            id: local,
            name: Atom::from("obj"),
            ty: int_ty,
            mutable: false,
            narrowed: false,
        }],
        block: MirBlock {
            stmts: vec![MirStmt::Return(Some(MirExpr::OptionalChain {
                base: Box::new(MirExpr::Local(local)),
                ty: int_ty,
            }))],
        },
    };
    let tokens = emit_function(&f, &types).expect("OptionalChain must emit");
    let s = tokens.to_string();
    assert!(
        s.contains("return obj ;") || s.contains("return obj;"),
        "MVP: OptionalChain must emit the base expression directly (treat as non-null). \
         Phase 5+ will replace this with proper Option<T> short-circuit. Got: {s}"
    );
}

#[test]
fn optional_chain_field_with_optional_base_emits_as_ref_map() {
    let mut types = TypeTable::new();
    let point = StructId::from_raw(0);
    let point_ty = types.intern(&Type::Struct { id: point });
    let opt = types.intern(&Type::Optional {
        inner: point_ty,
        nullish: ts_aot_core::NullishKinds::BOTH,
    });
    let i64_ty = types.intern(&Type::I64);
    let opt_i64 = types.intern(&Type::Optional {
        inner: i64_ty,
        nullish: ts_aot_core::NullishKinds::BOTH,
    });
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Struct(MirStructDecl {
        id: point,
        name: Atom::from("Point"),
        fields: vec![MirFieldDecl {
            id: FieldId::from_raw(0),
            name: Atom::from("x"),
            ty: i64_ty,
            mutable: false,
            visibility: Visibility::Public,
        }],
        methods: Vec::new(),
    }));
    let local = LocalId::from_raw(0);
    let mut f = empty_func("caller");
    f.ret = opt_i64;
    f.body = MirBody {
        locals: vec![MirLocalDecl {
            id: local,
            name: Atom::from("obj"),
            ty: opt,
            mutable: false,
            narrowed: false,
        }],
        block: MirBlock {
            stmts: vec![MirStmt::Return(Some(MirExpr::Field {
                base: Box::new(MirExpr::OptionalChain {
                    base: Box::new(MirExpr::Local(local)),
                    ty: opt,
                }),
                field: FieldId::from_raw(0),
                ty: opt_i64,
            }))],
        },
    };
    prog.push_decl(MirDecl::Function(f));
    let tokens = emit_decls(&prog, &types).expect("OptionalChain field must emit");
    let s = tokens.to_string();
    assert!(
        s.contains("obj . as_ref () . map (| o | o . x . borrow () . clone ())"),
        "Optional base + Field must emit `obj.as_ref().map(|o| o.x.borrow().clone())` — access \
         must be inside the Option::map closure, not on Option<&T> directly (which doesn't \
         compile). Got: {s}"
    );
}

#[test]
fn optional_chain_field_with_optional_declared_field_emits_and_then() {
    let mut types = TypeTable::new();
    let point = StructId::from_raw(0);
    let point_ty = types.intern(&Type::Struct { id: point });
    let opt = types.intern(&Type::Optional {
        inner: point_ty,
        nullish: ts_aot_core::NullishKinds::BOTH,
    });
    let f64_ty = types.intern(&Type::F64);
    let opt_f64 = types.intern(&Type::Optional {
        inner: f64_ty,
        nullish: ts_aot_core::NullishKinds::BOTH,
    });
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Struct(MirStructDecl {
        id: point,
        name: Atom::from("Point"),
        fields: vec![MirFieldDecl {
            id: FieldId::from_raw(0),
            name: Atom::from("n"),
            ty: opt_f64,
            mutable: false,
            visibility: Visibility::Public,
        }],
        methods: Vec::new(),
    }));
    let local = LocalId::from_raw(0);
    let mut f = empty_func("caller");
    f.ret = opt_f64;
    f.body = MirBody {
        locals: vec![MirLocalDecl {
            id: local,
            name: Atom::from("obj"),
            ty: opt,
            mutable: false,
            narrowed: false,
        }],
        block: MirBlock {
            stmts: vec![MirStmt::Return(Some(MirExpr::Field {
                base: Box::new(MirExpr::OptionalChain {
                    base: Box::new(MirExpr::Local(local)),
                    ty: opt,
                }),
                field: FieldId::from_raw(0),
                ty: opt_f64,
            }))],
        },
    };
    prog.push_decl(MirDecl::Function(f));
    let tokens = emit_decls(&prog, &types).expect("OptionalChain field must emit");
    let s = tokens.to_string();
    assert!(
        s.contains("obj . as_ref () . and_then (| o | o . n . borrow () . clone ())"),
        "an optional declared field must flatten to one `Option` via `and_then` \
         (a `map` would produce `Option<Option<f64>>`). Got: {s}"
    );
}

#[test]
fn optional_chain_non_copy_optional_field_emits_and_then_with_clone() {
    let mut types = TypeTable::new();
    let point = StructId::from_raw(0);
    let point_ty = types.intern(&Type::Struct { id: point });
    let opt = types.intern(&Type::Optional {
        inner: point_ty,
        nullish: ts_aot_core::NullishKinds::BOTH,
    });
    let string_ty = types.intern(&Type::String);
    let opt_string = types.intern(&Type::Optional {
        inner: string_ty,
        nullish: ts_aot_core::NullishKinds::BOTH,
    });
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Struct(MirStructDecl {
        id: point,
        name: Atom::from("Point"),
        fields: vec![MirFieldDecl {
            id: FieldId::from_raw(0),
            name: Atom::from("s"),
            ty: opt_string,
            mutable: false,
            visibility: Visibility::Public,
        }],
        methods: Vec::new(),
    }));
    let local = LocalId::from_raw(0);
    let mut f = empty_func("caller");
    f.ret = opt_string;
    f.body = MirBody {
        locals: vec![MirLocalDecl {
            id: local,
            name: Atom::from("obj"),
            ty: opt,
            mutable: false,
            narrowed: false,
        }],
        block: MirBlock {
            stmts: vec![MirStmt::Return(Some(MirExpr::Field {
                base: Box::new(MirExpr::OptionalChain {
                    base: Box::new(MirExpr::Local(local)),
                    ty: opt,
                }),
                field: FieldId::from_raw(0),
                ty: opt_string,
            }))],
        },
    };
    prog.push_decl(MirDecl::Function(f));
    let tokens = emit_decls(&prog, &types).expect("OptionalChain field must emit");
    let s = tokens.to_string();
    assert!(
        s.contains("obj . as_ref () . and_then (| o | o . s . borrow () . clone ())"),
        "a non-Copy optional field must be read through its `RefCell` and cloned inside the \
         `and_then` closure, so the generated Rust compiles. Got: {s}"
    );
}

#[test]
fn optional_chain_index_with_optional_element_emits_and_then() {
    let mut types = TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let opt_i64 = types.intern(&Type::Optional {
        inner: i64_ty,
        nullish: ts_aot_core::NullishKinds::BOTH,
    });
    let arr_ty = types.intern(&Type::Array { element: opt_i64 });
    let opt_arr = types.intern(&Type::Optional {
        inner: arr_ty,
        nullish: ts_aot_core::NullishKinds::BOTH,
    });
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    let local = LocalId::from_raw(0);
    let idx_local = LocalId::from_raw(1);
    let mut f = empty_func("caller");
    f.ret = opt_i64;
    f.body = MirBody {
        locals: vec![
            MirLocalDecl {
                id: local,
                name: Atom::from("arr"),
                ty: opt_arr,
                mutable: false,
                narrowed: false,
            },
            MirLocalDecl {
                id: idx_local,
                name: Atom::from("i"),
                ty: i64_ty,
                mutable: false,
                narrowed: false,
            },
        ],
        block: MirBlock {
            stmts: vec![MirStmt::Return(Some(MirExpr::Index {
                base: Box::new(MirExpr::OptionalChain {
                    base: Box::new(MirExpr::Local(local)),
                    ty: opt_arr,
                }),
                index: Box::new(MirExpr::Local(idx_local)),
                ty: opt_i64,
            }))],
        },
    };
    prog.push_decl(MirDecl::Function(f));
    let tokens = emit_decls(&prog, &types).expect("OptionalChain index must emit");
    let s = tokens.to_string();
    assert!(
        s.contains(
            "arr . as_ref () . and_then (| o | o . get (i as usize) . copied () . flatten ())"
        ),
        "an optional array element must be read through the checked `get` accessor (an \
         out-of-range index yields `undefined`) and must flatten to one `Option` via `and_then` \
         followed by `flatten`. Got: {s}"
    );
}

#[test]
fn optional_chain_index_with_optional_base_emits_as_ref_and_then() {
    let mut types = TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let arr_ty = types.intern(&Type::Array { element: i64_ty });
    let opt_arr = types.intern(&Type::Optional {
        inner: arr_ty,
        nullish: ts_aot_core::NullishKinds::BOTH,
    });
    let opt_i64 = types.intern(&Type::Optional {
        inner: i64_ty,
        nullish: ts_aot_core::NullishKinds::BOTH,
    });
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    let local = LocalId::from_raw(0);
    let idx_local = LocalId::from_raw(1);
    let mut f = empty_func("caller");
    f.ret = opt_i64;
    f.body = MirBody {
        locals: vec![
            MirLocalDecl {
                id: local,
                name: Atom::from("arr"),
                ty: opt_arr,
                mutable: false,
                narrowed: false,
            },
            MirLocalDecl {
                id: idx_local,
                name: Atom::from("i"),
                ty: i64_ty,
                mutable: false,
                narrowed: false,
            },
        ],
        block: MirBlock {
            stmts: vec![MirStmt::Return(Some(MirExpr::Index {
                base: Box::new(MirExpr::OptionalChain {
                    base: Box::new(MirExpr::Local(local)),
                    ty: opt_arr,
                }),
                index: Box::new(MirExpr::Local(idx_local)),
                ty: opt_i64,
            }))],
        },
    };
    prog.push_decl(MirDecl::Function(f));
    let tokens = emit_decls(&prog, &types).expect("OptionalChain index must emit");
    let s = tokens.to_string();
    assert!(
        s.contains("arr . as_ref () . and_then (| o | o . get (i as usize) . copied ())"),
        "Optional base + Index must emit `arr.as_ref().and_then(|o| o.get(i as usize).copied())` \
         — the index must be inside the closure, not on Option<&T> directly (which doesn't \
         compile), and the checked `get` accessor must turn an out-of-range index into `undefined` \
         instead of panicking while still flattening to a single `Option`. Got: {s}"
    );
}

#[test]
fn optional_chain_index_with_non_copy_element_clones_through_the_checked_accessor() {
    let mut types = TypeTable::new();
    let string_ty = types.intern(&Type::String);
    let opt_string = types.intern(&Type::Optional {
        inner: string_ty,
        nullish: ts_aot_core::NullishKinds::BOTH,
    });
    let arr_ty = types.intern(&Type::Array {
        element: opt_string,
    });
    let opt_arr = types.intern(&Type::Optional {
        inner: arr_ty,
        nullish: ts_aot_core::NullishKinds::BOTH,
    });
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    let local = LocalId::from_raw(0);
    let idx_local = LocalId::from_raw(1);
    let mut f = empty_func("caller");
    f.ret = opt_string;
    f.body = MirBody {
        locals: vec![
            MirLocalDecl {
                id: local,
                name: Atom::from("arr"),
                ty: opt_arr,
                mutable: false,
                narrowed: false,
            },
            MirLocalDecl {
                id: idx_local,
                name: Atom::from("i"),
                ty: types.intern(&Type::I64),
                mutable: false,
                narrowed: false,
            },
        ],
        block: MirBlock {
            stmts: vec![MirStmt::Return(Some(MirExpr::Index {
                base: Box::new(MirExpr::OptionalChain {
                    base: Box::new(MirExpr::Local(local)),
                    ty: opt_arr,
                }),
                index: Box::new(MirExpr::Local(idx_local)),
                ty: opt_string,
            }))],
        },
    };
    prog.push_decl(MirDecl::Function(f));
    let tokens = emit_decls(&prog, &types).expect("OptionalChain index must emit");
    let s = tokens.to_string();
    assert!(
        s.contains(
            "arr . as_ref () . and_then (| o | o . get (i as usize) . cloned () . flatten ())"
        ),
        "a non-Copy optional element must be read through the checked `get` accessor, cloned and \
         flattened; got: {s}"
    );
}

#[test]
fn optional_chain_index_with_non_optional_base_falls_back_to_mvp() {
    let mut types = TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let arr_ty = types.intern(&Type::Array { element: i64_ty });
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    let local = LocalId::from_raw(0);
    let idx_local = LocalId::from_raw(1);
    let mut f = empty_func("caller");
    f.ret = i64_ty;
    f.body = MirBody {
        locals: vec![
            MirLocalDecl {
                id: local,
                name: Atom::from("arr"),
                ty: arr_ty,
                mutable: false,
                narrowed: false,
            },
            MirLocalDecl {
                id: idx_local,
                name: Atom::from("i"),
                ty: i64_ty,
                mutable: false,
                narrowed: false,
            },
        ],
        block: MirBlock {
            stmts: vec![MirStmt::Return(Some(MirExpr::Index {
                base: Box::new(MirExpr::OptionalChain {
                    base: Box::new(MirExpr::Local(local)),
                    ty: arr_ty,
                }),
                index: Box::new(MirExpr::Local(idx_local)),
                ty: i64_ty,
            }))],
        },
    };
    prog.push_decl(MirDecl::Function(f));
    let tokens = emit_decls(&prog, &types).expect("MVP fallback must emit");
    let s = tokens.to_string();
    assert!(
        s.contains("arr [i as usize]") && !s.contains("as_ref"),
        "Non-Optional base + Index must fall back to MVP rr[i] (no Option machinery). Got: {s}"
    );
}
