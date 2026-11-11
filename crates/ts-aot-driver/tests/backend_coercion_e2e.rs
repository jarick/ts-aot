//! Regression tests for two review findings in the backend's expression
//! emission:
//!
//! * `UnaryOp::Neg` had a special emission for literal operands (`-1` against a
//!   float target becomes `-(1.0)`), but every *other* negation fell into the
//!   same arm and was returned unchanged. `return -y` with `y: i64` from a
//!   `number` function therefore emitted `-(y)`, i.e. an `i64` where the `f64`
//!   return type is required (`E0308`). Only a literal operand keeps the
//!   special emission now; a negation of a typed value is cast from the
//!   operand's own type, the same coercion the binary operators use.
//! * `a === b` between two optional operands lowers to Rust's `Option`
//!   equality, where `null` and `undefined` share one `Option::None`. That is
//!   JavaScript's answer only while both sides admit the *same* single nullish
//!   kind; a mismatch (`T | null` vs `T | undefined`) or an operand that admits
//!   both kinds is now rejected instead of silently answering `None == None`.
//!
//! The accepted modules are compiled with `cargo check`
//! (`check_generated_crate`) and then built and run (`run_generated_crate`), so
//! the assertions cover real Rust compilation and real values, not the emitted
//! text alone.

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

fn norm(rust: &str) -> String {
    rust.chars().filter(|c| !c.is_whitespace()).collect()
}

/// All diagnostic messages of a failed compilation, joined for substring
/// assertions.
fn messages(source: &str) -> String {
    let out = compile_module(source);
    assert!(
        out.has_errors(),
        "the comparison must be rejected, not silently lowered; got {:?}",
        out.diagnostics
    );
    out.diagnostics
        .iter()
        .map(|d| format!("{}: {}", d.code.as_str(), d.message))
        .collect::<Vec<_>>()
        .join("\n")
}

/// `-y` where `y: i64` is returned from a `number` function must be cast to
/// `f64`; the negated literal (`-5`) keeps its special emission.
#[test]
fn negated_i64_value_returned_as_number_is_cast_to_f64() {
    const SOURCE: &str = "export function negLocal(): number { let y: i64 = 5; return -y; } \
         export function negParam(y: i64): number { return -y; } \
         export function negLit(): number { return -5; } \
         export function truncFloat(): i64 { return -5.5; } \
         export function negEmbed(a: i64): number { return 1.5 + -a; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(backend_neg_coercion::negLocal(&ctx), -5.0);
    assert_eq!(backend_neg_coercion::negParam(&ctx, 5), -5.0);
    assert_eq!(backend_neg_coercion::negParam(&ctx, -3), 3.0);
    // The literal case keeps the special emission and stays correct.
    assert_eq!(backend_neg_coercion::negLit(&ctx), -5.0);
    // A negated float literal against an integer target truncates towards zero,
    // exactly like the non-negated `let z: i64 = 5.5;` cast.
    assert_eq!(backend_neg_coercion::truncFloat(&ctx), -5);
    // A negation used as an operand of a float binary expression is widened too:
    // 1.5 + (-3) = -1.5.
    assert_eq!(backend_neg_coercion::negEmbed(&ctx, 3), -1.5);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let flat = norm(&rust);
    assert!(
        flat.contains("((-(y))asf64)"),
        "a negated `i64` returned as `number` must be cast to `f64`; got:\n{rust}"
    );
    assert!(
        !flat.contains("return-(y);"),
        "the uncast `-(y)` (an `i64` in an `f64` position) must be gone; got:\n{rust}"
    );
    assert!(
        flat.contains("-(5.0)"),
        "the negated integer literal keeps its special emission; got:\n{rust}"
    );
    assert!(
        flat.contains("((-(5.5))asi64)"),
        "a negated float literal against an integer target is cast; got:\n{rust}"
    );
    assert!(
        flat.contains("(((-(a)))asf64)"),
        "a negation nested in a float binary expression is widened to `f64`; got:\n{rust}"
    );
    check_generated_crate("backend_neg_coercion", &rust);

    let stdout = run_generated_crate("backend_neg_coercion", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

/// Two optionals with the same single nullish kind keep the ordinary `Option`
/// comparison, which matches JavaScript for every input.
#[test]
fn strict_equality_of_two_same_kind_optionals_matches_javascript() {
    const SOURCE: &str = "export function same(a: number | null, b: number | null): boolean { return a === b; } \
         export function sameUndef(a: number | undefined, b: number | undefined): boolean { return a === b; } \
         export function sameNe(a: number | null, b: number | null): boolean { return a !== b; } \
         export function looseSame(a: number | null, b: number | null): boolean { return a == b; } \
         export function looseMixed(a: number | null, b: number | undefined): boolean { return a == b; } \
         export function looseMixedNe(a: number | null, b: number | undefined): boolean { return a != b; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert!(backend_optional_strict_eq::same(&ctx, Some(1.0), Some(1.0)));
    assert!(!backend_optional_strict_eq::same(&ctx, Some(1.0), Some(2.0)));
    // Both sides absent: one `Option::None` per side, so both are `null` here.
    assert!(backend_optional_strict_eq::same(&ctx, None, None));
    assert!(!backend_optional_strict_eq::same(&ctx, Some(1.0), None));
    assert!(!backend_optional_strict_eq::same(&ctx, None, Some(1.0)));
    // The same for `T | undefined`.
    assert!(backend_optional_strict_eq::sameUndef(&ctx, Some(1.0), Some(1.0)));
    assert!(backend_optional_strict_eq::sameUndef(&ctx, None, None));
    assert!(!backend_optional_strict_eq::sameUndef(&ctx, Some(1.0), None));
    // `!==` is the negation of `===`.
    assert!(!backend_optional_strict_eq::sameNe(&ctx, Some(1.0), Some(1.0)));
    assert!(backend_optional_strict_eq::sameNe(&ctx, Some(1.0), None));
    assert!(!backend_optional_strict_eq::sameNe(&ctx, None, None));
    // Loose `==` treats both absent values as equal, like JavaScript.
    assert!(backend_optional_strict_eq::looseSame(&ctx, None, None));
    // Loose `==`/`!=` keeps its lowering even for mismatched kinds: JavaScript's
    // `null == undefined` is `true`, which is what `None == None` already says.
    assert!(backend_optional_strict_eq::looseMixed(&ctx, None, None));
    assert!(!backend_optional_strict_eq::looseMixed(&ctx, Some(1.0), None));
    assert!(backend_optional_strict_eq::looseMixedNe(&ctx, Some(1.0), None));
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let flat = norm(&rust);
    assert!(
        flat.contains("a==b"),
        "two same-kind optionals keep the plain `Option` comparison; got:\n{rust}"
    );
    assert!(
        flat.contains("Option<f64>"),
        "an optional parameter is an `Option<f64>`; got:\n{rust}"
    );
    check_generated_crate("backend_optional_strict_eq", &rust);

    let stdout = run_generated_crate("backend_optional_strict_eq", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

/// `T | null` compared with `T | undefined` is undecidable in the `Option`
/// representation (`null === undefined` is `false` in JavaScript, `None ==
/// None` is `true` in Rust), so it must be reported instead of lowered.
#[test]
fn strict_equality_of_different_nullish_kinds_is_reported() {
    let report = messages(
        "export function mixedKinds(a: number | null, b: number | undefined): boolean { return a === b; }",
    );
    assert!(
        report.contains("P0019"),
        "a nullish-kind mismatch must be reported as P0019; got:\n{report}"
    );
    assert!(
        report.contains("==="),
        "the message must name the operator; got:\n{report}"
    );

    let report = messages(
        "export function neMixed(a: number | null, b: number | undefined): boolean { return a !== b; }",
    );
    assert!(
        report.contains("P0019") && report.contains("!=="),
        "`!==` over mismatched kinds must be reported too; got:\n{report}"
    );
}

/// An operand that admits both kinds (`T | null | undefined`) cannot be decided
/// either: the runtime value may be either nullish kind.
#[test]
fn strict_equality_with_a_both_kinds_operand_is_reported() {
    let report = messages(
        "export function bothKinds(a: number | null | undefined, b: number | null | undefined): boolean { return a === b; }",
    );
    assert!(
        report.contains("P0020"),
        "an operand allowing both nullish kinds must be reported as P0020; got:\n{report}"
    );
}
