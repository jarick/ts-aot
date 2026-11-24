use super::super::emit_decls;
use super::empty_func;
use ts_aot_core::{Atom, LocalId, ModuleId, Type, TypeTable, Visibility};
use ts_aot_ir_mir::{
    MirBlock, MirBody, MirDecl, MirExpr, MirGlobalDecl, MirParam, MirPlace, MirProgram, MirStmt,
};

#[test]
fn string_global_uses_owned_string_storage() {
    let mut types = TypeTable::new();
    let string_ty = types.intern(&Type::String);
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Global(MirGlobalDecl {
        name: Atom::from("greeting"),
        ty: string_ty,
        mutable: true,
        visibility: Visibility::Public,
        export_name: None,
        init: None,
    }));
    let tokens = emit_decls(&prog, &types).expect("string global must emit");
    let s = tokens.to_string();
    let norm: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        norm.contains("pubstaticmutgreeting:::std::string::String"),
        "a string global must be stored as an owned `String` so the value stays valid after \
         the per-call arena resets; got: {s}"
    );
    assert!(
        norm.contains("::std::string::String::new()"),
        "an uninitialized string global must default to `String::new()` (a `const fn` usable \
         in a static initializer); got: {s}"
    );
    assert!(
        !s.contains("'static"),
        "a string global must not borrow the arena as `'static`; got: {s}"
    );
}

#[test]
fn assignment_to_string_global_converts_to_owned_string() {
    let mut types = TypeTable::new();
    let string_ty = types.intern(&Type::String);
    let void_ty = types.intern(&Type::Void);
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Global(MirGlobalDecl {
        name: Atom::from("greeting"),
        ty: string_ty,
        mutable: true,
        visibility: Visibility::Public,
        export_name: None,
        init: None,
    }));
    let mut f = empty_func("setGreeting");
    f.params = vec![MirParam {
        id: LocalId::from_raw(0),
        name: Atom::from("s"),
        ty: string_ty,
    }];
    f.ret = void_ty;
    f.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Assign {
                target: MirPlace::Global(Atom::from("greeting")),
                value: MirExpr::Local(LocalId::from_raw(0)),
            }],
        },
    };
    prog.push_decl(MirDecl::Function(f));
    let tokens = emit_decls(&prog, &types).expect("assignment must emit");
    let s = tokens.to_string();
    let norm: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        norm.contains("unsafe{greeting=") && norm.contains(".to_string_lossy()"),
        "assigning a borrowed `JsString` to a `String`-backed global must convert through \
         `to_string_lossy` inside the `unsafe` write; got: {s}"
    );
}

#[test]
fn read_of_string_global_reallocates_in_arena() {
    let mut types = TypeTable::new();
    let string_ty = types.intern(&Type::String);
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Global(MirGlobalDecl {
        name: Atom::from("greeting"),
        ty: string_ty,
        mutable: true,
        visibility: Visibility::Public,
        export_name: None,
        init: None,
    }));
    let mut f = empty_func("getGreeting");
    f.ret = string_ty;
    f.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Return(Some(MirExpr::Global(Atom::from(
                "greeting",
            ))))],
        },
    };
    prog.push_decl(MirDecl::Function(f));
    let tokens = emit_decls(&prog, &types).expect("read must emit");
    let s = tokens.to_string();
    let norm: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        norm.contains("JsString::Valid(__ctx.arena.alloc_str("),
        "reading a `String`-backed global must reallocate a `JsString` in the current arena; \
         got: {s}"
    );
    assert!(
        norm.contains("addr_of!(greeting)"),
        "reading a mutable `String`-backed global must not form a `static_mut_refs` borrowed \
         reference, it must go through `addr_of!`; got: {s}"
    );
}

#[test]
fn optional_string_global_uses_owned_option_string_storage() {
    let mut types = TypeTable::new();
    let string_ty = types.intern(&Type::String);
    let opt_ty = types.intern(&Type::Optional {
        inner: string_ty,
        nullish: ts_aot_core::NullishKinds::BOTH,
    });
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Global(MirGlobalDecl {
        name: Atom::from("greeting"),
        ty: opt_ty,
        mutable: true,
        visibility: Visibility::Public,
        export_name: None,
        init: None,
    }));
    let tokens = emit_decls(&prog, &types).expect("optional-string global must emit");
    let s = tokens.to_string();
    let norm: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        norm.contains("pubstaticmutgreeting:::core::option::Option<::std::string::String>"),
        "an optional-string global must be stored as `Option<String>` so a write from a \
         `string | null` parameter is accepted and the value stays valid after the arena \
         resets; got: {s}"
    );
    assert!(
        norm.contains("::core::option::Option::None"),
        "an uninitialized optional-string global must default to `Option::None`; got: {s}"
    );
    assert!(
        !s.contains("'static"),
        "an optional-string global must not borrow the arena as `'static`; got: {s}"
    );
}

#[test]
fn assignment_to_optional_string_global_converts_to_owned_string() {
    let mut types = TypeTable::new();
    let string_ty = types.intern(&Type::String);
    let opt_ty = types.intern(&Type::Optional {
        inner: string_ty,
        nullish: ts_aot_core::NullishKinds::BOTH,
    });
    let void_ty = types.intern(&Type::Void);
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Global(MirGlobalDecl {
        name: Atom::from("greeting"),
        ty: opt_ty,
        mutable: true,
        visibility: Visibility::Public,
        export_name: None,
        init: None,
    }));
    let mut f = empty_func("setGreeting");
    f.params = vec![MirParam {
        id: LocalId::from_raw(0),
        name: Atom::from("s"),
        ty: opt_ty,
    }];
    f.ret = void_ty;
    f.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Assign {
                target: MirPlace::Global(Atom::from("greeting")),
                value: MirExpr::Local(LocalId::from_raw(0)),
            }],
        },
    };
    prog.push_decl(MirDecl::Function(f));
    let tokens = emit_decls(&prog, &types).expect("assignment must emit");
    let s = tokens.to_string();
    let norm: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        norm.contains("unsafe{greeting=")
            && norm.contains(".map(")
            && norm.contains(".to_string_lossy()"),
        "assigning an `Option<JsString>` to an `Option<String>`-backed global must convert the \
         inner value through `to_string_lossy` inside the `unsafe` write; got: {s}"
    );
}

#[test]
fn assignment_of_string_to_optional_string_global_wraps_in_some_owned() {
    let mut types = TypeTable::new();
    let string_ty = types.intern(&Type::String);
    let opt_ty = types.intern(&Type::Optional {
        inner: string_ty,
        nullish: ts_aot_core::NullishKinds::BOTH,
    });
    let void_ty = types.intern(&Type::Void);
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Global(MirGlobalDecl {
        name: Atom::from("greeting"),
        ty: opt_ty,
        mutable: true,
        visibility: Visibility::Public,
        export_name: None,
        init: None,
    }));
    let mut f = empty_func("setGreeting");
    f.params = vec![MirParam {
        id: LocalId::from_raw(0),
        name: Atom::from("s"),
        ty: string_ty,
    }];
    f.ret = void_ty;
    f.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Assign {
                target: MirPlace::Global(Atom::from("greeting")),
                value: MirExpr::Local(LocalId::from_raw(0)),
            }],
        },
    };
    prog.push_decl(MirDecl::Function(f));
    let tokens = emit_decls(&prog, &types).expect("assignment must emit");
    let s = tokens.to_string();
    let norm: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        norm.contains("unsafe{greeting=")
            && norm.contains("::core::option::Option::Some(")
            && norm.contains(".to_string_lossy()"),
        "assigning a borrowed `JsString` to an `Option<String>`-backed global must wrap it in \
         `Some(..to_string_lossy())` inside the `unsafe` write; got: {s}"
    );
    assert!(
        !norm.contains(".map("),
        "a plain `JsString` source must not be treated as an `Option` and mapped; got: {s}"
    );
}

#[test]
fn assignment_of_null_to_optional_string_global_writes_none() {
    let mut types = TypeTable::new();
    let string_ty = types.intern(&Type::String);
    let opt_ty = types.intern(&Type::Optional {
        inner: string_ty,
        nullish: ts_aot_core::NullishKinds::BOTH,
    });
    let void_ty = types.intern(&Type::Void);
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Global(MirGlobalDecl {
        name: Atom::from("greeting"),
        ty: opt_ty,
        mutable: true,
        visibility: Visibility::Public,
        export_name: None,
        init: None,
    }));
    let mut f = empty_func("clearGreeting");
    f.ret = void_ty;
    f.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Assign {
                target: MirPlace::Global(Atom::from("greeting")),
                value: MirExpr::Null { ty: opt_ty },
            }],
        },
    };
    prog.push_decl(MirDecl::Function(f));
    let tokens = emit_decls(&prog, &types).expect("assignment must emit");
    let s = tokens.to_string();
    let norm: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        norm.contains("unsafe{greeting=::core::option::Option::None"),
        "assigning `null` to an `Option<String>`-backed global must write `Option::None`; got: {s}"
    );
}

#[test]
fn read_of_optional_string_global_reallocates_in_arena() {
    let mut types = TypeTable::new();
    let string_ty = types.intern(&Type::String);
    let opt_ty = types.intern(&Type::Optional {
        inner: string_ty,
        nullish: ts_aot_core::NullishKinds::BOTH,
    });
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Global(MirGlobalDecl {
        name: Atom::from("greeting"),
        ty: opt_ty,
        mutable: true,
        visibility: Visibility::Public,
        export_name: None,
        init: None,
    }));
    let mut f = empty_func("getGreeting");
    f.ret = opt_ty;
    f.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Return(Some(MirExpr::Global(Atom::from(
                "greeting",
            ))))],
        },
    };
    prog.push_decl(MirDecl::Function(f));
    let tokens = emit_decls(&prog, &types).expect("read must emit");
    let s = tokens.to_string();
    let norm: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        norm.contains("as_ref().map(") && norm.contains("alloc_str("),
        "reading an `Option<String>`-backed global must reallocate the inner `JsString` in the \
         current arena; got: {s}"
    );
    assert!(
        norm.contains("addr_of!(greeting)"),
        "reading a mutable `Option<String>`-backed global must go through `addr_of!`; got: {s}"
    );
}
