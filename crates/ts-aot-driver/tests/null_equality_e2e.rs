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

const SOURCE: &str = "export function isNull(x: number | null): boolean { return x === null; } \
     export function isNotNull(x: number | null): boolean { return x !== null; } \
     export function reverse(s: string | null): boolean { return null === s; } \
     export function looseEq(x: number | null): boolean { return x == null; } \
     export function plainNull(x: number): boolean { return x === null; } \
     export function optParam(x?: number): boolean { return x === null; } \
     export function optParamUndef(x?: number): boolean { return x === undefined; }";

#[test]
fn nullable_value_compared_to_null_compiles() {
    let out = compile_module(SOURCE);
    assert!(
        !out.has_errors(),
        "comparing a `number | null` / `string | null` value to null must compile; got {:?}",
        out.diagnostics
    );
    let rust = out
        .rust_source
        .expect("emit-rust must populate rust_source for the null-equality check");
    let norm: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        norm.contains(".is_none()"),
        "`=== null` / `== null` must lower to `is_none()`; got:\n{rust}"
    );
    assert!(
        norm.contains(".is_some()"),
        "`!== null` must lower to `is_some()`; got:\n{rust}"
    );
    check_generated_crate("null_equality", &rust);
}

const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert!(null_eq::isNull(&ctx, ::core::option::Option::None));
    assert!(!null_eq::isNull(&ctx, ::core::option::Option::Some(1.0)));
    assert!(null_eq::isNotNull(&ctx, ::core::option::Option::Some(1.0)));
    assert!(!null_eq::isNotNull(&ctx, ::core::option::Option::None));
    assert!(null_eq::reverse(&ctx, ::core::option::Option::None));
    assert!(null_eq::looseEq(&ctx, ::core::option::Option::None));
    assert!(!null_eq::looseEq(&ctx, ::core::option::Option::Some(1.0)));
    assert!(!null_eq::plainNull(&ctx, 3.0));
    assert!(!null_eq::optParam(&ctx, ::core::option::Option::Some(3.0)));
    assert!(
        !null_eq::optParam(&ctx, ::core::option::Option::None),
        "`x?: number` is `number | undefined`, so `x === null` is always false in JS"
    );
    assert!(!null_eq::optParamUndef(&ctx, ::core::option::Option::Some(3.0)));
    assert!(null_eq::optParamUndef(&ctx, ::core::option::Option::None));
    println!("ok");
}
"#;

#[test]
fn nullable_comparison_reports_none_as_null_at_runtime() {
    let out = compile_module(SOURCE);
    assert!(!out.has_errors(), "got {:?}", out.diagnostics);
    let rust = out.rust_source.expect("rust_source");
    let stdout = run_generated_crate("null_eq", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

const STRICTNESS_SOURCE: &str = "export function strictVsLoose(): boolean { return void 0 === null; } \
     export function looseVsStrict(): boolean { return void 0 == null; } \
     export function nullNull(): boolean { return null === null; } \
     export function undefStrictNeNull(): boolean { return undefined !== null; }";

#[test]
fn strict_and_loose_nullish_literals_emit_distinct_constants() {
    let out = compile_module(STRICTNESS_SOURCE);
    assert!(!out.has_errors(), "got {:?}", out.diagnostics);
    let rust = out.rust_source.expect("rust_source");
    let norm: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        norm.contains("fnstrictVsLoose") && norm.contains("returnfalse"),
        "`void 0 === null` must fold to `false`; got:\n{rust}"
    );
    assert!(
        norm.contains("fnlooseVsStrict") && norm.contains("returntrue"),
        "`void 0 == null` must fold to `true`; got:\n{rust}"
    );
    assert!(
        !norm.contains("==()"),
        "`=== null`/`== null` between two nullish literals must not emit `() == ()`; got:\n{rust}"
    );
}

const STRICTNESS_MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert!(!strictness::strictVsLoose(&ctx));
    assert!(strictness::looseVsStrict(&ctx));
    assert!(strictness::nullNull(&ctx));
    assert!(strictness::undefStrictNeNull(&ctx));
    println!("ok");
}
"#;

#[test]
fn strict_and_loose_nullish_literals_match_js_at_runtime() {
    let out = compile_module(STRICTNESS_SOURCE);
    assert!(!out.has_errors(), "got {:?}", out.diagnostics);
    let rust = out.rust_source.expect("rust_source");
    let stdout = run_generated_crate("strictness", &rust, STRICTNESS_MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn generic_type_param_compared_to_null_is_rejected() {
    let out = compile_module("export function gen<T>(x: T): boolean { return x === null; }");
    assert!(
        out.has_errors(),
        "comparing a generic type parameter to null must be rejected, not emitted as broken \
         Rust; got {:?}",
        out.diagnostics
    );
    assert!(
        out.diagnostics
            .iter()
            .any(|d| d.code.as_str() == "P0005" && d.message.contains("generic type parameter")),
        "expected a P0005 diagnostic about a generic type parameter; got {:?}",
        out.diagnostics
    );
}

#[test]
fn omitted_optional_argument_passes_none() {
    let out = compile_module(
        "function opt2(x?: number): boolean { return x === null; } \
         export function call(): boolean { return opt2(); }",
    );
    assert!(!out.has_errors(), "got {:?}", out.diagnostics);
    let rust = out.rust_source.expect("rust_source");
    let norm: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        norm.contains("x:Option<f64>"),
        "`x?: number` must become an `Option<f64>` parameter; got:\n{rust}"
    );
    assert!(
        norm.contains("opt2(__ctx,::core::option::Option::None)"),
        "an omitted optional argument must be passed as `Option::None`; got:\n{rust}"
    );
    check_generated_crate("omitted_optional_arg", &rust);
}

#[test]
fn given_optional_argument_is_wrapped() {
    let out = compile_module(
        "function opt2(x?: number): boolean { return x === null; } \
         export function call(): boolean { return opt2(3); }",
    );
    assert!(!out.has_errors(), "got {:?}", out.diagnostics);
    let rust = out.rust_source.expect("rust_source");
    let norm: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        norm.contains("opt2(__ctx,::core::option::Option::Some(3.0))"),
        "a supplied optional argument must be wrapped in `Some(..)`; got:\n{rust}"
    );
    check_generated_crate("given_optional_arg", &rust);
}

#[test]
fn omitting_a_required_argument_is_rejected() {
    let out = compile_module(
        "function need(x: number): number { return x; } \
         export function call(): number { return need(); }",
    );
    assert!(out.has_errors(), "got {:?}", out.diagnostics);
    assert!(
        out.diagnostics
            .iter()
            .any(|d| d.code.as_str() == "P0004" && d.message.contains("need")),
        "expected a P0004 diagnostic for a missing required argument; got {:?}",
        out.diagnostics
    );
}

#[test]
fn passing_an_extra_argument_is_rejected() {
    let out = compile_module(
        "function one(x: number): number { return x; } \
         export function call(): number { return one(1, 2); }",
    );
    assert!(out.has_errors(), "got {:?}", out.diagnostics);
    assert!(
        out.diagnostics
            .iter()
            .any(|d| d.code.as_str() == "P0004" && d.message.contains("one")),
        "expected a P0004 diagnostic for an extra argument; got {:?}",
        out.diagnostics
    );
}

const CALL_SOURCE: &str = "function num(): number { return 1; } \
     export function callEqNull(): boolean { return num() === null; } \
     export function callNeNull(): boolean { return num() !== null; }";

#[test]
fn call_result_compared_to_null_folds_but_keeps_the_call() {
    let out = compile_module(CALL_SOURCE);
    assert!(!out.has_errors(), "got {:?}", out.diagnostics);
    let rust = out.rust_source.expect("rust_source");
    let norm: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        !norm.contains("==()") && !norm.contains("!=()"),
        "a concrete call result compared to null must fold instead of emitting `() == ()`; \
         got:\n{rust}"
    );
    assert!(
        norm.contains("let_=&(num(__ctx))"),
        "the call must still be evaluated for its side effects; got:\n{rust}"
    );
    assert!(norm.contains("false"), "got:\n{rust}");
    assert!(norm.contains("true"), "got:\n{rust}");
    check_generated_crate("call_null_fold", &rust);
}

const NULLISH_KINDS_SOURCE: &str = "\
     export function nullOnlyVsNull(x: number | null): boolean { return x === null; } \
     export function nullOnlyVsUndefined(x: number | null): boolean { return x === undefined; } \
     export function undefinedOnlyVsNull(x: number | undefined): boolean { return x === null; } \
     export function undefinedOnlyVsUndefined(x: number | undefined): boolean { return x === undefined; } \
     export function nullOnlyLooseVsUndefined(x: number | null): boolean { return x == undefined; } \
     export function undefinedOnlyLooseVsNull(x: number | undefined): boolean { return x == null; }";

#[test]
fn null_and_undefined_are_distinguished_in_optional_comparisons() {
    let out = compile_module(NULLISH_KINDS_SOURCE);
    assert!(!out.has_errors(), "got {:?}", out.diagnostics);
    let rust = out.rust_source.expect("rust_source");
    let norm: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        !norm.contains("==()") && !norm.contains("!=()"),
        "every nullish comparison here must fold or lower to `is_none()`/`is_some()`; got:\n{rust}"
    );
    check_generated_crate("nullish_kinds", &rust);
}

const NULLISH_KINDS_MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert!(nullish_kinds::nullOnlyVsNull(&ctx, ::core::option::Option::None));
    assert!(!nullish_kinds::nullOnlyVsNull(&ctx, ::core::option::Option::Some(1.0)));
    assert!(
        !nullish_kinds::nullOnlyVsUndefined(&ctx, ::core::option::Option::None),
        "`number | null` has no undefined, so `=== undefined` must be false"
    );
    assert!(
        !nullish_kinds::undefinedOnlyVsNull(&ctx, ::core::option::Option::None),
        "`number | undefined` has no null, so `=== null` must be false"
    );
    assert!(nullish_kinds::undefinedOnlyVsUndefined(&ctx, ::core::option::Option::None));
    assert!(nullish_kinds::nullOnlyLooseVsUndefined(&ctx, ::core::option::Option::None));
    assert!(nullish_kinds::undefinedOnlyLooseVsNull(&ctx, ::core::option::Option::None));
    println!("ok");
}
"#;

#[test]
fn nullish_kinds_match_js_semantics_at_runtime() {
    let out = compile_module(NULLISH_KINDS_SOURCE);
    assert!(!out.has_errors(), "got {:?}", out.diagnostics);
    let rust = out.rust_source.expect("rust_source");
    let stdout = run_generated_crate("nullish_kinds", &rust, NULLISH_KINDS_MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

/// A type that admits *both* `null` and `undefined` cannot answer a strict
/// comparison: the AOT representation stores both as `Option::None`, so
/// `x === null` and `x === undefined` would both have to be true. The compiler
/// rejects the comparison instead of answering one of them incorrectly.
#[test]
fn strict_comparison_of_a_both_kinds_optional_is_rejected() {
    let out = compile_module(
        "export function bothVsNull(x: number | null | undefined): boolean { return x === null; }",
    );
    assert!(out.has_errors(), "got {:?}", out.diagnostics);
    assert!(
        out.diagnostics.iter().any(
            |d| d.code.as_str() == "P0020" && d.message.contains("both `null` and `undefined`")
        ),
        "expected a P0020 diagnostic explaining that the strict comparison cannot be decided; \
         got {:?}",
        out.diagnostics
    );

    // The loose form is fine: `== null` treats `null` and `undefined` alike.
    let loose = compile_module(
        "export function bothLoose(x: number | null | undefined): boolean { return x == null; }",
    );
    assert!(
        !loose.has_errors(),
        "`x == null` is decidable for a `null | undefined` operand; got {:?}",
        loose.diagnostics
    );
    let rust = loose.rust_source.expect("rust_source");
    let norm: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        norm.contains(".is_none()"),
        "`x == null` must lower to `is_none()`; got:\n{rust}"
    );
}

/// Writing a `null` into a `T | undefined` slot (and vice versa) used to be
/// accepted silently, which then made `=== null` / `=== undefined` answer the
/// opposite of JavaScript. It is now rejected.
#[test]
fn nullish_kind_mismatch_writes_are_rejected() {
    let out = compile_module(
        "export function f(): boolean { const x: number | undefined = null; return x === undefined; }",
    );
    assert!(out.has_errors(), "got {:?}", out.diagnostics);
    assert!(
        out.diagnostics
            .iter()
            .any(|d| d.code.as_str() == "P0019" && d.message.contains("only allows `undefined`")),
        "expected a P0019 diagnostic for `null` written into `number | undefined`; got {:?}",
        out.diagnostics
    );

    let args = compile_module(
        "function f(x: number | null): boolean { return x === null; } \
         export function g(): boolean { return f(undefined); }",
    );
    assert!(args.has_errors(), "got {:?}", args.diagnostics);
    assert!(
        args.diagnostics
            .iter()
            .any(|d| d.code.as_str() == "P0019" && d.message.contains("parameter 1")),
        "expected a P0019 diagnostic for `undefined` passed to `number | null`; got {:?}",
        args.diagnostics
    );
}

/// Passing an optional value where the callee requires a non-optional one has
/// no AOT lowering; it must be reported instead of emitting `Option<T>` where
/// `T` is expected.
#[test]
fn optional_value_into_a_required_parameter_is_rejected() {
    let out = compile_module(
        "function need(x: number): number { return x; } \
         export function g(v: number | null): number { return need(v); }",
    );
    assert!(out.has_errors(), "got {:?}", out.diagnostics);
    assert!(
        out.diagnostics
            .iter()
            .any(|d| d.code.as_str() == "P0018" && d.message.contains("non-optional")),
        "expected a P0018 diagnostic for an optional argument passed to a required parameter; \
         got {:?}",
        out.diagnostics
    );
    assert!(
        out.rust_source.is_none(),
        "no Rust may be emitted once the write is rejected"
    );
}

/// Comparing an optional value with a plain value used to emit
/// `Option<T> == T`, which does not compile. JavaScript compares the values, so
/// the plain side is compared against `Some(..)`.
#[test]
fn mixed_optional_and_plain_equality_compiles_and_matches_js() {
    const SOURCE: &str = "export function same(x: string | null, y: string): boolean { return x === y; } \
         export function differ(x: string | null, y: string): boolean { return x !== y; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host, JsString};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert!(mixed_optional_equality::same(&ctx, ::core::option::Option::Some(JsString::from("a")), JsString::from("a")));
    assert!(!mixed_optional_equality::same(&ctx, ::core::option::Option::None, JsString::from("a")));
    assert!(mixed_optional_equality::differ(&ctx, ::core::option::Option::None, JsString::from("a")));
    assert!(!mixed_optional_equality::differ(&ctx, ::core::option::Option::Some(JsString::from("a")), JsString::from("a")));
    println!("ok");
}
"#;
    let out = compile_module(SOURCE);
    assert!(!out.has_errors(), "got {:?}", out.diagnostics);
    let rust = out.rust_source.expect("rust_source");
    let norm: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        norm.contains(".as_ref()==::core::option::Option::Some("),
        "a mixed optional/plain comparison must compare against `Some(..)`; got:\n{rust}"
    );
    check_generated_crate("mixed_optional_equality", &rust);

    let out = compile_module(SOURCE);
    let rust = out.rust_source.expect("rust_source");
    let stdout = run_generated_crate("mixed_optional_equality", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}
