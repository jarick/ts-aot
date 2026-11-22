use super::super::emit_decls;
use super::empty_func;
use ts_aot_core::{Atom, FieldId, LocalId, ModuleId, StructId, Type, TypeTable, Visibility};
use ts_aot_ir_mir::{MirDecl, MirFieldDecl, MirParam, MirProgram, MirStructDecl};

#[test]
fn a_generic_param_is_detected_at_any_nesting_depth() {
    let mut types = TypeTable::new();
    let generic_ty = types.intern(&Type::GenericParam {
        id: ts_aot_core::GenericParamId::from_raw(0),
    });
    let mut deep = generic_ty;
    for _ in 0..12 {
        deep = types.intern(&Type::Array { element: deep });
    }
    assert!(
        super::super::mentions_generic_param(deep, &types),
        "a generic parameter nested 12 arrays deep must still be detected"
    );
    let mut concrete = types.intern(&Type::F64);
    for _ in 0..12 {
        concrete = types.intern(&Type::Array { element: concrete });
    }
    assert!(
        !super::super::mentions_generic_param(concrete, &types),
        "a fully concrete type nested 12 arrays deep is not generic"
    );
}

#[test]
fn a_deeply_nested_concrete_signature_is_not_dropped() {
    let mut types = TypeTable::new();
    let mut field_ty = types.intern(&Type::F64);
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    for depth in 0..6 {
        let id = StructId::from_raw(depth);
        prog.push_decl(MirDecl::Struct(MirStructDecl {
            id,
            name: Atom::from(format!("S{depth}")),
            fields: vec![MirFieldDecl {
                id: FieldId::from_raw(0),
                name: Atom::from("next"),
                ty: field_ty,
                mutable: false,
                visibility: Visibility::Public,
            }],
            methods: Vec::new(),
        }));
        field_ty = types.intern(&Type::Struct { id });
    }
    let mut f = empty_func("take_deep");
    f.ret = field_ty;
    assert!(
        !super::super::signature_mentions_generic_param(&f, &types, &prog),
        "a fully concrete signature six structs deep must still be emitted"
    );
}

#[test]
fn a_self_referential_struct_terminates_the_walk() {
    let mut types = TypeTable::new();
    let node_id = StructId::from_raw(0);
    let node_ty = types.intern(&Type::Struct { id: node_id });
    let link_ty = types.intern(&Type::Array { element: node_ty });
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Struct(MirStructDecl {
        id: node_id,
        name: Atom::from("Node"),
        fields: vec![MirFieldDecl {
            id: FieldId::from_raw(0),
            name: Atom::from("children"),
            ty: link_ty,
            mutable: true,
            visibility: Visibility::Public,
        }],
        methods: Vec::new(),
    }));
    let mut f = empty_func("take_node");
    f.params = vec![MirParam {
        id: LocalId::from_raw(0),
        name: Atom::from("n"),
        ty: node_ty,
    }];
    assert!(
        !super::super::signature_mentions_generic_param(&f, &types, &prog),
        "a self-referential concrete struct is not generic"
    );
}

#[test]
fn a_generic_field_nested_deep_is_still_detected() {
    let mut types = TypeTable::new();
    let mut field_ty = types.intern(&Type::GenericParam {
        id: ts_aot_core::GenericParamId::from_raw(0),
    });
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    for depth in 0..5 {
        let id = StructId::from_raw(depth);
        prog.push_decl(MirDecl::Struct(MirStructDecl {
            id,
            name: Atom::from(format!("G{depth}")),
            fields: vec![MirFieldDecl {
                id: FieldId::from_raw(0),
                name: Atom::from("value"),
                ty: field_ty,
                mutable: false,
                visibility: Visibility::Public,
            }],
            methods: Vec::new(),
        }));
        field_ty = types.intern(&Type::Struct { id });
    }
    let mut f = empty_func("take_generic");
    f.ret = field_ty;
    assert!(
        super::super::signature_mentions_generic_param(&f, &types, &prog),
        "a generic field five structs deep must still drop the function"
    );
}

#[test]
fn a_struct_naming_a_dropped_generic_struct_is_dropped_too() {
    let mut types = TypeTable::new();
    let generic_ty = types.intern(&Type::GenericParam {
        id: ts_aot_core::GenericParamId::from_raw(0),
    });
    let inner_id = StructId::from_raw(0);
    let inner_ty = types.intern(&Type::Struct { id: inner_id });
    let outer_id = StructId::from_raw(1);
    let outer_ty = types.intern(&Type::Struct { id: outer_id });
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    let field = |id: u32, name: &str, ty| MirFieldDecl {
        id: FieldId::from_raw(id),
        name: Atom::from(name),
        ty,
        mutable: false,
        visibility: Visibility::Public,
    };
    prog.push_decl(MirDecl::Struct(MirStructDecl {
        id: inner_id,
        name: Atom::from("Inner"),
        fields: vec![field(0, "value", generic_ty)],
        methods: Vec::new(),
    }));
    prog.push_decl(MirDecl::Struct(MirStructDecl {
        id: outer_id,
        name: Atom::from("Outer"),
        fields: vec![field(0, "direct", inner_ty)],
        methods: Vec::new(),
    }));
    let mut f = empty_func("take_outer");
    f.params = vec![MirParam {
        id: LocalId::from_raw(0),
        name: Atom::from("o"),
        ty: outer_ty,
    }];
    prog.push_decl(MirDecl::Function(f));
    let s = emit_decls(&prog, &types)
        .expect("a dropped generic struct must not make emission fail")
        .to_string();
    assert!(
        !s.contains("struct Inner"),
        "the generic struct must be dropped; got: {s}"
    );
    assert!(
        !s.contains("struct Outer"),
        "a struct naming a dropped generic struct must be dropped too; got: {s}"
    );
    assert!(
        !s.contains("fn take_outer"),
        "a function taking such a struct must be dropped; got: {s}"
    );
}

#[test]
fn a_struct_with_only_an_array_of_a_dropped_generic_struct_is_dropped_too() {
    let mut types = TypeTable::new();
    let f64_ty = types.intern(&Type::F64);
    let generic_ty = types.intern(&Type::GenericParam {
        id: ts_aot_core::GenericParamId::from_raw(0),
    });
    let inner_id = StructId::from_raw(0);
    let inner_ty = types.intern(&Type::Struct { id: inner_id });
    let outer_id = StructId::from_raw(1);
    let outer_ty = types.intern(&Type::Struct { id: outer_id });
    let array_of_inner = types.intern(&Type::Array { element: inner_ty });
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    let field = |id: u32, name: &str, ty| MirFieldDecl {
        id: FieldId::from_raw(id),
        name: Atom::from(name),
        ty,
        mutable: false,
        visibility: Visibility::Public,
    };
    prog.push_decl(MirDecl::Struct(MirStructDecl {
        id: inner_id,
        name: Atom::from("Inner"),
        fields: vec![field(0, "value", generic_ty)],
        methods: Vec::new(),
    }));
    prog.push_decl(MirDecl::Struct(MirStructDecl {
        id: outer_id,
        name: Atom::from("Outer"),
        fields: vec![field(0, "items", array_of_inner)],
        methods: Vec::new(),
    }));
    prog.push_decl(MirDecl::Struct(MirStructDecl {
        id: StructId::from_raw(2),
        name: Atom::from("Kept"),
        fields: vec![field(0, "value", f64_ty)],
        methods: Vec::new(),
    }));
    let mut f = empty_func("take_outer");
    f.params = vec![MirParam {
        id: LocalId::from_raw(0),
        name: Atom::from("o"),
        ty: outer_ty,
    }];
    prog.push_decl(MirDecl::Function(f));
    let s = emit_decls(&prog, &types)
        .expect("a dropped generic struct must not make emission fail")
        .to_string();
    assert!(
        s.contains("struct Kept"),
        "a concrete struct in the same program must still be emitted, so the drops below are \
         checked against a non-empty emission; got: {s}"
    );
    assert!(
        !s.contains("struct Inner"),
        "the generic struct must be dropped; got: {s}"
    );
    assert!(
        !s.contains("struct Outer"),
        "a struct whose only dependency on a dropped generic struct is an array field must be \
         dropped too; got: {s}"
    );
    assert!(
        !s.contains("fn take_outer"),
        "a function taking such a struct must be dropped; got: {s}"
    );
}

#[test]
fn a_struct_naming_a_deeply_nested_concrete_struct_is_emitted() {
    let mut types = TypeTable::new();
    let mut field_ty = types.intern(&Type::F64);
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    for depth in 0..6 {
        let id = StructId::from_raw(depth);
        prog.push_decl(MirDecl::Struct(MirStructDecl {
            id,
            name: Atom::from(format!("C{depth}")),
            fields: vec![MirFieldDecl {
                id: FieldId::from_raw(0),
                name: Atom::from("next"),
                ty: field_ty,
                mutable: false,
                visibility: Visibility::Public,
            }],
            methods: Vec::new(),
        }));
        field_ty = types.intern(&Type::Struct { id });
    }
    let s = emit_decls(&prog, &types)
        .expect("a deeply nested concrete chain must emit")
        .to_string();
    assert!(
        s.contains("struct C0") && s.contains("struct C5"),
        "every concrete struct in the chain must be emitted; got: {s}"
    );
}
