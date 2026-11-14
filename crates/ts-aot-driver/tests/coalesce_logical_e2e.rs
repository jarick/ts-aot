//! `??`, `||` and `&&` over optional and mixed `int`/`float` operands.
//!
//! These operators yield one of their *operands*, so the destination type they
//! lower into has to hold every branch that can reach it: a compatible mixed
//! `int`/`float` pair meets in the float type (or a fractional fallback would be
//! truncated), a nullish fallback decides the optional representation, and the
//! result's nullish kinds come from the branches that can actually be yielded.
//! Every generated crate is compiled and its runtime values are compared with
//! JavaScript.

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

/// The crate stage (`--emit crate`) emits the lib-style module, whose exports are
/// `pub` and whose module-init body is reachable as `__ts_aot_init`.
fn crate_rust_of(source: &str) -> String {
    let out = Driver::new().compile_source(
        "test.ts",
        source,
        &CompileOptions {
            emit: EmitStage::Crate,
            module: true,
        },
    );
    assert!(
        !out.has_errors(),
        "must compile through the full pipeline; got {:?}",
        out.diagnostics
    );
    out.rust_source
        .expect("emit-crate must populate rust_source")
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

/// R1: a float fallback keeps its fraction. The destination is the common
/// numeric type of the operands, not the left operand's `i64`.
#[test]
fn a_float_fallback_keeps_its_fraction_through_the_common_numeric_type() {
    const SOURCE: &str = "export function c1(a: number | null): number { return a ?? 1.5; } \
         export function c2(a: number): number { return a || 1.5; } \
         export function c3(a: number): number { return a && 1.5; } \
         export function c4(a: number | null, b: number | null): number { return (a ?? b) ?? 1.5; } \
         export function m1(a: i64 | null): number { return a ?? 1.5; } \
         export function m2(a: i64 | null): number { return a || 1.5; } \
         export function m3(): number { let z = 2; return z && 1.5; } \
         export function m4(): number { let z = 0; return z || 1.5; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    let some = ::core::option::Option::Some;
    // JavaScript: `a ?? 1.5` is 1.5 for `null` and `a` otherwise.
    assert_eq!(common_numeric::c1(&ctx, ::core::option::Option::None), 1.5);
    assert_eq!(common_numeric::c1(&ctx, some(2.0)), 2.0);
    // `0 || 1.5` is 1.5, `2 || 1.5` is 2.
    assert_eq!(common_numeric::c2(&ctx, 0.0), 1.5);
    assert_eq!(common_numeric::c2(&ctx, 2.0), 2.0);
    // `0 && 1.5` is 0, `2 && 1.5` is 1.5.
    assert_eq!(common_numeric::c3(&ctx, 0.0), 0.0);
    assert_eq!(common_numeric::c3(&ctx, 2.0), 1.5);
    assert_eq!(common_numeric::c4(&ctx, ::core::option::Option::None, ::core::option::Option::None), 1.5);
    assert_eq!(common_numeric::c4(&ctx, some(2.0), some(3.0)), 2.0);
    assert_eq!(common_numeric::c4(&ctx, ::core::option::Option::None, some(3.0)), 3.0);
    // The same operators over an `i64` left operand: the fraction must survive.
    assert_eq!(common_numeric::m1(&ctx, ::core::option::Option::None), 1.5);
    assert_eq!(
        common_numeric::m1(&ctx, ::core::option::Option::Some(4_i64)),
        4.0
    );
    assert_eq!(common_numeric::m2(&ctx, ::core::option::Option::None), 1.5);
    assert_eq!(
        common_numeric::m2(&ctx, ::core::option::Option::Some(0_i64)),
        1.5
    );
    assert_eq!(
        common_numeric::m2(&ctx, ::core::option::Option::Some(4_i64)),
        4.0
    );
    assert_eq!(common_numeric::m3(&ctx), 1.5);
    assert_eq!(common_numeric::m4(&ctx), 1.5);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let norm = normalized(&rust);
    // `((1.5) as i64)` is the old truncation: the fallback used to be coerced
    // into the left operand's integer type.
    assert!(
        !norm.contains("(1.5)asi64"),
        "a fractional fallback must not be truncated into the left operand's integer type; got:\n{rust}"
    );
    assert!(
        norm.contains("asf64"),
        "the integer left operand must be widened into the common `f64` type; got:\n{rust}"
    );
    assert!(
        !norm.contains("||"),
        "a value-producing `||` must not become Rust's `bool`-only `||`; got:\n{rust}"
    );
    check_generated_crate("common_numeric", &rust);
    let stdout = run_generated_crate("common_numeric", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

/// R2: an always-nullish left operand still runs its effects. The left operand
/// is converted to an inline expression (a call), so dropping it would drop the
/// call; an effect-free left operand (a `null`-typed local) must not emit
/// anything.
#[test]
fn an_always_nullish_left_operand_keeps_its_effects() {
    const SOURCE: &str = "let hits: number = 0; \
         export function side(): null { hits = hits + 1; return null; } \
         export function e1(): number { return side() ?? 7; } \
         export function quiet(): number | null { let s: null = null; return s ?? 7; } \
         export function hitsNow(): number { return hits; } \
         export function reset(): number { hits = 0; return hits; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    common_numeric_effects::reset(&ctx);
    assert_eq!(common_numeric_effects::e1(&ctx), 7.0);
    assert_eq!(
        common_numeric_effects::hitsNow(&ctx),
        1.0,
        "`side()` must still run: the left operand is evaluated before the fallback"
    );
    assert_eq!(
        common_numeric_effects::quiet(&ctx),
        ::core::option::Option::Some(7.0)
    );
    assert_eq!(
        common_numeric_effects::hitsNow(&ctx),
        1.0,
        "an effect-free left operand must not add anything"
    );
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let e1 = normalized(body_of(&rust, "e1"));
    assert!(
        e1.contains("side(__ctx);"),
        "the effectful left operand must be emitted before the fallback; got:\n{e1}"
    );
    assert!(
        e1.contains("return7.0;"),
        "an always-nullish left operand makes the expression the fallback; got:\n{e1}"
    );
    let quiet = normalized(body_of(&rust, "quiet"));
    assert!(
        !quiet.contains("s;"),
        "an effect-free left operand must not be emitted; got:\n{quiet}"
    );
    check_generated_crate("common_numeric_effects", &rust);
    let stdout = run_generated_crate("common_numeric_effects", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

/// R3: the result's nullish kinds come from the branches that can be yielded.
/// `&&` also yields the (nullish-capable) left operand, `||` yields the left
/// value only when it is truthy, so only the right operand's kinds survive.
#[test]
fn logical_results_carry_the_nullish_kinds_of_the_reachable_branch() {
    const SOURCE: &str = "export function l1(o: number | null, p: number | undefined): number | null | undefined { \
             return o && p; } \
         export function l2(o: number | null, p: number | undefined): number | null | undefined { \
             return o || p; } \
         export function orUndef(o: number | null, p: number | undefined): boolean { \
             return (o || p) === undefined; } \
         export function orNull(o: number | null, p: number | undefined): boolean { \
             return (o || p) === null; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    let some = ::core::option::Option::Some;
    let none = ::core::option::Option::None;
    // `o && p`: `null` when `o` is nullish, `p` when `o` is truthy, `o` when it
    // is falsy but present.
    assert_eq!(logical_kinds::l1(&ctx, none, some(2.0)), none);
    assert_eq!(logical_kinds::l1(&ctx, some(0.0), none), some(0.0));
    assert_eq!(logical_kinds::l1(&ctx, some(1.0), none), none);
    assert_eq!(logical_kinds::l1(&ctx, some(1.0), some(2.0)), some(2.0));
    // `o || p`: `o` when it is truthy, `p` otherwise.
    assert_eq!(logical_kinds::l2(&ctx, none, none), none);
    assert_eq!(logical_kinds::l2(&ctx, some(0.0), some(3.0)), some(3.0));
    assert_eq!(logical_kinds::l2(&ctx, some(2.0), some(3.0)), some(2.0));
    // `o || p` is `number | undefined` (a truthy `o` is a number, otherwise `p`
    // is), so `=== undefined` is `Option::is_none` and `=== null` is always
    // false. With the left operand's kinds the first would fold to `false` and
    // the second to `is_none`, both wrong in JavaScript.
    assert!(logical_kinds::orUndef(&ctx, none, none));
    assert!(logical_kinds::orUndef(&ctx, some(0.0), none));
    assert!(!logical_kinds::orUndef(&ctx, some(5.0), none));
    assert!(!logical_kinds::orUndef(&ctx, none, some(2.0)));
    assert!(!logical_kinds::orNull(&ctx, none, none));
    assert!(!logical_kinds::orNull(&ctx, some(5.0), some(1.0)));
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    assert!(
        normalized(body_of(&rust, "orUndef")).contains("is_none()"),
        "`||` over `number | undefined` can yield `undefined`, so `=== undefined` must test \
         presence; got:\n{rust}"
    );
    assert!(
        normalized(body_of(&rust, "orNull")).contains("returnfalse;"),
        "`||` over `number | undefined` can never yield `null`, so `=== null` is `false`; got:\n{rust}"
    );
    check_generated_crate("logical_kinds", &rust);
    let stdout = run_generated_crate("logical_kinds", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

/// R4: `0n` is falsy and every other `BigInt` is truthy, so `||`/`&&` must pick
/// their operand by JavaScript truthiness, not by the always-truthy object rule.
#[test]
fn bigint_operands_use_their_javascript_truthiness() {
    const SOURCE: &str = "export function b1(x: bigint): bigint { return x || 1n; } \
         export function b2(x: bigint): bigint { return x && 1n; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, BigIntHandle, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn text(handle: BigIntHandle) -> String {
    ts_aot_runtime::__ts_aot_bigint_to_string(&handle, None)
}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    let zero = || BigIntHandle::new("0");
    let two = || BigIntHandle::new("2");
    // `0n || 1n` is 1n, `2n || 1n` is 2n.
    assert_eq!(text(bigint_truth::b1(&ctx, zero())), "1");
    assert_eq!(text(bigint_truth::b1(&ctx, two())), "2");
    // `0n && 1n` is 0n, `2n && 1n` is 1n.
    assert_eq!(text(bigint_truth::b2(&ctx, zero())), "0");
    assert_eq!(text(bigint_truth::b2(&ctx, two())), "1");
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let norm = normalized(&rust);
    assert!(
        norm.contains("__ts_aot_bigint_neq"),
        "BigInt truthiness must compare against `0n`; got:\n{rust}"
    );
    assert!(
        !norm.contains("iftrue"),
        "a BigInt must not be treated as always truthy; got:\n{rust}"
    );
    check_generated_crate("bigint_truth", &rust);
    let stdout = run_generated_crate("bigint_truth", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

/// R5 + R6: a nullish or nullable fallback decides the destination. `a ?? null`,
/// `a ?? undefined` and `a ?? f()` (with `f(): number | null`) all have to pick a
/// destination that accepts the fallback's actual representation - an optional
/// whose nullish kinds are the *fallback's* - and the declared target type has
/// to hold those kinds too (`number | null` for a `null` fallback,
/// `number | undefined` for an `undefined` one; the mismatch is a `P0019`).
#[test]
fn a_nullish_fallback_selects_a_destination_that_accepts_it() {
    const SOURCE: &str = "export function f6(a: number | null): number | null { return a ?? null; } \
         export function f9(a: number | null): number | undefined { return a ?? undefined; } \
         export function g7(): number | null { return 1; } \
         export function f7(a: number | null): number | null { return a ?? g7(); } \
         export function coNull(a: number | null, b: number | undefined): boolean { \
             return (a ?? b) === null; } \
         export function coUndef(a: number | null, b: number | undefined): boolean { \
             return (a ?? b) === undefined; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    let some = ::core::option::Option::Some;
    let none = ::core::option::Option::None;
    // `a ?? null` is `a`: a present value survives, `null` stays `null`.
    assert_eq!(nullish_destination::f6(&ctx, none), none);
    assert_eq!(nullish_destination::f6(&ctx, some(2.0)), some(2.0));
    assert_eq!(nullish_destination::f9(&ctx, none), none);
    assert_eq!(nullish_destination::f9(&ctx, some(3.0)), some(3.0));
    // `a ?? g7()`: the call's declared `number | null` keeps the result optional.
    assert_eq!(nullish_destination::f7(&ctx, none), some(1.0));
    assert_eq!(nullish_destination::f7(&ctx, some(2.0)), some(2.0));
    // `a ?? b` is `number | undefined` (never `null`), so `=== null` is false
    // and `=== undefined` is presence.
    assert!(!nullish_destination::coNull(&ctx, none, none));
    assert!(!nullish_destination::coNull(&ctx, some(1.0), none));
    assert!(!nullish_destination::coNull(&ctx, none, some(2.0)));
    assert!(nullish_destination::coUndef(&ctx, none, none));
    assert!(!nullish_destination::coUndef(&ctx, some(1.0), none));
    assert!(!nullish_destination::coUndef(&ctx, none, some(2.0)));
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let norm = normalized(&rust);
    assert!(
        !norm.contains("__tmp2=()"),
        "a nullish fallback must not be assigned as `()` into a value slot; got:\n{rust}"
    );
    for name in ["f6", "f7", "f9"] {
        assert!(
            normalized(body_of(&rust, name)).contains("Option<f64>"),
            "`{name}` must keep an optional destination; got:\n{}",
            body_of(&rust, name)
        );
    }
    assert!(
        normalized(body_of(&rust, "coNull")).contains("returnfalse;"),
        "`a ?? b` over `number | null` / `number | undefined` can never be `null`; got:\n{rust}"
    );
    check_generated_crate("nullish_destination", &rust);
    let stdout = run_generated_crate("nullish_destination", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

/// R6: a nullish fallback is evaluated inside the nullish branch only. The call
/// returning `null` runs when the left operand is nullish and stays untouched
/// otherwise.
#[test]
fn a_nullish_call_fallback_runs_only_in_the_nullish_branch() {
    const SOURCE: &str = "let hits: number = 0; \
         export function side(): null { hits = hits + 1; return null; } \
         export function e3(a: number | null): number | null { return a ?? side(); } \
         export function hitsNow(): number { return hits; } \
         export function reset(): number { hits = 0; return hits; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    let some = ::core::option::Option::Some;
    nullish_call_fallback::reset(&ctx);
    assert_eq!(nullish_call_fallback::e3(&ctx, some(4.0)), some(4.0));
    assert_eq!(
        nullish_call_fallback::hitsNow(&ctx),
        0.0,
        "the fallback must not run when the left operand holds a value"
    );
    assert_eq!(nullish_call_fallback::e3(&ctx, ::core::option::Option::None), ::core::option::Option::None);
    assert_eq!(
        nullish_call_fallback::hitsNow(&ctx),
        1.0,
        "the fallback must run when the left operand is nullish"
    );
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let e3 = normalized(body_of(&rust, "e3"));
    let else_pos = e3
        .find("}else{")
        .unwrap_or_else(|| panic!("`??` must lower to an `if`/`else`; got:\n{e3}"));
    assert!(
        e3[else_pos..].contains("side(__ctx);"),
        "the nullish fallback must be emitted inside its branch; got:\n{e3}"
    );
    check_generated_crate("nullish_call_fallback", &rust);
    let stdout = run_generated_crate("nullish_call_fallback", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

/// R6 (diagnostics): a `??` whose result stays optional cannot be returned from
/// a function declared to return a plain value. The lowering temp is invisible
/// to the `return` check, so the operator reports it itself.
#[test]
fn an_optional_coalescing_result_in_a_non_optional_position_is_reported() {
    let out = compile_module("export function f(a: number | null): number { return a ?? null; }");
    assert!(out.has_errors(), "got {:?}", out.diagnostics);
    assert!(
        out.diagnostics.iter().any(|d| d.code.as_str() == "P0018"
            && d.message.contains("result of `??`")
            && d.message.contains("number | null")),
        "a nullish `??` result returned as `number` must be a P0018 naming the operator; got {:?}",
        out.diagnostics
    );
    // The non-nullish fallback stays a plain value.
    assert!(
        codes("export function f(a: number | null): number { return a ?? 1; }").is_empty(),
        "`a ?? 1` is a plain `number`"
    );
}

/// R6 (known limitation): `a ?? null` over a `string | null` left operand in a
/// `number | null` function is a TypeScript type error, but this frontend does
/// not run an assignability check. What is pinned here is the *lowering*: the
/// operator keeps the left operand's optional representation instead of emitting
/// `()` into a value slot. The generated crate itself does not compile, because
/// the mismatched declared return type is unrelated to `??`.
#[test]
fn a_mismatched_nullish_fallback_is_lowered_without_being_diagnosed() {
    let out =
        compile_module("export function h6(a: string | null): number | null { return a ?? null; }");
    assert!(
        !out.diagnostics
            .iter()
            .any(|d| matches!(d.code.as_str(), "P0021" | "P0019")),
        "`??` itself is representable (`string | null`), so neither P0021 nor P0019 belongs here; \
         got {:?}",
        out.diagnostics
    );
    let rust = out
        .rust_source
        .expect("emit-rust must populate rust_source");
    let norm = normalized(&rust);
    assert!(
        !norm.contains("__tmp2=()"),
        "a nullish fallback must not be assigned as `()` into a value slot; got:\n{rust}"
    );
    assert!(
        norm.contains("Option<ts_aot_runtime::JsString"),
        "`a ?? null` must lower to the left operand's optional representation; got:\n{rust}"
    );
}

/// The right operand of `||=`, `&&=`, `||` and `&&` is evaluated only when the
/// short circuit reaches it. Rust's own operators keep it lazy, but only while
/// lowering the operand emits no *statements*: a statement is appended to the
/// enclosing block and would run before the operator (the `s.length` call used to
/// be evaluated even when the left operand already decided the result). Such an
/// operand is guarded by hand instead.
#[test]
fn a_statement_producing_right_operand_stays_guarded() {
    const OR_SOURCE: &str =
        "export function f(x: boolean, s: string): boolean { x ||= (s.length > 0); return x; }";
    const AND_SOURCE: &str =
        "export function g(x: boolean, s: string): boolean { x &&= (s.length > 0); return x; }";
    for (source, name) in [(OR_SOURCE, "f"), (AND_SOURCE, "g")] {
        let rust = rust_of(source);
        let flat = normalized(body_of(&rust, name));
        let if_pos = flat
            .find("if")
            .unwrap_or_else(|| panic!("the guarded operand needs an `if`; got:\n{flat}"));
        let call_pos = flat
            .find("__ts_aot_string_len")
            .unwrap_or_else(|| panic!("the operand is still evaluated; got:\n{flat}"));
        assert!(
            if_pos < call_pos,
            "the operand's statement must be inside the guarded branch; got:\n{flat}"
        );
    }

    // A right operand whose lowering emits no statements keeps Rust's own
    // operator, which is lazy by itself.
    let rust =
        rust_of("export function f(x: boolean, n: i64): boolean { x ||= (n > 0); return x; }");
    let flat = normalized(body_of(&rust, "f"));
    assert!(
        flat.contains("||") && !flat.contains("if"),
        "a statement-free operand keeps Rust's `||`; got:\n{flat}"
    );
}

/// `||` over a nullable right operand meets in the common numeric type too: the
/// destination holds the left operand's inner value *and* the right operand's, so
/// a truthy fractional left value keeps its fraction (the destination used to be
/// the right operand's `i64`, which truncated `1.5` to `1`).
#[test]
fn a_nullable_right_operand_still_meets_in_the_common_numeric_type() {
    const SOURCE: &str =
        "export function h(x: f64 | null, y: i64 | null): f64 | null { return x || y; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    // `Some`/`None` are called through their full paths: a `let some = Some;`
    // alias would pin its type parameter to the first call's type.
    // JavaScript: a truthy left operand is the result, a falsy one yields the right.
    assert_eq!(
        common_rhs::h(
            &ctx,
            ::core::option::Option::Some(1.5_f64),
            ::core::option::Option::None
        ),
        ::core::option::Option::Some(1.5_f64)
    );
    assert_eq!(
        common_rhs::h(
            &ctx,
            ::core::option::Option::Some(0.0_f64),
            ::core::option::Option::Some(3_i64)
        ),
        ::core::option::Option::Some(3.0_f64)
    );
    assert_eq!(
        common_rhs::h(
            &ctx,
            ::core::option::Option::None,
            ::core::option::Option::None
        ),
        ::core::option::Option::None
    );
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    check_generated_crate("common_rhs", &rust);
    let stdout = run_generated_crate("common_rhs", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

/// The fallback of `??` is typed by the destination, so a literal has to fit it: a
/// string or a bool cannot fill a numeric left-hand side, and an integer literal
/// has to be representable in it (`a ?? 300` over a `u8 | null` would emit an
/// out-of-range Rust literal). A literal used to be accepted unconditionally,
/// because it carries no type of its own.
#[test]
fn an_incompatible_literal_fallback_is_rejected() {
    for source in [
        "export function f(a: number | null): number { return a ?? \"text\"; }",
        "export function f(a: number | null): number { return a ?? true; }",
        "export function f(a: u8 | null): u8 { return a ?? 300; }",
    ] {
        let diagnostics = codes(source);
        assert!(
            diagnostics.iter().any(|code| code == "P0021"),
            "`{source}` must be rejected; got {diagnostics:?}"
        );
    }

    // Controls: a fitting literal, a call the passes cannot type from HIR and the
    // two nullish fallbacks keep their long-standing lowering.
    for source in [
        "export function f(a: number | null): number { return a ?? 1.5; }",
        "export function f(a: u8 | null): u8 { return a ?? 1; }",
        "function g(): number { return 1; } \
         export function f(a: number | null): number { return a ?? g(); }",
        "export function f(a: number | null): number | null { return a ?? null; }",
        "export function f(a: number | null): number | undefined { return a ?? undefined; }",
    ] {
        let diagnostics = codes(source);
        assert!(
            !diagnostics.iter().any(|code| code == "P0021"),
            "`{source}` must keep lowering; got {diagnostics:?}"
        );
    }
}

/// The generated Rust of a class method, which is emitted inside an `impl` block
/// and therefore carries no `pub` (unlike `body_of`'s free functions).
fn method_body_of<'r>(rust: &'r str, name: &str) -> &'r str {
    let start = rust
        .find(&format!("fn {name} <"))
        .unwrap_or_else(|| panic!("the generated crate must contain `{name}`; got:\n{rust}"));
    let tail = &rust[start..];
    let end = tail[1..].find("fn ").map_or(tail.len(), |i| i + 1);
    &tail[..end]
}

/// `??=`, `&&=` and `||=` on an accessor are not a Rust binary operator over the
/// getter's value and the right-hand side: `convert_binop` maps `??` to equality,
/// so the setter used to receive a comparison result (a `bool` for the `f64` value
/// type, which the generated crate could not even build) and the right-hand side
/// ran before the getter's value was inspected. The getter's value now feeds the
/// same MIR-level lowering a plain target uses, so the right-hand side stays in the
/// branch the short circuit guards.
#[test]
fn an_accessor_compound_update_lowers_like_a_plain_target() {
    const SOURCE: &str = r"
class Box {
  v: number | null = null;
  get w(): number | null { return this.v; }
  set w(value: number) { this.v = value; }
  fill(): void { this.w ??= 7; }
  keep(): void { this.w ||= 9; }
  put(value: number): void { this.v = value; }
  value(): number | null { return this.w; }
}
export function filled(): number { const b = new Box(); b.fill(); return b.value() ?? -1; }
export function kept(): number { const b = new Box(); b.put(5); b.keep(); return b.value() ?? -1; }
";
    // A static accessor has no receiver, so the setter call takes the value alone;
    // the lowering itself is the same.
    const STATIC_SOURCE: &str = r"
class S {
  static get w(): number | null { return 5; }
  static set w(value: number) { }
  static fill(): void { S.w ??= 4; }
}
export function g(): void { S.fill(); }
";
    // JavaScript: `null ?? 7` is `7`, and a truthy `5 || 9` keeps the `5`.
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(accessor_compound::filled(&ctx), 7.0);
    assert_eq!(accessor_compound::kept(&ctx), 5.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);

    // `??` is a nullish test, not equality, and the fallback is assigned inside the
    // branch it guards.
    let fill = normalized(method_body_of(&rust, "Box__fill"));
    assert!(
        !fill.contains("as_ref()=="),
        "`??=` must not be lowered as equality; got:\n{fill}"
    );
    let test_pos = fill
        .find("is_some")
        .unwrap_or_else(|| panic!("`??=` must test for nullishness; got:\n{fill}"));
    let literal_pos = fill
        .find("7.0")
        .unwrap_or_else(|| panic!("the fallback must be lowered; got:\n{fill}"));
    assert!(
        test_pos < literal_pos,
        "the fallback must sit inside the nullish branch; got:\n{fill}"
    );

    // `||=` over a nullable accessor keeps its truthiness test inside the unwrap.
    let keep = normalized(method_body_of(&rust, "Box__keep"));
    assert!(
        keep.contains("is_some") && keep.contains("9.0"),
        "`||=` must test the unwrapped value and keep the fallback; got:\n{keep}"
    );

    // The crate has to build (the equality lowering fed a `bool` into the `f64`
    // value type) and to *run*: a mutating accessor borrows its receiver
    // (`&mut self`), so the setter's write reaches the caller's instance -
    // `filled()` reads the `7` its setter stored, and the truthy `kept()` keeps
    // the `5` the test put there.
    check_generated_crate("accessor_compound", &rust);
    let stdout = run_generated_crate("accessor_compound", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");

    let static_rust = rust_of(STATIC_SOURCE);
    let static_fill = normalized(method_body_of(&static_rust, "S__fill"));
    assert!(
        static_fill.contains("is_some")
            && static_fill.contains("4.0")
            && !static_fill.contains("as_ref()=="),
        "a static accessor update lowers the same way; got:\n{static_fill}"
    );
    check_generated_crate("accessor_static_compound", &static_rust);
}

/// A skipped assignment never passes the nullable getter value to the setter.
#[test]
fn an_accessor_logical_update_checks_only_the_rhs_against_its_setter() {
    const NULLISH_RESULT: &str = r"
class Box {
  v: number | null = null;
  get w(): number | null { return this.v; }
  set w(value: number) { this.v = value; }
  drop(): void { this.w &&= 3; }
}
";
    // A setter that takes the nullish value keeps lowering.
    const NULLABLE_SETTER: &str = r"
class Maybe {
  v: number | null = null;
  get w(): number | null { return this.v; }
  set w(value: number | null) { this.v = value; }
  keep(): void { this.w &&= 3; }
}
";
    let diagnostics = codes(NULLISH_RESULT);
    assert!(
        !diagnostics.iter().any(|code| code == "P0018"),
        "the skipped nullish branch does not call the setter; got {diagnostics:?}"
    );

    let diagnostics = codes(NULLABLE_SETTER);
    assert!(
        !diagnostics.iter().any(|code| code == "P0018"),
        "a nullable setter accepts the result; got {diagnostics:?}"
    );
}

/// `null` and `undefined` are falsy, and a `void`-typed operand does reach the
/// truthiness test when both sides are nullish: `u || v` over two `void` values
/// takes the right one. The always-truthy fallback used to answer `true` there
/// (`if true` in the generated Rust), which kept the nullish left operand.
#[test]
fn a_nullish_operand_is_falsy_in_a_logical_expression() {
    const SOURCE: &str = r"
function nop(): void { return; }
export function f(): void { const u: void = nop(); const v: void = nop(); const w: void = u || v; }
";
    let rust = rust_of(SOURCE);
    let flat = normalized(body_of(&rust, "f"));
    assert!(
        !flat.contains("iftrue") && !flat.contains("iffalse"),
        "a constant falsy operand resolves the operator statically, with no branch; got:\n{flat}"
    );
    assert!(
        flat.contains("letw:()=v.clone()"),
        "`||` yields the right operand; got:\n{flat}"
    );
}

/// A nullish *expression* (a call declared `void`/`null`) is as nullish as the
/// literal syntax: `undefined || x` is `x` and the operator must not reject the
/// pair just because no Rust type holds both a `()` and a `number`. The result of
/// `undefined && x` is the `undefined`, stored as `None` - not `Some(())`.
#[test]
fn a_nullish_expression_operand_decides_the_logical_result() {
    const OR_SOURCE: &str = r"
function nop(): void { return; }
export function f(): number { return nop() || 5; }
";
    const AND_SOURCE: &str = r"
function nop(): void { return; }
export function f(): number | undefined { return nop() && 5; }
";
    const RETURN_SOURCE: &str = r"
function nop(): void { return; }
export function f(): number | undefined { return nop(); }
";
    let rust = rust_of(OR_SOURCE);
    let flat = normalized(body_of(&rust, "f"));
    assert!(
        flat.contains("return5.0"),
        "a falsy left operand yields the right one; got:\n{flat}"
    );

    let and_rust = rust_of(AND_SOURCE);
    let and_flat = normalized(body_of(&and_rust, "f"));
    assert!(
        and_flat.contains("Option::None") && !and_flat.contains("Some(())"),
        "a falsy left operand is the result, stored as `None`; got:\n{and_flat}"
    );
    assert!(
        !and_flat.contains("5.0"),
        "the right operand is not evaluated at all; got:\n{and_flat}"
    );
    check_generated_crate("nullish_expression_operand", &and_rust);

    // A `void` value in an optional position is `None`, not `Some(())`, with or
    // without an operator around it.
    let return_rust = rust_of(RETURN_SOURCE);
    let return_flat = normalized(body_of(&return_rust, "f"));
    assert!(
        return_flat.contains("Option::None") && !return_flat.contains("Some(())"),
        "a `void` value is `undefined`, i.e. `None`; got:\n{return_flat}"
    );
    check_generated_crate("nullish_return", &return_rust);
}

/// A method that writes a field of `this` mutates the instance it is called on:
/// the receiver is borrowed (`&mut self`) instead of being taken by value, which
/// wrote into a clone and silently dropped the mutation.
#[test]
fn a_mutating_method_reaches_the_callers_instance() {
    const SOURCE: &str = r"
class C {
  v: number = 0;
  set_(x: number): void { this.v = x; }
  get_(): number { return this.v; }
}
export function f(): number { const c = new C(); c.set_(5); return c.get_(); }
";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(mutating_method::f(&ctx), 5.0);
    println!("ok");
}
"#;
    // A captured instance has no ident to borrow (it lives in the closure
    // environment), so it keeps the by-value receiver instead of emitting a
    // borrow of a local that does not exist.
    const CAPTURED: &str = r"
class C {
  v: number = 0;
  set_(x: number): void { this.v = x; }
  get_(): number { return this.v; }
}
export function f(c: C): number { const g = () => { c.set_(5); }; g(); return c.get_(); }
";
    // A generator method keeps the by-value receiver: its body runs later, so a
    // `&mut self` borrow would have to outlive the generator. The write it makes
    // therefore lands in its own copy of the instance, never in the caller's.
    const GENERATOR: &str = r"
class G {
  v: number = 0;
  *m(): number { this.v = 1; yield 1; }
  get_(): number { return this.v; }
}
export function run(): number { const g = new G(); g.m(); return g.get_(); }
";
    // `g.m()` only builds the generator - its body has not run - so the write it
    // will make is not visible on the instance the caller still holds.
    const GENERATOR_MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(mutating_generator::run(&ctx), 0.0);
    println!("ok");
}
"#;

    let rust = rust_of(SOURCE);
    let flat = normalized(&rust);
    assert!(
        flat.contains("fnC__set_<'a>(&mutself,"),
        "a mutating method borrows its receiver; got:\n{flat}"
    );
    assert!(
        flat.contains("C::C__set_(&mut(c),"),
        "the call site borrows the caller's instance; got:\n{flat}"
    );

    check_generated_crate("mutating_method", &rust);
    let stdout = run_generated_crate("mutating_method", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");

    check_generated_crate("mutating_captured", &rust_of(CAPTURED));

    let generator_rust = rust_of(GENERATOR);
    let generator_flat = normalized(&generator_rust);

    assert!(
        generator_flat.contains("fnG__m<'a>(mutself,"),
        "a generator method keeps the by-value receiver; got:\n{generator_flat}"
    );
    assert!(
        generator_flat.contains("yield_((1asf64))"),
        "`yield 1` is coerced to the generator's `number` yield type; got:\n{generator_flat}"
    );

    // The whole crate builds and runs now that the yielded literal carries the
    // generator's yield type: the method's write happens in its own by-value copy,
    // so it is not visible on the instance the caller still holds.
    check_generated_crate("mutating_generator", &generator_rust);
    let stdout = run_generated_crate("mutating_generator", &generator_rust, GENERATOR_MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

/// A global instance is a place too: `held.set_(5)` has to write the global rather
/// than a clone of it, so the global is emitted as a `static mut` and the call that
/// borrows it sits in an `unsafe` block.
#[test]
fn a_mutating_method_on_a_global_writes_the_global() {
    const GLOBAL_RECEIVER: &str = r"
class Holder {
  v: number = 0;
  set_(x: number): void { this.v = x; }
  get_(): number { return this.v; }
}
let held: Holder = new Holder();
export function via_global(): number { held.set_(5); return held.get_(); }
";
    const GLOBAL_MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(global_receiver::via_global(&ctx), 5.0);
    println!("ok");
}
"#;
    let rust = rust_of(GLOBAL_RECEIVER);
    let flat = normalized(&rust);
    assert!(
        flat.contains("unsafe{Holder::Holder__set_(&mut(held),"),
        "a mutating method borrows the global itself, in an unsafe context; got:\n{flat}"
    );
    check_generated_crate("global_receiver", &rust);
    let stdout = run_generated_crate("global_receiver", &rust, GLOBAL_MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

/// `held?.set_(5)` maps the call over the optional receiver, so the receiver the
/// chain is mapped over has to be a *place*: `held.clone().as_mut()` would take the
/// mutation into a temporary clone and throw it away.
#[test]
fn a_mutating_method_through_an_optional_chain_writes_the_receiver() {
    const SOURCE: &str = r"
class Holder {
  v: number = 0;
  set_(x: number): void { this.v = x; }
  get_(): number { return this.v; }
}
let held: Holder | null = new Holder();
function read_(h: Holder | null): number { return h?.get_() ?? -1; }
export function via_opt(): number { held?.set_(5); return read_(held); }
";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    // `held: Holder | null = new Holder()` is seeded by the module init, so the
    // global is `None` until it runs.
    optional_chain_mutation::__ts_aot_init();
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(optional_chain_mutation::via_opt(&ctx), 5.0);
    println!("ok");
}
"#;
    let rust = crate_rust_of(SOURCE);
    let flat = normalized(&rust);
    assert!(
        flat.contains("unsafe{held.as_mut().map("),
        "the chain borrows the receiver instead of cloning it; got:\n{flat}"
    );
    check_generated_crate("optional_chain_mutation", &rust);
    let stdout = run_generated_crate("optional_chain_mutation", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}
