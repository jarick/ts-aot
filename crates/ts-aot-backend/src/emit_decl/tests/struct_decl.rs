use super::super::emit_decls;
use super::empty_func;
use ts_aot_core::{
    Atom, FieldId, FunctionId, LocalId, ModuleId, StructId, Type, TypeId, TypeTable, Visibility,
};
use ts_aot_ir_mir::{
    MirBlock, MirBody, MirDecl, MirExpr, MirFieldDecl, MirParam, MirProgram, MirStmt, MirStructDecl,
};
use ts_aot_test_support::check_generated_crate;

#[test]
fn struct_decl_emits_struct_keyword() {
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Struct(MirStructDecl {
        id: StructId::from_raw(0),
        name: Atom::from("Point"),
        fields: Vec::new(),
        methods: Vec::new(),
    }));
    let tokens = emit_decls(&prog, &TypeTable::new()).expect("decls should emit");
    let s = tokens.to_string();
    assert!(s.contains("pub struct Point"), "got: {s}");
}

#[test]
fn struct_decl_emits_ts_class_id_impl_with_struct_id() {
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Struct(MirStructDecl {
        id: StructId::from_raw(42),
        name: Atom::from("Animal"),
        fields: Vec::new(),
        methods: Vec::new(),
    }));
    let tokens = emit_decls(&prog, &TypeTable::new()).expect("decls should emit");
    let s = tokens.to_string();
    assert!(
        s.contains("impl ts_aot_runtime :: TsClassId for Animal"),
        "struct must emit `impl ts_aot_runtime::TsClassId for <name>` so __ts_aot_op_instanceof \
         trait bound is satisfied without a missing import; got: {s}"
    );
    assert!(
        s.contains("fn class_id () -> u32"),
        "TsClassId impl must expose `fn class_id() -> u32`; got: {s}"
    );
    assert!(
        s.contains("42u32") || s.contains("42 u32"),
        "TsClassId impl must use the struct_id raw value (42) as class_id; got: {s}"
    );
}

#[test]
fn struct_with_fields_emits_field_list() {
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Struct(MirStructDecl {
        id: StructId::from_raw(0),
        name: Atom::from("Rect"),
        fields: vec![
            MirFieldDecl {
                id: FieldId::from_raw(0),
                name: Atom::from("width"),
                ty: TypeId::from_raw(0),
                mutable: false,
                visibility: Visibility::Public,
            },
            MirFieldDecl {
                id: FieldId::from_raw(1),
                name: Atom::from("height"),
                ty: TypeId::from_raw(0),
                mutable: false,
                visibility: Visibility::Private,
            },
        ],
        methods: Vec::new(),
    }));
    let tokens = emit_decls(&prog, &TypeTable::new()).expect("decls should emit");
    let s = tokens.to_string();
    assert!(s.contains("pub struct Rect"), "got: {s}");
    assert!(s.contains("width :"), "expected field width, got: {s}");
    assert!(s.contains("height :"), "expected field height, got: {s}");
    assert!(
        s.contains("pub width"),
        "expected `pub` on public field, got: {s}"
    );
    assert!(
        !s.contains("pub height"),
        "private field must not have `pub`, got: {s}"
    );
}

#[test]
fn struct_type_reference_uses_declared_struct_name() {
    let struct_id = StructId::from_raw(7);
    let mut types = TypeTable::new();
    let point_ty = types.intern(&Type::Struct { id: struct_id });
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Struct(MirStructDecl {
        id: struct_id,
        name: Atom::from("Point"),
        fields: Vec::new(),
        methods: Vec::new(),
    }));
    let mut f = empty_func("make_point");
    f.ret = point_ty;
    prog.push_decl(MirDecl::Function(f));

    let tokens = emit_decls(&prog, &types).expect("decls should emit");
    let s = tokens.to_string();
    assert!(s.contains("pub struct Point"), "got: {s}");
    assert!(s.contains("-> Point"), "got: {s}");
    assert!(!s.contains("__struct7"), "got: {s}");
}

#[test]
fn struct_typed_field_emits_struct_lifetime() {
    let mut types = TypeTable::new();
    let inner_id = StructId::from_raw(0);
    let outer_id = StructId::from_raw(1);
    let string_ty = types.intern(&Type::String);
    let inner_ty = types.intern(&Type::Struct { id: inner_id });

    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Struct(MirStructDecl {
        id: inner_id,
        name: Atom::from("Inner"),
        fields: vec![MirFieldDecl {
            id: FieldId::from_raw(0),
            name: Atom::from("s"),
            ty: string_ty,
            mutable: false,
            visibility: Visibility::Public,
        }],
        methods: Vec::new(),
    }));
    prog.push_decl(MirDecl::Struct(MirStructDecl {
        id: outer_id,
        name: Atom::from("Outer"),
        fields: vec![MirFieldDecl {
            id: FieldId::from_raw(0),
            name: Atom::from("inner"),
            ty: inner_ty,
            mutable: false,
            visibility: Visibility::Public,
        }],
        methods: Vec::new(),
    }));

    let tokens = emit_decls(&prog, &types).expect("decls should emit");
    let norm: String = tokens
        .to_string()
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    assert!(
        norm.contains("pubinner:::std::rc::Rc<::core::cell::RefCell<Inner<'a>>>"),
        "a struct-typed field must carry the generated struct's lifetime (`inner: \
         Rc<RefCell<Inner<'a>>>`) instead of relying on elision; got: {norm}"
    );
}

#[test]
fn struct_typed_return_emits_struct_lifetime() {
    let mut types = TypeTable::new();
    let point_id = StructId::from_raw(0);
    let string_ty = types.intern(&Type::String);
    let point_ty = types.intern(&Type::Struct { id: point_id });

    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Struct(MirStructDecl {
        id: point_id,
        name: Atom::from("Point"),
        fields: vec![MirFieldDecl {
            id: FieldId::from_raw(0),
            name: Atom::from("s"),
            ty: string_ty,
            mutable: false,
            visibility: Visibility::Public,
        }],
        methods: Vec::new(),
    }));
    let mut f = empty_func("make_point");
    f.ret = point_ty;
    prog.push_decl(MirDecl::Function(f));

    let tokens = emit_decls(&prog, &types).expect("decls should emit");
    let norm: String = tokens
        .to_string()
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    assert!(
        norm.contains("->Point<'a>"),
        "a struct-typed return must carry the generated struct's lifetime (`-> Point<'a>`) \
         instead of relying on elision; got: {norm}"
    );
}

#[test]
fn static_method_of_lifetime_struct_is_emitted_as_a_free_function() {
    let mut types = TypeTable::new();
    let string_ty = types.intern(&Type::String);
    let box_id = StructId::from_raw(0);

    let param = LocalId::from_raw(0);
    let mut describe = empty_func("Box__describe");
    describe.ret = string_ty;
    describe.params = vec![MirParam {
        id: param,
        name: Atom::from("s"),
        ty: string_ty,
    }];
    describe.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Return(Some(MirExpr::Local(param)))],
        },
    };

    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Struct(MirStructDecl {
        id: box_id,
        name: Atom::from("Box"),
        fields: vec![MirFieldDecl {
            id: FieldId::from_raw(0),
            name: Atom::from("label"),
            ty: string_ty,
            mutable: false,
            visibility: Visibility::Public,
        }],
        methods: vec![describe],
    }));

    let rust = emit_decls(&prog, &types)
        .expect("a struct carrying a static method must emit")
        .to_string();
    let compact: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        compact.contains("impl<'a>Box<'a>{}"),
        "a static method has no receiver, so it must NOT live in the struct's `impl`; the impl \
         block must stay empty, got:\n{rust}"
    );
    assert!(
        compact.contains("fnBox__describe<'a>(__ctx:&ts_aot_runtime::Ctx<'a>,"),
        "a static method must be emitted as a top-level free function declaring its own `'a`; \
         emitting it inside `impl<'a> Box<'a>` would shadow the impl lifetime and Rust rejects \
         the crate with E0496, got:\n{rust}"
    );
    check_generated_crate("static_method_free_function_lifetime", &rust);
}

#[test]
fn called_static_method_resolves_as_a_free_function() {
    let mut types = TypeTable::new();
    let f64_ty = types.intern(&Type::F64);

    let mut zero = empty_func("Counter__zero");
    zero.id = FunctionId::from_raw(0);
    zero.ret = f64_ty;
    zero.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Return(Some(MirExpr::Float {
                value: 0.0,
                ty: f64_ty,
            }))],
        },
    };

    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Struct(MirStructDecl {
        id: StructId::from_raw(0),
        name: Atom::from("Counter"),
        fields: Vec::new(),
        methods: vec![zero],
    }));

    let mut call = empty_func("f");
    call.id = FunctionId::from_raw(1);
    call.export_name = Some("f".to_owned());
    call.ret = f64_ty;
    call.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Return(Some(MirExpr::Call {
                callee: FunctionId::from_raw(0),
                args: Vec::new(),
                ty: f64_ty,
            }))],
        },
    };
    prog.push_decl(MirDecl::Function(call));

    let rust = emit_decls(&prog, &types)
        .expect("a program calling a static method must emit")
        .to_string();
    let compact: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        compact.contains("fnCounter__zero<'a>(__ctx:&ts_aot_runtime::Ctx<'a>,"),
        "a static method must be a top-level free function so that a call site can name it \
         without a `Counter::` qualifier, got:\n{rust}"
    );
    assert!(
        compact.contains("returnCounter__zero(__ctx);"),
        "a call to a static method must stay an unqualified call into the emitted free function, \
         got:\n{rust}"
    );
    check_generated_crate("called_static_method", &rust);
}
