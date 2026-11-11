//! Review fixes in the expression walker (`ts-aot-frontend/src/expr.rs`).
//!
//! R1: `a && b` / `a || b` yield one of their operands, so a literal operand
//! takes the other operand's statically known type when it can represent it. The
//! literal used to be compared as its own default type, which never matched, so
//! `x && 1` over an `i64` local stayed `Error` - and the inferred return type of
//! a closure with no annotation is read from that `Error`, which emitted a
//! fn-value enum whose `__Uninhabited` variant no constructor could produce.
//!
//! R2: `await` and `yield` are call-like suspension points - the continuation
//! (or the caller that resumes the generator) runs arbitrary code - so a
//! narrowing of a variable written by a closure of the same body must be
//! withdrawn exactly like after a plain call.
//!
//! R3: `a?.[i]` over an array whose element may itself be nullish flattens into
//! a single optional: the element's own nullish kinds plus the `undefined` the
//! short-circuit adds, matching `walk_member`'s `a?.b`. Nesting
//! `Option<Option<T>>` reported the element's `null` as a plain `undefined` and
//! let `v === null` lower to `is_none()`, which is also true for `undefined`.

use ts_aot_driver::{CompileOptions, Driver, EmitStage};
use ts_aot_test_support::check_generated_crate;

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

/// R1: `() => x && 1` over an `i64` parameter must be inferred as `i64` from the
/// other operand, not left `Error`. The regression is visible in the generated
/// crate: with `Error` the returned closure's enum stayed `__Uninhabited` while
/// the closure literal was built as a second enum, and `cargo check` failed with
/// `E0308`; a spurious `E0415` warned that the return type could not be inferred.
#[test]
fn a_literal_operand_types_an_inferred_closure_return() {
    const SOURCE: &str = "export function mk(x: i64): () => i64 { return () => x && 1; } \
         export function mkOr(x: i64): () => i64 { return () => x || 1; }";
    let out = compile_module(SOURCE);
    assert!(
        !out.has_errors(),
        "`x && 1` over `i64` must be typed from the other operand; got {:?}",
        out.diagnostics
    );
    assert!(
        !out.diagnostics.iter().any(|d| d.code.as_str() == "E0415"),
        "the closure return type must be inferred from `x && 1`; got {:?}",
        out.diagnostics
    );
    let rust = out
        .rust_source
        .expect("emit-rust must populate rust_source");
    check_generated_crate("literal_operand_closure", &rust);
}

/// R2: a suspension point withdraws the narrowing of a variable that a closure
/// of the same body writes. Without the withdrawal the stale narrowed copy was
/// returned and the program was accepted.
#[test]
fn await_and_yield_withdraw_written_captures() {
    for source in [
        "export async function f(): Promise<number> { let x: number | null = null; \
         const set = () => { x = null; }; x = 1; await 0; return x; }",
        "export function* g(): Generator<number> { let x: number | null = null; \
         const set = () => { x = null; }; x = 1; yield 0; return x; }",
    ] {
        let diagnostics = codes(source);
        assert!(
            diagnostics.iter().any(|c| c == "P0018"),
            "`{source}` must be rejected with P0018 (await/yield may run the closure \
             that writes the captured variable); got {diagnostics:?}"
        );
    }
    // Control: the same bodies without the suspension point keep the narrowing.
    for source in [
        "export async function f(): Promise<number> { let x: number | null = null; \
         const set = () => { x = null; }; x = 1; return x; }",
        "export function* g(): Generator<number> { let x: number | null = null; \
         const set = () => { x = null; }; x = 1; return x; }",
    ] {
        let diagnostics = codes(source);
        assert!(
            !diagnostics.iter().any(|c| c == "P0018"),
            "without `await`/`yield` the narrowing is still sound; got {diagnostics:?}"
        );
    }
}

/// R3: `a?.[i]` over a nullable element is `T | null | undefined`, so a target
/// that only allows `null` is rejected with the accurate kinds in the message,
/// and a strict comparison over both kinds is undecidable. The write target that
/// allows both kinds stays a single `Option`.
#[test]
fn an_optional_element_flattens_into_one_optional() {
    let out = compile_module(
        "export function f(a: (number | null)[] | null, i: number): number | null \
         { return a?.[i]; }",
    );
    let rejection = out
        .diagnostics
        .iter()
        .find(|d| d.code.as_str() == "P0019")
        .unwrap_or_else(|| {
            panic!(
                "`a?.[i]` over a nullable element must be rejected against a `null`-only \
                 target; got {:?}",
                out.diagnostics
            )
        });
    assert!(
        rejection.message.contains("null | undefined"),
        "the rejected value is the flattened `null | undefined`, not a bare `undefined`; \
         got {:?}",
        rejection.message
    );

    let out = compile_module(
        "export function f(a: (number | null)[] | null, i: number): bool \
         { const v = a?.[i]; return v === null; }",
    );
    assert!(
        out.diagnostics.iter().any(|d| d.code.as_str() == "P0020"),
        "`v === null` over the flattened `number | null | undefined` cannot be decided; \
         got {:?}",
        out.diagnostics
    );

    let rust = rust_of(
        "export function f(a: (number | null)[] | null, i: number): number | null | undefined \
         { return a?.[i]; }",
    );
    let flat: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        flat.contains("->Option<f64>{"),
        "`a?.[i]` must flatten to a single `Option<f64>`; got:\n{rust}"
    );
    assert!(
        !flat.contains("Option<Option<"),
        "`a?.[i]` must not nest `Option<Option<_>>`; got:\n{rust}"
    );
    check_generated_crate("optional_index_flatten", &rust);
}
