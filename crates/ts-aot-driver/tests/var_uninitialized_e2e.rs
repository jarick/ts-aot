use ts_aot_driver::{CompileOptions, Driver, EmitStage};
use ts_aot_test_support::run_generated_crate;

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

fn normalized(rust: &str) -> String {
    rust.chars().filter(|c| !c.is_whitespace()).collect()
}

fn codes(source: &str) -> Vec<String> {
    compile_module(source)
        .diagnostics
        .iter()
        .map(|d| d.code.as_str().to_owned())
        .collect()
}

const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(uninit_var::fromUndefined(&ctx), 1.0);
    assert_eq!(uninit_var::fromAssigned(&ctx), 6.0);
    assert_eq!(uninit_var::fromInitializer(&ctx), 6.0);
    assert_eq!(uninit_var::fromLetInitializer(&ctx), 6.0);
    println!("ok");
}
"#;

#[test]
fn an_uninitialized_var_is_undefined_until_it_is_assigned() {
    const SOURCE: &str = "export function fromUndefined(): number { \
             var v: number; \
             if (v === undefined) { return 1; } \
             return 2; \
         } \
         export function fromAssigned(): number { \
             var v: number; v = 5; return v + 1; \
         } \
         export function fromInitializer(): number { \
             var v: number = 5; return v + 1; \
         } \
         export function fromLetInitializer(): number { \
             let v: number = 5; return v + 1; \
         }";
    let rust = rust_of(SOURCE);
    let norm = normalized(&rust);
    assert!(
        norm.contains("letv:Option<f64>=::core::option::Option::None;"),
        "a `var` without an initializer must be an `Option<T>` initialized with `None`, so \
         reading it yields JavaScript `undefined` instead of a silent `0`; got:\n{rust}"
    );
    assert!(
        norm.contains("letmutv:Option<f64>=::core::option::Option::None;"),
        "an uninitialized `var` that is later assigned must still be mutable; got:\n{rust}"
    );
    assert!(
        norm.contains("v=::core::option::Option::Some("),
        "assigning a value to an uninitialized `var` must wrap it in `Some`; got:\n{rust}"
    );
    assert!(
        norm.contains("letv:f64=5.0;"),
        "a `var`/`let` with an initializer must keep its plain non-optional type; got:\n{rust}"
    );
    let stdout = run_generated_crate("uninit_var", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

const TRUTHY_MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(truthy_var::fromUndefined(&ctx), 2.0);
    assert_eq!(truthy_var::fromZero(&ctx), 2.0);
    assert_eq!(truthy_var::fromFive(&ctx), 1.0);
    assert_eq!(truthy_var::fromTernary(&ctx), 2.0);
    println!("ok");
}
"#;

#[test]
fn an_uninitialized_var_uses_javascript_truthiness() {
    const SOURCE: &str = "export function fromUndefined(): number { \
             var v: number; if (v) { return 1; } return 2; \
         } \
         export function fromZero(): number { \
             var v: number; v = 0; if (v) { return 1; } return 2; \
         } \
         export function fromFive(): number { \
             var v: number; v = 5; if (v) { return 1; } return 2; \
         } \
         export function fromTernary(): number { \
             var v: number; return v ? 1 : 2; \
         }";
    let rust = rust_of(SOURCE);
    let norm = normalized(&rust);
    assert!(
        norm.contains("is_some()"),
        "`if (v)` on an optional must test presence before the inner truthiness; got:\n{rust}"
    );
    let stdout = run_generated_crate("truthy_var", &rust, TRUTHY_MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

const UNDEFINED_MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(undefined_var::loose(&ctx), 1.0);
    assert_eq!(undefined_var::typed(&ctx), 1.0);
    assert_eq!(undefined_var::ternary(&ctx), 1.0);
    assert_eq!(undefined_var::stringVar(&ctx), 1.0);
    assert_eq!(undefined_var::booleanVar(&ctx), 1.0);
    assert_eq!(undefined_var::typeofString(&ctx), 1.0);
    println!("ok");
}
"#;

#[test]
fn an_uninitialized_var_is_undefined_for_equality_and_typeof() {
    const SOURCE: &str = "export function loose(): number { \
             var v: number; if (v == undefined) { return 1; } return 2; \
         } \
         export function typed(): number { \
             var v: number; if (typeof v === \"undefined\") { return 1; } return 2; \
         } \
         export function ternary(): number { \
             var v: number; return v === undefined ? 1 : 0; \
         } \
         export function stringVar(): number { \
             var s: string; if (s === undefined) { return 1; } return 2; \
         } \
         export function booleanVar(): number { \
             var b: boolean; if (b === undefined) { return 1; } return 2; \
         } \
         export function typeofString(): number { \
             var s: string; if (typeof s === \"undefined\") { return 1; } return 2; \
         }";
    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate("undefined_var", &rust, UNDEFINED_MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn an_uninitialized_var_in_arithmetic_is_refused() {
    let diagnostics = codes("export function got(): number { var v: number; return v + 1; }");
    assert!(
        diagnostics.iter().any(|c| c == "P0018"),
        "arithmetic on an uninitialized `var` (an `Option<T>`) must be refused with P0018 \
         instead of emitting `Option<f64> + f64`; got {diagnostics:?}"
    );
}

#[test]
fn an_uninitialized_var_in_a_relational_comparison_is_refused() {
    let diagnostics =
        codes("export function got(): number { var v: number; return v < 2 ? 1 : 0; }");
    assert!(
        diagnostics.iter().any(|c| c == "P0018"),
        "a relational comparison against an uninitialized `var` must be refused with P0018; \
         got {diagnostics:?}"
    );
}

#[test]
fn an_uninitialized_var_with_a_nullable_annotation_refuses_a_strict_undefined_test() {
    let diagnostics = codes(
        "export function got(): number { \
             var v: number | null; if (v === undefined) { return 1; } return 2; \
         }",
    );
    assert!(
        diagnostics.iter().any(|c| c == "P0020"),
        "`var v: number | null` is `number | null | undefined`, and `null` and `undefined` share \
         one `Option::None`, so `v === undefined` cannot be decided and must be refused; \
         got {diagnostics:?}"
    );
}

const PARAM_MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(param_truthy::zero(&ctx), 2.0);
    assert_eq!(param_truthy::five(&ctx), 1.0);
    assert_eq!(param_truthy::ternaryNone(&ctx), 2.0);
    println!("ok");
}
"#;

#[test]
fn an_optional_parameter_in_a_condition_uses_javascript_truthiness() {
    const SOURCE: &str = "function f(v: number | undefined): number { \
             if (v) { return 1; } return 2; \
         } \
         export function zero(): number { return f(0); } \
         export function five(): number { return f(5); } \
         function ternary(v: number | undefined): number { return v ? 1 : 2; } \
         export function ternaryNone(): number { return ternary(undefined); }";
    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate("param_truthy", &rust, PARAM_MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}
