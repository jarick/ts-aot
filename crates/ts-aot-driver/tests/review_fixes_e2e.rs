//! Regression tests for the issues found in the second review round: the type
//! of a module-level global reference (`??` used to fold silently), compound
//! logical assignment, `Option`-typed global initialisers, spurious warnings,
//! conditional branch typing, capture types inside closures, `Generator<T>`
//! annotations and `Result` propagation from throwing callees.

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

/// R1 + R4: `??` on a module-level global must lower to the fallback, and a
/// nullish global initialiser must become `Option::None`/`Option::Some`.
#[test]
fn global_optionals_coalesce_and_initialise() {
    const SOURCE: &str = "let g: number | null = null; \
         let h: number | null = 5; \
         export function f(): number { return (g ?? 7) + (h ?? 9); }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    // g is null -> 7, h is 5 -> 5.
    assert_eq!(global_optionals::f(&ctx), 12.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let norm: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        norm.contains("is_some()") || norm.contains("Option::None"),
        "the global `??` must be lowered to an optional check; got:\n{rust}"
    );
    check_generated_crate("global_optionals", &rust);

    let stdout = run_generated_crate("global_optionals", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

/// R2 + R3: `??=`, `||=` and `&&=` are short-circuiting assignments, not Rust's
/// operators and not an equality comparison.
#[test]
fn compound_logical_assignment_matches_javascript() {
    // The optional target is a global: a *local* would need narrowing after the
    // compound assignment, which this AOT does not model yet.
    const SOURCE: &str = "let g: number | null = null; \
         let h: number | null = 1; \
         export function nullish(): number { g ??= 5; return g ?? 0; } \
         export function keep(): number { h ??= 5; return h ?? 0; } \
         export function plain(x: number): number { x ??= 5; return x; } \
         export function orDefault(s: string): string { s ||= \"d\"; return s; } \
         export function andKeep(s: string): string { s &&= \"y\"; return s; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(compound_logical::nullish(&ctx), 5.0);
    assert_eq!(compound_logical::keep(&ctx), 1.0);
    assert_eq!(compound_logical::plain(&ctx, 3.0), 3.0);
    let empty = ts_aot_runtime::JsString::from("");
    let text = ts_aot_runtime::JsString::from("a");
    assert_eq!(compound_logical::orDefault(&ctx, empty), ts_aot_runtime::JsString::from("d"));
    assert_eq!(compound_logical::orDefault(&ctx, text.clone()), text);
    assert_eq!(compound_logical::andKeep(&ctx, text.clone()), ts_aot_runtime::JsString::from("y"));
    assert_eq!(compound_logical::andKeep(&ctx, empty), empty);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let norm: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        !norm.contains("==5.0") && !norm.contains("==5"),
        "`??=` must not be lowered as an equality comparison; got:\n{rust}"
    );
    check_generated_crate("compound_logical", &rust);

    let stdout = run_generated_crate("compound_logical", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

/// R5: an absent annotation is not a resolution failure, and `undefined` is the
/// natural value of an optional target.
#[test]
fn unannotated_locals_and_nullish_optionals_do_not_warn() {
    let diagnostics = codes(
        "export function f(): number { const x = 1; let y = 2; y = x; return y; } \
         let g: number | undefined = undefined; \
         export function h(): number { return g ?? 3; }",
    );
    assert!(
        !diagnostics.iter().any(|c| c == "E0400"),
        "an absent annotation must not warn E0400; got {diagnostics:?}"
    );
    assert!(
        !diagnostics.iter().any(|c| c == "E0409"),
        "`undefined` written to an optional target must not warn E0409; got {diagnostics:?}"
    );
}

/// P2 + P3: a conditional branch that is an arithmetic expression over a
/// recursive call, and arithmetic over a captured variable inside a closure.
#[test]
fn conditional_branches_and_captures_are_typed() {
    const SOURCE: &str = "export function fact(n: number): number { \
             return n <= 1 ? 1 : n * fact(n - 1); \
         } \
         export function inc(x: number): number { const g = (): number => x + 1; return g(); }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(typed_branches::fact(&ctx, 5.0), 120.0);
    assert_eq!(typed_branches::inc(&ctx, 41.0), 42.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    check_generated_crate("typed_branches", &rust);

    let stdout = run_generated_crate("typed_branches", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

/// P4: the standard `Generator<T>` spelling declares the yielded type; the
/// dialect's bare `: T` keeps working.
#[test]
fn generator_annotations_accept_wrapper_and_bare_yield_type() {
    for source in [
        "export function* f(x: number): Generator<number> { yield x; }",
        "export function* f(x: number): IterableIterator<number> { yield x; }",
        "export function* f(x: number): number { yield x; }",
    ] {
        let diagnostics = codes(source);
        assert!(
            !diagnostics.iter().any(|c| c == "E0505"),
            "the yield type must be resolved for `{source}`; got {diagnostics:?}"
        );
        let rust = rust_of(source);
        let norm: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
        assert!(
            norm.contains("Generator<f64>"),
            "the generator must yield `f64`; got:\n{rust}"
        );
    }
    let rust = rust_of("export async function* f(x: number): AsyncGenerator<number> { yield x; }");
    let norm: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        norm.contains("AsyncGenerator<f64>"),
        "an async generator must yield `f64`; got:\n{rust}"
    );
}

/// P1: a call to a function that may throw returns a `Result`, and the error is
/// propagated with `?` through the whole call graph (the caller inherits the
/// callee's error type).
#[test]
fn throwing_calls_propagate_through_the_call_graph() {
    const SOURCE: &str = "function boom(): number { throw 1; } \
         function middle(): number { return boom(); } \
         export function pick(n: number): number { if (n > 0) { return 7; } return middle(); }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(throwing_chain::pick(&ctx, 1.0), Ok(7.0));
    assert_eq!(throwing_chain::pick(&ctx, 0.0), Err(1.0));
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let norm: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        norm.contains("Result<f64,f64>"),
        "the throwing chain must be emitted as `Result`; got:\n{rust}"
    );
    assert!(
        norm.contains('?'),
        "the call to a throwing callee must be propagated with `?`; got:\n{rust}"
    );
    check_generated_crate("throwing_chain", &rust);

    let stdout = run_generated_crate("throwing_chain", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

/// P1: the cases this AOT target cannot lower are reported instead of emitting
/// code that cannot compile.
/// A call-valued `throw` operand (`throw boom()`) has no HIR type of its own, so
/// the throws fixpoint resolves it through the callee's declared type before the
/// call graph is walked: the *caller* of that function then propagates the same
/// error type instead of being skipped as `Type::Error`.
#[test]
fn a_call_valued_throw_seeds_the_caller_error_type() {
    let rust = rust_of(
        "function boom(): number { throw 1; } \
         function f(): number { throw boom(); } \
         export function go(): number { return f(); }",
    );
    let norm: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        norm.contains("pubfngo<'a>(__ctx:&ts_aot_runtime::Ctx<'a>,)->Result<f64,f64>"),
        "`go` must propagate boom's thrown `number` through `f`; got:\n{rust}"
    );
    check_generated_crate("call_valued_throw", &rust);
}

#[test]
fn throwing_calls_that_cannot_be_lowered_are_reported() {
    // A callee's error cannot be caught by `try`/`catch`.
    let out = compile_module(
        "function boom(): number { throw 1; } \
         export function f(): number { try { return boom(); } catch (e) { return 0; } }",
    );
    assert!(out.has_errors(), "got {:?}", out.diagnostics);
    assert!(
        out.diagnostics
            .iter()
            .any(|d| d.code.as_str() == "E0416" && d.message.contains("try")),
        "expected E0416 for a throwing call inside `try`; got {:?}",
        out.diagnostics
    );

    // A generator body does not return a `Result`.
    let out = compile_module(
        "function boom(): number { throw 1; } \
         export function* f(): number { yield boom(); }",
    );
    assert!(
        out.diagnostics.iter().any(|d| d.code.as_str() == "E0416"),
        "expected E0416 for a throwing call inside a generator; got {:?}",
        out.diagnostics
    );
}
/// The type of a variable is narrowed by an assignment that cannot be nullish
/// (`x ??= v`, `x ||= v`, `x = v`), so a later use is no longer an optional
/// value - which is what TypeScript does and what the optional-write check
/// (`P0018`) otherwise rejects.
#[test]
fn assignment_narrowing_removes_the_nullish_part() {
    const SOURCE: &str = "export function fromDefault(x: number | null): number { x ??= 5; return x; } \
         export function fromAssign(): number { let x: number | null = null; x = 41; return x + 1; } \
         export function kept(): number { let x: number | null = 1; x ??= 5; return x; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(narrowing::fromDefault(&ctx, ::core::option::Option::None), 5.0);
    assert_eq!(narrowing::fromDefault(&ctx, ::core::option::Option::Some(1.0)), 1.0);
    assert_eq!(narrowing::fromAssign(&ctx), 42.0);
    assert_eq!(narrowing::kept(&ctx), 1.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    check_generated_crate("narrowing", &rust);
    let stdout = run_generated_crate("narrowing", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

/// Narrowing is flow-sensitive: a nullish assignment undoes it, and it must not
/// escape the branch or block that established it - those programs are rejected
/// (`P0018`) instead of mis-typing the variable.
#[test]
fn assignment_narrowing_is_undone_and_does_not_escape() {
    for source in [
        "export function f(): number { let x: number | null = null; x = 5; x = null; return x; }",
        "export function f(c: boolean): number { let x: number | null = null; if (c) { x = 5; } return x; }",
        "export function f(): number { let x: number | null = null; { x = 5; } return x; }",
        "export function f(): number { let x: number | null = null; x &&= 5; return x; }",
    ] {
        let out = compile_module(source);
        assert!(
            out.has_errors(),
            "`{source}` must be rejected; got {:?}",
            out.diagnostics
        );
        assert!(
            out.diagnostics.iter().any(|d| d.code.as_str() == "P0018"),
            "`{source}` must be rejected with P0018 (the variable is still optional); got {:?}",
            out.diagnostics
        );
    }
}
/// P6: an assignment to a captured variable has to write through the shared
/// environment. The write used to emit `Some(())` / `Some(<i64>)` for an
/// `Option<f64>` field - code that cannot compile - so a closure created before
/// the assignment never saw the new value either.
#[test]
fn assignments_to_captured_variables_are_visible_to_closures() {
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(capture_write::numberCapture(&ctx), 5.0);
    assert_eq!(capture_write::optionalCapture(&ctx), 5.0);
    assert_eq!(capture_write::narrowedCapture(&ctx), 7.0);
    assert_eq!(capture_write::stringCaptureOk(&ctx), 1.0);
    assert_eq!(capture_write::optionalStringCaptureOk(&ctx), 1.0);
    println!("ok");
}
"#;
    const SOURCE: &str = "export function numberCapture(): number { let x: number = 1; \
         const g = (): number => x; x = 5; return g(); } \
         export function optionalCapture(): number { let x: number | null = null; \
         const g = (): number => x ?? 7; x = 5; return g(); } \
         export function narrowedCapture(): number { let x: number | null = null; \
         const g = (): number => x ?? 0; x = 5; x = 7; return g(); } \
         export function stringCaptureOk(): number { let s: string = \"a\"; \
         const g = (): string => s; s = \"b\"; if (g() === \"b\") { return 1; } return 0; } \
         export function optionalStringCaptureOk(): number { let s: string | null = null; \
         const g = (): string => s ?? \"d\"; s = \"e\"; if (g() === \"e\") { return 1; } return 0; }";
    let rust = rust_of(SOURCE);
    check_generated_crate("capture_write", &rust);
    let stdout = run_generated_crate("capture_write", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

/// P7: `||=` and `&&=` on an optional target compare the value *inside* the
/// optional by JavaScript truthiness (`null`, `undefined`, `0` and `""` are
/// falsy). Rust's own operators cannot express that, so the lowering unwraps the
/// value in the "present" arm only.
#[test]
fn optional_logical_assignment_matches_javascript() {
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(logical_optional::orAssignNone(&ctx), 5.0);
    assert_eq!(logical_optional::orAssignZero(&ctx), 5.0);
    assert_eq!(logical_optional::orAssignTwo(&ctx), 2.0);
    assert_eq!(logical_optional::andAssignNone(&ctx), 99.0);
    assert_eq!(logical_optional::andAssignZero(&ctx), 0.0);
    assert_eq!(logical_optional::andAssignTwo(&ctx), 3.0);
    assert_eq!(logical_optional::orStringNone(&ctx), 1.0);
    assert_eq!(logical_optional::orStringEmpty(&ctx), 1.0);
    assert_eq!(logical_optional::orStringKept(&ctx), 1.0);
    assert_eq!(logical_optional::andStringEmpty(&ctx), 1.0);
    assert_eq!(logical_optional::andStringPresent(&ctx), 1.0);
    println!("ok");
}
"#;
    const SOURCE: &str = "export function orAssignNone(): number { let x: number | null = null; \
         x ||= 5; return x; } \
         export function orAssignZero(): number { let x: number | null = 0; x ||= 5; return x; } \
         export function orAssignTwo(): number { let x: number | null = 2; x ||= 5; return x; } \
         export function andAssignNone(): number { let x: number | null = null; \
         x &&= 3; return x ?? 99; } \
         export function andAssignZero(): number { let x: number | null = 0; \
         x &&= 3; return x ?? 99; } \
         export function andAssignTwo(): number { let x: number | null = 2; \
         x &&= 3; return x ?? 99; } \
         export function orStringNone(): number { let s: string | null = null; \
         s ||= \"d\"; if (s === \"d\") { return 1; } return 0; } \
         export function orStringEmpty(): number { let s: string | null = \"\"; \
         s ||= \"d\"; if (s === \"d\") { return 1; } return 0; } \
         export function orStringKept(): number { let s: string | null = \"x\"; \
         s ||= \"d\"; if (s === \"x\") { return 1; } return 0; } \
         export function andStringEmpty(): number { let s: string | null = \"\"; \
         s &&= \"d\"; if (s === \"\") { return 1; } return 0; } \
         export function andStringPresent(): number { let s: string | null = \"x\"; \
         s &&= \"d\"; if (s === \"d\") { return 1; } return 0; }";
    let rust = rust_of(SOURCE);
    check_generated_crate("logical_optional", &rust);
    let stdout = run_generated_crate("logical_optional", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}
/// B1: a narrowing must not survive the block/branch/`try` that set it - the
/// variable's own storage can be written back to a nullish value there, and the
/// rest of the function must see that.
#[test]
fn narrowing_does_not_escape_its_block_and_calls_withdraw_it() {
    for source in [
        "export function f(c: boolean): number { let x: number | null = null; x = 5; if (c) { x = null; } return x; }",
        "export function f(c: boolean): number { let x: number | null = null; x = 5; { x = null; } return x; }",
        "export function f(c: boolean): number { let x: number | null = null; x = 5; while (c) { x = null; } return x; }",
        "export function f(c: number): number { let x: number | null = null; switch (c) { case 1: x = 5; break; case 2: return x; default: return 0; } }",
        "export function f(c: boolean): number { let x: number | null = null; if (c) x = 5; return x; }",
        "export function f(): number { let x: number | null = null; x = 5; const g = (): number => { x = null; return 0; }; g(); return x; }",
    ] {
        let out = compile_module(source);
        assert!(
            out.has_errors(),
            "`{source}` must be rejected; got {:?}",
            out.diagnostics
        );
        assert!(
            out.diagnostics.iter().any(|d| d.code.as_str() == "P0018"),
            "`{source}` must be rejected with P0018; got {:?}",
            out.diagnostics
        );
    }
}

/// B2: `a || b` with two optional operands compares the value *inside* the
/// left one, so `Some(0)`/`None` fall through to `b`.
#[test]
fn both_optional_logical_operands_match_javascript() {
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(both_optional::f(&ctx, ::core::option::Option::None, ::core::option::Option::Some(3.0)), ::core::option::Option::Some(3.0));
    assert_eq!(both_optional::f(&ctx, ::core::option::Option::Some(0.0), ::core::option::Option::Some(3.0)), ::core::option::Option::Some(3.0));
    assert_eq!(both_optional::f(&ctx, ::core::option::Option::Some(2.0), ::core::option::Option::Some(3.0)), ::core::option::Option::Some(2.0));
    println!("ok");
}
"#;
    const SOURCE: &str = "export function f(x: number | undefined, y: number | undefined): number | undefined { return x || y; }";
    let rust = rust_of(SOURCE);
    check_generated_crate("both_optional", &rust);
    let stdout = run_generated_crate("both_optional", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

/// F1/F2 (backend): a write to a captured optional from *inside* the closure is
/// coerced to the capture's declared type, and `=== <numeric literal>` on an
/// optional compares against the value inside it.
#[test]
fn captured_writes_from_closures_and_literal_comparisons_are_coerced() {
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(coerced::writeFromClosure(&ctx), 5.0);
    assert_eq!(coerced::isZero(&ctx, ::core::option::Option::Some(0.0)), 1.0);
    assert_eq!(coerced::isZero(&ctx, ::core::option::Option::Some(2.0)), 0.0);
    assert_eq!(coerced::isZero(&ctx, ::core::option::Option::None), 0.0);
    println!("ok");
}
"#;
    const SOURCE: &str = "export function writeFromClosure(): number { \
         let x: number | null = 0; const set = (): void => { x = 5; }; set(); return x ?? -1; } \
         export function isZero(g: number | null): number { if (g === 0) { return 1; } return 0; }";
    let rust = rust_of(SOURCE);
    check_generated_crate("coerced", &rust);
    let stdout = run_generated_crate("coerced", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

/// B4/M3: a method that throws only through a call is marked throwing (no false
/// `E0416`), and the generated crate must actually compile (`!has_errors()` only
/// checks the passes; the emitted `?` used to be rejected by rustc).
#[test]
fn throwing_methods_chains_and_catch_calls_are_accepted() {
    const CHAIN: &str = "class A { m(): number { throw 1; } n(): number { return this.m(); } } \
         export function go(): number { const a = new A(); return a.n(); }";
    let out = compile_module(CHAIN);
    assert!(
        !out.has_errors(),
        "`{CHAIN}` must compile; got {:?}",
        out.diagnostics
    );
    let rust = out
        .rust_source
        .expect("emit-rust must populate rust_source");
    check_generated_crate("throwing_methods_chain", &rust);

    // A3: the `catch` body is emitted inside `catch_unwind`, where `?` cannot
    // leave the enclosing function, so a call to a throwing callee there is a
    // reported limitation (`E0416`) instead of silently emitted, non-compiling
    // Rust (it used to be `inner(__ctx)?` inside the closure).
    let out = compile_module(
        "function inner(): number { throw 1; } \
         export function f(): number { try { throw 2; } catch (e) { return inner(); } }",
    );
    assert!(
        out.has_errors(),
        "a throwing call inside `catch` cannot propagate; got {:?}",
        out.diagnostics
    );
    assert!(
        out.diagnostics
            .iter()
            .any(|d| d.code.as_str() == "E0416" && d.message.contains("try")),
        "expected E0416 for a throwing call inside `catch`; got {:?}",
        out.diagnostics
    );
}

/// A3: `finally` is emitted *outside* the `catch_unwind` closure, so a throwing
/// call there still propagates with `?` and the generated crate must compile.
#[test]
fn throwing_calls_inside_finally_are_propagated() {
    const MAIN_RETURN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    // `finally { return inner(); }` runs last, and `inner` throws 1.
    assert_eq!(finally_return::f(&ctx), Err(1.0));
    println!("ok");
}
"#;
    const MAIN_THROW: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    // `try {} finally { throw 1; } return 1;` throws from `finally`.
    assert_eq!(finally_throw::f(&ctx), Err(1.0));
    println!("ok");
}
"#;
    for (package, source, main) in [
        (
            "finally_return",
            "function inner(): number { throw 1; } \
             export function f(): number { try { } finally { return inner(); } }",
            MAIN_RETURN,
        ),
        (
            "finally_throw",
            "export function f(): number { try { } finally { throw 1; } return 1; }",
            MAIN_THROW,
        ),
    ] {
        let out = compile_module(source);
        assert!(
            !out.has_errors(),
            "`{source}` must compile; got {:?}",
            out.diagnostics
        );
        let rust = out
            .rust_source
            .expect("emit-rust must populate rust_source");
        let norm: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
        assert!(
            norm.contains("Result<f64,f64>"),
            "the function must be emitted as a `Result`; got:\n{rust}"
        );
        check_generated_crate(package, &rust);
        let stdout = run_generated_crate(package, &rust, main);
        assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
    }
}

/// B1: a `throw` whose value is a call the frontend typed as `Type::Error` (a
/// throwing callee, an unresolved generic, ...) is not a second error type: it
/// must not produce a false `E0417`, and the emitted crate must compile.
#[test]
fn dynamic_first_throws_do_not_conflict() {
    for source in [
        // The only statically known throw is `throw 1`.
        "function inner(): number { return 1; } \
         export function f(x: number): number { if (x > 0) { throw inner(); } throw 1; }",
        // `inner` throws too: `throw inner()` propagates its `Result`.
        "function inner(): number { throw 1; } \
         export function f(x: number): number { if (x > 0) { throw inner(); } throw 1; }",
        // The thrown value is a call the frontend typed as `Error`: the callee's
        // declared return type (`number`) is the error type.
        "function g(): number { return 1; } export function f(): number { throw g(); }",
    ] {
        let out = compile_module(source);
        assert!(
            !out.has_errors(),
            "`{source}` must compile; got {:?}",
            out.diagnostics
        );
        assert!(
            !out.diagnostics
                .iter()
                .any(|d| matches!(d.code.as_str(), "E0417" | "E0416")),
            "`{source}` must not report a throw-type conflict; got {:?}",
            out.diagnostics
        );
        let rust = out
            .rust_source
            .expect("emit-rust must populate rust_source");
        check_generated_crate("dynamic_throw", &rust);
    }
}
/// B5: `E0416`/`E0417` must name the type the way the user wrote it (`number`,
/// `string`, not `F64`/`String`) and point at the offending call/`throw`, not at
/// `0..0`.
#[test]
fn throw_diagnostics_have_readable_names_and_real_spans() {
    const SOURCE: &str = "function boom(): number { throw 1; } \
         export function f(): number { try { return boom(); } catch (e) { return 0; } }";
    // B5 (constructor): the message must name the class, not the mangled
    // `A::constructor` module function, and carry the `new` expression's span.
    const CTOR: &str = "class A { x: number; constructor() { this.x = 1; throw 1; } } \
         export function make(): number { try { const a = new A(); return a.x; } catch (e) { return 0; } }";
    let out = compile_module(SOURCE);
    let diag = out
        .diagnostics
        .iter()
        .find(|d| d.code.as_str() == "E0416")
        .unwrap_or_else(|| panic!("expected E0416; got {:?}", out.diagnostics));
    assert!(
        diag.message.contains("may throw `number`"),
        "E0416 must name the type as `number`; got: {}",
        diag.message
    );
    assert!(
        !diag.message.contains("F64"),
        "E0416 must not leak `F64`; got: {}",
        diag.message
    );
    assert!(
        diag.span.end > diag.span.start,
        "E0416 must carry the call's span, got {:?}",
        diag.span
    );
    let site = &SOURCE[diag.span.start as usize..diag.span.end as usize];
    assert_eq!(
        site, "boom",
        "E0416 must point at the `boom()` call; got {site:?}"
    );

    let out = compile_module(CTOR);
    let diag = out
        .diagnostics
        .iter()
        .find(|d| d.code.as_str() == "E0416")
        .unwrap_or_else(|| panic!("expected a constructor E0416; got {:?}", out.diagnostics));
    assert!(
        diag.message.contains("constructor of `A`"),
        "E0416 must name the class `A`; got: {}",
        diag.message
    );
    assert!(
        !diag.message.contains("A::constructor"),
        "E0416 must not print the mangled `A::constructor`; got: {}",
        diag.message
    );
    assert!(
        diag.span.end > diag.span.start,
        "the constructor E0416 must carry the `new` expression's span, got {:?}",
        diag.span
    );
    let site = &CTOR[diag.span.start as usize..diag.span.end as usize];
    assert_eq!(
        site, "A",
        "the constructor E0416 must point at `new A()`; got {site:?}"
    );
}
/// B3: the AOT target keeps one error type per function, so a function that
/// throws two different primitive types is rejected instead of emitting a
/// `Result` whose `Err` arms cannot both type-check.
#[test]
fn mixed_throw_types_are_rejected() {
    const SOURCE: &str =
        "export function f(x: number): number { if (x > 0) { throw 1; } throw \"boom\"; }";
    let out = compile_module(SOURCE);
    assert!(
        out.has_errors(),
        "mixed throws must be rejected; got {:?}",
        out.diagnostics
    );
    let diag = out
        .diagnostics
        .iter()
        .find(|d| d.code.as_str() == "E0417")
        .unwrap_or_else(|| {
            panic!(
                "mixed throws must be reported as E0417; got {:?}",
                out.diagnostics
            )
        });
    assert!(
        diag.message.contains("`number`") && diag.message.contains("`string`"),
        "E0417 must name the types as `number`/`string`; got: {}",
        diag.message
    );
    assert!(
        !diag.message.contains("Some(") && !diag.message.contains("F64"),
        "E0417 must not print internal `Option`/`Type` names; got: {}",
        diag.message
    );
    assert!(
        diag.span.end > diag.span.start,
        "E0417 must carry the offending `throw`'s span, got {:?}",
        diag.span
    );
    let site = &SOURCE[diag.span.start as usize..diag.span.end as usize];
    assert_eq!(
        site, "\"boom\"",
        "E0417 must point at the second `throw`; got {site:?}"
    );
}

/// B3: an optional argument passed to a generic parameter is legal TypeScript
/// (`T` is inferred with the nullish part), so it must not be a false `P0018`:
/// the empty `NullishKinds` of a generic parameter mean "unknown", not "not
/// optional".
#[test]
fn optional_arguments_to_generic_parameters_are_accepted() {
    const SOURCE: &str = "function id<T>(x: T): T { return x; } \
         export function f(a: number | undefined): number | undefined { return id(a); }";
    let out = compile_module(SOURCE);
    assert!(
        !out.has_errors(),
        "`id(a)` with an optional `a` must compile; got {:?}",
        out.diagnostics
    );
    assert!(
        !out.diagnostics.iter().any(|d| d.code.as_str() == "P0018"),
        "an optional argument to a generic parameter must not be rejected; got {:?}",
        out.diagnostics
    );
    // The emitted crate is *not* checked here: `id<T>` is not monomorphized yet,
    // so the backend emits an undeclared `__generic0` type (`E0425`) - a
    // separate defect of the monomorphize/backend path, not of this check.
    let rust = out
        .rust_source
        .expect("emit-rust must populate rust_source");
    assert!(
        rust.contains("id"),
        "the call must be emitted; got:\n{rust}"
    );
}

/// B3 must not weaken the real checks: a concrete non-optional target still
/// rejects an optional value and a bare `undefined` literal.
#[test]
fn optional_arguments_to_concrete_targets_are_still_rejected() {
    for source in [
        "function need(x: number): number { return x; } \
         export function g(v: number | undefined): number { return need(v); }",
        "function need(x: number): number { return x; } \
         export function g(): number { return need(undefined); }",
    ] {
        let out = compile_module(source);
        assert!(
            out.diagnostics.iter().any(|d| d.code.as_str() == "P0018"),
            "`{source}` must still be rejected with P0018; got {:?}",
            out.diagnostics
        );
    }
}

/// An unannotated local and a `return` must take the type of the *converted*
/// value: `g() || 5` yields a non-optional `number`, but its HIR node is still
/// typed `number | undefined` (and a call's HIR type is `Error`). The local used
/// to be declared `Option<f64>` while holding an `f64` (`let r: Option<f64> =
/// Some(__tmp)`), and returning an optional local from a `-> number` function
/// emitted `return Option<f64>` with no diagnostic.
#[test]
fn local_and_return_types_come_from_the_converted_value() {
    const SOURCE: &str = "function g(): number | undefined { return undefined; } \
         export function f(): number { const r = g() || 5; return r; } \
         export function viaReturn(): number { return g() || 5; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    // `g()` is `undefined`, so `|| 5` yields 5.
    assert_eq!(mir_local_ty::f(&ctx), 5.0);
    assert_eq!(mir_local_ty::viaReturn(&ctx), 5.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let norm: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        !norm.contains("letr:Option<f64>"),
        "the local over `g() || 5` must be `f64`, not `Option<f64>`; got:\n{rust}"
    );
    check_generated_crate("mir_local_ty", &rust);
    let stdout = run_generated_crate("mir_local_ty", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");

    // `g()` may be `undefined`, so returning it from `-> number` is a genuine
    // mismatch: it must be diagnosed instead of emitting `return Option<f64>`.
    let out = compile_module(
        "function g(): number | undefined { return undefined; } \
         export function h(): number { const r = g(); return r; }",
    );
    assert!(
        out.diagnostics.iter().any(|d| d.code.as_str() == "P0018"),
        "returning an optional value from a non-optional function must be P0018; got {:?}",
        out.diagnostics
    );
}

/// B5/M2: a throwing constructor makes its caller throwing (the `Result` is
/// propagated, not dropped), and a method call is attributed to the receiver's
/// class even when another class declares a method with the same name.
#[test]
fn throwing_constructors_propagate_and_methods_resolve_by_class() {
    let source = "class A { x: number; constructor() { this.x = 1; throw 1; } } \
         export function make(): number { const a = new A(); return a.x; }";
    let rust = rust_of(source);
    assert!(
        rust.contains("Result < f64 , f64 >") || rust.contains("Result<f64, f64>"),
        "the caller of a throwing constructor must return a Result; got:\n{rust}"
    );
    assert!(
        rust.contains("make"),
        "the caller must be emitted; got:\n{rust}"
    );

    let same_name = "class A { m(): number { throw 1; } } \
         class B { m(): number { throw \"boom\"; } } \
         export function go(): number { const b = new B(); return b.m(); }";
    let out = compile_module(same_name);
    assert!(
        !out.has_errors(),
        "same-named methods of different classes must not be confused; got {:?}",
        out.diagnostics
    );
    let rust = out.rust_source.expect("rust_source");
    assert!(
        rust.contains("JsString"),
        "`go` must propagate B::m's error type (a string); got:\n{rust}"
    );
}

/// A free function must not inherit the error type of a same-named *method*: the
/// fixpoint's `::name` suffix fallback exists for method calls whose receiver
/// class is not known statically, not for free-function calls. `caller` used to
/// be emitted as `Result<f64, f64>` because `C::m` throws.
#[test]
fn a_free_function_does_not_inherit_a_same_named_methods_error_type() {
    const SOURCE: &str = "class C { m(): number { throw 1; } } \
         function m(): number { return 1; } \
         export function caller(): number { return m(); }";
    let rust = rust_of(SOURCE);
    let norm: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        norm.contains("pubfncaller<'a>(__ctx:&ts_aot_runtime::Ctx<'a>,)->f64"),
        "`caller` calls the free `m`, which cannot throw; got:\n{rust}"
    );
    check_generated_crate("free_vs_method", &rust);
}
