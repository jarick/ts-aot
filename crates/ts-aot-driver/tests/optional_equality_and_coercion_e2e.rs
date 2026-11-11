//! Backend regressions around optional comparison and optional coercion.
//!
//! Three defects, all of which only show up in the *generated Rust*:
//!
//! * two optional operands whose inner types differ (`number | null` vs
//!   `i64 | null`) lower to `Option<f64> == Option<i64>`, which does not compile
//!   (E0308). The inner-type check used to run only for the strict operators, so
//!   the loose `==`/`!=` spelling silently emitted the mismatched comparison;
//! * a strict comparison against a nullish literal (`x === null`) on an operand
//!   that admits both `null` and `undefined` cannot be decided: both store as one
//!   `Option::None`, so `is_none()` answers `true` for an `undefined` value too.
//!   The HIR→MIR check already reports a *typed* operand (see
//!   `null_equality_e2e`); this file covers the operand whose type is only known
//!   after lowering (a call), where the backend check has to decide it instead of
//!   lowering to `is_none()`;
//! * coercing an *optional* source into an optional target returned the source
//!   untouched, so `Option<i64>` was assigned where `Option<f64>` is required
//!   (E0308). Only the present values need the numeric cast; `None` stays `None`.
//!
//! Every accepted case is compiled with `check_generated_crate` and executed with
//! `run_generated_crate`, so the assertions cover the Rust type and the runtime
//! value, not just the emitted text.

use ts_aot_driver::{CompileOptions, Driver, EmitStage};

fn compile(source: &str) -> ts_aot_driver::DriverOutput {
    Driver::new().compile_source(
        "test.ts",
        source,
        &CompileOptions {
            emit: EmitStage::Rust,
            module: true,
        },
    )
}

/// Every diagnostic as `code: message`, so a test can tell which side of the
/// pipeline reported it - a backend diagnostic keeps its specific code
/// (`P0022`, ...) rather than the generic `E0300`.
fn diagnostic_messages(source: &str) -> String {
    compile(source)
        .diagnostics
        .iter()
        .map(|d| format!("{}: {}", d.code.as_str(), d.message))
        .collect::<Vec<_>>()
        .join("\n")
}

fn rust_of(source: &str) -> String {
    let out = compile(source);
    assert!(
        !out.has_errors(),
        "must compile through the full pipeline; got {:?}",
        out.diagnostics
    );
    out.rust_source
        .expect("emit-rust must populate rust_source")
}

/// Two optional operands with different inner types cannot be compared as one
/// `Option<T>`: the Rust types differ. The diagnostic must appear for all four
/// equality operators, and the matching operator symbol must be in the message.
#[test]
fn differing_inner_types_are_rejected_for_all_equality_operators() {
    for (operator, symbol) in [("==", "=="), ("!=", "!="), ("===", "==="), ("!==", "!==")] {
        let source = format!(
            "export function cmp(a: number | null, b: i64 | null): boolean {{ return a {operator} b; }}"
        );
        let messages = diagnostic_messages(&source);
        assert!(
            messages.contains("P0022"),
            "`{operator}` must report P0022, got:\n{messages}"
        );
        assert!(
            messages.contains(&format!("`{symbol}`")),
            "the message must spell the operator as `{symbol}`, got:\n{messages}"
        );
    }
}

/// The same inner type keeps the ordinary `Option` comparison for loose and
/// strict spelling, and the generated crate compiles.
#[test]
fn the_same_inner_type_keeps_the_plain_optional_comparison() {
    const SOURCE: &str = "\
        export function sameLoose(a: number | null, b: number | null): boolean { return a == b; } \
        export function sameStrict(a: number | null, b: number | null): boolean { return a === b; }";
    let messages = diagnostic_messages(SOURCE);
    assert!(
        messages.is_empty(),
        "the same inner type must stay accepted, got:\n{messages}"
    );
    let rust = rust_of(SOURCE);
    assert!(
        rust.contains("a == b"),
        "the comparison must stay the plain `Option` equality; got:\n{rust}"
    );
    ts_aot_test_support::check_generated_crate("optional_same_inner", &rust);
}

/// A strict comparison against a nullish literal needs the operand to admit one
/// nullish kind. `x: number | null | undefined` admits both, so `x === null` is
/// undecidable and must be reported; the loose spelling `x == null` stays a
/// presence test (it is correct for both kinds), and a single-kind operand keeps
/// the ordinary lowering.
#[test]
fn a_strict_nullish_literal_comparison_needs_one_nullish_kind() {
    const BOTH_STRICT: &str = "\
        export function strictNull(x: number | null | undefined): boolean { return x === null; }";
    // A call carries no type in HIR, so the operand's both-kinds optional is only
    // known once it is lowered - that is the case the backend check decides.
    const BOTH_STRICT_CALL: &str = "\
        export function strictNullCall(): boolean { \
            const h = (): number | null | undefined => 1; return h() === null; }";
    const BOTH_LOOSE: &str = "\
        export function looseNull(x: number | null | undefined): boolean { return x == null; }";
    const SINGLE: &str = "\
        export function strictSingle(x: number | null): boolean { return x === null; }";
    // The typed operand is rejected by the HIR→MIR check (`null_equality_e2e`
    // asserts the same shape against the code field, which the driver only
    // carries in the message for backend diagnostics).
    let messages = diagnostic_messages(BOTH_STRICT);
    assert!(
        messages.contains("compares against a `null`/`undefined` literal"),
        "the HIR→MIR check must reject a typed both-kinds operand, got:\n{messages}"
    );
    let messages = diagnostic_messages(BOTH_STRICT_CALL);
    assert!(
        messages.contains("compares an optional value that allows both"),
        "a *call* operand is only typed after lowering, so the backend check must reject it, \
         got:\n{messages}"
    );
    assert!(
        messages.contains("P0020:"),
        "the backend report must keep the specific code rather than `E0300`, got:\n{messages}"
    );
    let messages = diagnostic_messages(BOTH_LOOSE);
    assert!(
        messages.is_empty(),
        "the loose presence test stays decidable, got:\n{messages}"
    );
    let messages = diagnostic_messages(SINGLE);
    assert!(
        messages.is_empty(),
        "a single-kind operand keeps the ordinary lowering, got:\n{messages}"
    );
    let rust = rust_of(SINGLE);
    ts_aot_test_support::check_generated_crate("optional_single_kind", &rust);
}

/// An optional source with a different numeric inner type must be converted
/// through its present values: `Option<i64>` becomes `Option<f64>` with a
/// `.map(.. as f64)` rather than being returned untouched (E0308) or wrapped in
/// another `Option`.
#[test]
fn an_optional_source_is_converted_through_its_present_values() {
    const SOURCE: &str = "\
        export function conv(a: i64 | null): number | null { const x: number | null = a; return x; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(optional_source_cast::conv(&ctx, Some(4)), Some(4.0));
    assert_eq!(optional_source_cast::conv(&ctx, None), None);
    println!("ok");
}
"#;
    let messages = diagnostic_messages(SOURCE);
    assert!(messages.is_empty(), "must stay accepted, got:\n{messages}");
    let rust = rust_of(SOURCE);
    let norm: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        norm.contains(".map(|__value|((__value)asf64))"),
        "the present values must be cast, and `None` preserved; got:\n{rust}"
    );
    assert!(
        !norm.contains("Some(Some("),
        "the optional source must not be nested in another `Option`; got:\n{rust}"
    );
    ts_aot_test_support::check_generated_crate("optional_source_cast", &rust);
    let stdout = ts_aot_test_support::run_generated_crate("optional_source_cast", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}
