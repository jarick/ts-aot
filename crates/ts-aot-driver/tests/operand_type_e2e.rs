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

fn crate_rust_of(source: &str) -> String {
    let out = Driver::new().compile_source(
        "test.ts",
        source,
        &CompileOptions {
            emit: EmitStage::Crate,
            module: true,
        },
    );
    assert!(
        !out.has_errors(),
        "must compile through the full pipeline; got {:?}",
        out.diagnostics
    );
    out.rust_source
        .expect("emit-crate must populate rust_source")
}

#[test]
fn an_optional_chain_generic_call_on_a_plain_receiver_is_specialized() {
    const SOURCE: &str = "
class K { m<T>(x: T): T { return x; } }
export function through(): number { const k = new K(); return k?.m(5) ?? 0; }
";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(chain_generic_plain::through(&ctx), 5.0);
    println!("ok");
}
"#;
    let out = compile_module(SOURCE);
    assert!(
        !out.has_errors(),
        "a generic method call inside an optional chain must be specialized so `??` sees a \
         concrete type; got {:?}",
        out.diagnostics
    );
    let rust = crate_rust_of(SOURCE);
    assert!(!rust.contains("__generic"), "got:\n{rust}");
    check_generated_crate("chain_generic_plain", &rust);
    let stdout = run_generated_crate("chain_generic_plain", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn an_optional_chain_generic_call_short_circuits_on_a_nullable_receiver() {
    const SOURCE: &str = "
class K { m<T>(x: T): T { return x; } }
export function maybe(k: K | null): number { return k?.m(6) ?? 1; }
";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(chain_generic_nullable::maybe(&ctx, None), 1.0);
    assert_eq!(
        chain_generic_nullable::maybe(&ctx, Some(chain_generic_nullable::K {})),
        6.0
    );
    println!("ok");
}
"#;
    let out = compile_module(SOURCE);
    assert!(
        !out.has_errors(),
        "a generic method call behind a nullable receiver must be specialized and the `??` \
         fallback must stay reachable; got {:?}",
        out.diagnostics
    );
    let rust = crate_rust_of(SOURCE);
    assert!(!rust.contains("__generic"), "got:\n{rust}");
    check_generated_crate("chain_generic_nullable", &rust);
    let stdout = run_generated_crate("chain_generic_nullable", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_coalesce_over_an_optional_chain_call_unwraps_the_chain_option() {
    const SOURCE: &str = "
class K { m(x: number): number { return x; } }
export function maybe(k: K | null): number { return k?.m(6) ?? 1; }
";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(chain_call_coalesce::maybe(&ctx, None), 1.0);
    assert_eq!(
        chain_call_coalesce::maybe(&ctx, Some(chain_call_coalesce::K {})),
        6.0
    );
    println!("ok");
}
"#;
    let out = compile_module(SOURCE);
    assert!(
        !out.has_errors(),
        "`k?.m(6) ?? 1` must compile; got {:?}",
        out.diagnostics
    );
    let rust = crate_rust_of(SOURCE);
    check_generated_crate("chain_call_coalesce", &rust);
    let stdout = run_generated_crate("chain_call_coalesce", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

const NULLISH_NUMBER_SOURCE: &str = "
export function plusNull(): number { return null + 1; }
export function timesUndefined(): number { return undefined * 2; }
export function minusNull(): number { return null - 1; }
export function nullOnTheRight(): number { return 1 + null; }
export function nullLess(): boolean { return null < 1; }
export function undefinedLess(): boolean { return undefined < 1; }
export function undefinedAtLeast(): boolean { return undefined >= 0; }
export function negatedNull(): number { return -null; }
export function negatedUndefined(): number { return -undefined; }
export function bitNotUndefined(): number { return ~undefined; }
";

#[test]
fn null_and_undefined_are_coerced_to_numbers_in_operators() {
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(nullish_number::plusNull(&ctx), 1.0);
    assert_eq!(nullish_number::minusNull(&ctx), -1.0);
    assert_eq!(nullish_number::nullOnTheRight(&ctx), 1.0);
    assert!(nullish_number::timesUndefined(&ctx).is_nan());
    assert!(nullish_number::negatedUndefined(&ctx).is_nan());
    assert!(nullish_number::nullLess(&ctx));
    assert!(!nullish_number::undefinedLess(&ctx));
    assert!(!nullish_number::undefinedAtLeast(&ctx));
    assert_eq!(nullish_number::negatedNull(&ctx), 0.0);
    assert!(nullish_number::negatedNull(&ctx).is_sign_negative());
    assert_eq!(nullish_number::bitNotUndefined(&ctx), -1.0);
    println!("ok");
}
"#;
    let out = compile_module(NULLISH_NUMBER_SOURCE);
    assert!(
        !out.has_errors(),
        "`null` and `undefined` must be coerced to numbers in operators; got {:?}",
        out.diagnostics
    );
    let rust = crate_rust_of(NULLISH_NUMBER_SOURCE);
    assert!(!rust.contains("() +"), "got:\n{rust}");
    check_generated_crate("nullish_number", &rust);
    let stdout = run_generated_crate("nullish_number", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_unary_operator_on_an_optional_operand_is_refused() {
    for (source, op) in [
        (
            "export function f(v: number | undefined): number { return -v; }",
            "`-`",
        ),
        (
            "let g: number | null = null; export function f(): number { return -g; }",
            "`-`",
        ),
        (
            "export function f(v: number | undefined): number { return ~v; }",
            "`~`",
        ),
        (
            "export function f(v: number | undefined): number { v++; return 1; }",
            "compound update",
        ),
        (
            "export function f(v: number | undefined): number { v += 1; return 1; }",
            "compound update",
        ),
    ] {
        let out = compile_module(source);
        assert!(
            out.diagnostics
                .iter()
                .any(|d| d.code.as_str() == "P0018" && d.message.contains(op)),
            "`{source}` must be refused with a P0018 naming {op}; got {:?}",
            out.diagnostics
        );
        assert!(
            out.rust_source.is_none() || !out.has_errors(),
            "a refused operand must not produce a Rust crate; got {:?}",
            out.rust_source
        );
    }
}

#[test]
fn an_optional_operand_in_arithmetic_is_still_refused() {
    for source in [
        "export function f(v: number | undefined): number { return v + 1; }",
        "export function f(v: number | null): number { return v * 2; }",
        "export function f(v: number | undefined): boolean { return v < 2; }",
        "export function f(v: number | undefined): string { return v + \"x\"; }",
    ] {
        let out = compile_module(source);
        assert!(
            out.diagnostics.iter().any(|d| d.code.as_str() == "P0018"),
            "`{source}` must be refused with P0018; got {:?}",
            out.diagnostics
        );
    }
}

const BITWISE_NOT_SOURCE: &str = "
export function fractional(): number { return ~1.5; }
export function zero(): number { return ~0.0; }
export function minusOne(): number { return ~-1; }
export function fromNull(): number { return ~null; }
export function fromUndefined(): number { return ~undefined; }
export function overInt32Max(): number { return ~2147483648; }
export function overUint32Max(): number { return ~4294967295; }
export function negativeFractional(): number { return ~-2.7; }
export function large(): number { return ~1e10; }
export function onI32(v: i32): i32 { return ~v; }
export function onNumber(v: number): number { return ~v; }
";

#[test]
fn a_bitwise_not_converts_its_operand_with_to_int32() {
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(bitwise_not::fractional(&ctx), -2.0);
    assert_eq!(bitwise_not::zero(&ctx), -1.0);
    assert_eq!(bitwise_not::minusOne(&ctx), 0.0);
    assert_eq!(bitwise_not::fromNull(&ctx), -1.0);
    assert_eq!(bitwise_not::fromUndefined(&ctx), -1.0);
    assert_eq!(bitwise_not::overInt32Max(&ctx), 2147483647.0);
    assert_eq!(bitwise_not::overUint32Max(&ctx), 0.0);
    assert_eq!(bitwise_not::negativeFractional(&ctx), 1.0);
    assert_eq!(bitwise_not::large(&ctx), -1410065409.0);
    assert_eq!(bitwise_not::onI32(&ctx, 3), -4);
    assert_eq!(bitwise_not::onNumber(&ctx, 3.9), -4.0);
    println!("ok");
}
"#;
    let out = compile_module(BITWISE_NOT_SOURCE);
    assert!(
        !out.has_errors(),
        "`~` must lower through ToInt32; got {:?}",
        out.diagnostics
    );
    let rust = crate_rust_of(BITWISE_NOT_SOURCE);
    check_generated_crate("bitwise_not", &rust);
    let stdout = run_generated_crate("bitwise_not", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_bitwise_not_of_a_non_numeric_value_is_refused() {
    for source in [
        "export function f(s: string): number { return ~s; }",
        "export function f(b: boolean): number { return -b; }",
    ] {
        let out = compile_module(source);
        assert!(
            out.diagnostics.iter().any(|d| d.code.as_str() == "P0018"),
            "`{source}` must be refused with P0018; got {:?}",
            out.diagnostics
        );
    }
}

const TERNARY_GENERIC_RECEIVER_SOURCE: &str = "
class BoxA {
  dbl<U>(u: U): number { return <number>u * 1; }
}
class BoxB {
  dbl<U>(u: U): number { return <number>u * 2; }
}
export function ternSameClass(cond: boolean): number {
  const a = new BoxA();
  const b = new BoxA();
  const chosen: BoxA = cond ? a : b;
  return chosen.dbl<number>(7);
}
export function directB(): number {
  const b = new BoxB();
  return b.dbl<number>(5);
}
export function directA(): number {
  const a = new BoxA();
  return a.dbl<number>(12);
}
";

#[test]
fn a_generic_method_on_an_annotated_ternary_local_is_specialized_per_class() {
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(ternary_generic::ternSameClass(&ctx, true), 7.0);
    assert_eq!(ternary_generic::ternSameClass(&ctx, false), 7.0);
    assert_eq!(ternary_generic::directB(&ctx), 10.0);
    assert_eq!(ternary_generic::directA(&ctx), 12.0);
    println!("ok");
}
"#;
    let out = compile_module(TERNARY_GENERIC_RECEIVER_SOURCE);
    assert!(
        !out.has_errors(),
        "a generic method call on a same-class ternary receiver and on distinct classes with an \
         identical method name must be specialized per receiver class; got {:?}",
        out.diagnostics
    );
    let rust = crate_rust_of(TERNARY_GENERIC_RECEIVER_SOURCE);
    assert!(!rust.contains("__generic"), "got:\n{rust}");
    check_generated_crate("ternary_generic", &rust);
    let stdout = run_generated_crate("ternary_generic", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}
