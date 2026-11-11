//! `??`, non-boolean `||`/`&&`, optional-chain method calls and constructors.
//!
//! These operators all yield one of their *operands* (not a `bool`) and
//! short-circuit, which Rust's own `||`/`&&` cannot express. The generated
//! crates are compiled and their runtime results compared with JavaScript.

use ts_aot_driver::{CompileOptions, Driver, EmitStage};
use ts_aot_test_support::{check_generated_crate, run_generated_crate};

fn compile_module(source: &str) -> ts_aot_driver::DriverOutput {
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
    let out = compile_module(source);
    assert!(
        !out.has_errors(),
        "must compile through the full pipeline; got {:?}",
        out.diagnostics
    );
    out.rust_source
        .expect("emit-rust must populate rust_source")
}

#[test]
fn nullish_coalescing_matches_javascript_not_logical_or() {
    const SOURCE: &str = "export function pick(a: number | null, b: number): number { return a ?? b; } \
         export function never(a: number, b: number): number { return a ?? b; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(nullish_coalescing::pick(&ctx, ::core::option::Option::None, 7.0), 7.0);
    // `0 ?? 7` is 0: only *nullish* triggers the fallback, unlike `0 || 7`.
    assert_eq!(nullish_coalescing::pick(&ctx, ::core::option::Option::Some(0.0), 7.0), 0.0);
    assert_eq!(nullish_coalescing::never(&ctx, 0.0, 7.0), 0.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let norm: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        !norm.contains("||"),
        "`??` must not be lowered to Rust's `||` (it is `bool`-only and would treat 0 as \
         nullish); got:\n{rust}"
    );
    check_generated_crate("nullish_coalescing", &rust);

    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate("nullish_coalescing", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn logical_or_and_and_yield_their_operands() {
    const SOURCE: &str = "export function orNum(a: number, b: number): number { return a || b; } \
         export function andNum(a: number, b: number): number { return a && b; } \
         export function orStr(a: string, b: string): string { return a || b; } \
         export function orBool(a: boolean, b: boolean): boolean { return a || b; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host, JsString};

struct NoHost;
impl Host for NoHost {}

fn text(v: JsString<'_>) -> String { v.to_string_lossy() }

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    // `||` yields the first truthy operand, `&&` the first falsy one.
    assert_eq!(logical_operands::orNum(&ctx, 0.0, 7.0), 7.0);
    assert_eq!(logical_operands::orNum(&ctx, 3.0, 7.0), 3.0);
    assert_eq!(logical_operands::andNum(&ctx, 0.0, 7.0), 0.0);
    assert_eq!(logical_operands::andNum(&ctx, 3.0, 7.0), 7.0);
    assert_eq!(text(logical_operands::orStr(&ctx, JsString::from(""), JsString::from("d"))), "d");
    assert_eq!(text(logical_operands::orStr(&ctx, JsString::from("a"), JsString::from("d"))), "a");
    assert!(logical_operands::orBool(&ctx, false, true));
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    check_generated_crate("logical_operands", &rust);

    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate("logical_operands", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_nullable_fallback_keeps_the_result_optional() {
    const SOURCE: &str = "export function pick(a: number | null, b: number | null): number | null { \
             return a ?? b; \
         }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    let some = |v: f64| ::core::option::Option::Some(v);
    assert_eq!(nullable_fallback::pick(&ctx, some(1.0), some(2.0)), some(1.0));
    assert_eq!(nullable_fallback::pick(&ctx, ::core::option::Option::None, some(2.0)), some(2.0));
    assert_eq!(nullable_fallback::pick(&ctx, ::core::option::Option::None, ::core::option::Option::None), ::core::option::Option::None);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    check_generated_crate("nullable_fallback", &rust);

    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate("nullable_fallback", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

/// The fallback stays inside its own branch, so it is not evaluated when the
/// left-hand side holds a value (the call must sit after the `else`).
#[test]
fn the_fallback_of_coalescing_stays_inside_its_branch() {
    const SOURCE: &str = "function boom(): number { return 9; } \
         export function present(a: number | null): number { return a ?? boom(); }";
    let rust = rust_of(SOURCE);
    let norm: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
    let else_pos = norm
        .find("}else{")
        .unwrap_or_else(|| panic!("`??` must lower to an `if`/`else`; got:\n{rust}"));
    assert!(
        norm[else_pos..].contains("boom(__ctx"),
        "the fallback of `??` must be emitted inside the nullish branch (after the `else`), but \
         it does not appear there; got:\n{rust}"
    );
    check_generated_crate("lazy_fallback", &rust);
}
#[test]
fn optional_chain_method_calls_map_over_the_receiver() {
    const SOURCE: &str = "class Box { value: number; constructor(v: number) { this.value = v; } \
             get(): number { return this.value; } } \
         export function read(b: Box | null): number { return b?.get() ?? -1; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    let boxed = optional_method_call::Box { value: 4.0 };
    assert_eq!(optional_method_call::read(&ctx, ::core::option::Option::Some(boxed)), 4.0);
    assert_eq!(optional_method_call::read(&ctx, ::core::option::Option::None), -1.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let norm: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        norm.contains(".as_ref().map("),
        "a method call through `?.` must map over the optional receiver; got:\n{rust}"
    );
    check_generated_crate("optional_method_call", &rust);

    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate("optional_method_call", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_method_call_on_a_nullable_receiver_without_the_question_dot_is_rejected() {
    let out = compile_module(
        "class Box { get(): number { return 1; } } \
         export function read(b: Box | null): number { return b.get(); }",
    );
    assert!(out.has_errors(), "got {:?}", out.diagnostics);
    assert!(
        out.diagnostics
            .iter()
            .any(|d| d.code.as_str() == "P0018" && d.message.contains("receiver")),
        "a `Box | null` receiver cannot be used without `?.`; got {:?}",
        out.diagnostics
    );
}

#[test]
fn constructor_arguments_run_the_constructor_body() {
    const SOURCE: &str = "class Point { x: number; y: number; \
             constructor(x: number, y: number) { this.x = x; this.y = y; } \
             sum(): number { return this.x + this.y; } } \
         export function make(): number { const p = new Point(2, 3); return p.sum(); }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(constructor_args::make(&ctx), 5.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    check_generated_crate("constructor_args", &rust);

    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate("constructor_args", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_constructor_without_parameters_still_runs() {
    const SOURCE: &str = "class Counter { value: number; \
             constructor() { this.value = 7; } \
             get(): number { return this.value; } } \
         export function make(): number { return new Counter().get(); }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(constructor_no_args::make(&ctx), 7.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    check_generated_crate("constructor_no_args", &rust);

    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate("constructor_no_args", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn arguments_for_a_class_without_a_constructor_are_rejected() {
    let out = compile_module(
        "class Empty { x: number = 0; } export function make(): number { return new Empty(5).x; }",
    );
    assert!(out.has_errors(), "got {:?}", out.diagnostics);
    assert!(
        out.diagnostics
            .iter()
            .any(|d| d.code.as_str() == "P0004" && d.message.contains("constructor")),
        "a class without a constructor takes no arguments; got {:?}",
        out.diagnostics
    );
}

/// The three operators combined, with the values JavaScript yields for each.
#[test]
fn nullish_operators_combine_like_javascript() {
    const SOURCE: &str = "export function f(a: number, b: number, c: string, d: string): number { \
             const x = a ?? b; const y = a || b; const z = a && b; \
             const s = c ?? d; const t = c || d; return x + y + z + s.length + t.length; \
         }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host, JsString};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    // x = 0 ?? 1 = 0; y = 0 || 1 = 1; z = 0 && 1 = 0; s = "" ?? "x" = "" (length 0);
    // t = "" || "x" = "x" (length 1).
    let total = nullish_operators_combined::f(&ctx, 0.0, 1.0, JsString::from(""), JsString::from("x"));
    assert_eq!(total, 2.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let norm: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        !norm.contains("||"),
        "no value-producing operator may be lowered to Rust's `bool`-only `||`; got:\n{rust}"
    );
    check_generated_crate("nullish_operators_combined", &rust);

    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate("nullish_operators_combined", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}
