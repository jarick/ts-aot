use super::super::{emit_decls, emit_decls_with_style};
use super::{empty_func, extract_dispatch_table, norm};
use ts_aot_core::{Atom, FunctionId, LocalId, ModuleId, Type, TypeId, TypeTable};
use ts_aot_ir_mir::{MirBlock, MirBody, MirDecl, MirParam, MirProgram};

#[test]
fn empty_program_emits_no_decls() {
    let prog = MirProgram::new(ModuleId::from_raw(0));
    let types = TypeTable::new();
    let tokens = emit_decls(&prog, &types).expect("decls should emit");
    assert!(tokens.is_empty());
}

#[test]
fn lib_style_omits_main_and_publishes_dispatch_table() {
    let mut types = TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let mut f = empty_func("add");
    f.ret = i64_ty;
    f.params = vec![
        MirParam {
            id: LocalId::from_raw(0),
            name: Atom::from("a"),
            ty: i64_ty,
        },
        MirParam {
            id: LocalId::from_raw(1),
            name: Atom::from("b"),
            ty: i64_ty,
        },
    ];
    let mut tla = empty_func("__tla_main");
    tla.id = FunctionId::from_raw(1);
    tla.ret = i64_ty;
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.is_module = true;
    prog.tla_main_name = Some(Atom::from("__tla_main"));
    prog.push_decl(MirDecl::Function(f));
    prog.push_decl(MirDecl::Function(tla));

    let s = emit_decls_with_style(&prog, &types, true)
        .expect("lib-style emit should succeed")
        .to_string();
    assert!(
        s.contains("pub fn __ts_aot_init ()"),
        "lib style with a configured top-level entry must publish the library initializer, got:\n{s}"
    );
    assert!(
        !s.contains("fn main ()"),
        "lib style must not emit `fn main` even when a top-level entry is configured, got:\n{s}"
    );
    assert!(
        s.contains("pub const __TS_AOT_DISPATCH_TABLE"),
        "lib style must publish the dispatch table, got:\n{s}"
    );
}

#[test]
fn lib_style_async_tla_init_uses_reentrant_runner() {
    let mut types = TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let mut f = empty_func("__tla_main");
    f.ret = i64_ty;
    f.effects.is_async = true;
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.is_module = true;
    prog.tla_main_name = Some(Atom::from("__tla_main"));
    prog.push_decl(MirDecl::Function(f));

    let s = emit_decls_with_style(&prog, &types, true)
        .expect("lib-style emit should succeed")
        .to_string();
    assert!(
        s.contains("__ts_aot_run_or_poll"),
        "async TLA init must use the reentrant runner, got:\n{s}"
    );
    assert!(
        !s.contains("__ts_aot_runtime_run"),
        "async TLA init must not use the non-reentrant runner, got:\n{s}"
    );
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
        s.contains(
            "pub fn __ts_aot_dispatch_add < 'a > (__ctx : & ts_aot_runtime :: Ctx < 'a > , _args : & [u64]) -> u64"
        ),
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
        s.contains(
            "(\"add\" , __ts_aot_dispatch_add as for < 'a > fn (& ts_aot_runtime :: Ctx < 'a > , & [u64]) -> u64)"
        ),
        "table entry must reference the wrapper with original function name as string, got: {s}"
    );
}

#[test]
fn bool_param_function_emits_wrapper_unpacking_via_ne_zero() {
    let mut types = TypeTable::new();
    let bool_ty = types.intern(&Type::Bool);
    let mut f = empty_func("negate");
    f.ret = bool_ty;
    f.params = vec![MirParam {
        id: LocalId::from_raw(0),
        name: Atom::from("x"),
        ty: bool_ty,
    }];
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(f));
    let tokens = emit_decls(&prog, &types).expect("decls should emit");
    let s = tokens.to_string();
    assert!(
        s.contains("let x : bool = __slot_0 != 0 ;"),
        "bool param must be unpacked via `__slot_0 != 0` (Rust rejects `u64 as bool`), got: {s}"
    );
    assert!(
        !s.contains("__slot_0 as bool"),
        "bool param must not be unpacked via `u64 as bool`, got: {s}"
    );
    assert!(
        s.contains("__result as u64"),
        "bool return must be packed via `as u64`, got: {s}"
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
    let norm_s = norm(&s);
    let wrapper_start = norm_s
        .find("pubfn__ts_aot_dispatch_log_something")
        .unwrap_or_else(|| panic!("void return must still get a dispatch wrapper, got: {s}"));
    let wrapper_body = norm_s[wrapper_start..]
        .split('}')
        .next()
        .unwrap_or_default();
    assert!(
        wrapper_body.ends_with(";0"),
        "the void wrapper must return the zero literal, got wrapper body: {wrapper_body}"
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
    let table_norm = norm(&table_str);
    assert!(
        table_norm.contains("(\"foo\",__ts_aot_dispatch_fooasfor"),
        "the `foo` table key must map to `__ts_aot_dispatch_foo`, got: {table_str}"
    );
    assert!(
        table_norm.contains("(\"bar\",__ts_aot_dispatch_barasfor"),
        "the `bar` table key must map to `__ts_aot_dispatch_bar`, got: {table_str}"
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
