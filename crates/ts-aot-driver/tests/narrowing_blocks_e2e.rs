//! Regression tests for the flow-sensitive narrowing defects found in the first
//! review round (`.review/rev1`).
//!
//! The narrowed binding is a *copy* of the variable taken when the assignment
//! guarantees it is not nullish, so:
//!
//! * a braceless body must withdraw that narrowing and write the variable's
//!   storage (it may not *establish* a new one), while a `switch` case may
//!   establish one for the rest of *that* case, which is discarded before the
//!   next case is walked;
//! * a nested block that declares a shadowing variable must keep its own
//!   storage, not the outer variable's;
//! * a call on the right-hand side of an assignment must not steal the target
//!   that was already walked;
//! * only a closure that *writes* a captured variable may withdraw a narrowing;
//! * an optional call (`o.m?.()`, `o?.m()`) or a tagged template is a call too
//!   and must withdraw it as well;
//! * the generated `_narrowed_N` binding must avoid every name the body binds,
//!   including the ones declared later inside a nested block.

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

/// A4: an assignment inside a braceless body (`if`/`while`/`do`) may not create
/// a narrowing, and one a `switch` case creates ends with that case, but either
/// of them must still withdraw the narrowing in effect and write the variable's
/// *storage* - otherwise the narrowed copy is assigned (invalid Rust:
/// `_narrowed_0 = ()`) and a later read returns a stale value.
#[test]
fn braceless_bodies_and_switch_cases_write_the_storage() {
    const SOURCE: &str = "export function fromIf(c: boolean): number | null { \
             let x: number | null = null; x = 2; if (c) x = null; return x; } \
         export function fromWhile(c: boolean): number | null { \
             let x: number | null = null; x = 1; while (c) x = null; return x; } \
         export function fromDoWhile(): number | null { \
             let x: number | null = null; x = 1; do x = null; while (false); return x; } \
         export function fromSwitch(c: i64): number | null { \
             let x: number | null = null; x = 1; \
             switch (c) { case 0: x = null; break; } return x; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(braceless::fromIf(&ctx, true), ::core::option::Option::None);
    assert_eq!(braceless::fromIf(&ctx, false), ::core::option::Option::Some(2.0));
    // `c` is false: the body never runs, so the storage keeps the narrowed value.
    assert_eq!(braceless::fromWhile(&ctx, false), ::core::option::Option::Some(1.0));
    assert_eq!(braceless::fromDoWhile(&ctx), ::core::option::Option::None);
    assert_eq!(braceless::fromSwitch(&ctx, 0), ::core::option::Option::None);
    assert_eq!(braceless::fromSwitch(&ctx, 1), ::core::option::Option::Some(1.0));
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    check_generated_crate("braceless", &rust);
    let stdout = run_generated_crate("braceless", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

/// A4: the braceless assignment must reach the storage that a call reads later.
/// `read()` only *reads* `x`, so it does not withdraw anything; `x = 2` inside
/// the braceless body must be visible through the storage afterwards.
#[test]
fn braceless_assignment_is_visible_to_a_closure() {
    const SOURCE: &str = "export function f(c: boolean): number { \
             let x: number | null = null; const read = () => x; x = 1; \
             if (c) x = 2; read(); return x!; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(braceless_storage::f(&ctx, true), 2.0);
    assert_eq!(braceless_storage::f(&ctx, false), 1.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    check_generated_crate("braceless_storage", &rust);
    let stdout = run_generated_crate("braceless_storage", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

/// A5: a nested block may declare a shadowing variable. The shadowing
/// assignment must write the *inner* local, never the outer variable's storage
/// (which the narrowing map used to redirect it to by name).
#[test]
fn shadowing_declaration_in_a_nested_block_keeps_its_own_storage() {
    const SOURCE: &str = "export function shadowingOptional(): number { \
             let x: number | null = 1; x = 2; \
             { let x: string = \"ab\"; x = \"c\"; return x.length; } } \
         export function shadowingPlain(): number { \
             let x: number = 1; x = 2; \
             { let x: string = \"ab\"; x = \"c\"; return x.length; } } \
         export function shadowingWithoutAssignment(): number { \
             let x: number | null = 1; x = 2; \
             { let x: string = \"ab\"; return x.length; } }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(shadowing::shadowingOptional(&ctx), 1.0);
    assert_eq!(shadowing::shadowingPlain(&ctx), 1.0);
    assert_eq!(shadowing::shadowingWithoutAssignment(&ctx), 2.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    check_generated_crate("shadowing", &rust);
    let stdout = run_generated_crate("shadowing", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

/// A5: a closure over a shadowing variable captures the *inner* variable; the
/// assignment that follows must not be redirected to the outer storage either.
#[test]
fn closure_over_a_shadowing_variable_captures_the_inner_one() {
    const SOURCE: &str = "export function f(): number { \
             let x: number | null = 1; x = 2; \
             { let x: string = \"ab\"; const g = (): string => x; x = \"c\"; \
               if (g() === \"c\") { return 1; } return 0; } }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(shadowing_capture::f(&ctx), 1.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    check_generated_crate("shadowing_capture", &rust);
    let stdout = run_generated_crate("shadowing_capture", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

/// A6: the left-hand side of an assignment is walked before its right-hand
/// side. When the right-hand side is a call that withdraws a narrowing, the
/// target that was already bound to the narrowed copy must still be redirected
/// to the storage - otherwise the storage keeps its old value.
#[test]
fn a_call_on_the_right_hand_side_does_not_steal_the_target() {
    const SOURCE: &str = "function id(v: number): number { return v; } \
         function bump(): number { return 7; } \
         export function afterUnrelatedCall(): number { \
             let x: number | null = null; const read = () => x; x = 1; \
             x = id(2); return x ?? 0; } \
         export function afterWritingCaptureCall(): number | null { \
             let x: number | null = null; const set = () => { x = null; }; x = 1; \
             x = bump(); return x; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(mid_walk::afterUnrelatedCall(&ctx), 2.0);
    assert_eq!(
        mid_walk::afterWritingCaptureCall(&ctx),
        ::core::option::Option::Some(7.0)
    );
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    check_generated_crate("mid_walk", &rust);
    let stdout = run_generated_crate("mid_walk", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

/// B6: only a closure that *writes* a captured variable may withdraw a
/// narrowing. A closure that only reads it cannot make the narrowing stale, so
/// an unrelated call must not reject the program with `P0018`.
#[test]
fn read_only_captures_do_not_withdraw_narrowings() {
    const SOURCE: &str = "function g(): number { return 1; } \
         export function unrelatedCall(): number { \
             let x: number | null = null; const read = () => x; x = 1; g(); return x; } \
         export function calledReader(): number { \
             let x: number | null = null; const read = () => x; x = 1; g(); read(); return x; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(read_only::unrelatedCall(&ctx), 1.0);
    assert_eq!(read_only::calledReader(&ctx), 1.0);
    println!("ok");
}
"#;
    let diagnostics = codes(SOURCE);
    assert!(
        !diagnostics.iter().any(|c| c == "P0018"),
        "a read-only capture cannot withdraw a narrowing; got {diagnostics:?}"
    );
    let rust = rust_of(SOURCE);
    check_generated_crate("read_only", &rust);
    let stdout = run_generated_crate("read_only", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

/// B6 (control): a closure that *writes* the captured variable really can make
/// the narrowing stale, so calling it must still be rejected - and a later
/// assignment re-narrows the variable, which is sound again.
#[test]
fn written_captures_still_withdraw_narrowings() {
    const WRITE_THROUGH_CALL: &str = "export function f(): number { let x: number | null = null; \
         const setNull = () => { x = null; }; x = 1; setNull(); return x; }";
    // The write may be performed by a closure nested inside the called one.
    const WRITE_THROUGH_NESTED_CLOSURE: &str = "export function f(): number { let x: number | null = null; \
         const outer = () => { const inner = () => { x = null; }; inner(); }; \
         x = 1; outer(); return x; }";
    const SOURCE: &str = "export function f(): number { let x: number | null = null; \
         const setNull = () => { x = null; }; x = 1; setNull(); x = 2; return x ?? 0; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    // `setNull()` sets `x` back to `null`, then `x = 2` narrows it again.
    assert_eq!(written_capture::f(&ctx), 2.0);
    println!("ok");
}
"#;

    let diagnostics = codes(WRITE_THROUGH_CALL);
    assert!(
        diagnostics.iter().any(|c| c == "P0018"),
        "a call of a closure that writes the captured variable must withdraw the \
         narrowing and be rejected; got {diagnostics:?}"
    );

    let diagnostics = codes(WRITE_THROUGH_NESTED_CLOSURE);
    assert!(
        diagnostics.iter().any(|c| c == "P0018"),
        "a write performed inside a nested closure must count as a write of the \
         capture; got {diagnostics:?}"
    );

    let rust = rust_of(SOURCE);
    check_generated_crate("written_capture", &rust);
    let stdout = run_generated_crate("written_capture", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

/// B7: an optional call (`o.m?.()`, `o?.m()`) and a tagged template are calls
/// too. They may run a closure that writes a captured variable, so the
/// narrowing must be withdrawn and the program rejected instead of returning
/// the stale narrowed copy.
#[test]
fn optional_calls_and_tagged_templates_withdraw_written_captures() {
    for source in [
        "export function f(): number { let x: number | null = null; \
         const o = { m: () => { x = null; } }; x = 1; o.m?.(); return x; }",
        "export function f(): number { let x: number | null = null; \
         const o = { m: () => { x = null; } }; x = 1; o?.m(); return x; }",
        "function tag(s: string): number { return 1; } \
         export function f(): number { let x: number | null = null; \
         const set = () => { x = null; }; x = 1; tag`a`; return x; }",
    ] {
        let diagnostics = codes(source);
        assert!(
            diagnostics.iter().any(|c| c == "P0018"),
            "`{source}` must be rejected with P0018 (the call may write the captured \
             variable); got {diagnostics:?}"
        );
    }
}

/// B8: the generated `_narrowed_N` binding must not be shadowed by a
/// declaration of the same name that comes *later* in a nested block: the
/// collision check only saw the names visible at the narrowing point.
#[test]
fn narrowed_binding_avoids_names_declared_later_in_a_nested_block() {
    const SOURCE: &str = "export function f(): number { \
             let x: number | null = null; x = 7; \
             { let _narrowed_0: number = 5; return x + _narrowed_0; } } \
         export function declaredBefore(): number { \
             let _narrowed_0: number = 5; let x: number | null = null; x = 7; \
             return x + _narrowed_0; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(narrowed_name::f(&ctx), 12.0);
    assert_eq!(narrowed_name::declaredBefore(&ctx), 12.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    check_generated_crate("narrowed_name", &rust);
    let stdout = run_generated_crate("narrowed_name", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

/// B8: a parameter called `_narrowed_0` is visible at the narrowing point, so
/// the generated binding must skip that name (the parameter is bound on the
/// function itself, while the generated binding starts from the same counter).
#[test]
fn narrowed_binding_skips_a_visible_parameter_name() {
    let rust = rust_of(
        "export function f(_narrowed_0: number): number { \
             let x: number | null = null; x = 7; return x + _narrowed_0; }",
    );
    let norm: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        norm.contains("let_narrowed_1:f64="),
        "the generated binding must skip the parameter name and be `_narrowed_1`; got:\n{rust}"
    );
}

/// A5 + B6: a call of a closure that writes a captured variable must withdraw
/// *every* narrowing of that variable. A nested block that narrows a shadowing
/// variable of the same name used to hide the outer variable's narrowing from
/// the withdrawal (the withdrawal was looked up by name and found the inner
/// one), so the rest of the function kept reading the stale narrowed copy.
#[test]
fn shadowing_narrowing_does_not_hide_the_outer_one_from_a_captured_write() {
    const SOURCE: &str = "export function f(): number { \
             let x: number | null = null; const set = () => { x = 7; }; x = 1; \
             { let x: number | null = 1; x = 2; set(); } \
             return x!; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    // `set()` writes the outer variable's storage, so `x!` must read `7`.
    assert_eq!(shadow_hidden::f(&ctx), 7.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    check_generated_crate("shadow_hidden", &rust);
    let stdout = run_generated_crate("shadow_hidden", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

/// B9: an assignment to a narrowed variable writes the variable's *storage* in
/// every expression context, not only as a statement. `const y = (x = 3)` and
/// `const y = (x ||= 2)` used to write the narrowed copy, so a closure that reads
/// the storage observed the value from before the assignment.
#[test]
fn assignments_inside_an_initializer_write_the_storage() {
    const SOURCE: &str = "export function plain(): number { \
             let x: number | null = null; const read = (): number => x ?? 0; x = 0; \
             const y: number = (x = 3); return read() * 10 + y; } \
         export function logical(): number { \
             let x: number | null = null; const read = (): number => x ?? 0; x = 0; \
             const y: number = (x ||= 2); return read() * 10 + y; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    // JavaScript: after `x = 3` / `x ||= 2` the closure reads the new value.
    assert_eq!(init_write::plain(&ctx), 33.0);
    assert_eq!(init_write::logical(&ctx), 22.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    check_generated_crate("init_write", &rust);
    let stdout = run_generated_crate("init_write", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

/// B10: iterating a *generator* runs its body (`for item in &mut gen`), which can
/// call a closure that writes a captured variable, so the narrowing of such a
/// variable is withdrawn before the loop body. A plain array is lowered to a Rust
/// `for` over a `Vec` and runs no user code, so its narrowing stays valid -
/// withdrawing it too rejected a correct program.
#[test]
fn a_for_of_withdraws_written_captures_only_for_a_generator() {
    const SOURCE: &str = "export function f(it: number[]): number | null { \
         let x: number | null = null; const set = (): void => { x = null; }; \
         x = 1; for (const v of it) { } return x; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    let items = ts_aot_runtime::bumpalo::vec![in ctx.arena; 1.0, 2.0];
    assert_eq!(
        array_iter::f(&ctx, items),
        ::core::option::Option::Some(1.0),
        "iterating an array cannot call the closure, so the narrowing keeps its value"
    );
    println!("ok");
}
"#;
    let diagnostics = codes(
        "function* produce(): number { yield 1; } \
         export function f(): number { let x: number | null = null; \
         const set = (): void => { x = null; }; x = 1; \
         for (const v of produce()) { } return x; }",
    );
    assert!(
        diagnostics.iter().any(|c| c == "P0018"),
        "iterating a generator must withdraw the narrowing of a captured variable; \
         got {diagnostics:?}"
    );

    // An array cannot run the closure while it is iterated, so the narrowing is
    // still sound and the generated code has to keep its value.
    let rust = rust_of(SOURCE);
    check_generated_crate("array_iter", &rust);
    let stdout = run_generated_crate("array_iter", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

/// B11: a `switch` case shares the switch's scope frame, so the narrowing it
/// establishes is discarded before the next case is walked (its binding is
/// emitted in the first case's arm). Within the case it stays available, up to
/// and including the `return` that ends it - that used to be rejected, because
/// every case suppressed narrowing entirely.
#[test]
fn a_switch_case_narrows_within_itself_but_not_across_cases() {
    const SOURCE: &str = "export function f(c: i64): number { \
            let x: number | null = null; \
            switch (c) { case 1: x = 5; return x; default: x = 0; return x; } }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    // `x = 5` / `x = 0` narrow the variable for the `return` of their own case.
    assert_eq!(switch_case_narrowing::f(&ctx, 1), 5.0);
    assert_eq!(switch_case_narrowing::f(&ctx, 2), 0.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    check_generated_crate("switch_case_narrowing", &rust);
    let stdout = run_generated_crate("switch_case_narrowing", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");

    // ... and it must not survive into the next case, whose arm is a different
    // Rust block.
    let diagnostics = codes(
        "export function f(c: i64): number { let x: number | null = null; \
         switch (c) { case 1: x = 5; break; case 2: return x; default: return 0; } }",
    );
    assert!(
        diagnostics.iter().any(|c| c == "P0018"),
        "a narrowing from one case must not reach the next; got {diagnostics:?}"
    );
}

/// B12: a declaration in a nested block shadows the outer variable, so the loop
/// that writes the *inner* one must not withdraw the outer narrowing. The
/// widening used to search the frames by name and would restore the outer
/// variable's storage binding, which then read as `number | null` and made
/// `return x` a spurious `P0018`.
#[test]
fn a_loop_writing_a_shadowing_variable_keeps_the_outer_narrowing() {
    const SOURCE: &str = "export function f(): number { \
         let x: number | null = null; x = 1; \
         { let x: number | null = null; for (let i: i64 = 0; i < 3; i = i + 1) { x = i; } } \
         return x; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(
        shadowed_loop::f(&ctx),
        1.0,
        "the loop writes the inner `x`; the outer one keeps the narrowed 1"
    );
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    check_generated_crate("shadowed_loop", &rust);
    let stdout = run_generated_crate("shadowed_loop", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}
