use ts_aot_driver::{CompileOptions, Driver, DriverOutput, EmitStage};

fn compile_rust(source: &str) -> DriverOutput {
    Driver::new().compile_source(
        "test.ts",
        source,
        &CompileOptions {
            emit: EmitStage::Rust,
            module: true,
        },
    )
}

#[test]
fn non_capturing_closure_value_and_indirect_call_compiles() {
    let out = compile_rust(
        "export function run(): i64 { const g: () => i64 = (): i64 => 7; return g(); }",
    );
    assert!(
        !out.has_errors(),
        "closure sample must compile through the pipeline; got {:?}",
        out.diagnostics
    );
    let source = out.rust_source.expect("rust_source");
    assert!(
        source.contains("__TsFnValue0"),
        "a non-capturing closure stored in a variable must become a Fn-value enum; got:\n{source}"
    );
    assert!(
        source.contains("__ts_fn_call_0"),
        "an indirect call through a function value must use the static-dispatch helper; got:\n{source}"
    );
}

#[test]
fn capturing_closure_compiles() {
    let out = compile_rust(
        "export function run(): i64 { const x: i64 = 5; const g: () => i64 = (): i64 => x; return g(); }",
    );
    assert!(
        !out.has_errors(),
        "capturing closure must compile through the pipeline; got {:?}",
        out.diagnostics
    );
    let source = out.rust_source.expect("rust_source");
    assert!(
        source.contains("__ts_closure_"),
        "a capturing closure must be defunctionalized into a static function; got:\n{source}"
    );
}

#[test]
fn refcell_shared_capture_self_assignment_hoists_value() {
    let out = compile_rust(
        "export function run(): i64 { let s: string = \"a\"; \
         const g: () => void = (): void => { s = s; }; \
         g(); return 0; }",
    );
    assert!(!out.has_errors(), "got {:?}", out.diagnostics);
    let source = out.rust_source.expect("rust_source");
    assert!(
        source.contains("__ts_shared_write"),
        "the assigned value must be evaluated into a local before the mutable borrow; got:\n{source}"
    );
}

#[test]
fn captured_function_parameter_compiles() {
    let out = compile_rust(
        "export function make(x: i64): () => i64 { return (): i64 => x; } \
         export function run(): i64 { const f: () => i64 = make(9); return f(); }",
    );
    assert!(!out.has_errors(), "got {:?}", out.diagnostics);
    let source = out.rust_source.expect("rust_source");
    assert!(
        source.contains("Some (x)") || source.contains("Some(x)"),
        "a captured function parameter must be stored in the scope env at entry; got:\n{source}"
    );
}

#[test]
fn mutable_capture_is_shared_with_enclosing_scope() {
    let out = compile_rust(
        "export function run(): i64 { let x: i64 = 0; const f: () => i64 = (): i64 => x; x = 1; return f(); }",
    );
    assert!(
        !out.has_errors(),
        "shared mutable capture must compile; got {:?}",
        out.diagnostics
    );
    let source = out.rust_source.expect("rust_source");
    assert!(
        source.contains("std :: rc :: Rc") || source.contains("std::rc::Rc"),
        "a mutable captured local must live in shared storage (Rc<Cell<..>>); got:\n{source}"
    );
}

#[test]
fn uninitialized_shared_let_resets_env_to_none() {
    let out = compile_rust(
        "export function run(): i64 { let x: i64; const f: () => i64 = (): i64 => x; x = 7; return f(); }",
    );
    assert!(
        !out.has_errors(),
        "uninitialized shared capture must compile; got {:?}",
        out.diagnostics
    );
    let source = out.rust_source.expect("rust_source");
    assert!(
        source.contains("set (None)") || source.contains("set(None)"),
        "an uninitialized captured `let` must reset its env field to None, not Default::default(); got:\n{source}"
    );
    assert!(
        !source.contains("= Default :: default") && !source.contains("= Default::default"),
        "an uninitialized captured `let` must not initialize its env field with Default::default(); got:\n{source}"
    );
}

#[test]
fn shared_array_element_read_does_not_clone_container() {
    let out = compile_rust(
        "export function run(): i64 { const a: i64[] = [1, 2]; const f: () => i64 = (): i64 => a[0]; return f(); }",
    );
    assert!(
        !out.has_errors(),
        "shared array element read must compile; got {:?}",
        out.diagnostics
    );
    let source = out.rust_source.expect("rust_source");
    assert!(
        !source.contains(".clone () [0]") && !source.contains(".clone()[0]"),
        "a shared array element read must index the borrow instead of cloning the whole container; got:\n{source}"
    );
    assert!(
        source.contains("[0] . clone ()") || source.contains("[0].clone()"),
        "expected the element to be cloned after indexing; got:\n{source}"
    );
}

#[test]
fn string_length_compiles() {
    let out =
        compile_rust("export function run(): i64 { const s: string = \"abc\"; return s.length; }");
    assert!(!out.has_errors(), "got {:?}", out.diagnostics);
    let _source = out.rust_source.expect("rust_source");
}

#[test]
fn rethrown_caught_value_recovers_in_typed_catch() {
    let out = compile_rust(
        "export function run(): i64 { try { try { throw 1; } catch (e) { throw e; } } catch (e2: i64) { return e2; } return 0; }",
    );
    assert!(!out.has_errors(), "got {:?}", out.diagnostics);
    let _source = out.rust_source.expect("rust_source");
}

#[test]
fn shared_local_let_initializer_clones_noncopy_source_compiles() {
    let out = compile_rust(
        "export function run(): i64 { const a: string = \"x\"; let f: string = a; const g = (): string => f; f = \"y\"; if (g() + a === \"yx\") { return 1; } return 0; }",
    );
    assert!(!out.has_errors(), "got {:?}", out.diagnostics);
    let _source = out.rust_source.expect("rust_source");
}

#[test]
fn caught_typed_equality_clones_noncopy_operand_compiles() {
    let out = compile_rust(
        "export function run(): i64 { let s: string = \"boom\"; let r: i64 = 0; try { throw \"boom\"; } catch (e) { if (e === s) { r = 1; } } return r + s.length; }",
    );
    assert!(!out.has_errors(), "got {:?}", out.diagnostics);
    let _source = out.rust_source.expect("rust_source");
}

#[test]
fn array_literal_clones_noncopy_elements_compiles() {
    let out = compile_rust(
        "export function run(): i64 { const s: string = \"hello\"; const arr: string[] = [s]; return s.length + arr.length; }",
    );
    assert!(!out.has_errors(), "got {:?}", out.diagnostics);
    let _source = out.rust_source.expect("rust_source");
}

#[test]
fn array_is_array_compiles() {
    let out = compile_rust(
        "export function run(): i64 { const arr: i64[] = [1, 2]; if (Array.isArray(arr)) { return arr.length; } return 0; }",
    );
    assert!(!out.has_errors(), "got {:?}", out.diagnostics);
    let _source = out.rust_source.expect("rust_source");
}

#[test]
fn array_from_with_mapfn_compiles() {
    let out = compile_rust(
        "function dbl(x: i64): i64 { return x * 2; } export function run(): i64 { const src: i64[] = [1, 2]; const a: i64[] = Array.from(src, dbl); return a[1] + a.length; }",
    );
    assert!(!out.has_errors(), "got {:?}", out.diagnostics);
    let _source = out.rust_source.expect("rust_source");
}

#[test]
fn new_date_constructor_compiles() {
    let out = compile_rust(
        "export function run(): i64 { const d = new Date(1000); return d.getTime(); }",
    );
    assert!(!out.has_errors(), "got {:?}", out.diagnostics);
    let _source = out.rust_source.expect("rust_source");
}

#[test]
fn array_from_with_fnvalue_mapfn_compiles() {
    let out = compile_rust(
        "function apply(f: (x: i64) => i64, v: i64): i64 { return f(v); } \
         function dbl(x: i64): i64 { return x * 2; } \
         export function run(): i64 { const src: i64[] = [1, 2]; const a: i64[] = Array.from(src, dbl); return apply(dbl, 1) + a[1]; }",
    );
    assert!(!out.has_errors(), "got {:?}", out.diagnostics);
    let _source = out.rust_source.expect("rust_source");
}

#[test]
fn in_float_index_key_compiles() {
    let out = compile_rust(
        "export function run(): i64 { const arr: i64[] = [10, 20]; if (1.0 in arr) { return 1; } return 0; }",
    );
    assert!(!out.has_errors(), "got {:?}", out.diagnostics);
    let _source = out.rust_source.expect("rust_source");
}

#[test]
fn in_noninteger_float_key_is_false_compiles() {
    let out = compile_rust(
        "export function run(): i64 { const arr: i64[] = [10, 20]; if (1.5 in arr) { return 1; } return 0; }",
    );
    assert!(!out.has_errors(), "got {:?}", out.diagnostics);
    let _source = out.rust_source.expect("rust_source");
}

#[test]
fn json_stringify_string_compiles() {
    let out = compile_rust(
        "export function run(): i64 { const s: string = \"hi\"; const j: string = JSON.stringify(s); return j.length; }",
    );
    assert!(!out.has_errors(), "got {:?}", out.diagnostics);
    let _source = out.rust_source.expect("rust_source");
}

#[test]
fn array_from_compiles() {
    let out = compile_rust(
        "export function run(): i64 { const src: i64[] = [1, 2]; const a: i64[] = Array.from(src); return a.length; }",
    );
    assert!(!out.has_errors(), "got {:?}", out.diagnostics);
    let _source = out.rust_source.expect("rust_source");
}

#[test]
fn in_array_index_compiles() {
    let out = compile_rust(
        "export function run(): i64 { const arr: i64[] = [10, 20]; if (0 in arr) { return 1; } return 0; }",
    );
    assert!(!out.has_errors(), "got {:?}", out.diagnostics);
    let _source = out.rust_source.expect("rust_source");
}

#[test]
fn ternary_result_type_is_inferred_compiles() {
    let out =
        compile_rust("export function run(): i64 { const c: bool = true; return c ? 10 : 20; }");
    assert!(!out.has_errors(), "got {:?}", out.diagnostics);
    let _source = out.rust_source.expect("rust_source");
}

#[test]
fn concat_null_and_undefined_compiles() {
    let out = compile_rust(
        "export function run(): i64 { const a: string = \"x\" + null; const b: string = \"y\" + undefined; if (a === \"xnull\") { if (b === \"yundefined\") { return 1; } } return 0; }",
    );
    assert!(!out.has_errors(), "got {:?}", out.diagnostics);
    let _source = out.rust_source.expect("rust_source");
}

#[test]
fn string_local_reuse_after_copy_compiles() {
    let out = compile_rust(
        "export function run(): i64 { const a: string = \"x\"; const s: string = a; const r: string = s + a; if (r === \"xx\") { return 1; } return 0; }",
    );
    assert!(!out.has_errors(), "got {:?}", out.diagnostics);
    let _source = out.rust_source.expect("rust_source");
}

#[test]
fn string_concat_operand_order_compiles() {
    let out = compile_rust(
        "export function run(): i64 { let s: string = \"a\"; const r: string = s + (s = \"b\"); if (r === \"ab\") { return 1; } return 0; }",
    );
    assert!(!out.has_errors(), "got {:?}", out.diagnostics);
    let _source = out.rust_source.expect("rust_source");
}

#[test]
fn string_concat_snapshots_lhs_before_mutating_call_compiles() {
    let out = compile_rust(
        "export function run(): i64 { let s: string = \"a\"; const set = (): string => { s = \"b\"; return \"\"; }; const r: string = s + set(); if (r === \"a\") { return 1; } return 0; }",
    );
    assert!(!out.has_errors(), "got {:?}", out.diagnostics);
    let _source = out.rust_source.expect("rust_source");
}

#[test]
fn string_concat_reusing_operand_compiles() {
    let out = compile_rust(
        "export function run(): i64 { const a: string = \"a\"; const b: string = \"b\"; const s: string = a + b + a; if (s === \"aba\") { return 1; } return 0; }",
    );
    assert!(!out.has_errors(), "got {:?}", out.diagnostics);
    let _source = out.rust_source.expect("rust_source");
}

#[test]
fn mixed_string_concat_chain_compiles() {
    let out = compile_rust(
        "export function run(): i64 { const s: string = \"a\" + 1 + 2; if (s === \"a12\") { return 1; } return 0; }",
    );
    assert!(!out.has_errors(), "got {:?}", out.diagnostics);
    let _source = out.rust_source.expect("rust_source");
}

#[test]
fn string_concat_with_function_typed_callee_compiles() {
    let out = compile_rust(
        "export function run(): i64 { const g: () => i64 = (): i64 => 5; const s: string = \"a\" + g(); if (s === \"a5\") { return 1; } return 0; }",
    );
    assert!(!out.has_errors(), "got {:?}", out.diagnostics);
    let _source = out.rust_source.expect("rust_source");
}

#[test]
fn string_concat_and_template_compiles() {
    let out = compile_rust(
        "export function run(): i64 { const s: string = \"a\" + \"b\"; const t: string = `v=${s}`; if (s === \"ab\") { return 1; } return 0; }",
    );
    assert!(!out.has_errors(), "got {:?}", out.diagnostics);
    let source = out.rust_source.expect("rust_source");
    assert!(
        source.contains("__ts_aot_string_concat"),
        "string `+` and template literals must lower to __ts_aot_string_concat; got:\n{source}"
    );
}

#[test]
fn nested_closures_share_one_scope_env() {
    let out = compile_rust(
        "export function run(): i64 { let x: i64 = 1; \
         const outer: () => i64 = (): i64 => { \
           const inner: () => i64 = (): i64 => x; \
           x = x + 1; \
           return inner(); \
         }; \
         return outer(); }",
    );
    assert!(!out.has_errors(), "got {:?}", out.diagnostics);
    let source = out.rust_source.expect("rust_source");
    assert!(
        source.contains("__ScopeEnv") && source.contains("Rc < __ScopeEnv"),
        "shared captures must be grouped into one Rc<ScopeEnv>; got:\n{source}"
    );
    assert_eq!(
        source.matches("struct __ScopeEnv").count(),
        1,
        "one scope env per scope (the nested closure reuses the outer env); got:\n{source}"
    );
}

#[test]
fn nested_closure_forwards_intermediate_capture() {
    let out = compile_rust(
        "export function run(): i64 { let x: i64 = 9; \
         const outer: () => i64 = (): i64 => { \
           const inner: () => i64 = (): i64 => x; \
           return inner(); \
         }; \
         return outer(); }",
    );
    assert!(!out.has_errors(), "got {:?}", out.diagnostics);
    let _source = out.rust_source.expect("rust_source");
}

#[test]
fn sibling_closures_share_one_scope() {
    let out = compile_rust(
        "export function run(): i64 { let x: i64 = 0; \
         const a: () => i64 = (): i64 => x; \
         const b: () => i64 = (): i64 => x; \
         x = 7; return a() + b(); }",
    );
    assert!(!out.has_errors(), "got {:?}", out.diagnostics);
    let _source = out.rust_source.expect("rust_source");
}

#[test]
fn function_named_self_used_as_value_compiles() {
    let out = compile_rust(
        "export function self(): i64 { return 1; } \
         export function run(): i64 { const f: () => i64 = self; return f(); }",
    );
    assert!(!out.has_errors(), "got {:?}", out.diagnostics);
    let source = out.rust_source.expect("rust_source");
    assert!(
        source.contains("Self_"),
        "a function named `self` must not produce a reserved-word enum variant; got:\n{source}"
    );
}

#[test]
fn closure_returning_closure_over_local_compiles() {
    let out = compile_rust(
        "export function run(): i64 { let x: i64 = 1; \
         const mk: () => (() => i64) = (): (() => i64) => { const y: i64 = x + 1; return (): i64 => y; }; \
         const g: () => i64 = mk(); return g(); }",
    );
    assert!(!out.has_errors(), "got {:?}", out.diagnostics);
    let _source = out.rust_source.expect("rust_source");
}

#[test]
fn for_of_item_captured_by_closure_compiles() {
    let out = compile_rust(
        "export function run(): i64 { const xs: i64[] = [1, 2, 3]; let total: i64 = 0; \
         for (const n of xs) { const g: () => i64 = (): i64 => n; total = total + g(); } \
         return total; }",
    );
    assert!(!out.has_errors(), "got {:?}", out.diagnostics);
    let _source = out.rust_source.expect("rust_source");
}

#[test]
fn shadowed_same_name_captured_by_distinct_closures() {
    let out = compile_rust(
        "export function run(): i64 { let x: i64 = 1; \
         const f: () => i64 = (): i64 => x; \
         { let x: i64 = 2; const g: () => i64 = (): i64 => x; x = 3; \
           return f() * 10 + g(); } }",
    );
    assert!(!out.has_errors(), "got {:?}", out.diagnostics);
    let _source = out.rust_source.expect("rust_source");
}

#[test]
fn closure_parameter_shadowing_outer_capture_compiles() {
    let out = compile_rust(
        "export function run(): i64 { let x: i64 = 1; \
         const outer: () => i64 = (): i64 => x; \
         const inner: (x: i64) => i64 = (x: i64): i64 => x; \
         x = 2; return outer() * 10 + inner(5); }",
    );
    assert!(!out.has_errors(), "got {:?}", out.diagnostics);
    let _source = out.rust_source.expect("rust_source");
}

#[test]
fn fn_value_enum_has_manual_debug() {
    let out = compile_rust(
        "export function run(): i64 { const f: () => i64 = (): i64 => 7; return f(); }",
    );
    assert!(!out.has_errors(), "got {:?}", out.diagnostics);
    let source = out.rust_source.expect("rust_source");
    assert!(
        source.contains("impl :: std :: fmt :: Debug for __TsFnValue0")
            || source.contains("impl ::std::fmt::Debug for __TsFnValue0"),
        "the FnValue enum must provide a manual Debug impl; got:\n{source}"
    );
    assert!(
        source.contains("# [derive (Clone)]") || source.contains("#[derive(Clone)]"),
        "Clone derive must be kept; got:\n{source}"
    );
}

#[test]
fn array_of_closures_compiles() {
    let out = compile_rust(
        "export function run(): i64 { \
         const fns: Array<() => i64> = [(): i64 => 1, (): i64 => 2]; \
         return fns[1](); }",
    );
    assert!(!out.has_errors(), "got {:?}", out.diagnostics);
    let source = out.rust_source.expect("rust_source");
    assert!(
        source.contains("bumpalo :: collections :: Vec < 'a , __TsFnValue0 < 'a > >")
            || source.contains("bumpalo::collections::Vec<'a,__TsFnValue0<'a>>"),
        "an annotated array of closures must be typed as Vec<FnValue>; got:\n{source}"
    );
}

#[test]
fn annotated_int_array_literal_is_typed() {
    let out =
        compile_rust("export function run(): i64 { const xs: i64[] = [1, 2, 3]; return xs[1]; }");
    assert!(!out.has_errors(), "got {:?}", out.diagnostics);
    let source = out.rust_source.expect("rust_source");
    assert!(
        source.contains("bumpalo :: collections :: Vec < 'a , i64 >")
            || source.contains("bumpalo::collections::Vec<'a,i64>"),
        "an annotated array literal must carry its element type; got:\n{source}"
    );
}

#[test]
fn function_value_passed_as_argument_compiles() {
    let out = compile_rust(
        "function apply(f: () => i64): i64 { return f(); } \
         export function run(): i64 { const g: () => i64 = (): i64 => 5; return apply(g); }",
    );
    assert!(
        !out.has_errors(),
        "passing a closure as an argument must compile; got {:?}",
        out.diagnostics
    );
    let source = out.rust_source.expect("rust_source");
    assert!(
        source.contains("__TsFnValue0"),
        "the parameter must be typed by the Fn-value enum; got:\n{source}"
    );
}
