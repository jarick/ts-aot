use ts_aot_driver::{CompileOptions, Driver, EmitStage};
use ts_aot_test_support::{check_generated_crate, run_generated_crate};

fn rust_of(source: &str) -> String {
    let out = Driver::new().compile_source(
        "test.ts",
        source,
        &CompileOptions {
            emit: EmitStage::Rust,
            module: true,
        },
    );
    assert!(
        !out.has_errors(),
        "must compile through the full pipeline; got {:?}",
        out.diagnostics
    );
    out.rust_source
        .expect("emit-rust must populate rust_source")
}

#[test]
fn fn_value_of_defaulted_nullable_declaration_uses_flat_fn_type() {
    const SOURCE: &str = "\
        export function withDefault(x: number | null = 7): number | null { return x; } \
        export function viaNull(): number | null { \
            const f: (x: number | null) => number | null = withDefault; return f(null); \
        } \
        export function viaValue(): number | null { \
            const f: (x: number | null) => number | null = withDefault; return f(5); \
        } \
        export function directNull(): number | null { return withDefault(null); } \
        export function directValue(): number | null { return withDefault(5); } \
        export function directOmitted(): number | null { return withDefault(); }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    // f(null) is a *supplied* argument, so the default must not apply: JS null.
    assert_eq!(fn_value_nested_optional::viaNull(&ctx), None);
    // f(5) -> JS 5
    assert_eq!(fn_value_nested_optional::viaValue(&ctx), Some(5.0));
    // the direct calls keep their behaviour
    assert_eq!(fn_value_nested_optional::directNull(&ctx), None);
    assert_eq!(fn_value_nested_optional::directValue(&ctx), Some(5.0));
    // withDefault() -> JS 7 (default)
    assert_eq!(fn_value_nested_optional::directOmitted(&ctx), Some(7.0));
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let norm: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        !norm.contains("__Uninhabited"),
        "no function-value bucket may be left empty; got:\n{rust}"
    );
    check_generated_crate("fn_value_nested_optional", &rust);
    let stdout = run_generated_crate("fn_value_nested_optional", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn fn_value_of_defaulted_nullable_declaration_uses_nested_fn_type() {
    const SOURCE: &str = "\
        export function withDefault(x: number | null = 7): number | null { return x; } \
        export function run(): number | null { \
            const f: (x?: number | null) => number | null = withDefault; return f(null); \
        } \
        export function run2(): number | null { \
            const f: (x?: number | null) => number | null = withDefault; return f(5); \
        } \
        export function run3(): number | null { \
            const f: (x?: number | null) => number | null = withDefault; return f(); \
        }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(fn_value_nested_optional_nested::run(&ctx), None);
    assert_eq!(fn_value_nested_optional_nested::run2(&ctx), Some(5.0));
    // an omitted argument DOES ask for the default
    assert_eq!(fn_value_nested_optional_nested::run3(&ctx), Some(7.0));
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let norm: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        !norm.contains("__Uninhabited"),
        "no function-value bucket may be left empty; got:\n{rust}"
    );
    check_generated_crate("fn_value_nested_optional_nested", &rust);
    let stdout = run_generated_crate("fn_value_nested_optional_nested", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn plain_and_optional_buckets_still_work_alongside_nested_one() {
    const SOURCE: &str = "\
        export function withDefault(x: number = 7): number { return x; } \
        export function maybe(x?: number): number { return x ?? 0; } \
        export function strict(x: number | undefined): number { return x ?? 0; } \
        export function run(): number { \
            const b: (x?: number) => number = withDefault; \
            const c: (x?: number) => number = maybe; \
            const d: (x: number | undefined) => number = strict; \
            return b() + b(1) + c(2) + d(3) + c(); \
        }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    // b() + b(1) + c(2) + d(3) + c() == 7 + 1 + 2 + 3 + 0
    assert_eq!(fn_value_nested_optional_plain::run(&ctx), 13.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let norm: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        !norm.contains("__Uninhabited"),
        "no function-value bucket may be left empty; got:\n{rust}"
    );
    assert!(
        !norm.contains("__ts_fn_value_lift_"),
        "an exact ABI match must not go through an adapter; got:\n{rust}"
    );
    check_generated_crate("fn_value_nested_optional_plain", &rust);
    let stdout = run_generated_crate("fn_value_nested_optional_plain", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn only_the_widened_parameter_is_wrapped() {
    const SOURCE: &str = "\
        export function two(x: number | null = 7, s: string): number | null { return x; } \
        export function one(x: number = 7): number { return x; } \
        export function viaTwo(): number | null { \
            const f: (x: number | null, s: string) => number | null = two; return f(5, \"a\"); \
        } \
        export function viaTwoNull(): number | null { \
            const f: (x: number | null, s: string) => number | null = two; return f(null, \"a\"); \
        } \
        export function viaOne(): number { \
            const f: (x: number) => number = one; return f(5); \
        }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(fn_value_nested_optional_mixed::viaTwo(&ctx), Some(5.0));
    assert_eq!(fn_value_nested_optional_mixed::viaTwoNull(&ctx), None);
    assert_eq!(fn_value_nested_optional_mixed::viaOne(&ctx), 5.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let norm: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        !norm.contains("__Uninhabited"),
        "no function-value bucket may be left empty; got:\n{rust}"
    );
    assert!(
        norm.contains("Some(__a0),__a1"),
        "only the widened parameter may be wrapped; got:\n{rust}"
    );
    check_generated_crate("fn_value_nested_optional_mixed", &rust);
    let stdout = run_generated_crate("fn_value_nested_optional_mixed", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn an_omitted_bucket_argument_still_selects_the_default() {
    const SOURCE: &str = "\
        export function withDefault(x: number | null = 7): number | null { return x; } \
        export function supplied(): number | null { \
            const g: (x?: number) => number | null = withDefault; return g(3); \
        } \
        export function omitted(): number | null { \
            const g: (x?: number) => number | null = withDefault; return g(); \
        }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(fn_value_nested_optional_omitted::supplied(&ctx), Some(3.0));
    // no argument at all -> the default, *not* JS null
    assert_eq!(fn_value_nested_optional_omitted::omitted(&ctx), Some(7.0));
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let norm: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        !norm.contains("__Uninhabited"),
        "no function-value bucket may be left empty; got:\n{rust}"
    );
    assert!(
        norm.contains(".map(::core::option::Option::Some)")
            || norm.contains(".map(Option::Some)")
            || norm.contains(".map(Some)"),
        "an omitted argument must not be wrapped into `Some`; got:\n{rust}"
    );
    check_generated_crate("fn_value_nested_optional_omitted", &rust);
    let stdout = run_generated_crate("fn_value_nested_optional_omitted", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_throwing_declaration_is_unwrapped_by_the_adapter() {
    const SOURCE: &str = "\
        export function withDefault(x: number | null = 7): number | null { \
            if (x === null) { throw 1; } return x; \
        } \
        export function run(): number | null { \
            const f: (x: number | null) => number | null = withDefault; return f(5); \
        }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    // the value path takes the adapter's `Ok` arm; the `Err` arm (which
    // re-raises the payload) only has to type-check
    assert_eq!(fn_value_nested_optional_throw::run(&ctx), Some(5.0));
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let norm: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        !norm.contains("__Uninhabited"),
        "no function-value bucket may be left empty; got:\n{rust}"
    );
    assert!(
        norm.contains("Ok(__v)=>__v,Err(__e)=>ts_aot_runtime::__ts_aot_throw(__e)"),
        "the adapter must unwrap the declaration's `Result`; got:\n{rust}"
    );
    check_generated_crate("fn_value_nested_optional_throw", &rust);
    let stdout = run_generated_crate("fn_value_nested_optional_throw", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_function_of_another_signature_is_reported_instead_of_emitting_a_bare_item() {
    const SOURCE: &str = "export function f(x: number): number { return x; } \
        export function use(): number { const h: (x: number | null) => number = f; return h(1); }";
    let out = Driver::new().compile_source(
        "test.ts",
        SOURCE,
        &CompileOptions {
            emit: EmitStage::Rust,
            module: true,
        },
    );
    assert!(
        out.rust_source.is_none(),
        "a function value that cannot fill the type must not emit Rust; got:\n{}",
        out.rust_source.unwrap_or_default()
    );
    assert!(
        out.diagnostics.iter().any(|d| d.code.as_str() == "E0300"
            && d.message
                .contains("cannot be stored in this function value")),
        "the unadapted function value must be reported; got {:?}",
        out.diagnostics
    );
}

#[test]
fn a_nested_parameter_cannot_join_a_bucket_that_may_omit_a_nullable() {
    const SOURCE: &str = "export function optionalNull(f: (x?: number | null) => number): number { return f(); } \
        export function g(x: number | null | undefined): number { return 1; } \
        export function use(): number { const h: (x?: number | null) => number = g; return optionalNull(g) + h(1); }";
    let out = Driver::new().compile_source(
        "test.ts",
        SOURCE,
        &CompileOptions {
            emit: EmitStage::Rust,
            module: true,
        },
    );
    assert!(
        out.rust_source.is_none(),
        "a refused registration must not emit Rust; got:\n{}",
        out.rust_source.unwrap_or_default()
    );
    assert!(
        out.diagnostics.iter().any(|d| d.code.as_str() == "E0300"
            && d.message
                .contains("cannot be stored in this function value")),
        "the unadapted function value must be reported; got {:?}",
        out.diagnostics
    );
}
