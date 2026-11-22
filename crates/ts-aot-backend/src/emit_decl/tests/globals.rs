use super::super::{emit_decls, emit_global};
use super::empty_func;
use crate::BackendError;
use ts_aot_core::{Atom, FieldId, ModuleId, StructId, Type, TypeId, TypeTable, Visibility};
use ts_aot_ir_mir::{
    MirBlock, MirBody, MirDecl, MirExpr, MirFieldDecl, MirGlobalDecl, MirPlace, MirProgram,
    MirStmt, MirStructDecl,
};

#[test]
fn global_without_init_emits_const_default_initializer() {
    let mut types = TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Global(MirGlobalDecl {
        name: Atom::from("counter"),
        ty: i64_ty,
        mutable: false,
        visibility: Visibility::Public,
        export_name: None,
        init: None,
    }));
    let tokens = emit_decls(&prog, &types)
        .expect("uninitialized global must emit a const-default-initialized static");
    let s = tokens.to_string();
    assert!(s.contains("static counter"), "expected static decl in: {s}");
    assert!(
        s.contains("= 0 ;"),
        "uninitialized globals must use a const-capable default \
         (Rust rejects `Default::default()` in a static initializer); got: {s}"
    );
    assert!(
        !s.contains("Default :: default"),
        "`Default::default()` is not allowed in a static initializer; got: {s}"
    );
}

#[test]
fn global_struct_without_init_emits_an_option_placeholder() {
    let mut types = TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let sid = StructId::from_raw(3);
    let struct_ty = types.intern(&Type::Struct { id: sid });
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Struct(MirStructDecl {
        id: sid,
        name: Atom::from("Point"),
        fields: vec![
            MirFieldDecl {
                id: FieldId::from_raw(0),
                name: Atom::from("x"),
                ty: i64_ty,
                mutable: true,
                visibility: Visibility::Public,
            },
            MirFieldDecl {
                id: FieldId::from_raw(1),
                name: Atom::from("y"),
                ty: i64_ty,
                mutable: true,
                visibility: Visibility::Public,
            },
        ],
        methods: Vec::new(),
    }));
    prog.push_decl(MirDecl::Global(MirGlobalDecl {
        name: Atom::from("origin"),
        ty: struct_ty,
        mutable: true,
        visibility: Visibility::Public,
        export_name: None,
        init: None,
    }));
    let s = emit_decls(&prog, &types)
        .expect("struct global must emit")
        .to_string();
    let norm: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        norm.contains(
            "staticmutorigin:::core::option::Option<Point>=\
             ::core::option::Option::None"
        ),
        "a struct global must be stored as a lazily initialized `Option` because `Rc::new` is not \
         a const function and the `Rc` field makes the struct non-`Sync`; got: {s}"
    );
    assert!(
        !norm.contains("staticmutorigin:Point"),
        "a struct global must not be stored as a bare struct: its `Rc<RefCell<_>>` fields have no \
         const initializer; got: {s}"
    );
    assert!(
        !s.contains("Default :: default"),
        "`Default::default()` must not appear for a struct global; got: {s}"
    );
}

fn push_point(types: &mut TypeTable, prog: &mut MirProgram, sid: StructId) -> TypeId {
    let f64_ty = types.intern(&Type::F64);
    prog.push_decl(MirDecl::Struct(MirStructDecl {
        id: sid,
        name: Atom::from("C"),
        fields: vec![MirFieldDecl {
            id: FieldId::from_raw(0),
            name: Atom::from("x"),
            ty: f64_ty,
            mutable: true,
            visibility: Visibility::Public,
        }],
        methods: Vec::new(),
    }));
    types.intern(&Type::Struct { id: sid })
}

fn struct_global(ty: TypeId, init: Option<MirExpr>) -> MirDecl {
    MirDecl::Global(MirGlobalDecl {
        name: Atom::from("g"),
        ty,
        mutable: true,
        visibility: Visibility::Public,
        export_name: None,
        init,
    })
}

fn assert_unrepresentable_init(init: MirExpr, expected: &str) {
    let mut types = TypeTable::new();
    let sid = StructId::from_raw(4);
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    let struct_ty = push_point(&mut types, &mut prog, sid);
    prog.push_decl(struct_global(struct_ty, Some(init)));
    let err = emit_decls(&prog, &types)
        .expect_err("a struct global whose initializer is not a lazy default must be rejected");
    assert_eq!(err.code(), "E0300");
    let message = err.message();
    assert!(
        message.contains("struct-typed global `g`") && message.contains(expected),
        "the diagnostic must name the global and the initializer shape, got: {message}"
    );
    assert!(
        message.contains("| null") && message.contains("__tla_main"),
        "the diagnostic must tell the user to declare the global as `<struct> | null` or to \
         initialize it in the module body; got: {message}"
    );
}

#[test]
fn initialized_struct_global_with_a_null_initializer_is_rejected() {
    let null_ty = TypeId::from_raw(0);
    assert_unrepresentable_init(MirExpr::Null { ty: null_ty }, "`null`");
}

#[test]
fn initialized_struct_global_with_a_number_initializer_is_rejected() {
    assert_unrepresentable_init(
        MirExpr::Int {
            value: 7,
            ty: TypeId::from_raw(0),
        },
        "a number",
    );
}

#[test]
fn initialized_struct_global_with_a_boolean_initializer_is_rejected() {
    assert_unrepresentable_init(MirExpr::Bool(true), "a boolean");
}

#[test]
fn initialized_struct_global_with_a_unit_initializer_is_rejected() {
    assert_unrepresentable_init(MirExpr::Unit, "`undefined`");
}

#[test]
fn struct_global_initialized_from_a_scalar_global_is_rejected() {
    let mut types = TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let sid = StructId::from_raw(4);
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    let struct_ty = push_point(&mut types, &mut prog, sid);
    prog.push_decl(MirDecl::Global(MirGlobalDecl {
        name: Atom::from("n"),
        ty: i64_ty,
        mutable: false,
        visibility: Visibility::Private,
        export_name: None,
        init: None,
    }));
    prog.push_decl(struct_global(
        struct_ty,
        Some(MirExpr::Global(Atom::from("n"))),
    ));
    let err = emit_decls(&prog, &types).expect_err(
        "a struct global initialized from a scalar global cannot supply the struct's lazy default",
    );
    let message = err.message();
    assert!(
        message.contains("reference that cannot provide this struct"),
        "the diagnostic must say the reference cannot provide the struct, got: {message}"
    );
}

#[test]
fn struct_global_initialized_from_another_struct_global_clones_its_lazy_default() {
    let mut types = TypeTable::new();
    let f64_ty = types.intern(&Type::F64);
    let sid = StructId::from_raw(4);
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    let struct_ty = push_point(&mut types, &mut prog, sid);
    prog.push_decl(MirDecl::Global(MirGlobalDecl {
        name: Atom::from("a"),
        ty: struct_ty,
        mutable: true,
        visibility: Visibility::Public,
        export_name: None,
        init: None,
    }));
    prog.push_decl(struct_global(
        struct_ty,
        Some(MirExpr::Global(Atom::from("a"))),
    ));
    let mut f = empty_func("got");
    f.ret = f64_ty;
    f.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Return(Some(MirExpr::Field {
                base: Box::new(MirExpr::Global(Atom::from("g"))),
                field: FieldId::from_raw(0),
                ty: f64_ty,
            }))],
        },
    };
    prog.push_decl(MirDecl::Function(f));
    let s = emit_decls(&prog, &types)
        .expect("a struct global initialized from another struct global must emit")
        .to_string();
    let norm: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        norm.contains("staticmutg:::core::option::Option<C>=::core::option::Option::None"),
        "the initialized struct global must keep the lazy `Option` representation; got: {s}"
    );
    assert_eq!(
        norm.matches("get_or_insert_with").count(),
        2,
        "the default of `g` must read `a` through its own lazy accessor; got: {s}"
    );
    let cloned_lazy_default = "addr_of_mut!(a)).get_or_insert_with(||C{x:::std::rc::Rc::new(::core::cell::RefCell::new(0.0))})})).clone()";
    assert!(
        norm.contains(cloned_lazy_default),
        "the default of `g` must clone `a`'s lazy value; got: {norm}"
    );
}

#[test]
fn assignment_to_mutable_global_is_wrapped_in_unsafe() {
    let mut types = TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Global(MirGlobalDecl {
        name: Atom::from("counter"),
        ty: i64_ty,
        mutable: true,
        visibility: Visibility::Private,
        export_name: None,
        init: Some(MirExpr::Int {
            value: 0,
            ty: i64_ty,
        }),
    }));
    let mut f = empty_func("bump");
    f.ret = i64_ty;
    f.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Assign {
                target: MirPlace::Global(Atom::from("counter")),
                value: MirExpr::Int {
                    value: 1,
                    ty: i64_ty,
                },
            }],
        },
    };
    prog.push_decl(MirDecl::Function(f));
    let tokens = emit_decls(&prog, &types).expect("decls should emit");
    let s = tokens.to_string();
    assert!(
        s.contains("static mut counter"),
        "reassigned global must be a mutable static, got: {s}"
    );
    assert!(
        s.contains("unsafe { counter ="),
        "assignment to a mutable static must be wrapped in `unsafe`, got: {s}"
    );
}

#[test]
fn read_of_mutable_global_is_wrapped_in_unsafe() {
    let mut types = TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Global(MirGlobalDecl {
        name: Atom::from("counter"),
        ty: i64_ty,
        mutable: true,
        visibility: Visibility::Private,
        export_name: None,
        init: Some(MirExpr::Int {
            value: 0,
            ty: i64_ty,
        }),
    }));
    let mut f = empty_func("get");
    f.ret = i64_ty;
    f.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Return(Some(MirExpr::Global(Atom::from(
                "counter",
            ))))],
        },
    };
    prog.push_decl(MirDecl::Function(f));
    let tokens = emit_decls(&prog, &types).expect("decls should emit");
    let s = tokens.to_string();
    assert!(
        s.contains("unsafe { counter }"),
        "read of a mutable static must be wrapped in `unsafe`, got: {s}"
    );
}

#[test]
fn struct_global_with_a_borrowed_string_field_is_rejected() {
    let mut types = TypeTable::new();
    let string_ty = types.intern(&Type::String);
    let sid = StructId::from_raw(9);
    let struct_ty = types.intern(&Type::Struct { id: sid });
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Struct(MirStructDecl {
        id: sid,
        name: Atom::from("Holder"),
        fields: vec![MirFieldDecl {
            id: FieldId::from_raw(0),
            name: Atom::from("s"),
            ty: string_ty,
            mutable: true,
            visibility: Visibility::Public,
        }],
        methods: Vec::new(),
    }));
    prog.push_decl(MirDecl::Global(MirGlobalDecl {
        name: Atom::from("holder"),
        ty: struct_ty,
        mutable: true,
        visibility: Visibility::Public,
        export_name: None,
        init: None,
    }));
    let err = emit_decls(&prog, &types).expect_err(
        "a struct global with a borrowed string field must be rejected: a `static` cannot hold \
         arena-borrowing data it has no owned representation for",
    );
    let msg = err.to_string();
    assert!(
        msg.contains("static") && msg.contains("string"),
        "the borrowed-struct-global rejection must explain that the type cannot live in a \
         `static` and mention the supported `string`/`string | null` forms; got: {msg}"
    );
}

#[test]
fn read_of_immutable_global_is_not_wrapped_in_unsafe() {
    let mut types = TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Global(MirGlobalDecl {
        name: Atom::from("answer"),
        ty: i64_ty,
        mutable: false,
        visibility: Visibility::Private,
        export_name: None,
        init: Some(MirExpr::Int {
            value: 42,
            ty: i64_ty,
        }),
    }));
    let mut f = empty_func("get");
    f.ret = i64_ty;
    f.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Return(Some(MirExpr::Global(Atom::from("answer"))))],
        },
    };
    prog.push_decl(MirDecl::Function(f));
    let tokens = emit_decls(&prog, &types).expect("decls should emit");
    let s = tokens.to_string();
    assert!(
        !s.contains("unsafe"),
        "reads of an immutable static must not require `unsafe`, got: {s}"
    );
}

#[test]
fn global_with_non_const_init_returns_not_implemented() {
    let err = emit_global(
        &MirGlobalDecl {
            name: Atom::from("counter"),
            ty: TypeId::from_raw(0),
            mutable: false,
            visibility: Visibility::Public,
            export_name: None,
            init: Some(MirExpr::Global(Atom::from("other"))),
        },
        &TypeTable::new(),
    )
    .expect_err("non-const global init must not emit invalid static initializer");
    assert_eq!(err, BackendError::NotImplemented("const literal kind"));
}

#[test]
fn global_with_const_int_init_emits_initializer() {
    let tokens = emit_global(
        &MirGlobalDecl {
            name: Atom::from("counter"),
            ty: TypeId::from_raw(0),
            mutable: false,
            visibility: Visibility::Public,
            export_name: None,
            init: Some(MirExpr::Int {
                value: 42,
                ty: TypeId::from_raw(0),
            }),
        },
        &TypeTable::new(),
    )
    .expect("const global init should emit");
    let s = tokens.to_string();
    assert!(s.contains("= 42"), "got: {s}");
    assert!(!s.contains("Default :: default"), "got: {s}");
    assert!(!s.contains("unimplemented"), "got: {s}");
}

#[test]
fn public_global_emits_pub_from_visibility_without_export_name() {
    let tokens = emit_global(
        &MirGlobalDecl {
            name: Atom::from("counter"),
            ty: TypeId::from_raw(0),
            mutable: false,
            visibility: Visibility::Public,
            export_name: None,
            init: Some(MirExpr::Int {
                value: 0,
                ty: TypeId::from_raw(0),
            }),
        },
        &TypeTable::new(),
    )
    .expect("const global init should emit");
    let s = tokens.to_string();
    assert!(s.starts_with("pub static counter"), "got: {s}");
}

#[test]
fn private_global_omits_pub_from_visibility() {
    let tokens = emit_global(
        &MirGlobalDecl {
            name: Atom::from("secret"),
            ty: TypeId::from_raw(0),
            mutable: false,
            visibility: Visibility::Private,
            export_name: Some("secret".to_owned()),
            init: Some(MirExpr::Bool(true)),
        },
        &TypeTable::new(),
    )
    .expect("const global init should emit");
    let s = tokens.to_string();
    assert!(s.starts_with("static secret"), "got: {s}");
}

#[test]
fn mutable_global_emits_mut_from_flag() {
    let tokens = emit_global(
        &MirGlobalDecl {
            name: Atom::from("counter"),
            ty: TypeId::from_raw(0),
            mutable: true,
            visibility: Visibility::Public,
            export_name: None,
            init: Some(MirExpr::Int {
                value: 0,
                ty: TypeId::from_raw(0),
            }),
        },
        &TypeTable::new(),
    )
    .expect("const global init should emit");
    let s = tokens.to_string();
    assert!(s.starts_with("pub static mut counter"), "got: {s}");
}

#[test]
fn immutable_global_omits_mut_from_flag() {
    let tokens = emit_global(
        &MirGlobalDecl {
            name: Atom::from("counter"),
            ty: TypeId::from_raw(0),
            mutable: false,
            visibility: Visibility::Public,
            export_name: None,
            init: Some(MirExpr::Int {
                value: 0,
                ty: TypeId::from_raw(0),
            }),
        },
        &TypeTable::new(),
    )
    .expect("const global init should emit");
    let s = tokens.to_string();
    assert!(s.starts_with("pub static counter"), "got: {s}");
    assert!(!s.contains("static mut"), "got: {s}");
}
