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

fn first_call_arg<'a>(rust: &'a str, call: &str) -> &'a str {
    let start = rust
        .find(call)
        .unwrap_or_else(|| panic!("no `{call}` in generated code:\n{rust}"))
        + call.len();
    let mut depth = 1i32;
    for (i, ch) in rust[start..].char_indices() {
        match ch {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => {
                depth -= 1;
                if depth == 0 {
                    return &rust[start..start + i];
                }
            }
            ',' if depth == 1 => return &rust[start..start + i],
            _ => {}
        }
    }
    panic!("unbalanced `{call}` call in generated code:\n{rust}");
}

#[test]
fn negating_an_absent_optional_is_true() {
    const SOURCE: &str = "
export function num(): boolean { const x: number | null = null; return !x; }
export function str(): boolean { const s: string | null = null; return !s; }
export function zero(): boolean { const x: number | null = 0; return !x; }
export function empty(): boolean { const s: string | null = \"\"; return !s; }
export function present(): boolean { const x: number | null = 3; return !x; }
";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert!(negated_optional::num(&ctx));
    assert!(negated_optional::str(&ctx));
    assert!(negated_optional::zero(&ctx));
    assert!(negated_optional::empty(&ctx));
    assert!(!negated_optional::present(&ctx));
    println!("ok");
}
"#;
    let rust = crate_rust_of(SOURCE);
    check_generated_crate("negated_optional", &rust);
    let stdout = run_generated_crate("negated_optional", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn negating_a_literal_uses_its_own_type() {
    let rust = rust_of(
        "export function zero(): boolean { return !0; } \
         export function text(): boolean { return !\"\"; }",
    );
    let zero = body_of(&rust, "zero");
    assert!(
        normalized(zero).contains("!=0"),
        "`!0` must be decided from the literal's own numeric type, not the type-id sentinel; \
         got:\n{zero}"
    );
    let text = body_of(&rust, "text");
    assert!(
        normalized(text).contains("StringLen") || normalized(text).contains("string_len"),
        "`!\"\"` must be decided from the literal's own string type; got:\n{text}"
    );
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

#[test]
fn a_void_fallback_behaves_like_the_undefined_literal() {
    const SOURCE: &str = r"
let hits: number = 0;
function bump(): number { hits = hits + 1; return hits; }
class K { m(): number | undefined { return 3; } }
export function fromUndef(a: number | undefined): number | undefined { const r = a ?? void 0; return r; }
export function fromNull(a: number | null): number | undefined { const r = a ?? void 0; return r; }
export function fromBoth(a: number | null | undefined): number | undefined { const r = a ?? void 0; return r; }
export function fromClass(k: K | undefined): K | undefined { const r = k ?? void 0; return r; }
export function fromString(s: string | undefined): string | undefined { const r = s ?? void 0; return r; }
export function fromPlain(n: number): number { const r = n ?? void 0; return r; }
export function fromCall(a: number | undefined): number | undefined { const r = a ?? void bump(); return r; }
export function classPresent(): K | undefined { return fromClass(new K()); }
export function classAbsent(): K | undefined { return fromClass(undefined); }
export function coNull(a: number | null): boolean { return (a ?? void 0) === null; }
export function coUndef(a: number | null): boolean { return (a ?? void 0) === undefined; }
export function hitsNow(): number { return hits; }
";
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
    // node: `void` yields `undefined`, so `a ?? void 0` is `a` when present and
    // `undefined` otherwise.
    assert_eq!(void_literal::fromUndef(&ctx, none), none);
    assert_eq!(void_literal::fromUndef(&ctx, some(2.0)), some(2.0));
    assert_eq!(void_literal::fromNull(&ctx, none), none);
    assert_eq!(void_literal::fromNull(&ctx, some(3.0)), some(3.0));
    assert_eq!(void_literal::fromBoth(&ctx, none), none);
    assert_eq!(void_literal::fromBoth(&ctx, some(4.0)), some(4.0));
    assert_eq!(void_literal::fromPlain(&ctx, 5.0), 5.0);
    assert!(void_literal::classPresent(&ctx).is_some());
    assert!(void_literal::classAbsent(&ctx).is_none());
    assert_eq!(void_literal::fromCall(&ctx, some(6.0)), some(6.0));
    assert_eq!(
        void_literal::hitsNow(&ctx),
        0.0,
        "the fallback must not run when the left operand holds a value"
    );
    assert_eq!(void_literal::fromCall(&ctx, none), none);
    assert_eq!(
        void_literal::hitsNow(&ctx),
        1.0,
        "`void bump()` must still evaluate `bump()` in the fallback"
    );
    // `a ?? void 0` is `number | undefined` (never `null`), so `=== null` is
    // false and `=== undefined` is presence.
    assert!(!void_literal::coNull(&ctx, none));
    assert!(!void_literal::coNull(&ctx, some(1.0)));
    assert!(void_literal::coUndef(&ctx, none));
    assert!(!void_literal::coUndef(&ctx, some(1.0)));
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let norm = normalized(&rust);
    assert!(
        !norm.contains("=()"),
        "a `void` fallback must lower as `None` and must never be assigned as `()` into a value \
         slot; got:\n{rust}"
    );
    for name in ["fromUndef", "fromNull", "fromBoth"] {
        assert!(
            normalized(body_of(&rust, name)).contains("Option<f64>"),
            "`{name}` must keep an optional destination, whatever the left operand's nullish \
             kinds are; got:\n{}",
            body_of(&rust, name)
        );
    }
    assert!(
        normalized(body_of(&rust, "fromClass")).contains("Option<K>"),
        "a class `T | undefined` left operand keeps an optional destination; got:\n{}",
        body_of(&rust, "fromClass")
    );
    assert!(
        normalized(body_of(&rust, "fromString")).contains("Option<ts_aot_runtime::JsString"),
        "a string `T | undefined` left operand keeps an optional destination; got:\n{}",
        body_of(&rust, "fromString")
    );
    assert!(
        !normalized(body_of(&rust, "fromPlain")).contains("Option<"),
        "a non-optional left operand stays non-optional; got:\n{}",
        body_of(&rust, "fromPlain")
    );
    assert!(
        normalized(body_of(&rust, "coNull")).contains("returnfalse;"),
        "`a ?? void 0` over `number | null` can never be `null`, so `=== null` is `false`; got:\n\
         {rust}"
    );
    assert!(
        normalized(body_of(&rust, "coUndef")).contains("is_none()"),
        "`a ?? void 0` can be `undefined`, so `=== undefined` must test presence; got:\n{rust}"
    );
    check_generated_crate("void_literal", &rust);
    let stdout = run_generated_crate("void_literal", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_void_fallback_is_not_widened_into_a_non_optional_binding() {
    const SOURCE: &str = "let hits: number = 0; \
         function bump(): number { hits = hits + 1; return hits; } \
         class K { m(): number | undefined { return 3; } } \
         export function a(k: K | null): number { let r: number = k?.m() ?? void 0; return r ? 1 : 0; } \
         export function b(k: K | null): number { let r: number = k?.m() ?? void bump(); return r ? 1 : 0; }";
    let out = compile_module(SOURCE);
    assert!(
        out.has_errors(),
        "a `void` fallback may be `undefined`, so the `number` annotations must be rejected; got \
         {:?}",
        out.diagnostics
    );
    let sites: Vec<&str> = out
        .diagnostics
        .errors()
        .filter(|d| d.code.as_str() == "P0018" && d.message.contains("the annotation of `r`"))
        .map(|d| &SOURCE[d.span.start as usize..d.span.end as usize])
        .collect();
    assert_eq!(
        sites,
        ["k?.m() ?? void 0", "k?.m() ?? void bump()"],
        "both annotated bindings must report P0018 at their own fallback expression; got {:?}",
        out.diagnostics
    );
    assert!(
        out.rust_source.is_none(),
        "a rejected annotated void fallback must not emit Rust; got {:?}",
        out.rust_source
    );
}

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

#[test]
fn an_optional_coalescing_result_in_a_non_optional_position_is_reported() {
    const EXPECTED: &str = "an optional value (it may be `null`) is used where a non-optional \
         value is required (the declared return type of this function); the AOT target represents \
         optional values as `Option<T>` and has no implicit nullish-to-value unwrapping. Handle \
         the nullish case first (for example with `x!` after a check, or by widening the target \
         type)";
    let out = compile_module("export function f(a: number | null): number { return a ?? null; }");
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
    assert!(
        codes("export function f(a: number | null): number { return a ?? 1; }").is_empty(),
        "`a ?? 1` is a plain `number`"
    );
}

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

    let rust =
        rust_of("export function f(x: boolean, n: i64): boolean { x ||= (n > 0); return x; }");
    let flat = normalized(body_of(&rust, "f"));
    assert!(
        flat.contains("||") && !flat.contains("if"),
        "a statement-free operand keeps Rust's `||`; got:\n{flat}"
    );
}

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

fn method_body_of<'r>(rust: &'r str, name: &str) -> &'r str {
    let start = rust
        .find(&format!("fn {name} <"))
        .unwrap_or_else(|| panic!("the generated crate must contain `{name}`; got:\n{rust}"));
    let tail = &rust[start..];
    let end = tail[1..].find("fn ").map_or(tail.len(), |i| i + 1);
    &tail[..end]
}

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
    const STATIC_SOURCE: &str = r"
class S {
  static get w(): number | null { return 5; }
  static set w(value: number) { }
  static fill(): void { S.w ??= 4; }
}
export function g(): void { S.fill(); }
";
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

    let keep = normalized(method_body_of(&rust, "Box__keep"));
    assert!(
        keep.contains("is_some") && keep.contains("9.0"),
        "`||=` must test the unwrapped value and keep the fallback; got:\n{keep}"
    );

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

    let return_rust = rust_of(RETURN_SOURCE);
    let return_flat = normalized(body_of(&return_rust, "f"));
    assert!(
        return_flat.contains("Option::None") && !return_flat.contains("Some(())"),
        "a `void` value is `undefined`, i.e. `None`; got:\n{return_flat}"
    );
    check_generated_crate("nullish_return", &return_rust);
}

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
    const CAPTURED: &str = r"
class C {
  v: number = 0;
  set_(x: number): void { this.v = x; }
  get_(): number { return this.v; }
}
export function f(c: C): number { const g = () => { c.set_(5); }; g(); return c.get_(); }
";
    const GENERATOR: &str = r"
class G {
  v: number = 0;
  *m(): number { this.v = 1; yield 1; }
  get_(): number { return this.v; }
}
export function run(): number { const g = new G(); g.m(); return g.get_(); }
";
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
        flat.contains("fnC__set_<'a>(&self,"),
        "a mutating method takes a shared receiver; got:\n{flat}"
    );
    assert!(
        flat.contains("C::C__set_(&(c),"),
        "the call site passes a shared borrow of the caller's instance; got:\n{flat}"
    );
    assert!(
        !flat.contains("__ts_receiver") && !flat.contains("WriteBack"),
        "no mutable receiver capture or write-back guard may be emitted; got:\n{flat}"
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

    check_generated_crate("mutating_generator", &generator_rust);
    let stdout = run_generated_crate("mutating_generator", &generator_rust, GENERATOR_MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

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
    let receiver = first_call_arg(&flat, "Holder__set_(");
    assert!(
        receiver.starts_with("&(") && receiver.contains("get_or_insert_with"),
        "the setter must run on a shared borrow of the lazily initialized global receiver; got \
         receiver `{receiver}` in:\n{flat}"
    );
    assert!(
        !flat.contains("Holder__set_(&mut("),
        "the receiver must never be borrowed mutably; got:\n{flat}"
    );
    assert!(
        !flat.contains("__ts_receiver"),
        "a mutable receiver is no longer captured through a raw address; got:\n{flat}"
    );
    check_generated_crate("global_receiver", &rust);
    let stdout = run_generated_crate("global_receiver", &rust, GLOBAL_MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

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
        flat.contains(
            "{let__ts_recv_opt=(unsafe{held.clone()}).clone();__ts_recv_opt.as_ref().map("
        ),
        "the chain must clone the optional receiver into a local and map a shared borrow over that \
         local instead of taking an exclusive borrow or holding a `Ref` across the callee; \
         got:\n{flat}"
    );
    check_generated_crate("optional_chain_mutation", &rust);
    let stdout = run_generated_crate("optional_chain_mutation", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn negating_nullish_literals_and_nested_not_follow_js_truthiness() {
    const SOURCE: &str = "export function notNull(): boolean { return !null; } \
         export function notUndefined(): boolean { return !undefined; } \
         export function notNotFive(): boolean { return !!5; } \
         export function notNotZero(): boolean { return !!0; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(not_literals::notNull(&ctx), true);
    assert_eq!(not_literals::notUndefined(&ctx), true);
    assert_eq!(not_literals::notNotFive(&ctx), true);
    assert_eq!(not_literals::notNotZero(&ctx), false);
    println!("ok");
}
"#;
    let rust = crate_rust_of(SOURCE);
    check_generated_crate("not_literals", &rust);
    let stdout = run_generated_crate("not_literals", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_nested_optional_fallback_keeps_its_inner_null_allowance() {
    const SOURCE: &str = "class K { m(v: number): number | null { return v > 0 ? v : null; } } \
         export function got(k: K | null, x: number | undefined, v: number): number { \
             const z = x ?? k?.m(v); return z === undefined ? 1 : 2; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(
        nested_fallback_null::got(
            &ctx,
            ::core::option::Option::None,
            ::core::option::Option::None,
            5.0
        ),
        1.0
    );
    assert_eq!(
        nested_fallback_null::got(
            &ctx,
            ::core::option::Option::None,
            ::core::option::Option::Some(5.0),
            5.0
        ),
        2.0
    );
    assert_eq!(
        nested_fallback_null::got(
            &ctx,
            ::core::option::Option::Some(nested_fallback_null::K {}),
            ::core::option::Option::None,
            5.0
        ),
        2.0
    );
    assert_eq!(
        nested_fallback_null::got(
            &ctx,
            ::core::option::Option::Some(nested_fallback_null::K {}),
            ::core::option::Option::None,
            -1.0
        ),
        2.0,
        "`k?.m(-1)` is `null`, which is not `undefined`, so the fallback must not be taken"
    );
    println!("ok");
}
"#;
    let rust = crate_rust_of(SOURCE);
    assert!(
        normalized(&rust).contains("letz:Option<Option<f64>>"),
        "the fallback's nested optional must be preserved with its inner `null` allowance; \
         got:\n{rust}"
    );
    check_generated_crate("nested_fallback_null", &rust);
    let stdout = run_generated_crate("nested_fallback_null", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_chain_call_lhs_with_a_non_optional_fallback_flattens_the_nested_optional() {
    const SOURCE: &str = "class K { m(v: number): number | null { return v > 0 ? v : null; } } \
         export function got(): number { const k: K | null = new K(); \
             const z = k?.m(-1) ?? 9; return z === 9 ? 1 : 0; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(
        chain_call_nested_lhs::got(&ctx),
        1.0,
        "`k?.m(-1)` is `null`, and `??` falls back on `null` too, so `z` is 9"
    );
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let norm = normalized(&rust);
    assert!(
        norm.contains("unwrap().is_some()"),
        "a chain LHS whose value is a nested optional must test the inner layer too, even when \
         the coalesce's inferred type equals the LHS's inner type; got:\n{rust}"
    );
    check_generated_crate("chain_call_nested_lhs", &rust);
    let stdout = run_generated_crate("chain_call_nested_lhs", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_local_read_of_a_nested_optional_is_flattened_for_coalescing() {
    const SOURCE: &str = "class K { m(v: number): number | null { return v > 0 ? v : null; } } \
         export function got(k: K | null, v: number): number { \
             const c = k?.m(v); const z = c ?? 9; \
             return z!; } \
         export function isNull(k: K | null, v: number): boolean { \
             const c = k?.m(v); const z = c ?? 9; return z === null; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    use ::core::option::Option::{None, Some};
    assert_eq!(
        local_nested_lhs::got(&ctx, Some(local_nested_lhs::K {}), -1.0),
        9.0,
        "the local reads the chain's `null`, so `??` must take the 9 fallback"
    );
    assert_eq!(
        local_nested_lhs::got(&ctx, Some(local_nested_lhs::K {}), 5.0),
        5.0,
        "a non-nullish value is kept exactly, not replaced by the fallback or by a branch code"
    );
    assert_eq!(
        local_nested_lhs::got(&ctx, None, 5.0),
        9.0,
        "an absent receiver is `undefined`, which also takes the 9 fallback"
    );
    assert!(
        !local_nested_lhs::isNull(&ctx, Some(local_nested_lhs::K {}), -1.0),
        "the flattened result is 9, never `null`"
    );
    assert!(
        !local_nested_lhs::isNull(&ctx, Some(local_nested_lhs::K {}), 5.0),
        "a non-nullish chain result is kept, never `null`"
    );
    assert!(
        !local_nested_lhs::isNull(&ctx, None, 5.0),
        "an absent receiver falls back to 9, never `null`"
    );
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let norm = normalized(&rust);
    assert!(
        norm.contains("letz:Option<f64>"),
        "a local read whose type is a nested optional must be flattened by its depth for `??`; \
         got:\n{rust}"
    );
    assert!(
        norm.contains("unwrap().is_some()"),
        "the local read's inner `null` layer must be tested as well; got:\n{rust}"
    );
    check_generated_crate("local_nested_lhs", &rust);
    let stdout = run_generated_crate("local_nested_lhs", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn two_nested_optional_chains_coalesce_at_the_same_depth() {
    const SOURCE: &str = "class K { m(v: number): number | null { return v > 0 ? v : null; } } \
         export function got(a: K | null, b: K | null, x: number, y: number): number { \
             const z = a?.m(x) ?? b?.m(y); return z === null ? 1 : 2; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(
        nested_chain_fallback::got(
            &ctx,
            ::core::option::Option::Some(nested_chain_fallback::K {}),
            ::core::option::Option::None,
            5.0,
            0.0
        ),
        2.0
    );
    assert_eq!(
        nested_chain_fallback::got(
            &ctx,
            ::core::option::Option::Some(nested_chain_fallback::K {}),
            ::core::option::Option::Some(nested_chain_fallback::K {}),
            -1.0,
            5.0
        ),
        2.0
    );
    assert_eq!(
        nested_chain_fallback::got(
            &ctx,
            ::core::option::Option::Some(nested_chain_fallback::K {}),
            ::core::option::Option::Some(nested_chain_fallback::K {}),
            -1.0,
            -1.0
        ),
        1.0,
        "both operands are `null`, so the coalescing result is `null`"
    );
    assert_eq!(
        nested_chain_fallback::got(
            &ctx,
            ::core::option::Option::None,
            ::core::option::Option::Some(nested_chain_fallback::K {}),
            0.0,
            -1.0
        ),
        1.0,
        "the left receiver is absent, so the fallback's `null` is the result"
    );
    assert_eq!(
        nested_chain_fallback::got(
            &ctx,
            ::core::option::Option::None,
            ::core::option::Option::None,
            0.0,
            0.0
        ),
        2.0
    );
    println!("ok");
}
"#;
    let out = compile_module(SOURCE);
    assert!(
        !out.diagnostics.iter().any(|d| d.code.as_str() == "P0021"),
        "both operands are nested optionals of the same shape, so no incompatibility belongs \
         here; got {:?}",
        out.diagnostics
    );
    let rust = out
        .rust_source
        .expect("emit-rust must populate rust_source");
    assert!(
        normalized(&rust).contains("letz:Option<Option<f64>>"),
        "the nested chain result must keep the nested representation; got:\n{rust}"
    );
    check_generated_crate("nested_chain_fallback", &rust);
    let stdout = run_generated_crate("nested_chain_fallback", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}
