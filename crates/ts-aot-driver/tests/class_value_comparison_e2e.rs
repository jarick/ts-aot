use ts_aot_driver::{CompileOptions, Driver, EmitStage};
use ts_aot_test_support::run_generated_crate;

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

fn codes(out: &ts_aot_driver::DriverOutput) -> Vec<&str> {
    out.diagnostics.iter().map(|d| d.code.as_str()).collect()
}

#[test]
fn equality_and_switch_patterns_on_a_class_value_match_node() {
    const SOURCE: &str = "class Holder { C = class { static k: number = 5; }; \
         D = class { static k: number = 7; }; } \
         export function got(): number { \
             const h = new Holder(); \
             let r = 0; \
             if (h.C == 1) { r = r + 1; } \
             if (h.C === 1) { r = r + 10; } \
             if (h.C != 1) { r = r + 100; } \
             if (h.C !== 1) { r = r + 1000; } \
             if (h.C == 0) { r = r + 10000; } \
             if (h.C == null) { r = r + 100000; } \
             switch (h.C) { case 1: r = r + 1000000; default: r = r + 1000000; } \
             if (h.C === h.C) { r = r + 10000000; } \
             if (h.C !== h.D) { r = r + 100000000; } \
             return r; \
         }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(class_value_comparisons::got(&ctx), 111001100.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate("class_value_comparisons", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_switch_with_only_primitive_cases_on_a_class_value_takes_the_default() {
    const SOURCE: &str = "class Holder { C = class { static k: number = 5; }; } \
         export function got(): number { \
             const h = new Holder(); \
             switch (h.C) { case 0: return 1; case 1: return 2; } \
             return 3; \
         }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(class_value_switch_default::got(&ctx), 3.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate("class_value_switch_default", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn an_ordering_comparison_of_a_class_value_is_refused() {
    const SOURCE: &str = "class Holder { C = class { static k: number = 5; }; } \
         export function got(): number { \
             const h = new Holder(); \
             return h.C < 1 ? 1 : 0; \
         }";
    let out = compile_module(SOURCE);
    assert!(
        codes(&out).contains(&"E0419"),
        "a class value is an object and cannot be ordered against a number; got {:?}",
        out.diagnostics
    );
}

#[test]
fn arithmetic_on_a_class_value_is_refused() {
    const SOURCE: &str = "class Holder { C = class { static k: number = 5; }; } \
         export function got(): number { \
             const h = new Holder(); \
             return h.C + 1; \
         }";
    let out = compile_module(SOURCE);
    assert!(
        codes(&out).contains(&"E0419"),
        "a class value is an object and cannot be used as a number; got {:?}",
        out.diagnostics
    );
}
