use std::fmt::Write as _;

use ts_aot_driver::{CompileOptions, Driver, DriverOutput, EmitStage};
use ts_aot_test_support::run_generated_crate;

fn compile(source: &str) -> DriverOutput {
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
    let out = compile(source);
    assert!(
        !out.has_errors(),
        "must compile through the full pipeline; got {:?}",
        out.diagnostics
    );
    out.rust_source
        .expect("emit-rust must populate rust_source")
}

fn numbers_main(package: &str, expectations: &[f64]) -> String {
    let mut checks = String::new();
    for (index, value) in expectations.iter().enumerate() {
        let _ = writeln!(
            checks,
            "    assert_eq!({package}::got{index}(&ctx) as f64, {value}_f64);"
        );
    }
    format!(
        "use ts_aot_runtime::{{Bump, Ctx, Host}};\n\nstruct NoHost;\nimpl Host for NoHost {{}}\n\n\
         fn main() {{\n    let host = NoHost;\n    let bump = Bump::new();\n    \
         let ctx = Ctx {{ arena: &bump, host: &host }};\n{checks}    println!(\"ok\");\n}}\n"
    )
}

fn check(package: &str, source: &str, expectations: &[f64]) {
    let rust = rust_of(source);
    let stdout = run_generated_crate(package, &rust, &numbers_main(package, expectations));
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn an_i64_argument_widens_into_an_i32_parameter() {
    const SOURCE: &str = r"
function value(): i64 { return 300; }
function f(x: i32): i32 { return x; }
export function got0(): i32 { return f(value()); }
";
    check("coerce_i64_i32_in_range", SOURCE, &[300.0]);
}

#[test]
fn an_i64_argument_truncates_like_to_int32_into_an_i32_parameter() {
    const SOURCE: &str = r"
function value(): i64 { return 4294967296; }
function f(x: i32): i32 { return x; }
export function got0(): i32 { return f(value()); }
";
    check("coerce_i64_i32_wrap", SOURCE, &[0.0]);
}

#[test]
fn a_negative_i64_argument_truncates_like_to_int32_into_an_i32_parameter() {
    const SOURCE: &str = r"
function value(): i64 { return -2147483649; }
function f(x: i32): i32 { return x; }
export function got0(): i32 { return f(value()); }
";
    check("coerce_i64_i32_negative", SOURCE, &[2_147_483_647.0]);
}

#[test]
fn an_i32_argument_widens_into_an_i64_parameter() {
    const SOURCE: &str = r"
function value(): i32 { return 7; }
function f(x: i64): i64 { return x * 1000000000; }
export function got0(): i64 { return f(value()); }
";
    check("coerce_i32_i64", SOURCE, &[7_000_000_000.0]);
}

#[test]
fn a_u8_argument_widens_into_an_i32_parameter() {
    const SOURCE: &str = r"
function value(): u8 { return 200; }
function f(x: i32): i32 { return x; }
export function got0(): i32 { return f(value()); }
";
    check("coerce_u8_i32", SOURCE, &[200.0]);
}

#[test]
fn a_u32_argument_narrows_into_an_i32_parameter() {
    const SOURCE: &str = r"
function value(): u32 { return 4294967295; }
function f(x: i32): i32 { return x; }
export function got0(): i32 { return f(value()); }
";
    check("coerce_u32_i32", SOURCE, &[-1.0]);
}

#[test]
fn an_i64_argument_narrows_into_an_i16_parameter() {
    const SOURCE: &str = r"
function value(): i64 { return 65541; }
function f(x: i16): i16 { return x; }
export function got0(): i16 { return f(value()); }
";
    check("coerce_i64_i16", SOURCE, &[5.0]);
}

#[test]
fn a_method_argument_is_coerced_to_the_parameter_type() {
    const SOURCE: &str = r"
class C {
  m(x: i32): i32 { return x + 1; }
}
function value(): i64 { return 41; }
export function got0(): i32 { return new C().m(value()); }
";
    check("coerce_method_arg", SOURCE, &[42.0]);
}

#[test]
fn a_call_result_is_coerced_before_being_passed_on() {
    const SOURCE: &str = r"
function value(): i64 { return 4294967297; }
function f(x: i32): i32 { return x; }
export function got0(): i32 { return f(value()) + 1; }
";
    check("coerce_call_result", SOURCE, &[2.0]);
}

#[test]
fn an_indirect_call_argument_is_coerced_to_the_parameter_type() {
    const SOURCE: &str = r"
function id(x: i32): i32 { return x; }
function value(): i64 { return 9; }
function apply(f: (x: i32) => i32, v: i64): i32 { return f(v); }
export function got0(): i32 { return apply(id, value()); }
";
    check("coerce_indirect_arg", SOURCE, &[9.0]);
}
