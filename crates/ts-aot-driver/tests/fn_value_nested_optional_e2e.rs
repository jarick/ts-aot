//! A defaulted *and* nullable parameter of a declaration cannot be named by a
//! plain function type, and that used to leave the function-value bucket empty.
//!
//! `withDefault(x: number | null = 7)` is emitted with the nested ABI parameter
//! `Option<Option<f64>>`: the outer `Option` answers "was an argument supplied?"
//! (the callee's prologue applies the default when it is `None`), the inner one
//! is the value's own nullishness. A function type can never spell a default, so
//! `(x: number | null) => number | null` has the flat parameter `Option<f64>`.
//! The registry used to bucket a declaration by exact `params` equality, so the
//! declaration matched no bucket at all: the enum stayed `__Uninhabited` and the
//! assignment `const f: (x: number | null) => number | null = withDefault` was an
//! E0308 (and would have been an `unreachable!()` panic at runtime).
//!
//! Bucketing a declaration now also accepts an ABI parameter that is one
//! `Option`-level *above* the bucket parameter ("this slot may be omitted"), and
//! the bucket calls a generated adapter that re-wraps the argument (`Some(..)`,
//! or `__a0.map(Some)` where the bucket itself may omit it) so the nested ABI
//! stays intact. A function type's *required* parameter is never omission-aware,
//! so `f(null)` must stay JS `null` (not the default 7) and `f(5)` must be `5`.

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

/// A defaulted nullable declaration used as a value of the flat function type
/// `(x: number | null) => number | null`.
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

/// The same declaration used through the *nesting* function type
/// `(x?: number | null) => number | null`, whose ABI parameter is already
/// `Option<Option<f64>>`: the exact match must win and must not be double
/// wrapped (`Some(Some(Some(..)))` would not type-check).
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

/// A non-nullable defaulted declaration and the `x?: number` /
/// `x: number | undefined` declarations keep working through their *exact*
/// buckets: their ABI parameter `Option<f64>` is what the bucket already says,
/// so the registry must still prefer that match and never route them through an
/// adapter.
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

/// The lifting is per position: a parameter that *is* named by the function type
/// unchanged (`s: string`) must be forwarded as-is, while the defaulted nullable
/// one is re-wrapped. `x: number = 7` (ABI `Option<f64>`, one level above the
/// flat `f64` the type says) is the same relation without the inner `Option`.
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
    // position 0 is re-wrapped into the "argument supplied" state, position 1 is
    // forwarded untouched
    assert!(
        norm.contains("Some(__a0),__a1"),
        "only the widened parameter may be wrapped; got:\n{rust}"
    );
    check_generated_crate("fn_value_nested_optional_mixed", &rust);
    let stdout = run_generated_crate("fn_value_nested_optional_mixed", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

/// A bucket that may be called *without* the argument has to keep "no argument"
/// distinguishable from a supplied value: the call site pads an omitted trailing
/// argument with `undefined`, which the flat ABI spells `None`, and `None` must
/// stay the *outer* `None` of the nested ABI so the callee's default applies.
/// Only a supplied value is wrapped.
///
/// The assignment itself is a TypeScript error (a required parameter cannot be
/// dropped by the target type), but the frontend does not check assignability
/// yet, so the emitted code still has to mean the JavaScript thing.
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

/// A throwing declaration returns `Result<ok, err>` while its bucket holds the
/// unwrapped `ok`, so the adapter has to own that shim too - otherwise the arm
/// would hand a `Result` to a helper that expects the value.
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

/// The adapter above covers a declaration whose parameter is one `Option` level
/// *wider* than the bucket's. The opposite direction has no adapter, and it is
/// reachable through parameter bivariance: `f(x: number)` can be stored in a
/// `(x: number | null) => number` value, whose bucket no declaration fits. The
/// bucket stayed `__Uninhabited` and the initialiser was emitted as a bare
/// function item where the enum is expected - invalid Rust - so it is reported
/// now.
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

/// The adapter described above cannot express "the caller passed `null`" and
/// "the caller omitted the argument" at the same time when the bucket parameter
/// is the flat `Option<T>`: both are `None`, so a passed `null` would select the
/// callee's default instead of staying `null`. `fn_value_bucket` therefore
/// refuses the registration (the declaration's parameter is one `Option` level
/// above a bucket parameter that may be omitted *and* admits `null`), which
/// leaves the value unfillable - and that is reported rather than adapted into a
/// call that miscompiles one of the two cases.
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
