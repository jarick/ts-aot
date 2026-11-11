//! Regression tests for `||` / `&&` over optional operands and for mixed
//! comparisons against an optional value.
//!
//! The blocker was `x || call()` / `x && call()`: HIR carries no type for the
//! call, so the right operand used to be typed like the (optional) left one and
//! the generated `Option<T>` was returned where a `T` was declared - invalid
//! Rust, no diagnostic. The result type now comes from the converted operand
//! (`mir_expr_static_ty`) and from the callee's declared return type
//! (`hir_static_ty`), and the cases the AOT target cannot express are reported
//! (`P0018`/`P0021`) instead of emitted.

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

fn codes(source: &str) -> Vec<String> {
    compile_module(source)
        .diagnostics
        .iter()
        .map(|d| d.code.as_str().to_owned())
        .collect()
}

/// A1: `x || call()` yields the call's value when `x` is nullish or falsy, and
/// `x && call()` stays optional. Both must compile and match JavaScript.
#[test]
fn optional_operand_with_a_call_rhs_keeps_the_non_optional_type() {
    const SOURCE: &str = "function call(): number { return 1; } \
         export function useCall(x: number | null): number { return x || call(); } \
         export function andCall(x: number | null): number | null { return x && call(); }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    let some = |v: f64| ::core::option::Option::Some(v);
    // `x || call()`: None -> 1, Some(3) -> 3, Some(0) -> 1 (0 is falsy).
    assert_eq!(logical_calls::useCall(&ctx, ::core::option::Option::None), 1.0);
    assert_eq!(logical_calls::useCall(&ctx, some(3.0)), 3.0);
    assert_eq!(logical_calls::useCall(&ctx, some(0.0)), 1.0);
    // `x && call()` is `number | null`: None -> None, Some(0) -> 0, Some(3) -> 1.
    assert_eq!(
        logical_calls::andCall(&ctx, ::core::option::Option::None),
        ::core::option::Option::None
    );
    assert_eq!(logical_calls::andCall(&ctx, some(0.0)), some(0.0));
    assert_eq!(logical_calls::andCall(&ctx, some(3.0)), some(1.0));
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let norm: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        norm.contains("pubfnuseCall<'a>(__ctx:&ts_aot_runtime::Ctx<'a>,x:Option<f64>)->f64"),
        "`x || call()` must be typed `f64` (not `Option<f64>`) for `useCall`; got:\n{rust}"
    );
    check_generated_crate("logical_calls", &rust);
    let stdout = run_generated_crate("logical_calls", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

/// The same call on the *other* side: `call() || x` / `call() && x`. A plain
/// (non-optional) left operand does not make the result plain when the right one
/// may be nullish, so the temporary stays `Option<T>` - and must still compile.
#[test]
fn a_call_operand_on_either_side_is_typed_from_its_return_type() {
    const SOURCE: &str = "function call(): number { return 1; } \
         function zero(): number { return 0; } \
         export function orCallLeft(x: number | null): number | null { return call() || x; } \
         export function andCallLeft(x: number | null): number | null { return call() && x; } \
         export function orZeroLeft(x: number | null): number | null { return zero() || x; } \
         export function andZeroLeft(x: number | null): number | null { return zero() && x; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    let some = |v: f64| ::core::option::Option::Some(v);
    let none = ::core::option::Option::None;
    // `1 || x` is always 1; `1 && x` is x.
    assert_eq!(logical_orders::orCallLeft(&ctx, none), some(1.0));
    assert_eq!(logical_orders::orCallLeft(&ctx, some(5.0)), some(1.0));
    assert_eq!(logical_orders::andCallLeft(&ctx, none), none);
    assert_eq!(logical_orders::andCallLeft(&ctx, some(5.0)), some(5.0));
    // `0 || x` is x; `0 && x` is 0.
    assert_eq!(logical_orders::orZeroLeft(&ctx, none), none);
    assert_eq!(logical_orders::orZeroLeft(&ctx, some(4.0)), some(4.0));
    assert_eq!(logical_orders::andZeroLeft(&ctx, none), some(0.0));
    assert_eq!(logical_orders::andZeroLeft(&ctx, some(4.0)), some(0.0));
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    check_generated_crate("logical_orders", &rust);
    let stdout = run_generated_crate("logical_orders", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

/// B7: the callee's declared return type is known statically, so
/// `g() || 5` (with `g(): number | undefined`) must be lowered, not rejected as
/// "operand types are not known". The literal does not make the result optional.
///
/// `g() && 5` is different: the operator yields its *left* operand when that is
/// falsy, and `undefined` is falsy, so the result keeps the `undefined` of
/// `g()` - the declared type has to say `number | undefined`, not `number | null`
/// (the null-only target is rejected with `P0019` by
/// `an_undefined_only_result_is_rejected_for_a_null_only_target`).
#[test]
fn call_with_a_literal_fallback_is_lowered() {
    const SOURCE: &str = "function g(): number | undefined { return undefined; } \
         function g3(): number | undefined { return 3; } \
         export function pick(): number { return g() || 5; } \
         export function pick3(): number { return g3() || 5; } \
         export function andPick(): number | undefined { return g() && 5; } \
         export function andPick3(): number | undefined { return g3() && 5; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(call_fallback::pick(&ctx), 5.0);
    assert_eq!(call_fallback::pick3(&ctx), 3.0);
    assert_eq!(
        call_fallback::andPick(&ctx),
        ::core::option::Option::None
    );
    assert_eq!(
        call_fallback::andPick3(&ctx),
        ::core::option::Option::Some(5.0)
    );
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    check_generated_crate("call_fallback", &rust);
    let stdout = run_generated_crate("call_fallback", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");

    // The review probe: an unannotated local initialised from `g() || 5` must
    // compile (it used to be rejected with P0021).
    let diagnostics = codes(
        "function g(): number | undefined { return undefined; } \
         export function f(): number { const r = g() || 5; return 1; }",
    );
    assert!(
        !diagnostics.iter().any(|code| code == "P0021"),
        "`g() || 5` must not be rejected as an unknown operand type; got {diagnostics:?}"
    );
}

/// The two optional sides of the same operator still have to agree on *which*
/// nullish value they hold: `g() && 5` over `number | undefined` yields the
/// `undefined` of the left operand when it is falsy, so it can never be `null`
/// and a `number | null` target must be rejected (`P0019`). The null-only target
/// is a separate case from the successful declarations above, which keep the
/// `undefined` their result really has.
#[test]
fn an_undefined_only_result_is_rejected_for_a_null_only_target() {
    let null_only = codes(
        "function g(): number | undefined { return undefined; } \
         export function andPick(): number | null { return g() && 5; }",
    );
    assert!(
        null_only.iter().any(|code| code == "P0019"),
        "`g() && 5` is `number | undefined`, so a `number | null` target is a P0019; \
         got {null_only:?}"
    );

    // The same result in the type it actually has is accepted, and so is the
    // both-kinds target that can hold either spelling.
    for source in [
        "function g(): number | undefined { return undefined; } \
         export function andPick(): number | undefined { return g() && 5; }",
        "function g(): number | undefined { return undefined; } \
         export function andPick(): number | null | undefined { return g() && 5; }",
    ] {
        let diagnostics = codes(source);
        assert!(
            !diagnostics
                .iter()
                .any(|code| code == "P0019" || code == "P0018"),
            "`{source}` must keep lowering; got {diagnostics:?}"
        );
    }
}

/// A `||`/`&&` result that may be nullish cannot be returned from a function
/// declared to return a plain value: report it instead of emitting `Option<T>`
/// where `T` is required.
#[test]
fn an_optional_result_in_a_non_optional_position_is_reported() {
    let diagnostics = codes(
        "function call(): number { return 1; } \
         export function andCall(x: number | null): number { return x && call(); }",
    );
    assert!(
        diagnostics.iter().any(|code| code == "P0018"),
        "`x && call()` is `number | null`, so returning it as `number` must be a P0018; got \
         {diagnostics:?}"
    );
}

/// The mixed comparisons whose generated Rust could never compile: an optional
/// operand against a non-optional type it cannot be compared with.
#[test]
fn incompatible_mixed_comparisons_are_reported() {
    for (source, code, left, right) in [
        (
            "export function f(g: number | null, s: string): boolean { return g === s; }",
            "P0021",
            "number | null",
            "string",
        ),
        (
            "export function f(g: number | null): boolean { return g === \"0\"; }",
            "P0021",
            "number | null",
            "string",
        ),
        (
            "export function f(t: string | null): boolean { return t === 1; }",
            "P0021",
            "string | null",
            "number",
        ),
        (
            "export function f(b: boolean | null): boolean { return b === 1; }",
            "P0021",
            "boolean | null",
            "number",
        ),
        (
            "export function f(g: number | null, s: string): boolean { return g !== s; }",
            "P0021",
            "number | null",
            "string",
        ),
        (
            "export function f(g: number | null): boolean { return g < 5; }",
            "P0018",
            "number | null",
            "number",
        ),
        (
            "export function f(g: number | null): boolean { return 5 > g; }",
            "P0018",
            "number",
            "number | null",
        ),
    ] {
        let out = compile_module(source);
        assert!(
            out.has_errors(),
            "`{source}` must be rejected; got {:?}",
            out.diagnostics
        );
        let diagnostic = out
            .diagnostics
            .iter()
            .find(|d| d.code.as_str() == code)
            .unwrap_or_else(|| {
                panic!(
                    "`{source}` must be diagnosed with {code}; got {:?}",
                    out.diagnostics
                )
            });
        assert!(
            diagnostic.message.contains(left) && diagnostic.message.contains(right),
            "the {code} message of `{source}` must name both operands (`{left}`, `{right}`); got \
             {:?}",
            diagnostic.message
        );
        assert!(
            out.rust_source.is_none(),
            "a rejected comparison must not emit Rust"
        );
    }
}

/// The comparisons the backend *can* lower must keep working: the optional's
/// inner type, a numeric literal, two optionals, a string/bool literal.
#[test]
fn compatible_mixed_and_optional_comparisons_still_work() {
    const SOURCE: &str = "export function same(x: string | null, y: string): boolean { return x === y; } \
         export function isZero(g: number | null): boolean { return g === 0; } \
         export function both(g: number | null, h: number | null): boolean { return g === h; } \
         export function lit(t: string | null): boolean { return t === \"a\"; } \
         export function flag(b: boolean | null): boolean { return b === true; } \
         export function rel(a: number, b: number): boolean { return a < b; } \
         export function loose(x: number | null): boolean { return x == null; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host, JsString};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    let some = |v: f64| ::core::option::Option::Some(v);
    let none_number = || ::core::option::Option::None::<f64>;
    let text = JsString::from("a");
    assert!(compatible_cmp::same(
        &ctx,
        ::core::option::Option::Some(text.clone()),
        text.clone()
    ));
    assert!(!compatible_cmp::same(
        &ctx,
        ::core::option::Option::None::<JsString<'_>>,
        text.clone()
    ));
    assert!(compatible_cmp::isZero(&ctx, some(0.0)));
    assert!(!compatible_cmp::isZero(&ctx, none_number()));
    assert!(compatible_cmp::both(&ctx, some(1.0), some(1.0)));
    assert!(!compatible_cmp::both(&ctx, some(1.0), none_number()));
    assert!(compatible_cmp::lit(
        &ctx,
        ::core::option::Option::Some(text.clone())
    ));
    assert!(!compatible_cmp::lit(
        &ctx,
        ::core::option::Option::None::<JsString<'_>>
    ));
    assert!(compatible_cmp::flag(&ctx, ::core::option::Option::Some(true)));
    assert!(!compatible_cmp::flag(&ctx, ::core::option::Option::None::<bool>));
    assert!(compatible_cmp::rel(&ctx, 1.0, 2.0));
    assert!(compatible_cmp::loose(&ctx, none_number()));
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    check_generated_crate("compatible_cmp", &rust);
    let stdout = run_generated_crate("compatible_cmp", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

/// B4: a `null`/`undefined` branch has no type of its own; the conditional must
/// take the other branch's nullable type instead of failing with E0414.
#[test]
fn a_conditional_with_an_undefined_branch_is_typed() {
    const SOURCE: &str = "export function fromParam(c: boolean, x: number | undefined): number | undefined { \
             return c ? x : undefined; } \
         export function fromLiteral(c: boolean): number | undefined { \
             let y: number | undefined = c ? 1 : undefined; return y; } \
         export function fromNull(c: boolean): number | null { return c ? 2 : null; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    let some = |v: f64| ::core::option::Option::Some(v);
    let none = ::core::option::Option::None;
    assert_eq!(conditional_nullish::fromParam(&ctx, true, some(2.0)), some(2.0));
    assert_eq!(conditional_nullish::fromParam(&ctx, true, none), none);
    assert_eq!(conditional_nullish::fromParam(&ctx, false, some(2.0)), none);
    assert_eq!(conditional_nullish::fromLiteral(&ctx, true), some(1.0));
    assert_eq!(conditional_nullish::fromLiteral(&ctx, false), none);
    assert_eq!(conditional_nullish::fromNull(&ctx, true), some(2.0));
    assert_eq!(conditional_nullish::fromNull(&ctx, false), none);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    check_generated_crate("conditional_nullish", &rust);
    let stdout = run_generated_crate("conditional_nullish", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

/// B8: a union has no single Rust representation and its variants disagree on
/// truthiness, so `x || y` must be reported instead of silently becoming
/// `if true`.
#[test]
fn union_logical_operands_are_reported_not_folded_to_true() {
    let out = compile_module(
        "export function unionOr(x: number | string | undefined, y: number | string | undefined): number { \
             let r = x || y; return 1; }",
    );
    assert!(out.has_errors(), "got {:?}", out.diagnostics);
    assert!(
        out.diagnostics.iter().any(
            |d| d.code.as_str() == "P0021" && d.message.contains("number | string | undefined")
        ),
        "comparing the truthiness of a union must be a P0021 naming the union; got {:?}",
        out.diagnostics
    );
    assert!(
        out.rust_source.is_none(),
        "a rejected union operator must not emit Rust"
    );
}

/// The lowerings that already worked must keep matching JavaScript: the value
/// inside an optional left operand decides, a member read and a local on the
/// right keep the plain result, and `||=` stays short-circuiting.
#[test]
fn existing_optional_logical_lowerings_keep_their_values() {
    const SOURCE: &str = "class C { f: number; constructor() { this.f = 3; } } \
         export function bothOpt(x: number | undefined, y: number | undefined): number | undefined { return x || y; } \
         export function field(x: number | null, c: C): number { return x || c.f; } \
         export function local(x: number | null, n: number): number { return x || n; } \
         let g: number | null = null; \
         function bump(): number { return 4; } \
         export function orAssign(): number { g ||= bump(); return g ?? 0; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    let some = |v: f64| ::core::option::Option::Some(v);
    let none = ::core::option::Option::None;
    assert_eq!(existing_logical::bothOpt(&ctx, none, some(3.0)), some(3.0));
    assert_eq!(existing_logical::bothOpt(&ctx, some(0.0), some(3.0)), some(3.0));
    assert_eq!(existing_logical::bothOpt(&ctx, some(2.0), some(3.0)), some(2.0));
    let c = existing_logical::C { f: 3.0 };
    assert_eq!(existing_logical::field(&ctx, none, c.clone()), 3.0);
    assert_eq!(existing_logical::field(&ctx, some(0.0), c.clone()), 3.0);
    assert_eq!(existing_logical::field(&ctx, some(4.0), c), 4.0);
    assert_eq!(existing_logical::local(&ctx, none, 7.0), 7.0);
    assert_eq!(existing_logical::local(&ctx, some(1.0), 7.0), 1.0);
    assert_eq!(existing_logical::local(&ctx, some(0.0), 7.0), 7.0);
    assert_eq!(existing_logical::orAssign(&ctx), 4.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    check_generated_crate("existing_logical", &rust);
    let stdout = run_generated_crate("existing_logical", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

/// A `null`/`undefined` literal is always nullish, on either side.
///
/// On the *right* it makes the result optional and keeps its own kinds:
/// `x || null` is a valid `number | null` (it used to be a `P0021` operand
/// mismatch), and the literal is stored as `Option::None` rather than coerced
/// into a plain-value temporary (`let tmp: f64 = ();`, which cannot compile).
///
/// On the *left* it decides the operator alone: `null || x` is `x`, `null && x`
/// is the nullish literal itself, and `x` is not evaluated at all for `&&`. It
/// must be handled before the plain-value temporary is created.
#[test]
fn nullish_literals_on_either_side_match_javascript() {
    const SOURCE: &str = "export function orNull(x: number): number | null { return x || null; } \
         export function andNull(x: number): number | null { return x && null; } \
         export function orUndef(x: number): number | undefined { return x || undefined; } \
         export function andUndef(x: number): number | undefined { return x && undefined; } \
         export function nullOr(x: number): number { return null || x; } \
         export function undefOr(x: number): number { return undefined || x; } \
         export function orNullIsNull(x: number): bool { const v = x || null; return v === null; } \
         export function orNullIsUndef(x: number): bool { const v = x || null; return v === undefined; } \
         export function nullAndIsNull(x: number): bool { const v = null && x; return v === null; } \
         let hits: number = 0; \
         export function bump(): number { hits = hits + 1; return 7; } \
         export function nullAndNoEval(): null { return null && bump(); } \
         export function nullOrEval(): number { return null || bump(); } \
         export function hitsNow(): number { return hits; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    use ::core::option::Option as O;
    // A falsy left value selects the literal, a truthy one is kept.
    assert_eq!(nullish_literals::orNull(&ctx, 0.0), O::None);
    assert_eq!(nullish_literals::orNull(&ctx, 3.0), O::Some(3.0));
    assert_eq!(nullish_literals::andNull(&ctx, 0.0), O::Some(0.0));
    assert_eq!(nullish_literals::andNull(&ctx, 3.0), O::None);
    assert_eq!(nullish_literals::orUndef(&ctx, 0.0), O::None);
    assert_eq!(nullish_literals::orUndef(&ctx, 3.0), O::Some(3.0));
    assert_eq!(nullish_literals::andUndef(&ctx, 0.0), O::Some(0.0));
    assert_eq!(nullish_literals::andUndef(&ctx, 3.0), O::None);
    // The literal keeps its own kind: `null` never compares equal to `undefined`.
    assert!(nullish_literals::orNullIsNull(&ctx, 0.0));
    assert!(!nullish_literals::orNullIsNull(&ctx, 3.0));
    assert!(!nullish_literals::orNullIsUndef(&ctx, 0.0));
    assert!(nullish_literals::nullAndIsNull(&ctx, 5.0));
    // The left literal decides alone, and `&&` does not evaluate the right side.
    assert_eq!(nullish_literals::nullOr(&ctx, 5.0), 5.0);
    assert_eq!(nullish_literals::undefOr(&ctx, 5.0), 5.0);
    let _ = nullish_literals::nullAndNoEval(&ctx);
    assert_eq!(nullish_literals::hitsNow(&ctx), 0.0);
    assert_eq!(nullish_literals::nullOrEval(&ctx), 7.0);
    assert_eq!(nullish_literals::hitsNow(&ctx), 1.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    check_generated_crate("nullish_literals", &rust);
    let stdout = run_generated_crate("nullish_literals", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

/// An integer literal has to fit the integer type both branches are lowered
/// against: `x || 300` over a `u8` may yield `300`, and a `u8` temporary holding
/// it is an out-of-range Rust literal, which does not compile at all.
#[test]
fn an_integer_literal_that_does_not_fit_the_other_operand_is_rejected() {
    let out = compile_module("export function f(x: u8): u8 { return x || 300; }");
    assert!(
        out.has_errors(),
        "an out-of-range literal must not lower; got {:?}",
        out.diagnostics
    );
    assert!(
        out.diagnostics
            .iter()
            .any(|d| d.code.as_str() == "P0021" && d.message.contains("does not fit")),
        "the out-of-range literal must be reported by name; got {:?}",
        out.diagnostics
    );

    // Controls: a literal that fits, a wider integer type and a float operand all
    // keep their ordinary lowering.
    for source in [
        "export function f(x: u8): u8 { return x || 1; }",
        "export function f(x: i64): i64 { return x || 300; }",
        "export function f(x: i64): f64 { return x || 1.5; }",
    ] {
        let out = compile_module(source);
        assert!(
            !out.has_errors(),
            "`{source}` must still lower; got {:?}",
            out.diagnostics
        );
    }
}
