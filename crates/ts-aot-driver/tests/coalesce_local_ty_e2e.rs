//! The type of a local bound to a `??` with a nullish literal.
//!
//! `const r = a ?? null` is valid TypeScript, but the frontend types the
//! operator - and therefore an unannotated binding - as the left operand's
//! non-optional inner type (`number` for `a: number | undefined`), because a
//! `null` literal carries no type of its own. The lowering is nonetheless
//! optional (`a` may be missing and the fallback really can be yielded), so
//! `hir_to_mir::convert_stmt::recover_local_ty` has to let the converted
//! initializer win over that stale non-optional declaration. Otherwise the
//! generated crate declares `let r: f64 = <Option<f64>>` and `cargo check`
//! rejects it with E0308.
//!
//! The controls pin what must not change: an explicitly optional annotation, an
//! initializer whose value is a plain value, and the `P0018` report for a `??`
//! whose optional result is returned from a function declared to return a plain
//! value.

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

fn normalized(rust: &str) -> String {
    rust.chars().filter(|c| !c.is_whitespace()).collect()
}

/// The generated Rust of one exported function, so an assertion can be scoped to
/// it (the module also carries the dispatch helpers and the ABI wrappers).
fn body_of<'r>(rust: &'r str, name: &str) -> &'r str {
    let start = rust
        .find(&format!("pub fn {name} "))
        .unwrap_or_else(|| panic!("the generated crate must contain `{name}`; got:\n{rust}"));
    let tail = &rust[start..];
    let end = tail[1..].find("pub fn ").map_or(tail.len(), |i| i + 1);
    &tail[..end]
}

fn codes(source: &str) -> Vec<String> {
    compile_module(source)
        .diagnostics
        .iter()
        .map(|d| d.code.as_str().to_owned())
        .collect()
}

/// The probe: the binding's stale `f64` declaration must give way to the
/// optional type the initializer actually converted to, or the generated crate
/// does not compile. Runtime values are compared with JavaScript (`undefined`
/// becomes `null`, a present value survives).
#[test]
fn an_unannotated_nullish_coalesce_binding_takes_the_converted_optional_type() {
    const SOURCE: &str = "export function run(a: number | undefined): number | null { \
             const r = a ?? null; return r; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    use ::core::option::Option::{None, Some};
    // `a ?? null` is `a`: `undefined` becomes `null`, a present value survives.
    assert_eq!(coalesce_local_ty::run(&ctx, None), None);
    assert_eq!(coalesce_local_ty::run(&ctx, Some(5.0)), Some(5.0));
    println!("ok");
}
"#;
    assert!(
        codes(SOURCE).is_empty(),
        "`const r = a ?? null` is valid TypeScript; got {:?}",
        codes(SOURCE)
    );
    let rust = rust_of(SOURCE);
    let run = normalized(body_of(&rust, "run"));
    assert!(
        run.contains("letr:Option<f64>="),
        "the binding must be declared with the initializer's `Option<f64>` type, not the \
         frontend's stale `f64`; got:\n{}",
        body_of(&rust, "run")
    );
    assert!(
        !run.contains("letr:f64="),
        "`let r: f64 = <Option<f64>>` is exactly the E0308 the fix removes; got:\n{}",
        body_of(&rust, "run")
    );
    check_generated_crate("coalesce_local_ty", &rust);
    let stdout = run_generated_crate("coalesce_local_ty", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

/// The controls: an explicitly optional annotation and an inferred non-optional
/// value keep their declarations, and a `??` over a typed optional fallback
/// (`a ?? b`) is unaffected too.
#[test]
fn nullish_coalesce_binding_controls_keep_their_declared_types() {
    const SOURCE: &str = "export function annotated(a: number | undefined): number | null { \
             const r: number | null = a ?? null; return r; } \
         export function inferredOptional(a: number | undefined, b: number | undefined): \
             number | undefined { const r = a ?? b; return r; } \
         export function plainValue(): number { const x: number = 5; return x; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    use ::core::option::Option::{None, Some};
    assert_eq!(coalesce_local_controls::annotated(&ctx, None), None);
    assert_eq!(coalesce_local_controls::annotated(&ctx, Some(3.0)), Some(3.0));
    assert_eq!(coalesce_local_controls::inferredOptional(&ctx, None, None), None);
    assert_eq!(
        coalesce_local_controls::inferredOptional(&ctx, Some(2.0), Some(9.0)),
        Some(2.0)
    );
    assert_eq!(coalesce_local_controls::inferredOptional(&ctx, None, Some(9.0)), Some(9.0));
    assert_eq!(coalesce_local_controls::plainValue(&ctx), 5.0);
    println!("ok");
}
"#;
    assert!(
        codes(SOURCE).is_empty(),
        "the controls are valid TypeScript; got {:?}",
        codes(SOURCE)
    );
    let rust = rust_of(SOURCE);
    assert!(
        normalized(body_of(&rust, "annotated")).contains("letr:Option<f64>"),
        "an explicitly optional annotation stays optional; got:\n{}",
        body_of(&rust, "annotated")
    );
    assert!(
        normalized(body_of(&rust, "inferredOptional")).contains("letr:Option<f64>"),
        "`a ?? b` over two optionals stays optional; got:\n{}",
        body_of(&rust, "inferredOptional")
    );
    assert!(
        normalized(body_of(&rust, "plainValue")).contains("letx:f64=5.0;"),
        "`const x: number = 5` must keep its plain `f64` declaration; got:\n{}",
        body_of(&rust, "plainValue")
    );
    check_generated_crate("coalesce_local_controls", &rust);
    let stdout = run_generated_crate("coalesce_local_controls", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

/// The other side of the fix: a local declaration is not a requirement of its
/// own, but a declared return type is. An optional `??` result returned from a
/// function declared to return a plain value keeps being reported.
#[test]
fn an_optional_coalescing_result_in_a_non_optional_return_is_still_reported() {
    let out =
        compile_module("export function f(a: number | undefined): number { return a ?? null; }");
    assert!(out.has_errors(), "got {:?}", out.diagnostics);
    assert!(
        out.diagnostics.iter().any(|d| d.code.as_str() == "P0018"
            && d.message.contains("result of `??`")
            && d.message.contains("number | null")),
        "a nullish `??` result returned as `number` must be a P0018 naming the operator; got {:?}",
        out.diagnostics
    );
}
