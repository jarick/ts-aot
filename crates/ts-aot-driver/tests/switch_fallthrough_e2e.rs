//! `switch` fallthrough.
//!
//! JavaScript runs the statements of every case from the matching label down to
//! the next `break`/`return`/`throw`/`continue`, so a case that does not end in a
//! terminator falls into the next one - and an empty case falls into the case
//! that follows it. The backend emits one Rust `match` arm per case, which has no
//! implicit fallthrough, so the frontend appends the falling-through case's body
//! to its arm (a case that already ends in a terminator gets nothing appended).
//!
//! These tests compile through the whole pipeline, `cargo check` the generated
//! crate and run it, comparing every value with the JavaScript answer.

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

#[test]
fn a_case_without_a_terminator_runs_the_next_case_body() {
    const SOURCE: &str = "export function f(c: i64): number { let s: number = 0; \
             switch (c) { case 1: s = 1; case 2: s = s + 10; break; } return s; } \
         export function shared(c: i64): number { let s: number = 0; \
             switch (c) { case 1: case 2: s = 2; break; case 3: s = 3; } return s; } \
         export function terminated(c: i64): number { let s: number = 0; \
             switch (c) { case 1: s = 1; return s; case 2: s = 2; } return s; } \
         export function viaDefault(c: i64): number { let s: number = 0; \
             switch (c) { default: s = 9; case 4: s = s + 1; break; } return s; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    // `1` runs case 1 and falls into case 2; `2` starts at case 2.
    assert_eq!(switch_fallthrough::f(&ctx, 1), 11.0);
    assert_eq!(switch_fallthrough::f(&ctx, 2), 10.0);
    assert_eq!(switch_fallthrough::f(&ctx, 3), 0.0);
    // An empty case shares the body of the case that follows it.
    assert_eq!(switch_fallthrough::shared(&ctx, 1), 2.0);
    assert_eq!(switch_fallthrough::shared(&ctx, 2), 2.0);
    assert_eq!(switch_fallthrough::shared(&ctx, 3), 3.0);
    assert_eq!(switch_fallthrough::shared(&ctx, 4), 0.0);
    // A `return` ends the chain, so case 1 does not run case 2's body.
    assert_eq!(switch_fallthrough::terminated(&ctx, 1), 1.0);
    assert_eq!(switch_fallthrough::terminated(&ctx, 2), 2.0);
    assert_eq!(switch_fallthrough::terminated(&ctx, 3), 0.0);
    // A `default` in the middle falls into case 4, while a direct `4` match
    // starts at case 4 and never runs the `default` statements.
    assert_eq!(switch_fallthrough::viaDefault(&ctx, 4), 1.0);
    assert_eq!(switch_fallthrough::viaDefault(&ctx, 1), 10.0);
    assert_eq!(switch_fallthrough::viaDefault(&ctx, 5), 10.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    check_generated_crate("switch_fallthrough", &rust);
    let stdout = run_generated_crate("switch_fallthrough", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}
