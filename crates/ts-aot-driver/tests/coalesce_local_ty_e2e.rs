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

#[test]
fn an_optional_coalescing_result_in_a_non_optional_return_is_still_reported() {
    const EXPECTED: &str = "an optional value (it may be `null`) is used where a non-optional \
         value is required (the declared return type of this function); the AOT target represents \
         optional values as `Option<T>` and has no implicit nullish-to-value unwrapping. Handle \
         the nullish case first (for example with `x!` after a check, or by widening the target \
         type)";
    let out =
        compile_module("export function f(a: number | undefined): number { return a ?? null; }");
    assert!(out.has_errors(), "got {:?}", out.diagnostics);
    assert!(
        out.diagnostics
            .iter()
            .any(|d| d.code.as_str() == "P0018" && d.message == EXPECTED),
        "a nullish `??` result returned as `number` is an optional value written where a \
         non-optional one is required; the repaired `??` type makes the write check report it by \
         its site; got {:?}",
        out.diagnostics
    );
    assert!(
        out.rust_source.is_none(),
        "a rejected program must not emit Rust; got {:?}",
        out.rust_source
    );
}
