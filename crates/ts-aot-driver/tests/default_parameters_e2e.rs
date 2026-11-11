//! Parameter default values (`x: T = <default>`).
//!
//! JavaScript evaluates a default at *call* time, inside the callee, only when
//! the argument is missing (or explicitly `undefined`), and in parameter order
//! so a default can use the parameters before it. The AOT target represents a
//! defaulted parameter as an `Option<T>` ABI parameter plus a body prologue
//! `let x: T = param ?? <default>;`, so the generated crates are compiled and
//! their runtime results compared with JavaScript.

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
fn omitted_argument_uses_the_default_and_a_given_one_wins() {
    const SOURCE: &str = "export function pick(a: number = 7, b: number = 2): number { \
             return a * 10 + b; \
         }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(default_values::pick(&ctx, ::core::option::Option::None, ::core::option::Option::None), 72.0);
    assert_eq!(default_values::pick(&ctx, ::core::option::Option::Some(1.0), ::core::option::Option::None), 12.0);
    assert_eq!(default_values::pick(&ctx, ::core::option::Option::None, ::core::option::Option::Some(3.0)), 73.0);
    assert_eq!(default_values::pick(&ctx, ::core::option::Option::Some(1.0), ::core::option::Option::Some(3.0)), 13.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let norm: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        norm.contains("Option<f64>"),
        "a defaulted parameter is optional in the ABI; got:\n{rust}"
    );
    check_generated_crate("default_values", &rust);
    let stdout = run_generated_crate("default_values", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

/// The defaults are applied by the callee, so a caller that omits the argument
/// passes `undefined`/`None` and never evaluates the default itself.
#[test]
fn defaults_are_applied_inside_the_callee_in_parameter_order() {
    const SOURCE: &str = "export function chain(a: number = 1, b: number = a + 1, c: number = a + b): number { \
             return c; \
         } \
         export function call_chain(a: number): number { return chain(a); }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    // `chain()` = c = 1 + 2 = 3; `chain(10)` = 10 + 11 = 21.
    assert_eq!(default_chain::chain(&ctx, ::core::option::Option::None, ::core::option::Option::None, ::core::option::Option::None), 3.0);
    assert_eq!(default_chain::call_chain(&ctx, 10.0), 21.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    check_generated_crate("default_chain", &rust);

    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate("default_chain", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

/// A default runs only when the argument is missing: the fallback bumps a
/// captured global, so the result shows whether it ran.
#[test]
fn a_default_is_not_evaluated_when_the_argument_is_present() {
    const SOURCE: &str = "let hits: number = 0; \
         function bump(): number { hits = hits + 1; return 9; } \
         export function f(x: number = bump()): number { return x + hits; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    // The argument is present, so the default must not run: 5 + 0.
    assert_eq!(default_lazy::f(&ctx, ::core::option::Option::Some(5.0)), 5.0);
    // Missing argument: the default runs once: 9 + 1.
    assert_eq!(default_lazy::f(&ctx, ::core::option::Option::None), 10.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    check_generated_crate("default_lazy", &rust);

    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate("default_lazy", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}
/// The default expression is evaluated once per call, not once per program.
#[test]
fn a_default_is_evaluated_per_call() {
    const SOURCE: &str = "let hits: number = 0; \
         function bump(): number { hits = hits + 1; return hits; } \
         export function f(x: number = bump()): number { return x; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(default_per_call::f(&ctx, ::core::option::Option::None), 1.0);
    assert_eq!(default_per_call::f(&ctx, ::core::option::Option::None), 2.0);
    assert_eq!(default_per_call::f(&ctx, ::core::option::Option::Some(9.0)), 9.0);
    assert_eq!(default_per_call::f(&ctx, ::core::option::Option::None), 3.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    check_generated_crate("default_per_call", &rust);

    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate("default_per_call", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

/// Methods, constructors and arrow functions take defaults as well.
#[test]
fn methods_constructors_and_arrows_accept_defaults() {
    const SOURCE: &str = "class Box { value: number; \
             constructor(v: number = 5) { this.value = v; } \
             scaled(factor: number = 2): number { return this.value * factor; } } \
         export function run(): number { \
             const b = new Box(); \
             const d = new Box(10); \
             const twice = (x: number = 3): number => x * 2; \
             return b.scaled() + d.scaled(3) + twice(); \
         }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    // 5 * 2 + 10 * 3 + 3 * 2 = 10 + 30 + 6.
    assert_eq!(default_members::run(&ctx), 46.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    check_generated_crate("default_members", &rust);

    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate("default_members", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

/// Omitting an argument is only allowed for a parameter that has a default.
#[test]
fn omitting_a_parameter_without_a_default_is_still_rejected() {
    let out = compile_module(
        "function need(a: number, b: number = 2): number { return a + b; } \
         export function run(): number { return need(); }",
    );
    assert!(out.has_errors(), "got {:?}", out.diagnostics);
    let message = out
        .diagnostics
        .iter()
        .find(|d| d.code.as_str() == "P0004")
        .map_or_else(
            || panic!("expected P0004, got {:?}", out.diagnostics),
            |d| d.message.clone(),
        );
    assert!(
        message.contains("omitting a required argument"),
        "the diagnostic must explain the missing argument; got: {message}"
    );
    assert!(
        !message.contains("P0017"),
        "defaults are supported now, so the message must not mention P0017; got: {message}"
    );
}

/// A parameter cannot be both `?`-optional and have an initializer.
#[test]
fn question_mark_with_an_initializer_is_rejected() {
    let out = compile_module("export function f(x?: number = 1): number { return x; }");
    assert!(out.has_errors(), "got {:?}", out.diagnostics);
    assert!(
        out.diagnostics
            .iter()
            .any(|d| d.code.as_str() == "E0200" && d.message.contains("question mark")),
        "expected a parse error for `x?: T = <default>`; got {:?}",
        out.diagnostics
    );
}
/// A JavaScript default applies only to `undefined`: a supplied `null` is a
/// value and must survive. The hidden ABI parameter therefore nests the
/// `Option` (`Option<Option<_>>` for a nullable `T`) instead of flattening
/// `null` into "no argument was passed".
#[test]
fn a_supplied_null_is_not_replaced_by_the_default() {
    const SOURCE: &str = "export function num(x: number | null = 5): number | null { return x; } \
         export function nil(x: number | null = null): number | null { return x; } \
         export function str(x: string | null = \"d\"): string | null { return x; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    use ::core::option::Option as O;
    // A missing argument takes the default.
    assert_eq!(null_defaults::num(&ctx, O::None), O::Some(5.0));
    // A supplied `null` stays `null`; this is the behaviour the default must not
    // overwrite (only `undefined` selects the default in JavaScript).
    assert_eq!(null_defaults::num(&ctx, O::Some(O::None)), O::None);
    assert_eq!(null_defaults::num(&ctx, O::Some(O::Some(3.0))), O::Some(3.0));
    assert_eq!(null_defaults::nil(&ctx, O::None), O::None);
    assert_eq!(null_defaults::nil(&ctx, O::Some(O::Some(3.0))), O::Some(3.0));
    assert!(null_defaults::str(&ctx, O::None).is_some());
    assert!(null_defaults::str(&ctx, O::Some(O::None)).is_none());
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let norm: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        norm.contains("Option<Option<f64>>"),
        "a nullable defaulted parameter nests the ABI Option so `null` stays distinct from a \
         missing argument; got:\n{rust}"
    );
    check_generated_crate("null_defaults", &rust);
    let stdout = run_generated_crate("null_defaults", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

/// A default on a *destructured* parameter has no single ABI slot that could
/// carry the "was the argument supplied?" marker, so it cannot be applied. It
/// used to be dropped silently; it is now reported, and the argument stays
/// required (the parameter is not wrapped in `Option`).
#[test]
fn a_default_on_a_destructured_parameter_is_reported_and_stays_required() {
    let out = compile_module("export function f([a]: number[] = [1]): number { return a; }");
    assert!(
        !out.has_errors(),
        "the ignored default is a warning, not an error; got {:?}",
        out.diagnostics
    );
    assert!(
        out.diagnostics.iter().any(|d| d.code.as_str() == "E0500"
            && d.severity == ts_aot_driver::Severity::Warning
            && d.message.contains("destructured parameter")),
        "the ignored default must be reported; got {:?}",
        out.diagnostics
    );
    let rust = out
        .rust_source
        .expect("emit-rust must populate rust_source");
    let norm: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        norm.contains("__tmp0:ts_aot_runtime::bumpalo::collections::Vec<'a,f64>")
            && !norm.contains("Option<ts_aot_runtime::bumpalo::collections::Vec<'a,f64>>"),
        "a destructured parameter has no `Option` ABI slot, so the argument stays required; \
         got:\n{rust}"
    );
    check_generated_crate("destructured_default", &rust);
}
