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
fn typed_nullish_arguments_keep_their_effects_and_their_target_type() {
    const SOURCE: &str = "\
        let calls: number = 0; \
        function makeNull(): null { calls = calls + 1; return null; } \
        function take(x: number | null = 1): number { return x ?? -1; } \
        function takeStr(s: string | null): number { if (s === null) { return -1; } return 1; } \
        export function viaNullableDefault(): number { return take(makeNull()); } \
        export function viaRequiredString(): number { return takeStr(makeNull()); } \
        export function callCount(): number { return calls; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(nullish_effects::viaNullableDefault(&ctx), -1.0);
    assert_eq!(nullish_effects::callCount(&ctx), 1.0);
    assert_eq!(nullish_effects::viaRequiredString(&ctx), -1.0);
    assert_eq!(nullish_effects::callCount(&ctx), 2.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let norm: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        norm.contains("let_=makeNull(__ctx)"),
        "the effectful `null`-typed argument must still be evaluated exactly once; got:\n{rust}"
    );
    check_generated_crate("nullish_effects", &rust);
    let stdout = run_generated_crate("nullish_effects", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn same_class_numeric_operands_share_the_wider_operand_type() {
    const SOURCE: &str = "\
        export function addInt(): i64 { let a: i32 = 2; let b: i64 = 40; return a + b; } \
        export function addFloat(): f64 { let a: f32 = 0.5; let b: f64 = 1.25; return a + b; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(numeric_widths::addInt(&ctx), 42);
    assert_eq!(numeric_widths::addFloat(&ctx), 1.75);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let norm: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        norm.contains("asi64"),
        "an `i32` operand added to an `i64` operand must be widened to `i64`; got:\n{rust}"
    );
    assert!(
        norm.contains("asf64"),
        "an `f32` operand added to an `f64` operand must be widened to `f64`; got:\n{rust}"
    );
    check_generated_crate("numeric_widths", &rust);
    let stdout = run_generated_crate("numeric_widths", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn optional_chain_index_read_past_the_end_yields_undefined() {
    const SOURCE: &str = "\
        export function at(a: number[] | undefined, i: number): number | undefined { return a?.[i]; } \
        export function mkA(): number[] { const a: number[] = [10, 20, 30]; return a; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    let a = optional_chain_bounds::mkA(&ctx);
    assert_eq!(optional_chain_bounds::at(&ctx, Some(a), 1.0), Some(20.0));
    let a = optional_chain_bounds::mkA(&ctx);
    assert_eq!(
        optional_chain_bounds::at(&ctx, Some(a), 7.0),
        None,
        "an out-of-range index on a present array is `undefined`, not a Rust panic"
    );
    assert_eq!(optional_chain_bounds::at(&ctx, None, 0.0), None);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let norm: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        norm.contains(".get(iasusize).copied()"),
        "`?.[i]` must read a Copy element through the checked `get` accessor; got:\n{rust}"
    );
    check_generated_crate("optional_chain_bounds", &rust);
    let stdout = run_generated_crate("optional_chain_bounds", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}
