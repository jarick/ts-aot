use ts_aot_driver::{CompileOptions, Driver, EmitStage};
use ts_aot_test_support::{check_generated_crate, run_generated_crate};

fn rust_of(source: &str) -> String {
    let out = Driver::new().compile_source(
        "test.ts",
        source,
        &CompileOptions {
            emit: EmitStage::Rust,
            module: true,
        },
    );
    assert!(
        !out.has_errors(),
        "must compile through the full pipeline; got {:?}",
        out.diagnostics
    );
    out.rust_source
        .expect("emit-rust must populate rust_source")
}

#[test]
fn negated_optional_and_primitive_loop_conditions_match_node() {
    const SOURCE: &str = "export function optionalBool(): number { \
             const b: boolean | null = false; let i = 0; \
             while (!b) { i = i + 1; if (i > 2) { break; } } return i; } \
         export function optionalString(): number { \
             const s: string | undefined = undefined; let i = 0; \
             while (!s) { i = i + 1; if (i > 2) { break; } } return i; } \
         export function boolCond(): number { \
             const b: boolean = false; let i = 0; \
             while (!b) { i = i + 1; if (i > 2) { break; } } return i; } \
         export function numberZero(): number { \
             const b: number = 0; let i = 0; \
             while (!b) { i = i + 1; if (i > 2) { break; } } return i; } \
         export function numberOne(): number { \
             const b: number = 1; let i = 0; \
             while (!b) { i = i + 1; if (i > 2) { break; } } return i; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    // node: optionalBool()=3 optionalString()=3 boolCond()=3 numberZero()=3 numberOne()=0
    assert_eq!(loop_cond_optional::optionalBool(&ctx), 3.0);
    assert_eq!(loop_cond_optional::optionalString(&ctx), 3.0);
    assert_eq!(loop_cond_optional::boolCond(&ctx), 3.0);
    assert_eq!(loop_cond_optional::numberZero(&ctx), 3.0);
    assert_eq!(loop_cond_optional::numberOne(&ctx), 0.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    check_generated_crate("loop_cond_optional", &rust);
    let stdout = run_generated_crate("loop_cond_optional", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn for_loops_and_optional_parameters_match_node() {
    const SOURCE: &str = "export function forEmpty(): number { \
             const b: boolean | null = false; let i = 0; \
             for (; !b;) { i = i + 1; if (i > 2) { break; } } return i; } \
         export function forFull(): number { \
             const b: boolean | null = false; let out = 0; \
             for (let i = 0; !b; i = i + 1) { out = i; if (i > 2) { break; } } return out; } \
         export function optionalParam(b?: boolean): number { \
             let i = 0; while (!b) { i = i + 1; if (i > 2) { break; } } return i; } \
         export function optionalParamUnion(b: boolean | null): number { \
             let i = 0; while (!b) { i = i + 1; if (i > 2) { break; } } return i; }";
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
    // node: forEmpty()=3 forFull()=3 optionalParam(false)=3 optionalParam()=3 optionalParam(true)=0
    assert_eq!(loop_cond_for::forEmpty(&ctx), 3.0);
    assert_eq!(loop_cond_for::forFull(&ctx), 3.0);
    assert_eq!(loop_cond_for::optionalParam(&ctx, some(false)), 3.0);
    assert_eq!(loop_cond_for::optionalParam(&ctx, none), 3.0);
    assert_eq!(loop_cond_for::optionalParam(&ctx, some(true)), 0.0);
    assert_eq!(loop_cond_for::optionalParamUnion(&ctx, some(false)), 3.0);
    assert_eq!(loop_cond_for::optionalParamUnion(&ctx, none), 3.0);
    assert_eq!(loop_cond_for::optionalParamUnion(&ctx, some(true)), 0.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate("loop_cond_for", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn continue_and_nested_loops_still_recheck_the_condition() {
    const SOURCE: &str = "export function withContinue(): number { \
             const b: boolean | null = false; let i = 0; let n = 0; \
             while (!b) { i = i + 1; if (i > 2) { break; } \
                 if (i === 1) { continue; } n = n + 1; } return n * 10 + i; } \
         export function nested(): number { \
             const b: boolean | null = false; let total = 0; let i = 0; \
             while (!b) { i = i + 1; let j = 0; \
                 while (j < 3) { j = j + 1; total = total + 1; } \
                 if (i > 1) { break; } } return total * 10 + i; } \
         export function doWhileContinue(): number { \
             const b: boolean | null = false; let i = 0; let n = 0; \
             do { i = i + 1; if (i < 3) { continue; } n = n + 1; \
                 if (i > 4) { break; } } while (!b); return n * 10 + i; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    // node: withContinue()=13 nested()=62 doWhileContinue()=35
    assert_eq!(loop_cond_flow::withContinue(&ctx), 13.0);
    assert_eq!(loop_cond_flow::nested(&ctx), 62.0);
    assert_eq!(loop_cond_flow::doWhileContinue(&ctx), 35.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate("loop_cond_flow", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn plain_boolean_comparison_and_call_conditions_are_unchanged() {
    const SOURCE: &str = "let calls: number = 0; \
         function step(): boolean { calls = calls + 1; return calls < 3; } \
         export function whileBool(): number { \
             let b = true; let i = 0; \
             while (b) { i = i + 1; if (i > 2) { break; } } return i; } \
         export function whileLess(n: number): number { \
             let i = 0; while (i < n) { i = i + 1; } return i; } \
         export function whileCall(): number { \
             let i = 0; while (step()) { i = i + 1; } return i * 10 + calls; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    // node: whileBool()=3 whileLess(5)=5 whileCall()=23 (step runs once per check)
    assert_eq!(loop_cond_plain::whileBool(&ctx), 3.0);
    assert_eq!(loop_cond_plain::whileLess(&ctx, 5.0), 5.0);
    assert_eq!(loop_cond_plain::whileCall(&ctx), 23.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    check_generated_crate("loop_cond_plain", &rust);
    let stdout = run_generated_crate("loop_cond_plain", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}
