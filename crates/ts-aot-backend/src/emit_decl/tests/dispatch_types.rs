use super::super::emit_decls;
use super::{empty_func, norm, signature_fragment};
use ts_aot_core::{Atom, FunctionId, LocalId, ModuleId, Type, TypeTable};
use ts_aot_ir_mir::{MirDecl, MirParam, MirProgram};

#[test]
fn union_typed_param_and_return_exclude_function_from_dispatch_table() {
    let mut types = TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let string_ty = types.intern(&Type::String);
    let union_param_ty = types.intern(&Type::Union {
        variants: vec![i64_ty, string_ty],
    });
    let union_return_ty = types.intern(&Type::Union {
        variants: vec![i64_ty, string_ty],
    });

    let mut with_union_param = empty_func("with_union_param");
    with_union_param.ret = i64_ty;
    with_union_param.params = vec![MirParam {
        id: LocalId::from_raw(0),
        name: Atom::from("v"),
        ty: union_param_ty,
    }];

    let mut with_union_return = empty_func("with_union_return");
    with_union_return.id = FunctionId::from_raw(1);
    with_union_return.ret = union_return_ty;
    with_union_return.params = vec![MirParam {
        id: LocalId::from_raw(0),
        name: Atom::from("x"),
        ty: i64_ty,
    }];

    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(with_union_param));
    prog.push_decl(MirDecl::Function(with_union_return));
    let tokens = emit_decls(&prog, &types).expect("decls should emit");
    let s = tokens.to_string();
    let param_sig = norm(signature_fragment(&s, "with_union_param"));
    assert!(
        param_sig.contains("v:()"),
        "function with union-typed param must still emit, with the union collapsed to the unit ABI \
         placeholder `()` in its own signature. Got signature fragment: `{param_sig}`"
    );
    let return_sig = norm(signature_fragment(&s, "with_union_return"));
    assert!(
        return_sig.contains("->()"),
        "function with union-typed return must still emit, with the union collapsed to the unit ABI \
         placeholder `()` in its own signature. Got signature fragment: `{return_sig}`"
    );
    assert!(
        !s.contains("__ts_aot_dispatch_with_union_param"),
        "union-typed param => `()` ABI => not u64-packable => no `fn(&[u64]) -> u64` wrapper. Got: {s}"
    );
    assert!(
        !s.contains("__ts_aot_dispatch_with_union_return"),
        "union-typed return => `()` ABI => not u64-packable => no `fn(&[u64]) -> u64` wrapper. Got: {s}"
    );
    assert!(
        !s.contains("__TS_AOT_DISPATCH_TABLE"),
        "no sync dispatchable function (both excluded by union types) => no dispatch table. Got: {s}"
    );
}

#[test]
fn union_type_emits_unit_placeholder_at_call_site() {
    let mut types = TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let string_ty = types.intern(&Type::String);
    let union_ty = types.intern(&Type::Union {
        variants: vec![i64_ty, string_ty],
    });

    let mut f = empty_func("identity");
    f.ret = union_ty;
    f.params = vec![MirParam {
        id: LocalId::from_raw(0),
        name: Atom::from("v"),
        ty: union_ty,
    }];
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(f));
    let tokens = emit_decls(&prog, &types).expect("decls should emit");
    let s = tokens.to_string();

    let sig = signature_fragment(&s, "identity");
    let sig_norm: String = sig.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        sig_norm.contains("v:()") && sig_norm.contains("->()"),
        "both param and return of `i64 | string` must emit as `()` (unit placeholder) in the \n         function signature, so the source-level TS union type stays representable in Rust \n         until Phase 5 introduces a real union runtime. Whitespace-insensitive. Got signature fragment: `{sig}`"
    );
}

#[test]
fn intersection_typed_param_and_return_exclude_function_from_dispatch_table() {
    let mut types = TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let string_ty = types.intern(&Type::String);
    let intersection_param_ty = types.intern(&Type::Intersection {
        parts: vec![i64_ty, string_ty],
    });
    let intersection_return_ty = types.intern(&Type::Intersection {
        parts: vec![i64_ty, string_ty],
    });

    let mut with_intersection_param = empty_func("with_intersection_param");
    with_intersection_param.ret = i64_ty;
    with_intersection_param.params = vec![MirParam {
        id: LocalId::from_raw(0),
        name: Atom::from("v"),
        ty: intersection_param_ty,
    }];

    let mut with_intersection_return = empty_func("with_intersection_return");
    with_intersection_return.id = FunctionId::from_raw(1);
    with_intersection_return.ret = intersection_return_ty;
    with_intersection_return.params = vec![MirParam {
        id: LocalId::from_raw(0),
        name: Atom::from("x"),
        ty: i64_ty,
    }];

    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(with_intersection_param));
    prog.push_decl(MirDecl::Function(with_intersection_return));
    let tokens = emit_decls(&prog, &types).expect("decls should emit");
    let s = tokens.to_string();
    let param_sig = norm(signature_fragment(&s, "with_intersection_param"));
    assert!(
        param_sig.contains("v:()"),
        "function with intersection-typed param must still emit, with the intersection collapsed \
         to the unit ABI placeholder `()` in its own signature. Got signature fragment: \
         `{param_sig}`"
    );
    let return_sig = norm(signature_fragment(&s, "with_intersection_return"));
    assert!(
        return_sig.contains("->()"),
        "function with intersection-typed return must still emit, with the intersection collapsed \
         to the unit ABI placeholder `()` in its own signature. Got signature fragment: \
         `{return_sig}`"
    );
    assert!(
        !s.contains("__ts_aot_dispatch_with_intersection_param"),
        "intersection-typed param => `()` ABI => not u64-packable => no `fn(&[u64]) -> u64` wrapper. \n         Got: {s}"
    );
    assert!(
        !s.contains("__ts_aot_dispatch_with_intersection_return"),
        "intersection-typed return => `()` ABI => not u64-packable => no `fn(&[u64]) -> u64` wrapper. \n         Got: {s}"
    );
    assert!(
        !s.contains("__TS_AOT_DISPATCH_TABLE"),
        "no sync dispatchable function (both excluded by intersection types) => no dispatch table. \n         Got: {s}"
    );
}

#[test]
fn intersection_type_emits_unit_placeholder_at_call_site() {
    let mut types = TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let string_ty = types.intern(&Type::String);
    let intersection_ty = types.intern(&Type::Intersection {
        parts: vec![i64_ty, string_ty],
    });

    let mut f = empty_func("combine");
    f.ret = intersection_ty;
    f.params = vec![MirParam {
        id: LocalId::from_raw(0),
        name: Atom::from("v"),
        ty: intersection_ty,
    }];
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(f));
    let tokens = emit_decls(&prog, &types).expect("decls should emit");
    let s = tokens.to_string();

    let sig = signature_fragment(&s, "combine");
    let sig_norm: String = sig.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        sig_norm.contains("v:()") && sig_norm.contains("->()"),
        "both param and return of `i64 & string` must emit as `()` (unit placeholder) in the \n         function signature, so the source-level TS intersection type stays representable in \n         Rust until Phase 5 introduces a real intersection runtime. Whitespace-insensitive. \n         Got signature fragment: `{sig}`"
    );
}

#[test]
fn tuple_typed_param_and_return_exclude_function_from_dispatch_table() {
    let mut types = TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let string_ty = types.intern(&Type::String);
    let tuple_param_ty = types.intern(&Type::Tuple {
        elements: vec![i64_ty, string_ty],
    });
    let tuple_return_ty = types.intern(&Type::Tuple {
        elements: vec![i64_ty, string_ty],
    });

    let mut with_tuple_param = empty_func("with_tuple_param");
    with_tuple_param.ret = i64_ty;
    with_tuple_param.params = vec![MirParam {
        id: LocalId::from_raw(0),
        name: Atom::from("v"),
        ty: tuple_param_ty,
    }];

    let mut with_tuple_return = empty_func("with_tuple_return");
    with_tuple_return.id = FunctionId::from_raw(1);
    with_tuple_return.ret = tuple_return_ty;
    with_tuple_return.params = vec![MirParam {
        id: LocalId::from_raw(0),
        name: Atom::from("x"),
        ty: i64_ty,
    }];

    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(with_tuple_param));
    prog.push_decl(MirDecl::Function(with_tuple_return));
    let tokens = emit_decls(&prog, &types).expect("decls should emit");
    let s = tokens.to_string();
    let param_sig = norm(signature_fragment(&s, "with_tuple_param"));
    assert!(
        param_sig.contains("v:()"),
        "function with tuple-typed param must still emit, with the tuple collapsed to the unit ABI \
         placeholder `()` in its own signature. Got signature fragment: `{param_sig}`"
    );
    let return_sig = norm(signature_fragment(&s, "with_tuple_return"));
    assert!(
        return_sig.contains("->()"),
        "function with tuple-typed return must still emit, with the tuple collapsed to the unit ABI \
         placeholder `()` in its own signature. Got signature fragment: `{return_sig}`"
    );
    assert!(
        !s.contains("__ts_aot_dispatch_with_tuple_param"),
        "tuple-typed param => `()` ABI => not u64-packable => no `fn(&[u64]) -> u64` wrapper. \n         Got: {s}"
    );
    assert!(
        !s.contains("__ts_aot_dispatch_with_tuple_return"),
        "tuple-typed return => `()` ABI => not u64-packable => no `fn(&[u64]) -> u64` wrapper. \n         Got: {s}"
    );
    assert!(
        !s.contains("__TS_AOT_DISPATCH_TABLE"),
        "no sync dispatchable function (both excluded by tuple types) => no dispatch table. \n         Got: {s}"
    );
}

#[test]
fn tuple_type_emits_unit_placeholder_at_call_site() {
    let mut types = TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let string_ty = types.intern(&Type::String);
    let tuple_ty = types.intern(&Type::Tuple {
        elements: vec![i64_ty, string_ty],
    });

    let mut f = empty_func("combine");
    f.ret = tuple_ty;
    f.params = vec![MirParam {
        id: LocalId::from_raw(0),
        name: Atom::from("v"),
        ty: tuple_ty,
    }];
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(f));
    let tokens = emit_decls(&prog, &types).expect("decls should emit");
    let s = tokens.to_string();

    let sig = signature_fragment(&s, "combine");
    let sig_norm: String = sig.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        sig_norm.contains("v:()") && sig_norm.contains("->()"),
        "both param and return of `[i64, string]` must emit as `()` (unit placeholder) in the \n         function signature, so the source-level TS tuple type stays representable in Rust \n         until Phase 5 introduces a real tuple runtime. Whitespace-insensitive. \n         Got signature fragment: `{sig}`"
    );
}

#[test]
fn array_typed_param_and_return_exclude_function_from_dispatch_table() {
    let mut types = TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let array_param_ty = types.intern(&Type::Array { element: i64_ty });
    let array_return_ty = types.intern(&Type::Array { element: i64_ty });

    let mut with_array_param = empty_func("with_array_param");
    with_array_param.ret = i64_ty;
    with_array_param.params = vec![MirParam {
        id: LocalId::from_raw(0),
        name: Atom::from("v"),
        ty: array_param_ty,
    }];

    let mut with_array_return = empty_func("with_array_return");
    with_array_return.id = FunctionId::from_raw(1);
    with_array_return.ret = array_return_ty;
    with_array_return.params = vec![MirParam {
        id: LocalId::from_raw(0),
        name: Atom::from("x"),
        ty: i64_ty,
    }];

    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(with_array_param));
    prog.push_decl(MirDecl::Function(with_array_return));
    let tokens = emit_decls(&prog, &types).expect("decls should emit");
    let s = tokens.to_string();

    assert!(
        !s.contains("__ts_aot_dispatch_with_array_param"),
        "array-typed param => Array not in is_u64_arg_packable set => no `fn(&[u64]) -> u64` wrapper. Got: {s}"
    );
    assert!(
        !s.contains("__ts_aot_dispatch_with_array_return"),
        "array-typed return => Array not in is_u64_ret_packable set => no `fn(&[u64]) -> u64` wrapper. Got: {s}"
    );
    assert!(
        !s.contains("__TS_AOT_DISPATCH_TABLE"),
        "no sync dispatchable function (both excluded by array types) => no dispatch table. Got: {s}"
    );
}

#[test]
fn array_typed_return_emits_vec_placeholder() {
    let mut types = TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let array_ty = types.intern(&Type::Array { element: i64_ty });

    let mut f = empty_func("make_array");
    f.ret = array_ty;
    f.params = vec![];
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(f));
    let tokens = emit_decls(&prog, &types).expect("decls should emit");
    let s = tokens.to_string();

    let sig = signature_fragment(&s, "make_array");
    let sig_norm: String = sig.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        sig_norm.contains("->ts_aot_runtime::bumpalo::collections::Vec<'a,i64>"),
        "return type `i64[]` must emit as `ts_aot_runtime::bumpalo::collections::Vec<'a, i64>` in the signature. \n         Whitespace-insensitive. Got signature fragment: `{sig}`"
    );
}

#[test]
fn array_typed_param_emits_vec_placeholder() {
    let mut types = TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let array_ty = types.intern(&Type::Array { element: i64_ty });

    let mut f = empty_func("take_array");
    f.ret = i64_ty;
    f.params = vec![MirParam {
        id: LocalId::from_raw(0),
        name: Atom::from("v"),
        ty: array_ty,
    }];
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(f));
    let tokens = emit_decls(&prog, &types).expect("decls should emit");
    let s = tokens.to_string();

    let sig = signature_fragment(&s, "take_array");
    let sig_norm: String = sig.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        sig_norm.contains("v:ts_aot_runtime::bumpalo::collections::Vec<'a,i64>"),
        "param typed `i64[]` must emit as `ts_aot_runtime::bumpalo::collections::Vec<'a, i64>` in the signature. \n         Whitespace-insensitive. Got signature fragment: `{sig}`"
    );
}
