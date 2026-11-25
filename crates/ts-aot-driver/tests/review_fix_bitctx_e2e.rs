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

fn run_checks(package: &str, source: &str, checks: &str) {
    let out = compile(source);
    assert!(
        !out.has_errors(),
        "must compile through the full pipeline; got {:?}",
        out.diagnostics
    );
    let rust = out
        .rust_source
        .expect("emit-rust must populate rust_source");
    let main = format!(
        "use ts_aot_runtime::{{Bump, Ctx, Host}};\n\nstruct NoHost;\nimpl Host for NoHost {{}}\n\n\
         fn main() {{\n    let host = NoHost;\n    let bump = Bump::new();\n    \
         let ctx = Ctx {{ arena: &bump, host: &host }};\n{checks}    println!(\"ok\");\n}}\n"
    );
    let stdout = run_generated_crate(package, &rust, &main);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

const BITWISE_CONTEXT_SOURCE: &str = "
function takesNumber(x: number): number { return x; }

export function addFraction(): number { return (3 & 1) + 0.5; }
export function divide(): number { return (3 & 1) / 2; }
export function compare(): boolean { return (3 & 1) < 0.5; }
export function nested(): number { return ((3 & 1) + 0.5) * 2; }
export function callArgument(): number { return takesNumber(3 & 1); }
export function arrayElement(): number { const a: number[] = [3 & 1]; return a[0]; }
export function template(): string { return `v=${3 & 1}`; }
export function concat(): string { return \"v=\" + (3 & 1); }
export function compound(): number { let x: number = 0; x += 3 & 1; return x; }
export function negated(): number { return -(3 & 1); }
export function ternary(flag: boolean): number { return flag ? 0.5 : 3 & 1; }
export function integerAdd(): number { let x = (3 & 1) + 5; return x; }
export function integerCompare(): boolean { return (3 & 1) < 5; }
export function inferredLocal(): number { let y = 3 & 1; return y; }
export function bitNot(): number { return ~0.5; }
export function shifted(): number { return (3 & 1) << 2; }
";

#[test]
fn a_bitwise_result_composes_with_the_surrounding_numeric_context() {
    run_checks(
        "bitwise_context",
        BITWISE_CONTEXT_SOURCE,
        r#"    assert_eq!(bitwise_context::addFraction(&ctx), 1.5_f64);
    assert_eq!(bitwise_context::divide(&ctx), 0.5_f64);
    assert!(!bitwise_context::compare(&ctx));
    assert_eq!(bitwise_context::nested(&ctx), 3.0_f64);
    assert_eq!(bitwise_context::callArgument(&ctx), 1.0_f64);
    assert_eq!(bitwise_context::arrayElement(&ctx), 1.0_f64);
    assert_eq!(bitwise_context::template(&ctx).to_string_lossy(), "v=1");
    assert_eq!(bitwise_context::concat(&ctx).to_string_lossy(), "v=1");
    assert_eq!(bitwise_context::compound(&ctx), 1.0_f64);
    assert_eq!(bitwise_context::negated(&ctx), -1.0_f64);
    assert_eq!(bitwise_context::ternary(&ctx, true), 0.5_f64);
    assert_eq!(bitwise_context::ternary(&ctx, false), 1.0_f64);
    assert_eq!(bitwise_context::integerAdd(&ctx), 6.0_f64);
    assert!(bitwise_context::integerCompare(&ctx));
    assert_eq!(bitwise_context::inferredLocal(&ctx), 1.0_f64);
    assert_eq!(bitwise_context::bitNot(&ctx), -1.0_f64);
    assert_eq!(bitwise_context::shifted(&ctx), 4.0_f64);
"#,
    );
}

const TO_INT32_ROUNDING_SOURCE: &str = "
export function i64Arithmetic(): number { let a: i64 = 9007199254740993; let b: i64 = 1; return (a + b) | 0; }
export function i64Direct(): number { let a: i64 = 9007199254740993; return a | 0; }
export function i64BelowBoundary(): number { let a: i64 = 9007199254740991; let b: i64 = 0; return (a + b) | 0; }
export function u64Arithmetic(): number { let a: u64 = 9007199254740993; let b: u64 = 1; return (a + b) | 0; }
export function plainNumber(): number { let a: number = 9007199254740993; let b: number = 1; return (a + b) | 0; }
export function floatLiteralSum(): number { return (9007199254740993 + 1) | 0; }
export function floatLiteralDirect(): number { return 9007199254740993 | 0; }
export function i32Arithmetic(): number { let a: i32 = 3; let b: i32 = 1; return (a + b) | 0; }
export function i32Largest(): number { let a: i32 = 2147483647; return a | 0; }
export function u8Arithmetic(): number { let a: u8 = 3; let b: u8 = 1; return (a + b) | 0; }
export function u8Largest(): number { let a: u8 = 255; return a | 0; }
export function floatChainOperand(): number { return ((3 & 1) + 1e10) | 0; }
export function smallFloatChainOperand(): number { return ((3 & 1) + 0.5) | 0; }
";

#[test]
fn integer_operands_round_to_f64_before_to_int32() {
    run_checks(
        "to_int32_rounding",
        TO_INT32_ROUNDING_SOURCE,
        r"    assert_eq!(to_int32_rounding::i64Arithmetic(&ctx), 0.0_f64);
    assert_eq!(to_int32_rounding::i64Direct(&ctx), 0.0_f64);
    assert_eq!(to_int32_rounding::i64BelowBoundary(&ctx), -1.0_f64);
    assert_eq!(to_int32_rounding::u64Arithmetic(&ctx), 0.0_f64);
    assert_eq!(to_int32_rounding::plainNumber(&ctx), 0.0_f64);
    assert_eq!(to_int32_rounding::floatLiteralSum(&ctx), 0.0_f64);
    assert_eq!(to_int32_rounding::floatLiteralDirect(&ctx), 0.0_f64);
    assert_eq!(to_int32_rounding::i32Arithmetic(&ctx), 4.0_f64);
    assert_eq!(to_int32_rounding::i32Largest(&ctx), 2147483647.0_f64);
    assert_eq!(to_int32_rounding::u8Arithmetic(&ctx), 4.0_f64);
    assert_eq!(to_int32_rounding::u8Largest(&ctx), 255.0_f64);
    assert_eq!(to_int32_rounding::floatChainOperand(&ctx), 1410065409.0_f64);
    assert_eq!(to_int32_rounding::smallFloatChainOperand(&ctx), 1.0_f64);
",
    );
}

#[test]
fn a_bitwise_operand_with_no_numeric_conversion_is_still_refused() {
    let out = compile("export function got(): number { return \"3\" | 0; }");
    assert!(
        out.diagnostics.errors().any(|d| d.code.as_str() == "E0300"),
        "a bitwise operand with no runtime ToNumber conversion must be refused; got {:?}",
        out.diagnostics
    );
    assert!(
        out.rust_source.is_none(),
        "a refused source must not emit rust; got:\n{}",
        out.rust_source.unwrap_or_default()
    );
}
