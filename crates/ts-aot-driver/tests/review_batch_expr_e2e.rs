use ts_aot_driver::{CompileOptions, Driver, DriverOutput, EmitStage};
use ts_aot_test_support::{check_generated_crate, run_generated_crate};

fn compile(source: &str) -> DriverOutput {
    Driver::new().compile_source(
        "test.ts",
        source,
        &CompileOptions {
            emit: EmitStage::Rust,
            module: true,
        },
    )
}

fn rust_of(source: &str) -> String {
    let out = compile(source);
    assert!(
        !out.has_errors(),
        "must compile through the full pipeline; got {:?}",
        out.diagnostics
    );
    out.rust_source
        .expect("emit-rust must populate rust_source")
}

fn codes(out: &DriverOutput) -> Vec<String> {
    out.diagnostics
        .iter()
        .map(|d| d.code.as_str().to_owned())
        .collect()
}

#[test]
fn a_field_write_on_one_holder_keeps_another_holders_classes() {
    const SOURCE: &str = "class A { static k: number = 1; } \
         class B { static k: number = 2; } \
         class Holder { C = class { static k: number = 0; }; } \
         export function got(): number { \
             const first = new Holder(); \
             const second = new Holder(); \
             const alias = second; \
             alias.C = A; \
             first.C = B; \
             return alias.C.k * 100 + first.C.k; \
         }";
    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate(
        "holder_isolation",
        &rust,
        "
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    holder_isolation::__ts_aot_dispatch_ts_aot_tla_main(&ctx, &[]);
    assert_eq!(holder_isolation::got(&ctx), 102.0);
    println!(\"ok\");
}
",
    );
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_local_class_expression_with_a_non_literal_static_field_is_refused() {
    const SOURCE: &str = "export function got(): number { \
             const n = 3; \
             const C = class { static k: number = n; }; \
             return C.k; \
         }";
    let out = compile(SOURCE);
    let diag = out
        .diagnostics
        .errors()
        .find(|d| d.code.as_str() == "E0419")
        .expect("a local class expression with a non-literal static field must report E0419");
    assert!(
        diag.message
            .contains("of a class declared inside a function cannot be initialized here"),
        "E0419 must name the shared module-level static global; got {:?}",
        diag.message
    );
    assert!(
        out.rust_source.is_none(),
        "no Rust may be emitted for a local class expression whose static field would silently \
         become the zero value; got:\n{:?}",
        out.rust_source
    );
}

#[test]
fn a_local_class_expression_with_a_literal_static_field_still_compiles_and_runs() {
    const SOURCE: &str = "export function got(): number { \
             const C = class { static k: number = 4; }; \
             return C.k; \
         }";
    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate(
        "local_class_expression_literal",
        &rust,
        "
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(local_class_expression_literal::got(&ctx), 4.0);
    println!(\"ok\");
}
",
    );
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_module_level_class_expression_static_field_still_initializes() {
    const SOURCE: &str = "const n = 3; \
         const C = class { static k: number = n; }; \
         export function got(): number { return C.k; }";
    let rust = rust_of(SOURCE);
    check_generated_crate("module_class_expression_static", &rust);
    let stdout = run_generated_crate(
        "module_class_expression_static",
        &rust,
        "
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    module_class_expression_static::__ts_aot_dispatch_ts_aot_tla_main(&ctx, &[]);
    assert_eq!(module_class_expression_static::got(&ctx), 3.0);
    println!(\"ok\");
}
",
    );
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_void_call_fallback_of_coalesce_contributes_undefined() {
    const SOURCE: &str = "function v(): void { } \
         export function got(a: number | null): number { \
             const r = a ?? v(); \
             if (r === undefined) { return 1; } \
             return 0; \
         }";
    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate(
        "coalesce_void_call",
        &rust,
        "
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    use ::core::option::Option::{None, Some};
    assert_eq!(coalesce_void_call::got(&ctx, None), 1.0);
    assert_eq!(coalesce_void_call::got(&ctx, Some(5.0)), 0.0);
    println!(\"ok\");
}
",
    );
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_null_returning_call_fallback_of_coalesce_contributes_null() {
    const SOURCE: &str = "function nothing(): null { return null; } \
         export function got(a: number | undefined): number { \
             const r = a ?? nothing(); \
             if (r === null) { return 1; } \
             return 0; \
         }";
    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate(
        "coalesce_null_call",
        &rust,
        "
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    use ::core::option::Option::{None, Some};
    assert_eq!(coalesce_null_call::got(&ctx, None), 1.0);
    assert_eq!(coalesce_null_call::got(&ctx, Some(5.0)), 0.0);
    println!(\"ok\");
}
",
    );
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn an_inherited_field_type_informs_a_parameter_hint() {
    const SOURCE: &str = "class Base { value: i32 = 0; } \
         class Derived extends Base { } \
         const d: Derived = new Derived(); \
         export function run(v): i32 { d.value = v; return d.value; }";
    let rust = rust_of(SOURCE);
    let norm: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        norm.contains("v:i32)"),
        "`d.value` is inherited from `Base`, so `v` must be inferred as `i32`; got:\n{rust}"
    );
    check_generated_crate("inherited_field_hint", &rust);
    let stdout = run_generated_crate(
        "inherited_field_hint",
        &rust,
        "
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(inherited_field_hint::run(&ctx, 7), 7);
    println!(\"ok\");
}
",
    );
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_capture_takes_the_type_of_the_binding_it_resolves() {
    const SOURCE: &str = "export function got0(): string { \
             let x: number = 100; \
             { \
                 const f = (): string => { \
                     class C { m(): string { return x; } } \
                     return new C().m(); \
                 }; \
                 let x: string = \"bbb\"; \
                 return f(); \
             } \
         }";
    let rust = rust_of(SOURCE);
    let norm: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        norm.contains("__capture_x:::std::rc::Rc<::core::cell::RefCell<f64>>"),
        "the capture field must take the type of the binding its id resolves to (the outer \
         `x: number`), not the type of an unrelated inner predeclaration; got:\n{rust}"
    );
    assert!(
        !norm.contains("__capture_x:::std::rc::Rc<::core::cell::RefCell<()>>"),
        "the capture must not combine the outer binding's id with another binding's placeholder \
         type; got:\n{rust}"
    );
}

#[test]
fn an_unreachable_if_branch_does_not_offer_parameter_hints() {
    const SOURCE: &str = "export function f(x): i32 { if (false) { x = 1; } return x; } \
         export function got(): i32 { return f(7); }";
    let rust = rust_of(SOURCE);
    let norm: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        norm.contains("x:i32)"),
        "the `if (false)` body never runs, so only the `return x` hint may type `x`; got:\n{rust}"
    );
    let stdout = run_generated_crate(
        "unreachable_hint",
        &rust,
        "
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(unreachable_hint::got(&ctx), 7);
    println!(\"ok\");
}
",
    );
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn an_unknown_condition_still_scans_both_parameter_hint_arms() {
    const SOURCE: &str = "export function f(x, flag: boolean): i32 { if (flag) { x = 1; } \
         return x; } \
         export function got(): i32 { return f(7, false); }";
    let out = compile(SOURCE);
    assert!(
        out.diagnostics.errors().any(|d| d.code.as_str() == "E0405"),
        "an unknown condition may run the branch, so the conflicting `f64` hint must stay; got {:?}",
        out.diagnostics
    );
    assert!(
        out.rust_source.is_none(),
        "the unresolved parameter must not reach codegen; got:\n{:?}",
        out.rust_source
    );
}

#[test]
fn an_uninitialized_var_does_not_shadow_a_parameter() {
    const SOURCE: &str = "export function f(x): i32 { var x; return x; } \
         export function got(): i32 { return f(7); }";
    let rust = rust_of(SOURCE);
    let norm: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        norm.contains("x:i32)"),
        "`var x;` never assigns the parameter, so the `return x` hint must still type it; got:\n\
         {rust}"
    );
    let stdout = run_generated_crate(
        "uninitialized_var",
        &rust,
        "
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(uninitialized_var::got(&ctx), 7);
    println!(\"ok\");
}
",
    );
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn an_uninitialized_var_still_hides_a_module_function() {
    let out = compile(
        "function g(a: string): string { return a; } \
         export function f(x): string { const r = g(x); { var g; } return r; }",
    );
    assert!(
        codes(&out).contains(&"E0405".to_owned()),
        "a function-scoped `var g` must stay in scope and hide the module `g`, so the call above \
         it may not type `x`; got {:?}",
        out.diagnostics
    );
}
