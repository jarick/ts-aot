use super::ident::sanitize_ident;
use super::types::emit_type_id;
use super::{emit_decls, emit_function, emit_global};
use crate::BackendError;
use ts_aot_core::{
    Atom, FieldId, FunctionId, LocalId, ModuleId, StructId, Type, TypeId, TypeTable, Visibility,
};
use ts_aot_ir_mir::{
    BinaryOp, ConstValue, FunctionEffects, FunctionKind, MirBlock, MirBody, MirDecl, MirExpr,
    MirFieldDecl, MirFunctionDecl, MirGlobalDecl, MirLocalDecl, MirParam, MirPlace, MirPlaceBase,
    MirProgram, MirStmt, MirStructDecl, RuntimeOp, SwitchCase,
};

fn empty_func(name: &str) -> MirFunctionDecl {
    MirFunctionDecl {
        id: FunctionId::from_raw(0),
        name: Atom::from(name),
        export_name: None,
        params: Vec::new(),
        ret: TypeId::from_raw(0),
        throws: None,
        body: MirBody::default(),
        kind: FunctionKind::Plain,
        effects: FunctionEffects::default(),
    }
}

fn signature_fragment<'a>(s: &'a str, fn_name: &str) -> &'a str {
    s.split(&format!("fn {fn_name} ("))
        .nth(1)
        .and_then(|rest| rest.split('{').next())
        .unwrap_or("")
}

fn for_header_fragment<'a>(s: &'a str, fn_name: &str) -> &'a str {
    let after_fn = s.split(&format!("fn {fn_name} (")).nth(1).unwrap_or("");
    after_fn
        .split(" for ")
        .nth(1)
        .and_then(|rest| rest.split('{').next())
        .unwrap_or("")
}

#[test]
fn empty_program_emits_no_decls() {
    let prog = MirProgram::new(ModuleId::from_raw(0));
    let types = TypeTable::new();
    let tokens = emit_decls(&prog, &types).expect("decls should emit");
    assert!(tokens.is_empty());
}

#[test]
fn dispatchable_i64_function_emits_wrapper_and_table_entry() {
    let mut types = TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let mut f = empty_func("add");
    f.ret = i64_ty;
    f.params = vec![MirParam {
        id: LocalId::from_raw(0),
        name: Atom::from("x"),
        ty: i64_ty,
    }];
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(f));
    let tokens = emit_decls(&prog, &types).expect("decls should emit");
    let s = tokens.to_string();
    assert!(
        s.contains("pub fn __ts_aot_dispatch_add (_args : & [u64]) -> u64"),
        "dispatch wrapper must be emitted for i64-typed plain function, got: {s}"
    );
    assert!(
        s.contains("let x : i64 = __slot_0 as i64 ;"),
        "wrapper must unpack i64 arg via `as i64` cast, got: {s}"
    );
    assert!(
        s.contains("__result as u64"),
        "wrapper must pack i64 return via `as u64`, got: {s}"
    );
    assert!(
        s.contains("const __TS_AOT_DISPATCH_TABLE"),
        "dispatch table constant must be emitted when any dispatchable function exists, got: {s}"
    );
    assert!(
        s.contains("(\"add\" , __ts_aot_dispatch_add as fn (& [u64]) -> u64)"),
        "table entry must reference the wrapper with original function name as string, got: {s}"
    );
}

#[test]
fn f64_param_function_emits_wrapper_with_from_bits() {
    let mut types = TypeTable::new();
    let f64_ty = types.intern(&Type::F64);
    let mut f = empty_func("sqrt_wrap");
    f.ret = f64_ty;
    f.params = vec![MirParam {
        id: LocalId::from_raw(0),
        name: Atom::from("x"),
        ty: f64_ty,
    }];
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(f));
    let tokens = emit_decls(&prog, &types).expect("decls should emit");
    let s = tokens.to_string();
    assert!(
        s.contains("from_bits"),
        "f64 param must be unpacked via f64::from_bits(u64), got: {s}"
    );
    assert!(
        s.contains("to_bits"),
        "f64 return must be packed via to_bits(), got: {s}"
    );
}

#[test]
fn non_dispatchable_function_omits_wrapper() {
    let mut types = TypeTable::new();
    let string_ty = types.intern(&Type::String);
    let mut f = empty_func("greet");
    f.ret = TypeId::from_raw(0);
    f.params = vec![MirParam {
        id: LocalId::from_raw(0),
        name: Atom::from("name"),
        ty: string_ty,
    }];
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(f));
    let tokens = emit_decls(&prog, &types).expect("decls should emit");
    let s = tokens.to_string();
    assert!(
        !s.contains("__ts_aot_dispatch_greet"),
        "String-typed param is not u64-packable, so no wrapper/entry must be emitted, got: {s}"
    );
    assert!(
        !s.contains("__TS_AOT_DISPATCH_TABLE"),
        "no dispatchable function => no dispatch table emitted, got: {s}"
    );
}

#[test]
fn void_typed_param_excludes_function_from_dispatch_table() {
    let mut types = TypeTable::new();
    let void_ty = types.intern(&Type::Void);
    let i64_ty = types.intern(&Type::I64);
    let mut f = empty_func("weird");
    f.ret = i64_ty;
    f.params = vec![MirParam {
        id: LocalId::from_raw(0),
        name: Atom::from("ghost"),
        ty: void_ty,
    }];
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(f));
    let tokens = emit_decls(&prog, &types).expect("decls should emit");
    let s = tokens.to_string();
    assert!(
        !s.contains("__ts_aot_dispatch_weird"),
        "Void-typed param has no `&[u64]` representation (wrapper would `let __slot_N = args[N]` without binding the param name, leaving `ghost` undefined). Got: {s}"
    );
    assert!(
        !s.contains("__TS_AOT_DISPATCH_TABLE"),
        "no dispatchable function => no dispatch table, got: {s}"
    );
}

#[test]
fn void_typed_return_is_still_dispatchable() {
    let mut types = TypeTable::new();
    let void_ty = types.intern(&Type::Void);
    let mut f = empty_func("log_something");
    f.ret = void_ty;
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(f));
    let tokens = emit_decls(&prog, &types).expect("decls should emit");
    let s = tokens.to_string();
    assert!(
        s.contains("fn __ts_aot_dispatch_log_something"),
        "Void return must still be dispatchable (packed as 0), got: {s}"
    );
    assert!(
        s.contains('0'),
        "Void return must pack as `0` literal, got: {s}"
    );
}

#[test]
fn two_dispatchable_functions_emit_two_separate_table_entries() {
    let mut types = TypeTable::new();
    let i64_ty = types.intern(&Type::I64);

    let mut foo = empty_func("foo");
    foo.ret = i64_ty;
    foo.params = vec![MirParam {
        id: LocalId::from_raw(0),
        name: Atom::from("x"),
        ty: i64_ty,
    }];
    foo.body = MirBody {
        locals: vec![],
        block: MirBlock { stmts: vec![] },
    };

    let mut bar = empty_func("bar");
    bar.id = FunctionId::from_raw(1);
    bar.ret = i64_ty;
    bar.params = vec![MirParam {
        id: LocalId::from_raw(0),
        name: Atom::from("y"),
        ty: i64_ty,
    }];
    bar.body = MirBody {
        locals: vec![],
        block: MirBlock { stmts: vec![] },
    };

    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(foo));
    prog.push_decl(MirDecl::Function(bar));
    let tokens = emit_decls(&prog, &types).expect("decls should emit");
    let s = tokens.to_string();

    assert!(
        s.contains("__ts_aot_dispatch_foo"),
        "function `foo` must get its own dispatch wrapper, got: {s}"
    );
    assert!(
        s.contains("__ts_aot_dispatch_bar"),
        "function `bar` must get its own dispatch wrapper, got: {s}"
    );

    let table_str = extract_dispatch_table(&s);
    assert!(
        table_str.contains("\"foo\""),
        "dispatch table must contain entry for `foo`, got: {table_str}"
    );
    assert!(
        table_str.contains("\"bar\""),
        "dispatch table must contain entry for `bar`, got: {table_str}"
    );
    let foo_pos = table_str.find("\"foo\"").expect("foo present");
    let bar_pos = table_str.find("\"bar\"").expect("bar present");
    assert!(
        bar_pos > foo_pos,
        "`bar` entry must come after `foo` entry (list order), got foo@{foo_pos} bar@{bar_pos}: {table_str}"
    );
    let between = &table_str[foo_pos..bar_pos];
    assert!(
        between.contains(") ,"),
        "entries must be separated by `) ,` (close-then-comma) — concatenated entries like `)(\"bar\"` would produce invalid Rust. Got between: {between}"
    );
}

fn extract_dispatch_table(s: &str) -> String {
    let after_table = s.find("__TS_AOT_DISPATCH_TABLE").unwrap_or(0);
    let from = s[after_table..]
        .find('&')
        .map_or(after_table, |i| after_table + i);
    let to = s[from..].find(';').map_or(s.len(), |i| from + i);
    s[from..to].to_string()
}

#[test]
fn async_function_is_excluded_from_dispatch_table() {
    let mut types = TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let mut f = empty_func("async_add");
    f.ret = i64_ty;
    f.params = vec![MirParam {
        id: LocalId::from_raw(0),
        name: Atom::from("x"),
        ty: i64_ty,
    }];
    f.effects.is_async = true;
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(f));
    let tokens = emit_decls(&prog, &types).expect("decls should emit");
    let s = tokens.to_string();
    assert!(
        !s.contains("__ts_aot_dispatch_async_add"),
        "async functions cannot be wrapped in sync fn(&[u64]) -> u64, so they must be excluded from dispatch table. Got: {s}"
    );
    assert!(
        !s.contains("__TS_AOT_DISPATCH_TABLE"),
        "no sync dispatchable function => no dispatch table, got: {s}"
    );
    assert!(
        s.contains("async fn async_add"),
        "async function itself must still emit with `async fn` keyword, got: {s}"
    );
}

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
    let s_norm: String = s.chars().filter(|c| !c.is_whitespace()).collect();

    assert!(
        s_norm.contains("fnwith_union_param") && s_norm.contains("v:()"),
        "function with union-typed param must still emit, with the union collapsed to the unit \n         ABI placeholder `()`. Whitespace-insensitive (handles `v: ()` or `v : ()`). Got: {s}"
    );
    assert!(
        s_norm.contains("fnwith_union_return") && s_norm.contains("->()"),
        "function with union-typed return must still emit, with the union collapsed to the unit \n         ABI placeholder `()`. Whitespace-insensitive. Got: {s}"
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
    let s_norm: String = s.chars().filter(|c| !c.is_whitespace()).collect();

    assert!(
        s_norm.contains("fnwith_intersection_param") && s_norm.contains("v:()"),
        "function with intersection-typed param must still emit, with the intersection collapsed \n         to the unit ABI placeholder `()`. Whitespace-insensitive (handles `v: ()` or `v : ()`). Got: {s}"
    );
    assert!(
        s_norm.contains("fnwith_intersection_return") && s_norm.contains("->()"),
        "function with intersection-typed return must still emit, with the intersection collapsed \n         to the unit ABI placeholder `()`. Whitespace-insensitive. Got: {s}"
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
    let s_norm: String = s.chars().filter(|c| !c.is_whitespace()).collect();

    assert!(
        s_norm.contains("fnwith_tuple_param") && s_norm.contains("v:()"),
        "function with tuple-typed param must still emit, with the tuple collapsed to the unit ABI placeholder `()`. Whitespace-insensitive (handles `v: ()` or `v : ()`). Got: {s}"
    );
    assert!(
        s_norm.contains("fnwith_tuple_return") && s_norm.contains("->()"),
        "function with tuple-typed return must still emit, with the tuple collapsed to the unit ABI placeholder `()`. Whitespace-insensitive. Got: {s}"
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
        sig_norm.contains("->Vec<i64>"),
        "return type `i64[]` must emit as `Vec<i64>` in the function signature. \n         Whitespace-insensitive. Got signature fragment: `{sig}`"
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
        sig_norm.contains("v:Vec<i64>"),
        "param typed `i64[]` must emit as `Vec<i64>` in the function signature. \n         Whitespace-insensitive. Got signature fragment: `{sig}`"
    );
}

#[test]
fn plain_function_emits_fn_signature() {
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(empty_func("greet")));
    let types = TypeTable::new();
    let tokens = emit_decls(&prog, &types).expect("decls should emit");
    let s = tokens.to_string();
    assert!(s.contains("fn greet"), "got: {s}");
    assert!(s.contains("->"), "expected ret arrow, got: {s}");
}

#[test]
fn exported_function_emits_pub_keyword() {
    let mut f = empty_func("render");
    f.export_name = Some("render".to_owned());
    let tokens = emit_function(&f, &TypeTable::new()).expect("function should emit");
    let s = tokens.to_string();
    assert!(s.starts_with("pub "), "expected `pub` prefix, got: {s}");
}

#[test]
fn private_function_omits_pub_keyword() {
    let tokens =
        emit_function(&empty_func("internal"), &TypeTable::new()).expect("function should emit");
    let s = tokens.to_string();
    assert!(
        !s.contains("pub "),
        "private function should not have `pub`, got: {s}"
    );
}

#[test]
fn async_function_emits_async_keyword() {
    let mut f = empty_func("fetch_data");
    f.effects.is_async = true;
    let tokens = emit_function(&f, &TypeTable::new()).expect("function should emit");
    let s = tokens.to_string();
    assert!(s.contains("async fn"), "got: {s}");
}

#[test]
fn function_body_return_emits_expression() {
    let mut types = TypeTable::new();
    let int_ty = types.intern(&Type::I32);
    let mut f = empty_func("answer");
    f.ret = int_ty;
    f.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Return(Some(MirExpr::Int {
                value: 42,
                ty: int_ty,
            }))],
        },
    };
    let tokens = emit_function(&f, &types).expect("function should emit");
    let s = tokens.to_string();
    assert!(s.contains("return 42"), "got: {s}");
    assert!(!s.contains("unimplemented"), "got: {s}");
}

#[test]
fn function_body_let_binary_and_return_emits_statements() {
    let mut types = TypeTable::new();
    let int_ty = types.intern(&Type::I32);
    let local = LocalId::from_raw(0);
    let mut f = empty_func("sum");
    f.ret = int_ty;
    f.body = MirBody {
        locals: vec![MirLocalDecl {
            id: local,
            name: Atom::from("total"),
            ty: int_ty,
            mutable: false,
        }],
        block: MirBlock {
            stmts: vec![
                MirStmt::Let {
                    local,
                    ty: int_ty,
                    init: Some(MirExpr::Binary {
                        op: BinaryOp::Add,
                        left: Box::new(MirExpr::Int {
                            value: 1,
                            ty: int_ty,
                        }),
                        right: Box::new(MirExpr::Int {
                            value: 2,
                            ty: int_ty,
                        }),
                        ty: int_ty,
                    }),
                    mutable: false,
                },
                MirStmt::Return(Some(MirExpr::Local(local))),
            ],
        },
    };
    let tokens = emit_function(&f, &types).expect("function should emit");
    let s = tokens.to_string();
    assert!(s.contains("let total : i32 = (1 + 2)"), "got: {s}");
    assert!(s.contains("return total"), "got: {s}");
}

#[test]
fn method_kind_emits_self_param() {
    let mut f = empty_func("method");
    f.kind = FunctionKind::Method {
        owner: StructId::from_raw(0),
        self_param: LocalId::from_raw(0),
    };
    let tokens = emit_function(&f, &TypeTable::new()).expect("function should emit");
    let s = tokens.to_string();
    assert!(s.contains("self"), "method must emit `self`, got: {s}");
}

#[test]
fn method_kind_omits_synthetic_this_param() {
    let mut types = TypeTable::new();
    let number_ty = types.intern(&Type::I32);
    let mut f = empty_func("method");
    f.kind = FunctionKind::Method {
        owner: StructId::from_raw(0),
        self_param: LocalId::from_raw(0),
    };
    f.params = vec![
        MirParam {
            id: LocalId::from_raw(0),
            name: Atom::from("this"),
            ty: number_ty,
        },
        MirParam {
            id: LocalId::from_raw(1),
            name: Atom::from("value"),
            ty: number_ty,
        },
    ];
    let tokens = emit_function(&f, &types).expect("function should emit");
    let s = tokens.to_string();
    assert!(
        s.contains("self"),
        "method must emit Rust receiver, got: {s}"
    );
    assert!(
        !s.contains("this :"),
        "method signature must hide synthetic receiver param, got: {s}"
    );
    assert!(s.contains("value : i32"), "expected value param, got: {s}");
}

#[test]
fn method_writing_self_field_emits_mut_self() {
    let mut types = TypeTable::new();
    let i32_ty = types.intern(&Type::I32);
    let owner = StructId::from_raw(7);
    let self_id = LocalId::from_raw(0);
    let field_id = FieldId::from_raw(0);

    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Struct(MirStructDecl {
        id: owner,
        name: Atom::from("Box"),
        fields: vec![MirFieldDecl {
            id: field_id,
            name: Atom::from("x"),
            ty: i32_ty,
            mutable: true,
            visibility: Visibility::Public,
        }],
        methods: Vec::new(),
    }));

    let mut f = empty_func("set_x");
    f.ret = TypeId::from_raw(0);
    f.kind = FunctionKind::Method {
        owner,
        self_param: self_id,
    };
    f.params = vec![MirParam {
        id: self_id,
        name: Atom::from("self"),
        ty: types.intern(&Type::Struct { id: owner }),
    }];
    f.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Assign {
                target: MirPlace::Field {
                    base: Box::new(MirPlaceBase::Local(self_id)),
                    field: field_id,
                    ty: i32_ty,
                },
                value: MirExpr::Int {
                    value: 1,
                    ty: i32_ty,
                },
            }],
        },
    };
    prog.push_decl(MirDecl::Function(f));

    let tokens = emit_decls(&prog, &types).expect("decls should emit");
    let s = tokens.to_string();
    let sig = signature_fragment(&s, "set_x");
    assert!(
        sig.contains("mut self ,"),
        "method that writes to a self field must emit `mut self,` in the signature, got signature fragment: `{sig}`"
    );
    assert!(
        !sig.contains("self , mut self"),
        "signature must not emit `self, mut self`, got: `{sig}`"
    );
}

#[test]
fn method_consuming_self_via_generator_next_emits_mut_self() {
    let mut types = TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let gen_result_ty = types.intern(&Type::GeneratorResult { inner: i64_ty });
    let self_id = LocalId::from_raw(0);
    let temp = LocalId::from_raw(1);

    let mut f = empty_func("drive");
    f.ret = TypeId::from_raw(0);
    f.kind = FunctionKind::Method {
        owner: StructId::from_raw(0),
        self_param: self_id,
    };
    f.params = vec![MirParam {
        id: self_id,
        name: Atom::from("self"),
        ty: i64_ty,
    }];
    f.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Runtime {
                op: RuntimeOp::GeneratorNext,
                args: vec![MirExpr::Local(self_id)],
                dest: Some(temp),
                ty: gen_result_ty,
                target_ty: None,
            }],
        },
    };
    let tokens = emit_function(&f, &types).expect("function should emit");
    let s = tokens.to_string();
    let sig = signature_fragment(&s, "drive");
    assert!(
        sig.contains("mut self ,"),
        "method that drives self.next() must emit `mut self,` in the signature, got signature fragment: `{sig}`"
    );
}

#[test]
fn method_reading_self_only_emits_immutable_self() {
    let mut types = TypeTable::new();
    let i32_ty = types.intern(&Type::I32);
    let self_id = LocalId::from_raw(0);
    let result = LocalId::from_raw(1);
    let mut f = empty_func("get_x");
    f.ret = TypeId::from_raw(0);
    f.kind = FunctionKind::Method {
        owner: StructId::from_raw(0),
        self_param: self_id,
    };
    f.params = vec![MirParam {
        id: self_id,
        name: Atom::from("self"),
        ty: i32_ty,
    }];
    f.body = MirBody {
        locals: vec![MirLocalDecl {
            id: result,
            name: Atom::from("result"),
            ty: i32_ty,
            mutable: false,
        }],
        block: MirBlock {
            stmts: vec![MirStmt::Assign {
                target: MirPlace::Local { id: result },
                value: MirExpr::Local(self_id),
            }],
        },
    };
    let tokens = emit_function(&f, &types).expect("function should emit");
    let s = tokens.to_string();
    let sig = signature_fragment(&s, "get_x");
    assert!(
        sig.contains("self ,") && !sig.contains("mut self"),
        "method that only reads self must emit `self,` (no `mut`), got signature fragment: `{sig}`"
    );
}

#[test]
fn reassigned_param_emits_mut_in_signature() {
    let mut types = TypeTable::new();
    let i32_ty = types.intern(&Type::I32);
    let param = LocalId::from_raw(1);
    let mut f = empty_func("reset");
    f.ret = TypeId::from_raw(0);
    f.params = vec![MirParam {
        id: param,
        name: Atom::from("p"),
        ty: i32_ty,
    }];
    f.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Assign {
                target: MirPlace::Local { id: param },
                value: MirExpr::Int {
                    value: 0,
                    ty: i32_ty,
                },
            }],
        },
    };
    let tokens = emit_function(&f, &types).expect("function should emit");
    let s = tokens.to_string();
    let sig = signature_fragment(&s, "reset");
    assert!(
        sig.contains("mut p : i32"),
        "reassigned param must emit `mut p : i32` in the function signature, got signature fragment: `{sig}`"
    );
}

#[test]
fn generator_next_consumes_param_emits_mut_in_signature() {
    let mut types = TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let gen_ty = types.intern(&Type::Generator { inner: i64_ty });
    let gen_result_ty = types.intern(&Type::GeneratorResult { inner: i64_ty });
    let g_param = LocalId::from_raw(1);
    let temp = LocalId::from_raw(2);

    let mut f = empty_func("drive");
    f.ret = gen_ty;
    f.kind = FunctionKind::Generator;
    f.params = vec![MirParam {
        id: g_param,
        name: Atom::from("g"),
        ty: gen_ty,
    }];
    f.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Runtime {
                op: RuntimeOp::GeneratorNext,
                args: vec![MirExpr::Local(g_param)],
                dest: Some(temp),
                ty: gen_result_ty,
                target_ty: None,
            }],
        },
    };

    let tokens = emit_function(&f, &types).expect("function should emit");
    let s = tokens.to_string();
    let sig = signature_fragment(&s, "drive");
    assert!(
        sig.contains("mut g : ts_aot_runtime :: Generator < i64 >"),
        "generator param consumed by RuntimeOp::GeneratorNext must emit `mut g : ts_aot_runtime :: Generator < i64 >` in the function signature, got signature fragment: `{sig}`"
    );
}

#[test]
fn indirect_generator_next_consumes_param_emits_mut() {
    let mut types = TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let gen_ty = types.intern(&Type::Generator { inner: i64_ty });
    let gen_result_ty = types.intern(&Type::GeneratorResult { inner: i64_ty });
    let struct_id = StructId::from_raw(0);
    let box_ty = types.intern(&Type::Struct { id: struct_id });
    let g_field = FieldId::from_raw(0);
    let obj_param = LocalId::from_raw(1);
    let temp = LocalId::from_raw(2);

    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Struct(MirStructDecl {
        id: struct_id,
        name: Atom::from("Box"),
        fields: vec![MirFieldDecl {
            id: g_field,
            name: Atom::from("g"),
            ty: gen_ty,
            mutable: true,
            visibility: Visibility::Public,
        }],
        methods: Vec::new(),
    }));

    let mut f = empty_func("drive_indirect");
    f.ret = gen_ty;
    f.kind = FunctionKind::Generator;
    f.params = vec![MirParam {
        id: obj_param,
        name: Atom::from("obj"),
        ty: box_ty,
    }];
    f.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Runtime {
                op: RuntimeOp::GeneratorNext,
                args: vec![MirExpr::Field {
                    base: Box::new(MirExpr::Local(obj_param)),
                    field: g_field,
                    ty: gen_ty,
                }],
                dest: Some(temp),
                ty: gen_result_ty,
                target_ty: None,
            }],
        },
    };
    prog.push_decl(MirDecl::Function(f));

    let tokens = emit_decls(&prog, &types).expect("decls should emit");
    let s = tokens.to_string();
    let sig = signature_fragment(&s, "drive_indirect");
    assert!(
        sig.contains("mut obj"),
        "param consumed by indirect RuntimeOp::GeneratorNext (obj.g.next()) must emit `mut obj` in the function signature, got signature fragment: `{sig}`"
    );
    assert!(
        s.contains("obj . g"),
        "field access on a struct param must resolve to the declared field name `g` via EmitCtx, \
         not the placeholder `__field0` that standalone EmitCtx would emit. Pushing the struct \
         decl into the same `prog` and calling `emit_decls` is what wires the field name into the \
         ctx. Got: `{s}`"
    );
}

#[test]
fn param_used_in_nested_place_assign_emits_mut() {
    let mut types = TypeTable::new();
    let i32_ty = types.intern(&Type::I32);
    let struct_id = StructId::from_raw(0);
    let obj_ty = types.intern(&Type::Struct { id: struct_id });
    let param = LocalId::from_raw(1);

    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Struct(MirStructDecl {
        id: struct_id,
        name: Atom::from("Box"),
        fields: vec![MirFieldDecl {
            id: FieldId::from_raw(0),
            name: Atom::from("x"),
            ty: i32_ty,
            mutable: true,
            visibility: Visibility::Public,
        }],
        methods: Vec::new(),
    }));

    let mut f = empty_func("poke");
    f.ret = TypeId::from_raw(0);
    f.params = vec![MirParam {
        id: param,
        name: Atom::from("obj"),
        ty: obj_ty,
    }];
    f.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Assign {
                target: MirPlace::Field {
                    base: Box::new(MirPlaceBase::Local(param)),
                    field: FieldId::from_raw(0),
                    ty: i32_ty,
                },
                value: MirExpr::Int {
                    value: 0,
                    ty: i32_ty,
                },
            }],
        },
    };
    prog.push_decl(MirDecl::Function(f));

    let tokens = emit_decls(&prog, &types).expect("decls should emit");
    let s = tokens.to_string();
    let sig = signature_fragment(&s, "poke");
    assert!(
        sig.contains("mut obj"),
        "param used in a nested place assign (obj.x = 0) must emit `mut obj` in the function signature, got signature fragment: `{sig}`"
    );
}

#[test]
fn param_used_in_optional_chain_place_assign_emits_mut() {
    let mut types = TypeTable::new();
    let i32_ty = types.intern(&Type::I32);
    let struct_id = StructId::from_raw(0);
    let obj_ty = types.intern(&Type::Struct { id: struct_id });
    let opt_ty = types.intern(&Type::Optional { inner: obj_ty });
    let param = LocalId::from_raw(1);

    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Struct(MirStructDecl {
        id: struct_id,
        name: Atom::from("Box"),
        fields: vec![MirFieldDecl {
            id: FieldId::from_raw(0),
            name: Atom::from("x"),
            ty: i32_ty,
            mutable: true,
            visibility: Visibility::Public,
        }],
        methods: Vec::new(),
    }));

    let mut f = empty_func("poke_opt");
    f.ret = TypeId::from_raw(0);
    f.params = vec![MirParam {
        id: param,
        name: Atom::from("obj"),
        ty: obj_ty,
    }];
    f.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Assign {
                target: MirPlace::Field {
                    base: Box::new(MirPlaceBase::Chain {
                        base: Box::new(MirExpr::OptionalChain {
                            base: Box::new(MirExpr::Local(param)),
                            ty: opt_ty,
                        }),
                        ty: opt_ty,
                    }),
                    field: FieldId::from_raw(0),
                    ty: i32_ty,
                },
                value: MirExpr::Int {
                    value: 0,
                    ty: i32_ty,
                },
            }],
        },
    };
    prog.push_decl(MirDecl::Function(f));

    let tokens = emit_decls(&prog, &types).expect("decls should emit");
    let s = tokens.to_string();
    let sig = signature_fragment(&s, "poke_opt");
    assert!(
        sig.contains("mut obj"),
        "param used in a nested place assign through an optional chain (MirPlaceBase::Chain whose base is MirExpr::OptionalChain) must emit `mut obj` in the function signature, got signature fragment: `{sig}`"
    );
}

#[test]
fn non_reassigned_param_omits_mut_in_signature() {
    let mut types = TypeTable::new();
    let i32_ty = types.intern(&Type::I32);
    let param = LocalId::from_raw(1);
    let mut f = empty_func("read");
    f.ret = i32_ty;
    f.params = vec![MirParam {
        id: param,
        name: Atom::from("p"),
        ty: i32_ty,
    }];
    f.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Return(Some(MirExpr::Local(param)))],
        },
    };
    let tokens = emit_function(&f, &types).expect("function should emit");
    let s = tokens.to_string();
    let sig = signature_fragment(&s, "read");
    assert!(
        sig.contains("p : i32"),
        "non-reassigned param must emit `p : i32` in the function signature, got signature fragment: `{sig}`"
    );
    assert!(
        !sig.contains("mut p"),
        "non-reassigned param must NOT emit `mut`, got signature fragment: `{sig}`"
    );
}

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
        s.contains("impl TsClassId for Animal"),
        "struct must emit `impl TsClassId for <name>` so __ts_aot_op_instanceof \
         trait bound is satisfied; got: {s}"
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
fn global_without_init_emits_default_initializer() {
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Global(MirGlobalDecl {
        name: Atom::from("counter"),
        ty: TypeId::from_raw(0),
        mutable: false,
        visibility: Visibility::Public,
        export_name: None,
        init: None,
    }));
    let tokens = emit_decls(&prog, &TypeTable::new())
        .expect("uninitialized global must emit a default-initialized static");
    let s = tokens.to_string();
    assert!(s.contains("static counter"), "expected static decl in: {s}");
    assert!(
        s.contains("Default :: default ()"),
        "expected Default::default() initializer in: {s}"
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
    assert_eq!(err, BackendError::NotImplemented);
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

#[test]
fn sanitize_ident_replaces_dash_with_underscore() {
    assert_eq!(sanitize_ident("foo-bar"), "foo_bar");
}

#[test]
fn sanitize_ident_prefixes_digit_start() {
    assert_eq!(sanitize_ident("7greet"), "_7greet");
}

#[test]
fn sanitize_ident_appends_underscore_to_keyword() {
    assert_eq!(sanitize_ident("type"), "type_");
    assert_eq!(sanitize_ident("fn"), "fn_");
    assert_eq!(sanitize_ident("try"), "try_");
    assert_eq!(sanitize_ident("gen"), "gen_");
}

#[test]
fn emit_type_resolves_primitives() {
    let mut types = TypeTable::new();
    let i32_id = types.intern(&Type::I32);
    let bool_id = types.intern(&Type::Bool);
    let tokens = emit_type_id(i32_id, &types);
    assert_eq!(tokens.to_string(), "i32");
    let tokens = emit_type_id(bool_id, &types);
    assert_eq!(tokens.to_string(), "bool");
}

#[test]
fn emit_type_for_unknown_id_emits_placeholder() {
    let types = TypeTable::new();
    let tokens = emit_type_id(TypeId::from_raw(42), &types);
    assert!(tokens.to_string().contains("__ty42"), "got: {tokens}");
}

#[test]
fn emit_type_optional_resolves_inner_via_table() {
    let mut types = TypeTable::new();
    let i32_id = types.intern(&Type::I32);
    let opt_id = types.intern(&Type::Optional { inner: i32_id });
    let tokens = emit_type_id(opt_id, &types);
    assert_eq!(tokens.to_string(), "Option < i32 >");
}

#[test]
fn emit_type_array_resolves_element_via_table() {
    let mut types = TypeTable::new();
    let str_id = types.intern(&Type::String);
    let arr_id = types.intern(&Type::Array { element: str_id });
    let tokens = emit_type_id(arr_id, &types);
    assert_eq!(tokens.to_string(), "Vec < ts_aot_runtime :: JsString >");
}

#[test]
fn emit_type_result_resolves_ok_and_err_via_table() {
    let mut types = TypeTable::new();
    let i32_id = types.intern(&Type::I32);
    let str_id = types.intern(&Type::String);
    let res_id = types.intern(&Type::Result {
        ok: i32_id,
        err: str_id,
    });
    let tokens = emit_type_id(res_id, &types);
    assert_eq!(
        tokens.to_string(),
        "Result < i32 , ts_aot_runtime :: JsString >"
    );
}

#[test]
fn emit_type_generator_emits_namespaced_generic() {
    let mut types = TypeTable::new();
    let i64_id = types.intern(&Type::I64);
    let gen_id = types.intern(&Type::Generator { inner: i64_id });
    let tokens = emit_type_id(gen_id, &types);
    assert_eq!(
        tokens.to_string(),
        "ts_aot_runtime :: Generator < i64 >",
        "Generator must emit as namespaced generic with its yield type"
    );
}

#[test]
fn emit_type_generator_result_emits_namespaced_generic() {
    let mut types = TypeTable::new();
    let i64_id = types.intern(&Type::I64);
    let res_id = types.intern(&Type::GeneratorResult { inner: i64_id });
    let tokens = emit_type_id(res_id, &types);
    assert_eq!(
        tokens.to_string(),
        "ts_aot_runtime :: GeneratorResult < i64 >"
    );
}

#[test]
fn function_returning_result_emits_result_type_with_ok_and_err_stmts() {
    let mut types = TypeTable::new();
    let int_ty = types.intern(&Type::I32);
    let str_ty = types.intern(&Type::String);
    let res_ty = types.intern(&Type::Result {
        ok: int_ty,
        err: str_ty,
    });

    let mut f = empty_func("try_get");
    f.ret = res_ty;
    f.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![
                MirStmt::ReturnResultErr {
                    error: MirExpr::String {
                        id: Atom::from("oops"),
                        ty: str_ty,
                    },
                    err_ty: str_ty,
                },
                MirStmt::Return(Some(MirExpr::ResultOk {
                    value: Box::new(MirExpr::Int {
                        value: 42,
                        ty: int_ty,
                    }),
                    ty: res_ty,
                })),
            ],
        },
    };

    let tokens = emit_function(&f, &types).expect("function should emit");
    let s = tokens.to_string();
    assert!(
        s.contains("-> Result < i32 , ts_aot_runtime :: JsString >"),
        "ret type must be Result<i32, JsString>, got: {s}"
    );
    assert!(
        s.contains("Err (") && s.contains("\"oops\""),
        "ReturnResultErr must emit `Err(...)` containing the error, got: {s}"
    );
    assert!(
        s.contains("Ok (42)"),
        "ResultOk must emit `Ok(42)`, got: {s}"
    );
    assert!(
        !s.contains("-> ()"),
        "Result ret must not fall back to unit, got: {s}"
    );
}

#[test]
fn field_access_resolves_name_via_base_struct_id() {
    let mut types = TypeTable::new();
    let int_ty = types.intern(&Type::I32);

    let point_id = StructId::from_raw(0);
    let size_id = StructId::from_raw(1);
    let point_ty = types.intern(&Type::Struct { id: point_id });
    let size_ty = types.intern(&Type::Struct { id: size_id });

    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Struct(MirStructDecl {
        id: point_id,
        name: Atom::from("Point"),
        fields: vec![MirFieldDecl {
            id: FieldId::from_raw(0),
            name: Atom::from("x"),
            ty: int_ty,
            mutable: false,
            visibility: Visibility::Public,
        }],
        methods: Vec::new(),
    }));
    prog.push_decl(MirDecl::Struct(MirStructDecl {
        id: size_id,
        name: Atom::from("Size"),
        fields: vec![MirFieldDecl {
            id: FieldId::from_raw(0),
            name: Atom::from("width"),
            ty: int_ty,
            mutable: false,
            visibility: Visibility::Public,
        }],
        methods: Vec::new(),
    }));

    let p_local = LocalId::from_raw(0);
    let s_local = LocalId::from_raw(1);
    let mut f = empty_func("read");
    f.ret = int_ty;
    f.body = MirBody {
        locals: vec![
            MirLocalDecl {
                id: p_local,
                name: Atom::from("p"),
                ty: point_ty,
                mutable: false,
            },
            MirLocalDecl {
                id: s_local,
                name: Atom::from("s"),
                ty: size_ty,
                mutable: false,
            },
        ],
        block: MirBlock {
            stmts: vec![MirStmt::Return(Some(MirExpr::Binary {
                op: BinaryOp::Add,
                left: Box::new(MirExpr::Field {
                    base: Box::new(MirExpr::Local(p_local)),
                    field: FieldId::from_raw(0),
                    ty: int_ty,
                }),
                right: Box::new(MirExpr::Field {
                    base: Box::new(MirExpr::Local(s_local)),
                    field: FieldId::from_raw(0),
                    ty: int_ty,
                }),
                ty: int_ty,
            }))],
        },
    };
    prog.push_decl(MirDecl::Function(f));

    let tokens = emit_decls(&prog, &types).expect("decls should emit");
    let s = tokens.to_string();
    assert!(s.contains("p . x"), "expected `p.x`, got: {s}");
    assert!(s.contains("s . width"), "expected `s.width`, got: {s}");
    assert!(
        !s.contains("__field0"),
        "FieldId(0) must resolve to its struct's real name, got: {s}"
    );
}

#[test]
fn field_assign_resolves_name_via_base_struct_id() {
    let mut types = TypeTable::new();
    let int_ty = types.intern(&Type::I32);

    let point_id = StructId::from_raw(0);
    let size_id = StructId::from_raw(1);
    let point_ty = types.intern(&Type::Struct { id: point_id });
    let size_ty = types.intern(&Type::Struct { id: size_id });

    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Struct(MirStructDecl {
        id: point_id,
        name: Atom::from("Point"),
        fields: vec![MirFieldDecl {
            id: FieldId::from_raw(0),
            name: Atom::from("x"),
            ty: int_ty,
            mutable: false,
            visibility: Visibility::Public,
        }],
        methods: Vec::new(),
    }));
    prog.push_decl(MirDecl::Struct(MirStructDecl {
        id: size_id,
        name: Atom::from("Size"),
        fields: vec![MirFieldDecl {
            id: FieldId::from_raw(0),
            name: Atom::from("width"),
            ty: int_ty,
            mutable: false,
            visibility: Visibility::Public,
        }],
        methods: Vec::new(),
    }));

    let p_local = LocalId::from_raw(0);
    let s_local = LocalId::from_raw(1);
    let mut f = empty_func("touch");
    f.ret = TypeId::from_raw(0);
    f.body = MirBody {
        locals: vec![
            MirLocalDecl {
                id: p_local,
                name: Atom::from("p"),
                ty: point_ty,
                mutable: false,
            },
            MirLocalDecl {
                id: s_local,
                name: Atom::from("s"),
                ty: size_ty,
                mutable: false,
            },
        ],
        block: MirBlock {
            stmts: vec![
                MirStmt::Assign {
                    target: MirPlace::Field {
                        base: Box::new(MirPlaceBase::Local(p_local)),
                        field: FieldId::from_raw(0),
                        ty: int_ty,
                    },
                    value: MirExpr::Int {
                        value: 1,
                        ty: int_ty,
                    },
                },
                MirStmt::Assign {
                    target: MirPlace::Field {
                        base: Box::new(MirPlaceBase::Local(s_local)),
                        field: FieldId::from_raw(0),
                        ty: int_ty,
                    },
                    value: MirExpr::Int {
                        value: 2,
                        ty: int_ty,
                    },
                },
                MirStmt::Return(None),
            ],
        },
    };
    prog.push_decl(MirDecl::Function(f));

    let tokens = emit_decls(&prog, &types).expect("decls should emit");
    let s = tokens.to_string();
    assert!(s.contains("p . x = 1"), "expected `p.x = 1`, got: {s}");
    assert!(
        s.contains("s . width = 2"),
        "expected `s.width = 2`, got: {s}"
    );
    assert!(
        !s.contains("__field0"),
        "FieldId(0) must resolve to its struct's real name, got: {s}"
    );
}

#[test]
fn field_access_on_non_struct_returns_not_implemented() {
    let mut types = TypeTable::new();
    let int_ty = types.intern(&Type::I32);

    let point_id = StructId::from_raw(0);

    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Struct(MirStructDecl {
        id: point_id,
        name: Atom::from("Point"),
        fields: vec![MirFieldDecl {
            id: FieldId::from_raw(0),
            name: Atom::from("x"),
            ty: int_ty,
            mutable: false,
            visibility: Visibility::Public,
        }],
        methods: Vec::new(),
    }));

    let local = LocalId::from_raw(0);
    let mut f = empty_func("bad");
    f.ret = int_ty;
    f.body = MirBody {
        locals: vec![MirLocalDecl {
            id: local,
            name: Atom::from("n"),
            ty: int_ty,
            mutable: false,
        }],
        block: MirBlock {
            stmts: vec![MirStmt::Return(Some(MirExpr::Field {
                base: Box::new(MirExpr::Local(local)),
                field: FieldId::from_raw(0),
                ty: int_ty,
            }))],
        },
    };
    prog.push_decl(MirDecl::Function(f));

    let err = emit_decls(&prog, &types).expect_err("field access on non-struct must fail");
    assert_eq!(err, BackendError::NotImplemented);
}

#[test]
fn indirect_call_emits_callee_args_for_known_callee() {
    let mut types = TypeTable::new();
    let int_ty = types.intern(&Type::I32);
    let callee_local = LocalId::from_raw(0);
    let dest = LocalId::from_raw(1);
    let mut f = empty_func("caller");
    f.ret = int_ty;
    f.body = MirBody {
        locals: vec![
            MirLocalDecl {
                id: callee_local,
                name: Atom::from("callee_ref"),
                ty: int_ty,
                mutable: false,
            },
            MirLocalDecl {
                id: dest,
                name: Atom::from("result"),
                ty: int_ty,
                mutable: false,
            },
        ],
        block: MirBlock {
            stmts: vec![MirStmt::Return(Some(MirExpr::IndirectCall {
                callee: Box::new(MirExpr::Local(callee_local)),
                args: vec![
                    MirExpr::Int {
                        value: 1,
                        ty: int_ty,
                    },
                    MirExpr::Int {
                        value: 2,
                        ty: int_ty,
                    },
                ],
                ty: int_ty,
            }))],
        },
    };

    let tokens = emit_function(&f, &types).expect("function should emit");
    let s = tokens.to_string();

    assert!(
        s.contains("callee_ref (1 , 2)"),
        "MirExpr::IndirectCall with non-Optional callee must emit a direct Rust call `callee(args)`. \
         PR 1.4 expansion replaces the old `Runtime::CallIndirect` path that emitted \
         `__ts_aot_call_indirect(callee_str, args_slice, dispatch_table)`. \
         Got: {s}"
    );
}

#[test]
fn indirect_call_with_optional_chain_callee_emits_as_ref_map_call() {
    let mut types = TypeTable::new();
    let int_ty = types.intern(&Type::I64);
    let opt_int = types.intern(&Type::Optional { inner: int_ty });
    let obj = LocalId::from_raw(0);
    let mut f = empty_func("caller");
    f.ret = int_ty;
    f.body = MirBody {
        locals: vec![MirLocalDecl {
            id: obj,
            name: Atom::from("obj"),
            ty: opt_int,
            mutable: false,
        }],
        block: MirBlock {
            stmts: vec![MirStmt::Return(Some(MirExpr::IndirectCall {
                callee: Box::new(MirExpr::OptionalChain {
                    base: Box::new(MirExpr::Local(obj)),
                    ty: opt_int,
                }),
                args: vec![MirExpr::Int {
                    value: 7,
                    ty: int_ty,
                }],
                ty: int_ty,
            }))],
        },
    };

    let tokens = emit_function(&f, &types).expect("obj?.() must emit");
    let s = tokens.to_string();

    assert!(
        s.contains("obj . as_ref () . map (| f | f (7))"),
        "MirExpr::IndirectCall with OptionalChain callee (obj?.() pattern) must emit \
         `obj.as_ref().map(|f| f(args))` — Option-aware short-circuit. \
         Phase 5+ will replace with proper `obj.and_then(|f| Some(f(args))).unwrap_or_default()` etc. \
         Got: {s}"
    );
}

#[test]
fn assignment_to_optional_chain_field_emits_is_some_branch() {
    let mut types = TypeTable::new();
    let point = StructId::from_raw(0);
    let point_ty = types.intern(&Type::Struct { id: point });
    let opt = types.intern(&Type::Optional { inner: point_ty });
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Struct(MirStructDecl {
        id: point,
        name: Atom::from("Point"),
        fields: vec![MirFieldDecl {
            id: FieldId::from_raw(0),
            name: Atom::from("x"),
            ty: types.intern(&Type::I64),
            mutable: true,
            visibility: Visibility::Public,
        }],
        methods: Vec::new(),
    }));
    let obj = LocalId::from_raw(0);
    let mut f = empty_func("caller");
    f.ret = opt;
    f.body = MirBody {
        locals: vec![MirLocalDecl {
            id: obj,
            name: Atom::from("obj"),
            ty: opt,
            mutable: false,
        }],
        block: MirBlock {
            stmts: vec![MirStmt::Assign {
                target: MirPlace::Field {
                    base: Box::new(MirPlaceBase::Chain {
                        base: Box::new(MirExpr::OptionalChain {
                            base: Box::new(MirExpr::Local(obj)),
                            ty: opt,
                        }),
                        ty: opt,
                    }),
                    field: FieldId::from_raw(0),
                    ty: types.intern(&Type::I64),
                },
                value: MirExpr::Int {
                    value: 42,
                    ty: types.intern(&Type::I64),
                },
            }],
        },
    };
    prog.push_decl(MirDecl::Function(f));
    let tokens = emit_decls(&prog, &types).expect("obj?.x = y must emit");
    let s = tokens.to_string();

    assert!(
        s.contains("if obj . is_some ()"),
        "MirPlace::Field with MirPlaceBase::Chain + OptionalChain base must emit \
         `if obj.is_some() {{ ... }}` (PR 1.4 out-of-scope closure for obj?.x = y). \
         Got: {s}"
    );
    assert!(
        s.contains("obj . as_mut () . unwrap () . x = 42"),
        "obj?.x = 42 must unwrap and assign the field. Got: {s}"
    );
}

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
    let opt = types.intern(&Type::Optional { inner: point_ty });
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Struct(MirStructDecl {
        id: point,
        name: Atom::from("Point"),
        fields: vec![MirFieldDecl {
            id: FieldId::from_raw(0),
            name: Atom::from("x"),
            ty: types.intern(&Type::I64),
            mutable: false,
            visibility: Visibility::Public,
        }],
        methods: Vec::new(),
    }));
    let local = LocalId::from_raw(0);
    let mut f = empty_func("caller");
    f.ret = opt;
    f.body = MirBody {
        locals: vec![MirLocalDecl {
            id: local,
            name: Atom::from("obj"),
            ty: opt,
            mutable: false,
        }],
        block: MirBlock {
            stmts: vec![MirStmt::Return(Some(MirExpr::Field {
                base: Box::new(MirExpr::OptionalChain {
                    base: Box::new(MirExpr::Local(local)),
                    ty: opt,
                }),
                field: FieldId::from_raw(0),
                ty: opt,
            }))],
        },
    };
    prog.push_decl(MirDecl::Function(f));
    let tokens = emit_decls(&prog, &types).expect("OptionalChain field must emit");
    let s = tokens.to_string();
    assert!(
        s.contains("obj . as_ref () . map (| o | o . x)"),
        "Optional base + Field must emit `obj.as_ref().map(|o| o.x)` — access must be inside \
         the Option::map closure, not on Option<&T> directly (which doesn't compile). \
         Got: {s}"
    );
}

#[test]
fn optional_chain_index_with_optional_base_emits_as_ref_map() {
    let mut types = TypeTable::new();
    let inner = types.intern(&Type::I64);
    let opt = types.intern(&Type::Optional { inner });
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    let local = LocalId::from_raw(0);
    let idx_local = LocalId::from_raw(1);
    let mut f = empty_func("caller");
    f.ret = opt;
    f.body = MirBody {
        locals: vec![
            MirLocalDecl {
                id: local,
                name: Atom::from("arr"),
                ty: opt,
                mutable: false,
            },
            MirLocalDecl {
                id: idx_local,
                name: Atom::from("i"),
                ty: inner,
                mutable: false,
            },
        ],
        block: MirBlock {
            stmts: vec![MirStmt::Return(Some(MirExpr::Index {
                base: Box::new(MirExpr::OptionalChain {
                    base: Box::new(MirExpr::Local(local)),
                    ty: opt,
                }),
                index: Box::new(MirExpr::Local(idx_local)),
                ty: opt,
            }))],
        },
    };
    prog.push_decl(MirDecl::Function(f));
    let tokens = emit_decls(&prog, &types).expect("OptionalChain index must emit");
    let s = tokens.to_string();
    assert!(
        s.contains("arr . as_ref () . map (| o | o [i])"),
        "Optional base + Index must emit `arr.as_ref().map(|o| o[i])` — index must be inside \
         the Option::map closure, not on Option<&T> directly (which doesn't compile). \
         Got: {s}"
    );
}

#[test]
fn optional_chain_index_with_non_optional_base_falls_back_to_mvp() {
    let mut types = TypeTable::new();
    let inner = types.intern(&Type::I64);
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    let local = LocalId::from_raw(0);
    let idx_local = LocalId::from_raw(1);
    let mut f = empty_func("caller");
    f.ret = inner;
    f.body = MirBody {
        locals: vec![
            MirLocalDecl {
                id: local,
                name: Atom::from("arr"),
                ty: inner,
                mutable: false,
            },
            MirLocalDecl {
                id: idx_local,
                name: Atom::from("i"),
                ty: inner,
                mutable: false,
            },
        ],
        block: MirBlock {
            stmts: vec![MirStmt::Return(Some(MirExpr::Index {
                base: Box::new(MirExpr::OptionalChain {
                    base: Box::new(MirExpr::Local(local)),
                    ty: inner,
                }),
                index: Box::new(MirExpr::Local(idx_local)),
                ty: inner,
            }))],
        },
    };
    prog.push_decl(MirDecl::Function(f));
    let tokens = emit_decls(&prog, &types).expect("MVP fallback must emit");
    let s = tokens.to_string();
    assert!(
        s.contains("arr [i]") && !s.contains("as_ref"),
        "Non-Optional base + Index must fall back to MVP rr[i] (no Option machinery). Got: {s}"
    );
}

#[test]
fn float_nan_emits_f64_nan_literal() {
    let mut func = empty_func("nan_test");
    let f64_ty = TypeId::from_raw(7);
    func.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Return(Some(MirExpr::Float {
                value: f64::NAN,
                ty: f64_ty,
            }))],
        },
    };
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(func));
    let s = emit_decls(&prog, &ts_aot_core::TypeTable::new())
        .expect("emit must succeed")
        .to_string();
    assert!(
        s.contains("f64 :: NAN"),
        "NaN literal must emit f64::NAN, got: {s}"
    );
}

#[test]
fn float_positive_infinity_emits_f64_infinity_literal() {
    let mut func = empty_func("pos_inf_test");
    let f64_ty = TypeId::from_raw(7);
    func.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Return(Some(MirExpr::Float {
                value: f64::INFINITY,
                ty: f64_ty,
            }))],
        },
    };
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(func));
    let s = emit_decls(&prog, &ts_aot_core::TypeTable::new())
        .expect("emit must succeed")
        .to_string();
    assert!(
        s.contains("f64 :: INFINITY"),
        "+Infinity must emit f64::INFINITY, got: {s}"
    );
}

#[test]
fn float_negative_infinity_emits_f64_neg_infinity_literal() {
    let mut func = empty_func("neg_inf_test");
    let f64_ty = TypeId::from_raw(7);
    func.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Return(Some(MirExpr::Float {
                value: f64::NEG_INFINITY,
                ty: f64_ty,
            }))],
        },
    };
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(func));
    let s = emit_decls(&prog, &ts_aot_core::TypeTable::new())
        .expect("emit must succeed")
        .to_string();
    assert!(
        s.contains("f64 :: NEG_INFINITY"),
        "-Infinity must emit f64::NEG_INFINITY, got: {s}"
    );
}

#[test]
fn float_finite_still_uses_unsuffixed_literal() {
    let mut func = empty_func("finite_test");
    let f64_ty = TypeId::from_raw(7);
    func.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Return(Some(MirExpr::Float {
                value: 3.5,
                ty: f64_ty,
            }))],
        },
    };
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(func));
    let s = emit_decls(&prog, &ts_aot_core::TypeTable::new())
        .expect("emit must succeed")
        .to_string();
    assert!(
        s.contains("3.5") && !s.contains("f64 ::"),
        "finite float 3.5 must emit literal 3.5 (not f64::NAN/INFINITY), got: {s}"
    );
}

#[test]
fn yield_with_value_emits_co_yield_await() {
    let mut func = empty_func("yield_test");
    func.kind = FunctionKind::Generator;
    let i64_ty = TypeId::from_raw(7);
    let local = LocalId::from_raw(0);
    func.body = MirBody {
        locals: vec![MirLocalDecl {
            id: local,
            name: Atom::from("x"),
            ty: i64_ty,
            mutable: true,
        }],
        block: MirBlock {
            stmts: vec![
                MirStmt::Expr(MirExpr::Yield {
                    expr: Some(Box::new(MirExpr::Local(local))),
                    ty: i64_ty,
                }),
                MirStmt::Return(Some(MirExpr::Int {
                    value: 1,
                    ty: i64_ty,
                })),
            ],
        },
    };
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(func));
    let s = emit_decls(&prog, &ts_aot_core::TypeTable::new())
        .expect("emit must succeed")
        .to_string();
    assert!(
        s.contains("__gen_co_") && s.contains(" . yield_ (x) . await"),
        "Yield(Some) must emit `co.yield_(x).await` inside the producer, got: {s}"
    );
}

#[test]
fn yield_without_value_emits_unit_yield() {
    let mut func = empty_func("yield_unit_test");
    func.kind = FunctionKind::Generator;
    func.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Expr(MirExpr::Yield {
                expr: None,
                ty: TypeId::from_raw(0),
            })],
        },
    };
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(func));
    let s = emit_decls(&prog, &ts_aot_core::TypeTable::new())
        .expect("emit must succeed")
        .to_string();
    assert!(
        s.contains("__gen_co_") && s.contains(" . yield_ (()) . await"),
        "Yield(None) must emit `co.yield_(()).await`, got: {s}"
    );
}

#[test]
fn yield_in_non_generator_body_returns_internal_error() {
    let mut func = empty_func("yield_plain_test");
    func.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Expr(MirExpr::Yield {
                expr: None,
                ty: TypeId::from_raw(0),
            })],
        },
    };
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(func));
    let err = emit_decls(&prog, &ts_aot_core::TypeTable::new())
        .expect_err("non-generator Yield must be rejected with an error");
    let BackendError::Internal(msg) = err else {
        panic!("expected BackendError::Internal, got other error variant");
    };
    assert!(
        msg.contains("MirExpr::Yield"),
        "Internal error must mention the MirExpr::Yield context, got: {msg:?}"
    );
    assert!(
        msg.contains("non-generator body"),
        "Internal error must explain the non-generator body invariant, got: {msg:?}"
    );
}

#[test]
fn yield_inside_try_finally_is_allowed() {
    let i64_ty = TypeId::from_raw(7);
    let mut func = empty_func("yield_in_finally");
    func.kind = FunctionKind::Generator;
    func.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Try {
                body: MirBlock::new(),
                catch_param: None,
                catch: None,
                finally: Some(MirBlock {
                    stmts: vec![MirStmt::Expr(MirExpr::Yield {
                        expr: Some(Box::new(MirExpr::Int {
                            value: 1,
                            ty: i64_ty,
                        })),
                        ty: i64_ty,
                    })],
                }),
            }],
        },
    };
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(func));
    emit_decls(&prog, &ts_aot_core::TypeTable::new())
        .expect("yield in finally must remain valid (finally is outside the catch_unwind closure)");
}

#[test]
fn cast_to_bool_target_is_rejected_with_internal_error() {
    let mut types = ts_aot_core::TypeTable::new();
    let bool_ty = types.intern(&Type::Bool);
    let mut func = empty_func("cast_to_bool");
    func.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Expr(MirExpr::Cast {
                expr: Box::new(MirExpr::Int {
                    value: 1,
                    ty: bool_ty,
                }),
                ty: bool_ty,
            })],
        },
    };
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(func));
    let err = emit_decls(&prog, &types)
        .expect_err("Cast to Type::Bool must be rejected — Rust's `as bool` is not valid syntax");
    let BackendError::Internal(msg) = err else {
        panic!("expected BackendError::Internal, got other error variant");
    };
    assert!(
        msg.contains("MirExpr::Cast"),
        "Internal error must mention the MirExpr::Cast context, got: {msg:?}"
    );
    assert!(
        msg.contains("not a numeric primitive"),
        "Internal error must explain the numeric-primitive restriction, got: {msg:?}"
    );
    assert!(
        msg.contains(&bool_ty.raw().to_string()),
        "Internal error must include the offending TypeId raw id, got: {msg:?}"
    );
}

#[test]
fn cast_to_numeric_target_emits_as_cast() {
    let mut types = ts_aot_core::TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let f64_ty = types.intern(&Type::F64);
    let mut func = empty_func("cast_to_numeric");
    func.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Expr(MirExpr::Cast {
                expr: Box::new(MirExpr::Int {
                    value: 1,
                    ty: i64_ty,
                }),
                ty: f64_ty,
            })],
        },
    };
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(func));
    let tokens = emit_decls(&prog, &types).expect("numeric Cast must succeed");
    let s = tokens.to_string();
    assert!(
        s.contains("as f64"),
        "Cast to numeric primitive (f64) must emit `as f64`, got: {s}"
    );
}

#[test]
fn generator_return_inside_try_preserves_completion_shape() {
    let mut func = empty_func("gen_try");
    func.kind = FunctionKind::Generator;
    let i64_ty = TypeId::from_raw(7);
    func.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![
                MirStmt::Expr(MirExpr::Yield {
                    expr: Some(Box::new(MirExpr::Int {
                        value: 1,
                        ty: i64_ty,
                    })),
                    ty: i64_ty,
                }),
                MirStmt::Try {
                    body: MirBlock {
                        stmts: vec![MirStmt::Return(Some(MirExpr::Int {
                            value: 2,
                            ty: i64_ty,
                        }))],
                    },
                    catch_param: None,
                    catch: None,
                    finally: None,
                },
                MirStmt::Try {
                    body: MirBlock {
                        stmts: vec![MirStmt::Return(None)],
                    },
                    catch_param: None,
                    catch: None,
                    finally: None,
                },
            ],
        },
    };
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(func));
    let s = emit_decls(&prog, &ts_aot_core::TypeTable::new())
        .expect("emit must succeed")
        .to_string();
    assert!(
        s.contains("__return_slot_0 = Some (Some (2))"),
        "valued return inside a generator try block must set runtime slot to `Some(Some(2))`, got: {s}"
    );
    assert!(
        s.contains("__return_slot_1 = Some (None)"),
        "bare return inside a generator try block must set runtime slot to `Some(None)`, got: {s}"
    );
    assert!(
        s.contains("if let Some (__v) = __return_slot_1"),
        "generator try replay must read runtime slot via `if let Some(__v) = slot {{ return __v; }}`, \
         got: {s}"
    );
}

#[test]
fn generator_fn_body_emits_producer_async_block() {
    let mut func = empty_func("gen");
    func.kind = FunctionKind::Generator;
    let i64_ty = TypeId::from_raw(7);
    func.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![
                MirStmt::Expr(MirExpr::Yield {
                    expr: Some(Box::new(MirExpr::Int {
                        value: 1,
                        ty: i64_ty,
                    })),
                    ty: i64_ty,
                }),
                MirStmt::Return(Some(MirExpr::Int {
                    value: 2,
                    ty: i64_ty,
                })),
            ],
        },
    };
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(func));
    let s = emit_decls(&prog, &ts_aot_core::TypeTable::new())
        .expect("emit must succeed")
        .to_string();
    assert!(
        s.contains("__ts_aot_generator_new (| __gen_co_") && s.contains(" | async move {"),
        "generator body must be wrapped in __ts_aot_generator_new(|co| async move {{..}}), got: {s}"
    );
    assert!(
        s.contains("return Some (2) ;"),
        "generator return must map to `return Some(v)` (completion value), got: {s}"
    );
}

#[test]
fn switch_with_int_cases_emits_match() {
    let mut func = empty_func("switch_int");
    let i64_ty = TypeId::from_raw(7);
    let disc = LocalId::from_raw(0);
    let a = LocalId::from_raw(1);
    let b = LocalId::from_raw(2);
    func.ret = i64_ty;
    func.body = MirBody {
        locals: vec![
            MirLocalDecl {
                id: disc,
                name: Atom::from("x"),
                ty: i64_ty,
                mutable: false,
            },
            MirLocalDecl {
                id: a,
                name: Atom::from("a"),
                ty: i64_ty,
                mutable: true,
            },
            MirLocalDecl {
                id: b,
                name: Atom::from("b"),
                ty: i64_ty,
                mutable: true,
            },
        ],
        block: MirBlock {
            stmts: vec![MirStmt::Switch {
                disc: Box::new(MirExpr::Local(disc)),
                cases: vec![
                    SwitchCase {
                        value: ConstValue::Int(1),
                        body: MirBlock {
                            stmts: vec![MirStmt::Assign {
                                target: MirPlace::Local { id: a },
                                value: MirExpr::Int {
                                    value: 10,
                                    ty: i64_ty,
                                },
                            }],
                        },
                    },
                    SwitchCase {
                        value: ConstValue::Int(2),
                        body: MirBlock {
                            stmts: vec![MirStmt::Assign {
                                target: MirPlace::Local { id: b },
                                value: MirExpr::Int {
                                    value: 20,
                                    ty: i64_ty,
                                },
                            }],
                        },
                    },
                ],
                default: Some(MirBlock {
                    stmts: vec![MirStmt::Return(Some(MirExpr::Int {
                        value: 0,
                        ty: i64_ty,
                    }))],
                }),
            }],
        },
    };
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(func));
    let s = emit_decls(&prog, &ts_aot_core::TypeTable::new())
        .expect("emit must succeed")
        .to_string();
    assert!(
        s.contains("match x"),
        "Switch must emit `match <disc>` over disc local, got: {s}"
    );
    assert!(
        s.contains("1 =>") && s.contains("2 =>"),
        "Switch cases must emit numeric arms `1 => ...` and `2 => ...`, got: {s}"
    );
    assert!(
        s.contains("_ =>"),
        "Switch default must emit `_ => ...` arm, got: {s}"
    );
}

#[test]
fn switch_with_string_cases_emits_match() {
    let mut func = empty_func("switch_string");
    let string_ty = TypeId::from_raw(8);
    let disc = LocalId::from_raw(0);
    func.body = MirBody {
        locals: vec![MirLocalDecl {
            id: disc,
            name: Atom::from("key"),
            ty: string_ty,
            mutable: false,
        }],
        block: MirBlock {
            stmts: vec![MirStmt::Switch {
                disc: Box::new(MirExpr::Local(disc)),
                cases: vec![SwitchCase {
                    value: ConstValue::String(Atom::from("foo")),
                    body: MirBlock {
                        stmts: vec![MirStmt::Return(Some(MirExpr::String {
                            id: Atom::from("foo"),
                            ty: string_ty,
                        }))],
                    },
                }],
                default: None,
            }],
        },
    };
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(func));
    let s = emit_decls(&prog, &ts_aot_core::TypeTable::new())
        .expect("emit must succeed")
        .to_string();
    assert!(
        s.contains("match key"),
        "Switch over string local must emit `match key`, got: {s}"
    );
    assert!(
        s.contains("\"foo\" =>"),
        "Switch string case must emit `\"foo\" =>` arm, got: {s}"
    );
    assert!(
        s.contains("_ => { }"),
        "Switch without default must emit empty `_ => {{}}` arm, got: {s}"
    );
}

#[test]
fn try_with_catch_emits_catch_unwind() {
    let mut func = empty_func("try_catch");
    let i64_ty = TypeId::from_raw(7);
    func.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Try {
                body: MirBlock {
                    stmts: vec![MirStmt::Return(Some(MirExpr::Int {
                        value: 1,
                        ty: i64_ty,
                    }))],
                },
                catch_param: None,
                catch: Some(MirBlock {
                    stmts: vec![MirStmt::Return(Some(MirExpr::Int {
                        value: 2,
                        ty: i64_ty,
                    }))],
                }),
                finally: None,
            }],
        },
    };
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(func));
    let s = emit_decls(&prog, &ts_aot_core::TypeTable::new())
        .expect("emit must succeed")
        .to_string();
    assert!(
        s.contains("catch_unwind"),
        "Try/Catch must wrap body in std::panic::catch_unwind, got: {s}"
    );
    assert!(
        s.contains("AssertUnwindSafe"),
        "Try/Catch closure must use AssertUnwindSafe to bypass UnwindSafe bound, got: {s}"
    );
    assert!(
        s.contains("if let Err") || s.contains("if let  Err"),
        "Try/Catch must wrap catch arm in `if let Err(...)` check, got: {s}"
    );
}

#[test]
fn try_with_finally_emits_finally_after_try() {
    let mut func = empty_func("try_finally");
    let i64_ty = TypeId::from_raw(7);
    func.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Try {
                body: MirBlock {
                    stmts: vec![MirStmt::Return(Some(MirExpr::Int {
                        value: 1,
                        ty: i64_ty,
                    }))],
                },
                catch_param: None,
                catch: None,
                finally: Some(MirBlock {
                    stmts: vec![MirStmt::Return(Some(MirExpr::Int {
                        value: 2,
                        ty: i64_ty,
                    }))],
                }),
            }],
        },
    };
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(func));
    let s = emit_decls(&prog, &ts_aot_core::TypeTable::new())
        .expect("emit must succeed")
        .to_string();
    assert!(
        s.contains("catch_unwind"),
        "Try/Finally must wrap body in catch_unwind, got: {s}"
    );
    assert!(
        s.contains("__pending_throw"),
        "Try/Finally without catch must allocate a __pending_throw slot so a throw inside try \
         can be re-raised after finally runs (otherwise finally would silently swallow errors). \
         Got: {s}"
    );
    assert!(
        s.contains("resume_unwind"),
        "Try/Finally must rethrow via resume_unwind after finally so uncaught throw still \
         propagates up. Got: {s}"
    );
    assert!(
        s.contains("return 2"),
        "Try/Finally must emit finally body after try block, got: {s}"
    );
}

#[test]
fn try_with_catch_and_finally_emits_both() {
    let mut func = empty_func("try_catch_finally");
    let i64_ty = TypeId::from_raw(7);
    func.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Try {
                body: MirBlock {
                    stmts: vec![MirStmt::Return(Some(MirExpr::Int {
                        value: 1,
                        ty: i64_ty,
                    }))],
                },
                catch_param: Some(LocalId::from_raw(0)),
                catch: Some(MirBlock {
                    stmts: vec![MirStmt::Expr(MirExpr::Int {
                        value: 9,
                        ty: i64_ty,
                    })],
                }),
                finally: Some(MirBlock {
                    stmts: vec![MirStmt::Return(Some(MirExpr::Int {
                        value: 3,
                        ty: i64_ty,
                    }))],
                }),
            }],
        },
    };
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(func));
    let s = emit_decls(&prog, &ts_aot_core::TypeTable::new())
        .expect("emit must succeed")
        .to_string();
    assert!(
        s.contains("catch_unwind"),
        "Try/Catch/Finally must use catch_unwind, got: {s}"
    );
    assert!(
        s.contains("if let Err"),
        "Try/Catch/Finally must have Err branch with catch body, got: {s}"
    );
    assert!(
        s.contains("return 3"),
        "Try/Catch/Finally must have finally body after catch, got: {s}"
    );
}

#[test]
fn try_catch_param_is_bound_for_catch_body_references() {
    let mut func = empty_func("try_catch_param");
    let i64_ty = TypeId::from_raw(7);
    let err_local = LocalId::from_raw(0);
    func.body = MirBody {
        locals: vec![MirLocalDecl {
            id: err_local,
            name: Atom::from("err"),
            ty: i64_ty,
            mutable: false,
        }],
        block: MirBlock {
            stmts: vec![MirStmt::Try {
                body: MirBlock {
                    stmts: vec![MirStmt::Return(Some(MirExpr::Int {
                        value: 1,
                        ty: i64_ty,
                    }))],
                },
                catch_param: Some(err_local),
                catch: Some(MirBlock {
                    stmts: vec![MirStmt::Return(Some(MirExpr::Local(err_local)))],
                }),
                finally: None,
            }],
        },
    };
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(func));
    let s = emit_decls(&prog, &ts_aot_core::TypeTable::new())
        .expect("emit must succeed")
        .to_string();
    assert!(
        s.contains("downcast ::<") || s.contains("downcast :: <"),
        "Try/Catch with catch_param must downcast __e to the parameter type, got: {s}"
    );
    assert!(
        s.contains("downcast"),
        "Try/Catch with catch_param must downcast `__e` to the catch_param's local type so \
         `let err: T = match __e.downcast::<T>() ...` produces a typed binding that \
         round-trips the thrown value (e.g. `let err: i64 = 42`). Without downcast, the \
         binding would be `Box<dyn Any>` and the catch body couldn't use it as `i64`. Got: {s}"
    );
    assert!(
        s.contains("err : i64") || s.contains("err : __ty") || s.contains("err : i64"),
        "Try/Catch with catch_param must type-annotate the binding so the catch body sees \
         the downcast type, not Box<dyn Any>. Got: {s}"
    );
    assert!(
        !s.contains("let _ = __e"),
        "Try/Catch with catch_param must NOT drop the payload via `let _ = __e;` (would leave \
         catch_param undeclared for catch body). Got: {s}"
    );
}

#[test]
fn try_catch_without_param_does_not_bind_param() {
    let mut func = empty_func("try_catch_no_param");
    let i64_ty = TypeId::from_raw(7);
    func.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Try {
                body: MirBlock {
                    stmts: vec![MirStmt::Return(Some(MirExpr::Int {
                        value: 1,
                        ty: i64_ty,
                    }))],
                },
                catch_param: None,
                catch: Some(MirBlock {
                    stmts: vec![MirStmt::Return(Some(MirExpr::Int {
                        value: 2,
                        ty: i64_ty,
                    }))],
                }),
                finally: None,
            }],
        },
    };
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(func));
    let s = emit_decls(&prog, &ts_aot_core::TypeTable::new())
        .expect("emit must succeed")
        .to_string();
    assert!(
        s.contains("let __catch_result = std :: panic :: catch_unwind"),
        "Try/Catch without catch_param must wrap catch body in catch_unwind so the catch arm's \
         own panic does not propagate as an unhandled throw. Got: {s}"
    );
    assert!(
        !s.contains("__e . downcast ::<"),
        "Try/Catch without catch_param must NOT downcast __e (no parameter type). Got: {s}"
    );
}

#[test]
fn throw_inside_try_emits_panic_throw_helper() {
    let mut func = empty_func("try_throw");
    let i64_ty = TypeId::from_raw(7);
    func.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Try {
                body: MirBlock {
                    stmts: vec![MirStmt::Throw {
                        error: MirExpr::Int {
                            value: 42,
                            ty: i64_ty,
                        },
                        error_ty: i64_ty,
                    }],
                },
                catch_param: None,
                catch: Some(MirBlock {
                    stmts: vec![MirStmt::Return(Some(MirExpr::Int {
                        value: 1,
                        ty: i64_ty,
                    }))],
                }),
                finally: None,
            }],
        },
    };
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(func));
    let s = emit_decls(&prog, &ts_aot_core::TypeTable::new())
        .expect("emit must succeed")
        .to_string();
    assert!(
        s.contains("__ts_aot_throw"),
        "Throw inside try must emit `__ts_aot_throw(...)` (panic-based, catchable by \
         catch_unwind). If we emitted `return Err(...)` instead, catch_unwind would never see \
         the throw (Result::Err is normal control flow, not a panic). Got: {s}"
    );
    assert!(
        !s.contains("return Err"),
        "Throw inside try must NOT emit `return Err(...)` (would bypass catch_unwind), got: {s}"
    );
}

#[test]
fn throw_outside_try_still_emits_return_err() {
    let mut func = empty_func("bare_throw");
    let i64_ty = TypeId::from_raw(7);
    func.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Throw {
                error: MirExpr::Int {
                    value: 7,
                    ty: i64_ty,
                },
                error_ty: i64_ty,
            }],
        },
    };
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(func));
    let s = emit_decls(&prog, &ts_aot_core::TypeTable::new())
        .expect("emit must succeed")
        .to_string();
    assert!(
        s.contains("return Err"),
        "Throw outside try must still emit `return Err(...)` (Result-based, propagates up to \
         function return), got: {s}"
    );
    assert!(
        !s.contains("__ts_aot_throw"),
        "Throw outside try must NOT emit `__ts_aot_throw(...)` (would panic the whole process \
         instead of returning Result::Err to caller), got: {s}"
    );
}

#[test]
fn nested_try_restores_in_try_state_for_catch_body() {
    let mut func = empty_func("nested_try_catch");
    let i64_ty = TypeId::from_raw(7);
    func.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Try {
                body: MirBlock {
                    stmts: vec![MirStmt::Throw {
                        error: MirExpr::Int {
                            value: 1,
                            ty: i64_ty,
                        },
                        error_ty: i64_ty,
                    }],
                },
                catch_param: None,
                catch: Some(MirBlock {
                    stmts: vec![MirStmt::Throw {
                        error: MirExpr::Int {
                            value: 2,
                            ty: i64_ty,
                        },
                        error_ty: i64_ty,
                    }],
                }),
                finally: None,
            }],
        },
    };
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(func));
    let s = emit_decls(&prog, &ts_aot_core::TypeTable::new())
        .expect("emit must succeed")
        .to_string();
    let first_throw = s.find("__ts_aot_throw");
    let second_throw = s.find("__ts_aot_throw");
    let throw_count = s.matches("__ts_aot_throw").count();
    assert!(
        first_throw.is_some(),
        "Inner try body must emit `__ts_aot_throw` (first occurrence, panic path), got: {s}"
    );
    assert!(
        second_throw.is_some() && throw_count >= 2,
        "Catch body throw (in_try=true) must ALSO emit `__ts_aot_throw` (so the throw from \
         catch is captured via the catch's own catch_unwind, stored in __pending_throw, and \
         rethrown). Previously catch body used `return Err` (Result path), but that bypassed \
         the outer try scope — wrong because in JS the catch-body throw IS catchable by the \
         surrounding finally and propagates as a fresh throw. Got: {s}"
    );
    assert!(
        s.contains("__pending_throw") && s.contains("resume_unwind"),
        "Outer try must keep __pending_throw slot and resume_unwind rethrow even when throw \
         originates from the catch arm (not just from try body). Got: {s}"
    );
}

#[test]
fn try_throw_with_finally_runs_finally_then_rethrows() {
    let mut func = empty_func("try_throw_finally");
    let i64_ty = TypeId::from_raw(7);
    let finally_flag = LocalId::from_raw(0);
    func.body = MirBody {
        locals: vec![MirLocalDecl {
            id: finally_flag,
            name: Atom::from("finally_ran"),
            ty: i64_ty,
            mutable: true,
        }],
        block: MirBlock {
            stmts: vec![MirStmt::Try {
                body: MirBlock {
                    stmts: vec![MirStmt::Throw {
                        error: MirExpr::Int {
                            value: 1,
                            ty: i64_ty,
                        },
                        error_ty: i64_ty,
                    }],
                },
                catch_param: None,
                catch: None,
                finally: Some(MirBlock {
                    stmts: vec![MirStmt::Assign {
                        target: MirPlace::Local { id: finally_flag },
                        value: MirExpr::Int {
                            value: 1,
                            ty: i64_ty,
                        },
                    }],
                }),
            }],
        },
    };
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(func));
    let s = emit_decls(&prog, &ts_aot_core::TypeTable::new())
        .expect("emit must succeed")
        .to_string();
    let finally_pos = s.find("finally_ran =");
    let replay_pos = s.find("if let Some (__e) = __pending_throw");
    assert!(
        finally_pos.is_some() && replay_pos.is_some() && finally_pos < replay_pos,
        "Finally body must be emitted BEFORE the pending-throw replay block, otherwise the \
         throw would bypass finally. The first `resume_unwind` inside the sentinel gate is \
         structural (part of the try body's catch_unwind wrapper) and appears before finally; \
         the pending-throw replay after finally is the correctness check. \
         Expected `finally_ran = ...` before `if let Some(__e) = __pending_throw`. Got: {s}"
    );
}

#[test]
fn try_return_in_body_runs_finally_then_replays_return() {
    let mut func = empty_func("try_return_finally");
    let i64_ty = TypeId::from_raw(7);
    func.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Try {
                body: MirBlock {
                    stmts: vec![MirStmt::Return(Some(MirExpr::Int {
                        value: 42,
                        ty: i64_ty,
                    }))],
                },
                catch_param: None,
                catch: None,
                finally: Some(MirBlock {
                    stmts: vec![MirStmt::Expr(MirExpr::Int {
                        value: 7,
                        ty: i64_ty,
                    })],
                }),
            }],
        },
    };
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(func));
    let s = emit_decls(&prog, &ts_aot_core::TypeTable::new())
        .expect("emit must succeed")
        .to_string();
    let break_pos = s.find("break __try_");
    let finally_pos = s.find("7 ;");
    let slot_set_pos = s.find("__return_slot_0 = Some (42)");
    let replay_pos = s.find("if let Some (__v) = __return_slot_0");
    assert!(
        break_pos.is_some()
            && finally_pos.is_some()
            && slot_set_pos.is_some()
            && replay_pos.is_some(),
        "try-with-return-and-finally must emit: try body sets runtime slot `__return_slot_0 = Some(42);` \
         and signals `break #label;`, then finally body (`7 ;`), then `if let Some(__v) = __return_slot_0 \
         {{ return __v; }}` (replay). Got: {s}"
    );
    assert!(
        break_pos < finally_pos && finally_pos < replay_pos,
        "Order must be: try body return (slot set + break) -> finally body -> replay. \
         If replay appears before finally, finally is skipped (wrong). Got: {s}"
    );
}

#[test]
fn try_catch_return_in_catch_runs_finally_then_replays_return() {
    let mut func = empty_func("try_catch_return");
    let i64_ty = TypeId::from_raw(7);
    let err_local = LocalId::from_raw(0);
    func.body = MirBody {
        locals: vec![MirLocalDecl {
            id: err_local,
            name: Atom::from("err"),
            ty: i64_ty,
            mutable: false,
        }],
        block: MirBlock {
            stmts: vec![MirStmt::Try {
                body: MirBlock {
                    stmts: vec![MirStmt::Throw {
                        error: MirExpr::Int {
                            value: 1,
                            ty: i64_ty,
                        },
                        error_ty: i64_ty,
                    }],
                },
                catch_param: Some(err_local),
                catch: Some(MirBlock {
                    stmts: vec![MirStmt::Return(Some(MirExpr::Int {
                        value: 99,
                        ty: i64_ty,
                    }))],
                }),
                finally: Some(MirBlock {
                    stmts: vec![MirStmt::Expr(MirExpr::Int {
                        value: 5,
                        ty: i64_ty,
                    })],
                }),
            }],
        },
    };
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(func));
    let s = emit_decls(&prog, &ts_aot_core::TypeTable::new())
        .expect("emit must succeed")
        .to_string();
    assert!(
        s.contains("break __try_"),
        "catch body's `return 99` must be transformed to `break #try_label` so finally runs. \
         Bare `return 99` inside the catch would exit the function and skip finally. Got: {s}"
    );
    assert!(
        s.contains("__return_slot_0 = Some (99)"),
        "catch body's return must set runtime slot `__return_slot_0 = Some(99);` so finally can \
         observe and replay it. Got: {s}"
    );
    let break_pos = s.find("break __try_").expect("break in emit");
    let finally_pos = s.find("5 ;").expect("finally in emit");
    let replay_pos = s
        .find("if let Some (__v) = __return_slot_0")
        .expect("replay in emit");
    assert!(
        break_pos < finally_pos && finally_pos < replay_pos,
        "Order must be: catch return → break -> finally body -> replay (if let Some). \
         Got: {s}"
    );
}

#[test]
fn try_with_conditional_return_emits_closure_safe_signal() {
    let mut func = empty_func("try_cond_return");
    let i64_ty = TypeId::from_raw(7);
    let cond = LocalId::from_raw(0);
    func.body = MirBody {
        locals: vec![MirLocalDecl {
            id: cond,
            name: Atom::from("c"),
            ty: i64_ty,
            mutable: false,
        }],
        block: MirBlock {
            stmts: vec![MirStmt::Try {
                body: MirBlock {
                    stmts: vec![MirStmt::If {
                        cond: MirExpr::Local(cond),
                        then_block: MirBlock {
                            stmts: vec![MirStmt::Return(Some(MirExpr::Int {
                                value: 1,
                                ty: i64_ty,
                            }))],
                        },
                        else_block: Some(MirBlock {
                            stmts: vec![MirStmt::Return(Some(MirExpr::Int {
                                value: 2,
                                ty: i64_ty,
                            }))],
                        }),
                    }],
                },
                catch_param: None,
                catch: None,
                finally: None,
            }],
        },
    };
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(func));
    let s = emit_decls(&prog, &ts_aot_core::TypeTable::new())
        .expect("emit must succeed")
        .to_string();
    assert!(
        s.contains("__ReturnSignal"),
        "try with conditional return must declare __ReturnSignal enum as closure-safe \
         control signal, got: {s}"
    );
    assert!(
        s.contains("__return_slot_0 = Some (1)"),
        "if-branch return must set runtime slot to Some(1) so the executed branch's value \
         is preserved at runtime (not overwritten by codegen-time last-write-wins). Got: {s}"
    );
    assert!(
        s.contains("__return_slot_0 = Some (2)"),
        "else-branch return must set runtime slot to Some(2) so the executed branch's value \
         is preserved at runtime. Got: {s}"
    );
    assert!(
        s.contains("if let Some (__v) = __return_slot_0"),
        "after the try block, replay must use the runtime slot via `if let Some(__v) = slot \
         {{ return __v; }}` so whichever branch executed drives the actual return. Got: {s}"
    );
    assert!(
        s.contains("Ok (__ReturnSignal :: Break)"),
        "each return must also signal `return Ok(__ReturnSignal::Break);` so the outer loop \
         can perform the break (no cross-closure `break #label;`). Got: {s}"
    );
    assert!(
        s.contains("match __try_result"),
        "outer try loop must pattern-match on __try_result to perform the break. Got: {s}"
    );
}

#[test]
fn nested_try_finally_return_propagates_through_outer_replay() {
    let mut func = empty_func("nested_try_finally");
    let i64_ty = TypeId::from_raw(7);
    func.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Try {
                body: MirBlock {
                    stmts: vec![MirStmt::Try {
                        body: MirBlock {
                            stmts: vec![MirStmt::Return(Some(MirExpr::Int {
                                value: 42,
                                ty: i64_ty,
                            }))],
                        },
                        catch_param: None,
                        catch: None,
                        finally: Some(MirBlock {
                            stmts: vec![MirStmt::Expr(MirExpr::Int {
                                value: 1,
                                ty: i64_ty,
                            })],
                        }),
                    }],
                },
                catch_param: None,
                catch: None,
                finally: Some(MirBlock {
                    stmts: vec![MirStmt::Expr(MirExpr::Int {
                        value: 2,
                        ty: i64_ty,
                    })],
                }),
            }],
        },
    };
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(func));
    let s = emit_decls(&prog, &ts_aot_core::TypeTable::new())
        .expect("emit must succeed")
        .to_string();
    assert!(
        s.contains("__return_slot_1 = Some (42)"),
        "inner try's return 42 must set the inner runtime slot (__return_slot_1 since \
         counter is allocated on enter), got: {s}"
    );
    let inner_pos = s
        .find("__return_slot_1 = Some (42)")
        .expect("inner slot position");
    let inner_finally_pos = s.find("1 ;").expect("inner finally position");
    let inner_replay_pos = s
        .find("__return_slot_0 = Some (__v)")
        .or_else(|| s.find("if let Some (__v) = __return_slot_1"))
        .expect("inner replay position");
    assert!(
        inner_pos < inner_finally_pos && inner_finally_pos < inner_replay_pos,
        "Inner try order: set inner slot -> run inner finally (`1 ;`) -> inner replay. Got: {s}"
    );
    assert!(
        s.contains("__return_slot_0 = Some (__v)"),
        "Inner replay must propagate to the outer slot (__return_slot_0) for nested try, \
         not `return __v;` (which would skip the outer finally). Got: {s}"
    );
    let outer_finally_pos = s.rfind("2 ;").expect("outer finally position");
    let outer_replay_pos = s
        .rfind("if let Some (__v) = __return_slot_0")
        .expect("outer replay position");
    let outer_return_pos = s.rfind("return __v ;").expect("outer return position");
    assert!(
        outer_replay_pos < outer_return_pos,
        "Outermost try's replay must `return __v;` (no parent slot to propagate to). Got: {s}"
    );
    let inner_replay_to_outer_finally = inner_replay_pos < outer_finally_pos;
    let outer_finally_to_outer_replay = outer_finally_pos < outer_replay_pos;
    assert!(
        inner_replay_to_outer_finally && outer_finally_to_outer_replay,
        "Outer try order: inner replay propagates -> outer finally (`2 ;`) -> outer replay. \
         Got: {s}"
    );
}

#[test]
fn nested_try_in_generator_propagates_through_outer_replay() {
    let mut func = empty_func("nested_try_gen");
    let i64_ty = TypeId::from_raw(7);
    func.kind = FunctionKind::Generator;
    func.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Try {
                body: MirBlock {
                    stmts: vec![MirStmt::Try {
                        body: MirBlock {
                            stmts: vec![MirStmt::Return(Some(MirExpr::Int {
                                value: 7,
                                ty: i64_ty,
                            }))],
                        },
                        catch_param: None,
                        catch: None,
                        finally: None,
                    }],
                },
                catch_param: None,
                catch: None,
                finally: Some(MirBlock {
                    stmts: vec![MirStmt::Expr(MirExpr::Int {
                        value: 9,
                        ty: i64_ty,
                    })],
                }),
            }],
        },
    };
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(func));
    let s = emit_decls(&prog, &ts_aot_core::TypeTable::new())
        .expect("emit must succeed")
        .to_string();
    assert!(
        s.contains("__return_slot_1 = Some (Some (7))"),
        "Inner generator try return 7 must set inner slot to Some(Some(7)). Got: {s}"
    );
    assert!(
        s.contains("__return_slot_0 = Some (__v)"),
        "Inner generator replay must propagate Some(Some(7)) to outer slot via \
         `__return_slot_0 = Some(__v);` (not `return __v;` which would exit the generator). Got: {s}"
    );
    assert!(
        s.contains("if let Some (__v) = __return_slot_0 { return __v ; }")
            || s.contains("if let Some (__v) = __return_slot_0 { return __v;}"),
        "Outermost generator try's replay must `return __v;`. Got: {s}"
    );
}

#[test]
fn do_while_emits_loop_with_break_on_negated_cond() {
    let mut func = empty_func("do_while");
    let i64_ty = TypeId::from_raw(7);
    let cond = LocalId::from_raw(0);
    func.body = MirBody {
        locals: vec![MirLocalDecl {
            id: cond,
            name: Atom::from("c"),
            ty: i64_ty,
            mutable: false,
        }],
        block: MirBlock {
            stmts: vec![MirStmt::DoWhile {
                body: MirBlock {
                    stmts: vec![MirStmt::Expr(MirExpr::Local(cond))],
                },
                cond: MirExpr::Local(cond),
            }],
        },
    };
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(func));
    let s = emit_decls(&prog, &ts_aot_core::TypeTable::new())
        .expect("emit must succeed")
        .to_string();
    assert!(
        s.contains("loop"),
        "DoWhile must emit Rust `loop` (body executes at least once), got: {s}"
    );
    assert!(
        s.contains("if ! c")
            || s.contains("if  ! c")
            || s.contains("if ! (c")
            || s.contains("if ! c ;"),
        "DoWhile must break on negated cond `if !c {{ break; }}`, got: {s}"
    );
    assert!(
        s.contains("break"),
        "DoWhile must emit `break` for exit, got: {s}"
    );
}

#[test]
fn do_while_with_empty_body_emits_loop_break_only() {
    let mut func = empty_func("do_while_empty");
    func.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::DoWhile {
                body: MirBlock { stmts: Vec::new() },
                cond: MirExpr::Bool(false),
            }],
        },
    };
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(func));
    let s = emit_decls(&prog, &ts_aot_core::TypeTable::new())
        .expect("emit must succeed")
        .to_string();
    assert!(
        s.contains("loop") && s.contains("if !"),
        "DoWhile(empty body, false cond) must still emit `loop {{ if !false {{ break; }} }}`, got: {s}"
    );
}

#[test]
fn do_while_continue_in_body_uses_labeled_continue_for_cond_recheck() {
    let mut func = empty_func("do_while_continue");
    func.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::DoWhile {
                body: MirBlock {
                    stmts: vec![MirStmt::Continue],
                },
                cond: MirExpr::Bool(true),
            }],
        },
    };
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(func));
    let s = emit_decls(&prog, &ts_aot_core::TypeTable::new())
        .expect("emit must succeed")
        .to_string();
    assert!(
        s.contains("__do_while_") && s.contains(": loop"),
        "DoWhile must wrap body in a labeled loop `__do_while_N: loop {{ ... }}` so the cond \
         check fires on every iteration (including continue paths). Plain `loop` would let a \
         bare `continue;` skip the `if !cond {{ break; }}` check and break do-while semantics. \
         Got: {s}"
    );
    assert!(
        s.contains("continue") && !s.contains("continue ;"),
        "Continue in DoWhile body must use the do-while label (e.g. `continue __do_while_0;`) \
         so it lands on the `if !cond {{ break; }}` line instead of jumping to the loop's top. \
         Bare `continue;` would skip the cond recheck. Got: {s}"
    );
    assert!(
        s.contains("break __do_while_") || s.contains("break  __do_while_"),
        "Break in DoWhile's cond check must also use the label for consistency (and so nested \
         loops exit the right one). Got: {s}"
    );
}

#[test]
fn continue_outside_dowhile_uses_bare_continue() {
    let mut func = empty_func("bare_while_continue");
    let i64_ty = TypeId::from_raw(7);
    let cond = LocalId::from_raw(0);
    func.body = MirBody {
        locals: vec![MirLocalDecl {
            id: cond,
            name: Atom::from("c"),
            ty: i64_ty,
            mutable: false,
        }],
        block: MirBlock {
            stmts: vec![MirStmt::While {
                cond: MirExpr::Local(cond),
                body: MirBlock {
                    stmts: vec![MirStmt::Expr(MirExpr::Local(cond)), MirStmt::Continue],
                },
            }],
        },
    };
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(func));
    let s = emit_decls(&prog, &ts_aot_core::TypeTable::new())
        .expect("emit must succeed")
        .to_string();
    assert!(
        s.contains("continue ;") || s.contains("continue;") || s.contains("continue ;"),
        "Continue inside a regular While body (not DoWhile) must emit bare `continue;` — only \
         DoWhile needs labeled continue to re-evaluate its cond. Adding a label to a plain While \
         would compile but is noise. Got: {s}"
    );
    assert!(
        !s.contains("__do_while_"),
        "Continue in a regular While must NOT carry a __do_while_ label, got: {s}"
    );
}

#[test]
fn function_typed_param_and_return_exclude_function_from_dispatch_table() {
    let mut types = TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let fn_param_ty = types.intern(&Type::Fn {
        params: vec![i64_ty],
        ret: i64_ty,
        err: None,
    });
    let fn_return_ty = types.intern(&Type::Fn {
        params: vec![],
        ret: i64_ty,
        err: None,
    });

    let mut with_fn_param = empty_func("with_fn_param");
    with_fn_param.ret = i64_ty;
    with_fn_param.params = vec![MirParam {
        id: LocalId::from_raw(0),
        name: Atom::from("cb"),
        ty: fn_param_ty,
    }];

    let mut with_fn_return = empty_func("with_fn_return");
    with_fn_return.id = FunctionId::from_raw(1);
    with_fn_return.ret = fn_return_ty;
    with_fn_return.params = vec![MirParam {
        id: LocalId::from_raw(0),
        name: Atom::from("x"),
        ty: i64_ty,
    }];

    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(with_fn_param));
    prog.push_decl(MirDecl::Function(with_fn_return));
    let tokens = emit_decls(&prog, &types).expect("decls should emit");
    let s = tokens.to_string();

    assert!(
        !s.contains("__ts_aot_dispatch_with_fn_param"),
        "function-typed param => Type::Fn not in is_u64_arg_packable set => no `fn(&[u64]) -> u64` wrapper. Got: {s}"
    );
    assert!(
        !s.contains("__ts_aot_dispatch_with_fn_return"),
        "function-typed return => Type::Fn not in is_u64_ret_packable set => no `fn(&[u64]) -> u64` wrapper. Got: {s}"
    );
    assert!(
        !s.contains("__TS_AOT_DISPATCH_TABLE"),
        "no sync dispatchable function (both excluded by function types) => no dispatch table. Got: {s}"
    );
}

#[test]
fn function_typed_param_emits_unit_placeholder() {
    let mut types = TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let fn_ty = types.intern(&Type::Fn {
        params: vec![i64_ty],
        ret: i64_ty,
        err: None,
    });

    let mut f = empty_func("take_fn");
    f.ret = i64_ty;
    f.params = vec![MirParam {
        id: LocalId::from_raw(0),
        name: Atom::from("cb"),
        ty: fn_ty,
    }];
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(f));
    let tokens = emit_decls(&prog, &types).expect("decls should emit");
    let s = tokens.to_string();

    let sig = signature_fragment(&s, "take_fn");
    let sig_norm: String = sig.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        sig_norm.contains("cb:()"),
        "param typed `(a: i64) => i64` must emit as `cb:()` (unit placeholder) in the function signature \n         until Phase 5 introduces a real function-typed runtime. Whitespace-insensitive. \n         Got signature fragment: `{sig}`"
    );
}

#[test]
fn function_typed_return_emits_unit_placeholder() {
    let mut types = TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let fn_ty = types.intern(&Type::Fn {
        params: vec![i64_ty],
        ret: i64_ty,
        err: None,
    });

    let mut f = empty_func("make_fn");
    f.ret = fn_ty;
    f.params = vec![];
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(f));
    let tokens = emit_decls(&prog, &types).expect("decls should emit");
    let s = tokens.to_string();

    let sig = signature_fragment(&s, "make_fn");
    let sig_norm: String = sig.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        sig_norm.contains("->()"),
        "return type `(a: i64) => i64` must emit as `->()` (unit placeholder) in the function signature \n         until Phase 5 introduces a real function-typed runtime. Whitespace-insensitive. \n         Got signature fragment: `{sig}`"
    );
}

#[test]
fn dynamic_import_emits_turbofish_for_unused_namespace() {
    let mut types = TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let str_ty = types.intern(&Type::String);
    let mut f = empty_func("load_module");
    f.ret = i64_ty;
    f.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![
                MirStmt::Expr(MirExpr::Import {
                    source: Box::new(MirExpr::String {
                        id: Atom::from("./mod.js"),
                        ty: str_ty,
                    }),
                    ty: i64_ty,
                }),
                MirStmt::Return(Some(MirExpr::Int {
                    value: 0,
                    ty: i64_ty,
                })),
            ],
        },
    };
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(f));
    let tokens = emit_decls(&prog, &types).expect("decls should emit");
    let s = tokens.to_string();
    assert!(
        s.contains("__ts_aot_dynamic_import :: < i64 >"),
        "Standalone (unused) dynamic import must carry the resolved namespace type as a turbofish, \
         otherwise the generic parameter is unconstrained and the call fails to type-check. Got: {s}"
    );
}

#[test]
fn underscore_param_uses_consistent_ident_in_signature_and_body() {
    let mut types = TypeTable::new();
    let i32_ty = types.intern(&Type::I32);
    let param = LocalId::from_raw(0);
    let mut f = empty_func("forward");
    f.ret = i32_ty;
    f.params = vec![MirParam {
        id: param,
        name: Atom::from("_"),
        ty: i32_ty,
    }];
    f.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Return(Some(MirExpr::Local(param)))],
        },
    };
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(f));
    let tokens = emit_decls(&prog, &types).expect("decls should emit");
    let s = tokens.to_string();

    let sig = signature_fragment(&s, "forward");
    assert!(
        sig.contains("__tmp0 : i32"),
        "frontend-emitted `_` param must be renamed to a consistent `__tmp0` in the signature, not \
         left as a bare `_` (Rust would treat `_` as a wildcard binding and refuse to address it). \
         Got signature fragment: `{sig}`"
    );
    assert!(
        !sig.contains(" _ : i32") && !sig.contains("(_ : i32"),
        "signature must NOT emit a literal `_ : i32` (anonymous wildcard) for the param, got: `{sig}`"
    );
    assert!(
        s.contains("return __tmp0"),
        "body must use the SAME `__tmp0` ident the signature declared, otherwise the function \
         fails to compile (signature names one binding, body references another). Got: `{s}`"
    );
    assert!(
        !s.contains("return _ ;") && !s.contains("return (_"),
        "body must NOT use a bare `_` (Rust would not accept `_` as an expression). Got: `{s}`"
    );
}

#[test]
fn dispatch_wrapper_renames_underscore_param_to_usable_ident() {
    let mut types = TypeTable::new();
    let i32_ty = types.intern(&Type::I32);
    let param = LocalId::from_raw(0);
    let mut f = empty_func("dispatchable");
    f.ret = i32_ty;
    f.params = vec![MirParam {
        id: param,
        name: Atom::from("_"),
        ty: i32_ty,
    }];
    f.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Return(Some(MirExpr::Int {
                value: 0,
                ty: i32_ty,
            }))],
        },
    };
    f.kind = FunctionKind::Plain;
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(f));
    let tokens = emit_decls(&prog, &types).expect("decls should emit");
    let s = tokens.to_string();
    assert!(
        s.contains("pub fn __ts_aot_dispatch_dispatchable"),
        "dispatchable plain function with packable ret+args must produce a `__ts_aot_dispatch_*` \
         wrapper, got: {s}"
    );
    let wrapper_pos = s
        .find("pub fn __ts_aot_dispatch_dispatchable")
        .expect("wrapper position");
    let wrapper_body = s
        .get(wrapper_pos..)
        .and_then(|rest| {
            let open = rest.find('{')?;
            let close = rest.rfind('}')?;
            rest.get(open..=close)
        })
        .unwrap_or("");
    assert!(
        wrapper_body.contains("let __tmp0 : i32 = __slot_0 as i32 ;"),
        "dispatch wrapper must unpack `_args[0]` into a renamed `__tmp0 : i32` local (not a bare \
         `_` which Rust would treat as an anonymous wildcard binding and refuse to address), got: \
         `{wrapper_body}`"
    );
    assert!(
        wrapper_body.contains("dispatchable (__tmp0)"),
        "dispatch wrapper must call the inner function with the renamed `__tmp0` arg, got: \
         `{wrapper_body}`"
    );
    assert!(
        !wrapper_body.contains("dispatchable (_)"),
        "dispatch wrapper must NOT pass a bare `_` as the inner call argument (Rust would reject \
         the call site), got: `{wrapper_body}`"
    );
}

#[test]
fn array_push_runtime_marks_array_param_mut() {
    let mut types = TypeTable::new();
    let i32_ty = types.intern(&Type::I32);
    let arr_ty = types.intern(&Type::Array { element: i32_ty });
    let array_param = LocalId::from_raw(0);
    let mut f = empty_func("grow");
    f.ret = i32_ty;
    f.params = vec![MirParam {
        id: array_param,
        name: Atom::from("arr"),
        ty: arr_ty,
    }];
    let value = LocalId::from_raw(1);
    f.body = MirBody {
        locals: vec![MirLocalDecl {
            id: value,
            name: Atom::from("v"),
            ty: i32_ty,
            mutable: false,
        }],
        block: MirBlock {
            stmts: vec![
                MirStmt::Runtime {
                    op: RuntimeOp::ArrayPush,
                    args: vec![MirExpr::Local(array_param), MirExpr::Local(value)],
                    dest: None,
                    ty: arr_ty,
                    target_ty: None,
                },
                MirStmt::Return(Some(MirExpr::Local(value))),
            ],
        },
    };
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(f));
    let tokens = emit_decls(&prog, &types).expect("decls should emit");
    let s = tokens.to_string();
    let sig = signature_fragment(&s, "grow");
    assert!(
        sig.contains("mut arr"),
        "param passed as `args[0]` to RuntimeOp::ArrayPush must emit `mut arr` in the function \
         signature (array push needs &mut Vec). Got signature fragment: `{sig}`"
    );
}

#[test]
fn array_set_runtime_marks_array_param_mut() {
    let mut types = TypeTable::new();
    let i32_ty = types.intern(&Type::I32);
    let arr_ty = types.intern(&Type::Array { element: i32_ty });
    let array_param = LocalId::from_raw(0);
    let mut f = empty_func("poke");
    f.ret = TypeId::from_raw(0);
    f.params = vec![MirParam {
        id: array_param,
        name: Atom::from("arr"),
        ty: arr_ty,
    }];
    let value = LocalId::from_raw(1);
    f.body = MirBody {
        locals: vec![MirLocalDecl {
            id: value,
            name: Atom::from("v"),
            ty: i32_ty,
            mutable: false,
        }],
        block: MirBlock {
            stmts: vec![MirStmt::Runtime {
                op: RuntimeOp::ArraySet,
                args: vec![
                    MirExpr::Local(array_param),
                    MirExpr::Int {
                        value: 0,
                        ty: i32_ty,
                    },
                    MirExpr::Local(value),
                ],
                dest: None,
                ty: arr_ty,
                target_ty: None,
            }],
        },
    };
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(f));
    let tokens = emit_decls(&prog, &types).expect("decls should emit");
    let s = tokens.to_string();
    let sig = signature_fragment(&s, "poke");
    assert!(
        sig.contains("mut arr"),
        "param passed as `args[0]` to RuntimeOp::ArraySet must emit `mut arr` in the function \
         signature (array indexed-set needs &mut [T]). Got signature fragment: `{sig}`"
    );
}

#[test]
fn array_set_runtime_call_emits_mut_borrow() {
    let mut types = TypeTable::new();
    let i32_ty = types.intern(&Type::I32);
    let arr_ty = types.intern(&Type::Array { element: i32_ty });
    let array_param = LocalId::from_raw(0);
    let mut f = empty_func("poke");
    f.ret = TypeId::from_raw(0);
    f.params = vec![MirParam {
        id: array_param,
        name: Atom::from("arr"),
        ty: arr_ty,
    }];
    let value = LocalId::from_raw(1);
    f.body = MirBody {
        locals: vec![MirLocalDecl {
            id: value,
            name: Atom::from("v"),
            ty: i32_ty,
            mutable: false,
        }],
        block: MirBlock {
            stmts: vec![MirStmt::Runtime {
                op: RuntimeOp::ArraySet,
                args: vec![
                    MirExpr::Local(array_param),
                    MirExpr::Int {
                        value: 0,
                        ty: i32_ty,
                    },
                    MirExpr::Local(value),
                ],
                dest: None,
                ty: arr_ty,
                target_ty: None,
            }],
        },
    };
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(f));
    let tokens = emit_decls(&prog, &types).expect("decls should emit");
    let s = tokens.to_string();
    assert!(
        s.contains("__ts_aot_array_set (& mut arr"),
        "RuntimeOp::ArraySet call site must emit `&mut arr` to match __ts_aot_array_set's \
         &mut [T] parameter (dedicated arm in emit_runtime_call). Got emitted code: `{s}`"
    );
}

#[test]
fn map_set_runtime_marks_map_param_mut() {
    let mut types = TypeTable::new();
    let str_ty = types.intern(&Type::String);
    let map_ty_placeholder = types.intern(&Type::Error);
    let map_param = LocalId::from_raw(0);
    let mut f = empty_func("set_kv");
    f.ret = TypeId::from_raw(0);
    f.params = vec![MirParam {
        id: map_param,
        name: Atom::from("m"),
        ty: map_ty_placeholder,
    }];
    f.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Runtime {
                op: RuntimeOp::MapSet,
                args: vec![
                    MirExpr::Local(map_param),
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
    };
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(f));
    let tokens = emit_decls(&prog, &types).expect("decls should emit");
    let s = tokens.to_string();
    let sig = signature_fragment(&s, "set_kv");
    assert!(
        sig.contains("mut m"),
        "param passed as `args[0]` to RuntimeOp::MapSet must emit `mut m` in the function \
         signature (map set needs &mut map). Got signature fragment: `{sig}`"
    );
}

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
    let gen_ty = types.intern(&Type::Generator { inner: i64_ty });
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

#[test]
fn closure_param_for_of_generator_iterable_emits_mut() {
    let mut types = TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let gen_ty = types.intern(&Type::Generator { inner: i64_ty });
    let outer_param = LocalId::from_raw(0);
    let closure_param = LocalId::from_raw(10);
    let item = LocalId::from_raw(11);
    let mut f = empty_func("drain_via_closure");
    f.ret = TypeId::from_raw(0);
    f.params = vec![MirParam {
        id: outer_param,
        name: Atom::from("g"),
        ty: gen_ty,
    }];
    f.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Return(Some(MirExpr::Closure {
                params: vec![MirParam {
                    id: closure_param,
                    name: Atom::from("g"),
                    ty: gen_ty,
                }],
                captures: Vec::new(),
                locals: vec![MirLocalDecl {
                    id: item,
                    name: Atom::from("x"),
                    ty: i64_ty,
                    mutable: false,
                }],
                body: MirBlock {
                    stmts: vec![MirStmt::ForOf {
                        item,
                        iterable: MirExpr::Local(closure_param),
                        iter_ty: gen_ty,
                        body: MirBlock {
                            stmts: vec![MirStmt::Expr(MirExpr::Local(item))],
                        },
                    }],
                },
                ret_ty: i64_ty,
                fn_ty: gen_ty,
            }))],
        },
    };
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(f));
    let tokens = emit_decls(&prog, &types).expect("decls should emit");
    let s = tokens.to_string();
    let closure_sig = s
        .split("move |")
        .nth(1)
        .and_then(|after_opener| after_opener.split('|').next())
        .unwrap_or("");
    assert!(
        closure_sig.trim_start().starts_with("mut "),
        "closure param that is the iterable of a `for-of` over a Generator must emit \
         `mut g` in the move-closure signature (regression: closure body walk did not mark \
         the closure-local iterable as mutated when the closure was emitted); got \
         closure signature fragment: `{closure_sig}`"
    );
}

#[test]
fn closure_param_assigned_in_if_body_emits_mut() {
    let mut types = TypeTable::new();
    let int_ty = types.intern(&Type::I32);
    let param_id = LocalId::from_raw(0);
    let mut f = empty_func("make_closure");
    f.ret = int_ty;
    f.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Return(Some(MirExpr::Closure {
                params: vec![MirParam {
                    id: param_id,
                    name: Atom::from("x"),
                    ty: int_ty,
                }],
                captures: Vec::new(),
                locals: Vec::new(),
                body: MirBlock {
                    stmts: vec![
                        MirStmt::If {
                            cond: MirExpr::Bool(true),
                            then_block: MirBlock {
                                stmts: vec![MirStmt::Assign {
                                    target: ts_aot_ir_mir::MirPlace::Local { id: param_id },
                                    value: MirExpr::Int {
                                        value: 1,
                                        ty: int_ty,
                                    },
                                }],
                            },
                            else_block: None,
                        },
                        MirStmt::Return(Some(MirExpr::Local(param_id))),
                    ],
                },
                ret_ty: int_ty,
                fn_ty: int_ty,
            }))],
        },
    };
    let tokens = emit_function(&f, &types).expect("function should emit");
    let s = tokens.to_string();
    assert!(
        s.contains("mut x :"),
        "closure param assigned inside an `if` then-block must emit `mut x : i32`; got: {s}"
    );
}

#[test]
fn closure_param_unused_emits_immut() {
    let mut types = TypeTable::new();
    let int_ty = types.intern(&Type::I32);
    let param_id = LocalId::from_raw(0);
    let mut f = empty_func("make_closure_pure");
    f.ret = int_ty;
    f.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Return(Some(MirExpr::Closure {
                params: vec![MirParam {
                    id: param_id,
                    name: Atom::from("x"),
                    ty: int_ty,
                }],
                captures: Vec::new(),
                locals: Vec::new(),
                body: MirBlock {
                    stmts: vec![MirStmt::Return(Some(MirExpr::Local(param_id)))],
                },
                ret_ty: int_ty,
                fn_ty: int_ty,
            }))],
        },
    };
    let tokens = emit_function(&f, &types).expect("function should emit");
    let s = tokens.to_string();
    let param_sig = s
        .split("move |")
        .nth(1)
        .and_then(|after_opener| after_opener.split('|').next())
        .unwrap_or("");
    assert!(
        !param_sig.trim_start().starts_with("mut "),
        "closure param that is not assigned must remain `x : i32` (no `mut`); got: `{param_sig}`"
    );
}

#[test]
fn closure_param_passed_to_array_push_emits_mut() {
    let mut types = TypeTable::new();
    let int_ty = types.intern(&Type::I32);
    let arr_ty = types.intern(&Type::Array { element: int_ty });
    let param_id = LocalId::from_raw(0);
    let mut f = empty_func("make_closure_push");
    f.ret = int_ty;
    f.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Return(Some(MirExpr::Closure {
                params: vec![MirParam {
                    id: param_id,
                    name: Atom::from("arr"),
                    ty: arr_ty,
                }],
                captures: Vec::new(),
                locals: Vec::new(),
                body: MirBlock {
                    stmts: vec![MirStmt::Runtime {
                        op: ts_aot_ir_mir::RuntimeOp::ArrayPush,
                        args: vec![
                            MirExpr::Local(param_id),
                            MirExpr::Int {
                                value: 42,
                                ty: int_ty,
                            },
                        ],
                        dest: None,
                        ty: TypeId::from_raw(0),
                        target_ty: None,
                    }],
                },
                ret_ty: int_ty,
                fn_ty: int_ty,
            }))],
        },
    };
    let tokens = emit_function(&f, &types).expect("function should emit");
    let s = tokens.to_string();
    let param_sig = s
        .split("move |")
        .nth(1)
        .and_then(|after_opener| after_opener.split('|').next())
        .unwrap_or("");
    assert!(
        param_sig.trim_start().starts_with("mut "),
        "closure param passed as `args[0]` to RuntimeOp::ArrayPush must emit `mut arr : ...` \
         (runtime requires &mut Vec); got: `{param_sig}`"
    );
}

#[test]
fn closure_param_assigned_via_optional_chain_place_emits_mut() {
    let mut types = TypeTable::new();
    let i32_ty = types.intern(&Type::I32);
    let struct_id = StructId::from_raw(0);
    let obj_ty = types.intern(&Type::Struct { id: struct_id });
    let opt_ty = types.intern(&Type::Optional { inner: obj_ty });
    let param_id = LocalId::from_raw(0);

    let mut f = empty_func("make_closure_optional_chain");
    f.ret = i32_ty;
    f.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Return(Some(MirExpr::Closure {
                params: vec![MirParam {
                    id: param_id,
                    name: Atom::from("obj"),
                    ty: obj_ty,
                }],
                captures: Vec::new(),
                locals: Vec::new(),
                body: MirBlock {
                    stmts: vec![MirStmt::Assign {
                        target: MirPlace::Field {
                            base: Box::new(MirPlaceBase::Chain {
                                base: Box::new(MirExpr::OptionalChain {
                                    base: Box::new(MirExpr::Local(param_id)),
                                    ty: opt_ty,
                                }),
                                ty: opt_ty,
                            }),
                            field: FieldId::from_raw(0),
                            ty: i32_ty,
                        },
                        value: MirExpr::Int {
                            value: 0,
                            ty: i32_ty,
                        },
                    }],
                },
                ret_ty: i32_ty,
                fn_ty: i32_ty,
            }))],
        },
    };
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Struct(MirStructDecl {
        id: struct_id,
        name: Atom::from("Box"),
        fields: vec![MirFieldDecl {
            id: FieldId::from_raw(0),
            name: Atom::from("x"),
            ty: i32_ty,
            mutable: true,
            visibility: Visibility::Public,
        }],
        methods: Vec::new(),
    }));
    prog.push_decl(MirDecl::Function(f));
    let tokens = emit_decls(&prog, &types).expect("decls should emit");
    let s = tokens.to_string();
    let param_sig = s
        .split("move |")
        .nth(1)
        .and_then(|after_opener| after_opener.split('|').next())
        .unwrap_or("");
    assert!(
        param_sig.trim_start().starts_with("mut "),
        "closure param assigned through MirPlaceBase::Chain over OptionalChain over Local \
         must emit `mut obj : ...` (regression: visit_base for Chain in body.rs lacked the \
         early-insert that collect_written_locals in ctx.rs has via place_base_root_local_id); \
         got: `{param_sig}`"
    );
}

#[test]
fn closure_local_array_assigned_via_array_set_emits_mut() {
    let mut types = TypeTable::new();
    let i32_ty = types.intern(&Type::I32);
    let arr_ty = types.intern(&Type::Array { element: i32_ty });
    let arr_local = LocalId::from_raw(10);
    let mut f = empty_func("make_closure_array_set");
    f.ret = i32_ty;
    f.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Return(Some(MirExpr::Closure {
                params: Vec::new(),
                captures: Vec::new(),
                locals: vec![MirLocalDecl {
                    id: arr_local,
                    name: Atom::from("arr"),
                    ty: arr_ty,
                    mutable: false,
                }],
                body: MirBlock {
                    stmts: vec![
                        MirStmt::Let {
                            local: arr_local,
                            ty: arr_ty,
                            init: None,
                            mutable: false,
                        },
                        MirStmt::Runtime {
                            op: ts_aot_ir_mir::RuntimeOp::ArraySet,
                            args: vec![
                                MirExpr::Local(arr_local),
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
                            ty: TypeId::from_raw(0),
                            target_ty: None,
                        },
                    ],
                },
                ret_ty: i32_ty,
                fn_ty: i32_ty,
            }))],
        },
    };
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(f));
    let tokens = emit_decls(&prog, &types).expect("decls should emit");
    let s = tokens.to_string();
    assert!(
        s.contains("let mut arr"),
        "closure local array passed as `args[0]` to RuntimeOp::ArraySet must be re-registered \
         with `mutable=true` (regression: collect_assigned_locals in body.rs was filtering \
         candidates to closure params only, so mutably-borrowed closure locals stayed \
         `let arr = ...` and the generated Rust required `&mut Vec<T>` on an immutable \
         binding); got tokens: `{s}`"
    );
}
