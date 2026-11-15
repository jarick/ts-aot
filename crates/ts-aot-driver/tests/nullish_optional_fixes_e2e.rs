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

fn codes(source: &str) -> Vec<String> {
    compile_module(source)
        .diagnostics
        .iter()
        .map(|d| d.code.as_str().to_owned())
        .collect()
}

#[test]
fn an_optional_and_applies_the_common_numeric_type() {
    const SOURCE: &str =
        "export function andFrac(a: i64 | null): number | null { return a && 1.5; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    use ::core::option::Option::{None, Some};
    // `4 && 1.5` is 1.5 (not the truncated 1), `0 && 1.5` is 0, `null && 1.5`
    // is null.
    assert_eq!(optional_and_common_ty::andFrac(&ctx, Some(4_i64)), Some(1.5));
    assert_eq!(optional_and_common_ty::andFrac(&ctx, Some(0_i64)), Some(0.0));
    assert_eq!(optional_and_common_ty::andFrac(&ctx, None), None);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let norm = normalized(&rust);
    assert!(
        !norm.contains(")asi64"),
        "the fractional right operand must not be truncated into the left operand's integer type; \
         got:\n{rust}"
    );
    assert!(
        norm.contains("Option<f64>"),
        "the optional `&&` result must be the common `f64` type; got:\n{rust}"
    );
    assert!(
        norm.contains("asf64"),
        "the integer left operand must be re-wrapped in the common `f64` type; got:\n{rust}"
    );
    check_generated_crate("optional_and_common_ty", &rust);
    let stdout = run_generated_crate("optional_and_common_ty", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_nullish_fallback_bound_to_a_const_is_not_reported() {
    const PROBE: &str = "export function c1(a: number | undefined): number | null { \
             const r = a ?? null; return r; }";
    const CONTROLS: &str = "export function c2(a: number | undefined): number | null { \
             const r: number | null = a ?? null; return r; } \
         export function c3(a: number | undefined, b: number | undefined): number | undefined { \
             const r = a ?? b; return r; }";
    const ANNOTATED: &str = "export function c2(a: number | undefined): number | null { \
             const r: number | null = a ?? null; return r; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    use ::core::option::Option::{None, Some};
    // `a ?? null` is `a`: a present value survives, `undefined` becomes null.
    assert_eq!(coalesce_binding::c2(&ctx, Some(3.0)), Some(3.0));
    assert_eq!(coalesce_binding::c2(&ctx, None), None);
    println!("ok");
}
"#;
    assert!(
        codes(PROBE).is_empty(),
        "`const r = a ?? null` is valid TypeScript and must not be diagnosed; got {:?}",
        codes(PROBE)
    );
    assert!(
        codes(CONTROLS).is_empty(),
        "an explicitly optional binding and a typed optional fallback must stay accepted; got {:?}",
        codes(CONTROLS)
    );
    let probe_rust = rust_of(PROBE);
    assert!(
        normalized(&probe_rust).contains("Option<f64>"),
        "the `??` result stays optional; got:\n{probe_rust}"
    );
    let rust = rust_of(ANNOTATED);
    check_generated_crate("coalesce_binding", &rust);
    let stdout = run_generated_crate("coalesce_binding", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn an_optional_coalescing_result_in_a_non_optional_position_is_still_reported() {
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

#[test]
fn a_constructor_argument_is_checked_with_the_abi_argument_type() {
    const SOURCE: &str = "class C { v: number | null = 0; constructor(x: number | null = 1) { \
             this.v = x; } } \
         export function mkNull(): number | null { const c = new C(null); return c.v; } \
         export function mkFive(): number | null { const c = new C(5); return c.v; } \
         export function mkZero(): number | null { const c = new C(0); return c.v; } \
         export function mkUndefined(): number | null { const c = new C(undefined); return c.v; } \
         export function mkOmitted(): number | null { const c = new C(); return c.v; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    use ::core::option::Option::{None, Some};
    // `null` is not `undefined`, so it is stored; `undefined` selects the
    // default, exactly like an omitted argument.
    assert_eq!(ctor_argument::mkNull(&ctx), None, "`new C(null)` stores null");
    assert_eq!(ctor_argument::mkFive(&ctx), Some(5.0));
    assert_eq!(ctor_argument::mkZero(&ctx), Some(0.0));
    assert_eq!(
        ctor_argument::mkUndefined(&ctx),
        Some(1.0),
        "`new C(undefined)` selects the default"
    );
    assert_eq!(ctor_argument::mkOmitted(&ctx), Some(1.0), "the default applies");
    println!("ok");
}
"#;
    assert!(
        codes(SOURCE).is_empty(),
        "`new C(null)` for `x: number | null = 1` is valid TypeScript and must not be diagnosed; \
         got {:?}",
        codes(SOURCE)
    );
    let rust = rust_of(SOURCE);
    check_generated_crate("ctor_argument", &rust);
    let stdout = run_generated_crate("ctor_argument", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}
