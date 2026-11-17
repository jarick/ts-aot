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
fn negating_a_void_expression_is_true_and_evaluates_it_once() {
    const SOURCE: &str = "let hits: number = 0; \
         function bump(): void { hits = hits + 1; } \
         export function notNull(): boolean { return !null; } \
         export function notUndefined(): boolean { return !undefined; } \
         export function notVoidZero(): boolean { return !void 0; } \
         export function notVoidCall(): boolean { return !void bump(); } \
         export function notVoidCallTwice(): boolean { return (!void bump()) && (!void bump()); } \
         export function hitsNow(): number { return hits; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    // node: !null=true !undefined=true !void 0=true !void bump()=true
    assert!(void_not::notNull(&ctx));
    assert!(void_not::notUndefined(&ctx));
    assert!(void_not::notVoidZero(&ctx));
    assert_eq!(void_not::hitsNow(&ctx), 0.0, "`!void 0` must not call anything");
    assert!(void_not::notVoidCall(&ctx));
    assert_eq!(
        void_not::hitsNow(&ctx),
        1.0,
        "`!void bump()` must evaluate `bump()` exactly once"
    );
    assert!(void_not::notVoidCallTwice(&ctx));
    assert_eq!(
        void_not::hitsNow(&ctx),
        3.0,
        "each `!void bump()` in a `&&` chain must evaluate `bump()` exactly once"
    );
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    check_generated_crate("void_not", &rust);
    let stdout = run_generated_crate("void_not", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn void_still_lowers_as_a_value_and_a_statement() {
    const SOURCE: &str = "let hits: number = 0; \
         function bump(): void { hits = hits + 1; } \
         export function asStatement(): number { void bump(); return hits; } \
         export function asValue(): number { const u: void = void bump(); return hits; } \
         export function notOfAssigned(): boolean { const u: void = void 0; return !u; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(void_value::asStatement(&ctx), 1.0);
    assert_eq!(void_value::asValue(&ctx), 2.0);
    assert!(void_value::notOfAssigned(&ctx));
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate("void_value", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}
