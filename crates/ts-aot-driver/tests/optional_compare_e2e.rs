//! Regression tests for the review findings on optional comparisons and
//! name-based closure captures:
//!
//! * A2: a mixed comparison between `T | null` and a plain value must compare
//!   in the *common* numeric type. Coercing the plain operand to the optional's
//!   inner type truncated `1.5` to `i64`, so `i64EqFloat(Some(1))` answered
//!   `true` although JavaScript says `1 === 1.5` is `false`.
//! * H2: a variable captured through two levels of closure is emitted as a
//!   name-based `MirExpr::Global`/`MirPlace::Global`, which carries no type;
//!   the declared type has to come from the closure environment, otherwise the
//!   write and the read do not compile.

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

/// A2: `i64 | null` compared with `number`/float literals must compare as
/// numbers, not after truncating the float to the optional's inner type.
#[test]
fn optional_integer_compared_with_float_matches_javascript() {
    const SOURCE: &str = "export function i64EqNum(x: i64 | null, k: number): boolean { return x === k; } \
         export function i64EqFloat(x: i64 | null): boolean { return x === 1.5; } \
         export function i64NeFloat(x: i64 | null): boolean { return x !== 1.5; } \
         export function i64EqI64(x: i64 | null, k: i64): boolean { return x === k; } \
         export function numEqI64(g: number | null, k: i64): boolean { return g === k; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    // JavaScript: 1 === 1.5 is false. A truncated `(1.5) as i64` would make it
    // true for x = Some(1).
    assert!(!optional_integer_compared_with_float::i64EqFloat(&ctx, Some(1)));
    assert!(optional_integer_compared_with_float::i64NeFloat(&ctx, Some(1)));
    assert!(!optional_integer_compared_with_float::i64EqNum(&ctx, Some(1), 1.5));
    assert!(optional_integer_compared_with_float::i64EqNum(&ctx, Some(1), 1.0));
    // Integer pairs keep the exact integer comparison.
    assert!(optional_integer_compared_with_float::i64EqI64(&ctx, Some(1), 1));
    assert!(!optional_integer_compared_with_float::i64EqI64(&ctx, Some(2), 1));
    // The other operand order: f64 optional against an i64 plain value.
    assert!(optional_integer_compared_with_float::numEqI64(&ctx, Some(1.0), 1));
    assert!(!optional_integer_compared_with_float::numEqI64(&ctx, Some(1.5), 1));
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let norm: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        !norm.contains("(1.5)asi64") && !norm.contains("(k)asi64"),
        "the float operand must not be truncated to the optional's integer \
         inner type; got:\n{rust}"
    );
    assert!(
        norm.contains("__ts_numasf64"),
        "an integer optional compared with a float must be widened to f64; \
         got:\n{rust}"
    );
    check_generated_crate("optional_integer_compared_with_float", &rust);

    let stdout = run_generated_crate("optional_integer_compared_with_float", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

/// H2: writing *and* reading a variable captured through two closure levels
/// (`mid` returns the closure that uses `x`). The write used to be emitted
/// without the `Option`/numeric coercion of the env field type, and the read
/// had an unknown type, so neither the write nor a mixed comparison compiled.
#[test]
fn two_level_closure_capture_write_and_read() {
    const SOURCE: &str = "export function twoLevelWriteNum(): number { \
           let x: number | null = null; \
           const mid = (): (() => void) => { return (): void => { x = 7; }; }; \
           const inner: () => void = mid(); inner(); return x ?? -1; } \
         export function twoLevelWriteNull(): number { \
           let x: number | null = 5; \
           const mid = (): (() => void) => { return (): void => { x = null; }; }; \
           const inner: () => void = mid(); inner(); return x ?? -1; } \
         export function twoLevelWriteStr(): string { \
           let s: string | null = null; \
           const mid = (): (() => void) => { return (): void => { s = \"deep\"; }; }; \
           const inner: () => void = mid(); inner(); return s ?? \"?\"; } \
         export function twoLevelWriteBool(): boolean { \
           let b: boolean | null = false; \
           const mid = (): (() => void) => { return (): void => { b = true; }; }; \
           const inner: () => void = mid(); inner(); return b ?? false; } \
         export function twoLevelWriteI64(): i64 { \
           let x: i64 | null = null; \
           const mid = (): (() => void) => { return (): void => { x = 7; }; }; \
           const inner: () => void = mid(); inner(); return x ?? -1; } \
         export function twoLevelReadMixed(): boolean { \
           let x: number | null = 3; \
           const mid = (): (() => boolean) => { return (): boolean => { return x === 3; }; }; \
           const inner: () => boolean = mid(); return inner(); } \
         export function twoLevelReadMixedFalse(): boolean { \
           let x: number | null = 1; \
           const mid = (): (() => boolean) => { return (): boolean => { return x === 1.5; }; }; \
           const inner: () => boolean = mid(); return inner(); } \
         export function twoLevelReadNe(): boolean { \
           let x: number | null = 1; \
           const mid = (): (() => boolean) => { return (): boolean => { return x !== 3; }; }; \
           const inner: () => boolean = mid(); return inner(); } \
         export function twoLevelReadMixedStr(): boolean { \
           let s: string | null = \"a\"; \
           const mid = (): (() => boolean) => { return (): boolean => { return s === \"a\"; }; }; \
           const inner: () => boolean = mid(); return inner(); } \
         export function twoLevelI64Float(): boolean { \
           let x: i64 | null = 1; \
           const mid = (): (() => boolean) => { return (): boolean => { return x === 1.5; }; }; \
           const inner: () => boolean = mid(); return inner(); }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host, JsString};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    // `let x = null; ... x = 7; return x ?? -1` -> 7
    assert_eq!(two_level_closure_capture_write_and_read::twoLevelWriteNum(&ctx), 7.0);
    assert_eq!(two_level_closure_capture_write_and_read::twoLevelWriteNull(&ctx), -1.0);
    assert_eq!(
        two_level_closure_capture_write_and_read::twoLevelWriteStr(&ctx),
        JsString::from("deep")
    );
    assert!(two_level_closure_capture_write_and_read::twoLevelWriteBool(&ctx));
    assert_eq!(two_level_closure_capture_write_and_read::twoLevelWriteI64(&ctx), 7);
    // Reads through the same two-level capture must be typed and compared.
    assert!(two_level_closure_capture_write_and_read::twoLevelReadMixed(&ctx));
    assert!(!two_level_closure_capture_write_and_read::twoLevelReadMixedFalse(&ctx));
    assert!(two_level_closure_capture_write_and_read::twoLevelReadNe(&ctx));
    assert!(two_level_closure_capture_write_and_read::twoLevelReadMixedStr(&ctx));
    // JavaScript: 1 === 1.5 is false; no truncation of 1.5 to i64.
    assert!(!two_level_closure_capture_write_and_read::twoLevelI64Float(&ctx));
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let norm: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        norm.contains("__ScopeEnv"),
        "the captured variable must be stored in a ScopeEnv; got:\n{rust}"
    );
    check_generated_crate("two_level_closure_capture_write_and_read", &rust);

    let stdout = run_generated_crate("two_level_closure_capture_write_and_read", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}
