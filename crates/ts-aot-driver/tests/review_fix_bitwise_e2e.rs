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

fn f64_literal(value: f64) -> String {
    if value.is_nan() {
        "f64::NAN".to_owned()
    } else if value.is_infinite() {
        if value.is_sign_negative() {
            "f64::NEG_INFINITY".to_owned()
        } else {
            "f64::INFINITY".to_owned()
        }
    } else {
        format!("{value}_f64")
    }
}

fn numbers_main(package: &str, calls: &[(&str, f64)]) -> String {
    let mut checks = String::new();
    for (name, value) in calls {
        let _ = writeln!(
            checks,
            "    assert_eq!({package}::{name}(&ctx), {});",
            f64_literal(*value)
        );
    }
    format!(
        "use ts_aot_runtime::{{Bump, Ctx, Host}};\n\nstruct NoHost;\nimpl Host for NoHost {{}}\n\n\
         fn main() {{\n    let host = NoHost;\n    let bump = Bump::new();\n    \
         let ctx = Ctx {{ arena: &bump, host: &host }};\n{checks}    println!(\"ok\");\n}}\n"
    )
}

fn check(package: &str, source: &str, calls: &[(&str, f64)]) {
    let out = compile(source);
    assert!(
        !out.has_errors(),
        "must compile through the full pipeline; got {:?}",
        out.diagnostics
    );
    let rust = out
        .rust_source
        .expect("emit-rust must populate rust_source");
    let main = numbers_main(package, calls);
    let stdout = run_generated_crate(package, &rust, &main);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

const BITWISE_NOT_SOURCE: &str = "
export function negativeZero(): number { return ~(-0); }
export function fraction(): number { return ~0.5; }
export function integer(): number { return ~3; }
export function zero(): number { return ~0; }
export function negatedInteger(): number { return ~(-3); }
";

#[test]
fn bitwise_not_of_a_non_integer_operand_applies_to_int32() {
    check(
        "bitwise_not_non_integer",
        BITWISE_NOT_SOURCE,
        &[
            ("negativeZero", -1.0),
            ("fraction", -1.0),
            ("integer", -4.0),
            ("zero", -1.0),
            ("negatedInteger", 2.0),
        ],
    );
}

const BITWISE_BINARY_SOURCE: &str = "
export function orNegativeZero(): number { return (-0) | 0; }
export function shrNegativeZero(): number { return (-0) >> 1; }
export function andNegativeZero(): number { return (-0) & 1; }
export function xorNegativeZero(): number { return (-0) ^ 1; }
export function shlNegativeZero(): number { return (-0) << 1; }
export function orFraction(): number { return 1.5 | 0; }
export function orIntegers(): number { return 3 | 1; }
export function andIntegers(): number { return 3 & 1; }
export function xorIntegers(): number { return 3 ^ 1; }
export function shlIntegers(): number { return 3 << 1; }
export function shrIntegers(): number { return 3 >> 1; }
";

#[test]
fn bitwise_binary_operators_apply_to_int32_on_both_operands() {
    check(
        "bitwise_binary_int32",
        BITWISE_BINARY_SOURCE,
        &[
            ("orNegativeZero", 0.0),
            ("shrNegativeZero", 0.0),
            ("andNegativeZero", 0.0),
            ("xorNegativeZero", 1.0),
            ("shlNegativeZero", 0.0),
            ("orFraction", 1.0),
            ("orIntegers", 3.0),
            ("andIntegers", 1.0),
            ("xorIntegers", 2.0),
            ("shlIntegers", 6.0),
            ("shrIntegers", 1.0),
        ],
    );
}

const TO_INT32_SOURCE: &str = "
export function wrapsPositive(): number { return 2147483648 | 0; }
export function wrapsModulo(): number { return 4294967297 | 0; }
export function wrapsNegative(): number { return -2147483649 | 0; }
export function positiveInfinity(): number { return 1e400 | 0; }
export function negativeInfinity(): number { return -1e400 | 0; }
export function notANumber(): number { return (1e400 - 1e400) | 0; }
export function huge(): number { return 1e300 | 0; }
export function truncatesPositive(): number { return 1.9 | 0; }
export function truncatesNegative(): number { return -1.9 | 0; }
export function shlMaskedCount(): number { return 3 << 33; }
export function shrMaskedCount(): number { return 3 >> 33; }
export function shrNegative(): number { return -8 >> 1; }
";

#[test]
fn to_int32_wraps_large_and_non_finite_operands_like_javascript() {
    check(
        "to_int32_wrapping",
        TO_INT32_SOURCE,
        &[
            ("wrapsPositive", -2_147_483_648.0),
            ("wrapsModulo", 1.0),
            ("wrapsNegative", 2_147_483_647.0),
            ("positiveInfinity", 0.0),
            ("negativeInfinity", 0.0),
            ("notANumber", 0.0),
            ("huge", 0.0),
            ("truncatesPositive", 1.0),
            ("truncatesNegative", -1.0),
            ("shlMaskedCount", 6.0),
            ("shrMaskedCount", 1.0),
            ("shrNegative", -4.0),
        ],
    );
}

const NULLISH_OPERAND_SOURCE: &str = "
export function nullOperand(): number { return null | 0; }
export function undefinedOperand(): number { return undefined | 0; }
export function notNull(): number { return ~null; }
";

#[test]
fn a_nullish_bitwise_operand_is_to_int32_zero() {
    check(
        "bitwise_nullish_operand",
        NULLISH_OPERAND_SOURCE,
        &[
            ("nullOperand", 0.0),
            ("undefinedOperand", 0.0),
            ("notNull", -1.0),
        ],
    );
}

#[test]
fn a_non_numeric_bitwise_operand_is_refused_without_emitting_rust() {
    let out = compile("export function got(): number { return \"3\" | 0; }");
    assert!(
        out.diagnostics.errors().any(|d| d.code.as_str() == "E0300"),
        "a bitwise operand with no runtime ToNumber conversion must be refused by the backend; \
         got {:?}",
        out.diagnostics
    );
    assert!(
        out.rust_source.is_none(),
        "a refused source must not emit rust; got:\n{}",
        out.rust_source.unwrap_or_default()
    );
}

const BITWISE_OPERAND_SOURCE: &str = "
export function floatLocal(): number { let x: number = 1.5; return x | 0; }
export function integerLocal(): number { let x: number = 5; return x | 0; }
export function booleanOperand(): number { return (3 > 1) | 0; }
export function assignedToLocal(): number { let x: number = 0; x = 1.5 | 0; return x; }
export function passedToParameter(): number { return takesNumber(1.5 | 0); }
function takesNumber(x: number): number { return x; }
";

#[test]
fn a_bitwise_result_keeps_the_surrounding_numeric_type() {
    check(
        "bitwise_operand_shapes",
        BITWISE_OPERAND_SOURCE,
        &[
            ("floatLocal", 1.0),
            ("integerLocal", 5.0),
            ("booleanOperand", 1.0),
            ("assignedToLocal", 1.0),
            ("passedToParameter", 1.0),
        ],
    );
}

const NEGATIVE_ZERO_SOURCE: &str = "
export function reciprocal(): number { const z = -0; return 1 / z; }
export function identity(): number { const z = -0; return Object.is(z, -0) ? 1 : 0; }
export function index(): number { const a: number[] = [7, 8]; return a[-0]; }
export function negatedInteger(): number { return -3; }
export function parenthesizedInteger(): number { return (-3); }
export function parenthesizedPositive(): number { return (3); }
export function negatedFraction(): number { return -3.5; }
export function thousand(): number { return (1e3); }
export function tinyNegative(): number { return (-1e-3); }
";

#[test]
fn negative_zero_and_numeric_literal_cases_keep_their_values() {
    check(
        "negative_zero_kept",
        NEGATIVE_ZERO_SOURCE,
        &[
            ("reciprocal", f64::NEG_INFINITY),
            ("identity", 1.0),
            ("index", 7.0),
            ("negatedInteger", -3.0),
            ("parenthesizedInteger", -3.0),
            ("parenthesizedPositive", 3.0),
            ("negatedFraction", -3.5),
            ("thousand", 1000.0),
            ("tinyNegative", -0.001),
        ],
    );
}

fn capture_fixture(body: &str) -> String {
    format!(
        r"
function f(): number {{
  const items: number[] = [1, 2];
  const other: number[] = [9, 9];
  let alias: number[] = other;
  class C {{ read(): number {{ return items[0]; }} }}
  const c = new C();
  try {{
{body}
  }} catch {{
    alias[0] = 9;
  }}
  return c.read();
}}
export function got(): number {{ return f(); }}
"
    )
}

const DEAD_RETURN_IN_FALSE_BRANCH: &str = r"    try { if (false) { return 3; } alias = items; throw 1; } catch { alias = other; } finally { throw 9; }
    throw 2;";

const LIVE_RETURN_IN_RUNTIME_BRANCH: &str = r"    try { if (other[0] > 0) { return 3; } alias = items; throw 1; } catch { alias = other; } finally { throw 9; }
    throw 2;";

const MUTATED_MESSAGE: &str = "the local `items` is read by a method of a local class but the object it holds is modified in the enclosing function after the class is constructed; the class keeps a copy taken at construction, so the method could read stale contents; modify the object before constructing the class or pass it as a parameter";

fn refuses(source: &str) -> String {
    let out = compile(source);
    let diagnostic = out
        .diagnostics
        .errors()
        .find(|d| d.code.as_str() == "E0419")
        .unwrap_or_else(|| panic!("expected an E0419 refusal; got {:?}", out.diagnostics));
    let message = diagnostic.message.clone();
    assert!(
        out.rust_source.is_none(),
        "a refused source must not emit rust; got:\n{}",
        out.rust_source.unwrap_or_default()
    );
    message
}

#[test]
fn a_dead_return_under_a_statically_false_branch_does_not_count_as_returning() {
    check(
        "abrupt_dead_return",
        &capture_fixture(DEAD_RETURN_IN_FALSE_BRANCH),
        &[("got", 1.0)],
    );
}

#[test]
fn a_live_return_under_a_runtime_branch_still_counts_as_returning() {
    assert_eq!(
        refuses(&capture_fixture(LIVE_RETURN_IN_RUNTIME_BRANCH)),
        MUTATED_MESSAGE,
        "a reachable return in the try body must keep the fall-through state reaching the finally"
    );
}

const SWITCH_BREAK_IN_LOOP: &str = r"    while (true) { switch (1) { case 1: break; } throw 1; }
    alias = items;
    throw 2;";

const SWITCH_WITH_NESTED_LOOP_BREAK: &str = r"    while (true) { switch (1) { case 1: for (;;) { break; } } throw 1; }
    alias = items;
    throw 2;";

#[test]
fn a_switch_break_does_not_make_the_enclosing_loop_breakable() {
    check(
        "abrupt_switch_break",
        &capture_fixture(SWITCH_BREAK_IN_LOOP),
        &[("got", 1.0)],
    );
}

#[test]
fn a_loop_break_nested_inside_a_switch_does_not_make_the_enclosing_loop_breakable() {
    check(
        "abrupt_switch_nested_loop_break",
        &capture_fixture(SWITCH_WITH_NESTED_LOOP_BREAK),
        &[("got", 1.0)],
    );
}

const DIRECT_LOOP_BREAK: &str = r"    while (true) { try { alias = items; break; } catch { alias = other; } }
    throw 2;";

#[test]
fn a_direct_loop_break_still_makes_the_loop_breakable() {
    assert_eq!(
        refuses(&capture_fixture(DIRECT_LOOP_BREAK)),
        MUTATED_MESSAGE,
        "a break that belongs to the loop itself must keep counting as an exit"
    );
}
