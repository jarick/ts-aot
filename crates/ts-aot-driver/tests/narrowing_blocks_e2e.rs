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

#[test]
fn written_captures_still_withdraw_narrowings() {
    const WRITE_THROUGH_CALL: &str = "export function f(): number { let x: number | null = null; \
         const setNull = () => { x = null; }; x = 1; setNull(); return x; }";
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

    let rust = rust_of(SOURCE);
    check_generated_crate("array_iter", &rust);
    let stdout = run_generated_crate("array_iter", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

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

    let diagnostics = codes(
        "export function f(c: i64): number { let x: number | null = null; \
         switch (c) { case 1: x = 5; break; case 2: return x; default: return 0; } }",
    );
    assert!(
        diagnostics.iter().any(|c| c == "P0018"),
        "a narrowing from one case must not reach the next; got {diagnostics:?}"
    );
}

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
