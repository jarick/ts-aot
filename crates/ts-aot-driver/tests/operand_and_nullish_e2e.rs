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

fn assert_refused(source: &str, op: &str) {
    let out = compile_module(source);
    assert!(
        out.diagnostics
            .iter()
            .any(|d| d.code.as_str() == "P0018" && d.message.contains(op)),
        "`{source}` must be refused with a P0018 naming {op}; got {:?}",
        out.diagnostics
    );
}

fn assert_runs(package: &str, source: &str, checks: &[&str]) {
    let rust = crate_rust_of(source);
    let body = checks.join("\n");
    let main = format!(
        "use ts_aot_runtime::{{Bump, Ctx, Host}};\n\nstruct NoHost;\nimpl Host for NoHost {{}}\n\n\
         fn main() {{\n    let host = NoHost;\n    let bump = Bump::new();\n    \
         let ctx = Ctx {{ arena: &bump, host: &host }};\n{body}\n    println!(\"ok\");\n}}\n"
    );
    let stdout = run_generated_crate(package, &rust, &main);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_boolean_operand_combined_with_a_number_is_refused() {
    for (source, op) in [
        (
            "export function f(a: boolean, b: number): number { return a + b; }",
            "`+`",
        ),
        (
            "export function f(a: boolean, b: number): boolean { return a == b; }",
            "`==`",
        ),
        (
            "export function f(a: boolean, b: number): boolean { return a < b; }",
            "`<`",
        ),
        (
            "export function f(a: boolean, b: number): number { return a & b; }",
            "`&`",
        ),
        (
            "export function f(a: boolean, b: boolean): boolean { return a & b; }",
            "`&`",
        ),
        (
            "export function f(a: boolean, b: boolean): number { return a + b; }",
            "`+`",
        ),
    ] {
        assert_refused(source, op);
    }
    assert_runs(
        "boolean_operands",
        "export function equal(): boolean { return true == false; }\n\
         export function ordered(): boolean { return false < true; }",
        &[
            "    assert!(!boolean_operands::equal(&ctx));",
            "    assert!(boolean_operands::ordered(&ctx));",
        ],
    );
}

#[test]
fn a_string_operand_combined_with_a_number_is_refused() {
    for (source, op) in [
        (
            "export function f(a: string, b: number): boolean { return a == b; }",
            "`==`",
        ),
        (
            "export function f(a: string, b: number): boolean { return a < b; }",
            "`<`",
        ),
        (
            "export function f(a: string, b: number): number { return a * b; }",
            "`*`",
        ),
        (
            "export function f(a: string, b: number): number { return a - b; }",
            "`-`",
        ),
        (
            "export function f(a: number, b: string): number { return a / b; }",
            "`/`",
        ),
    ] {
        assert_refused(source, op);
    }
    assert_runs(
        "string_number_operands",
        "export function trailing(): boolean { return (\"x\" + 5) == \"x5\"; }\n\
         export function boolTail(): boolean { return (\"x\" + true) == \"xtrue\"; }\n\
         export function leading(): boolean { return (5 + \"x\") == \"5x\"; }\n\
         export function mixed(): number { return 1.5 + 2; }",
        &[
            "    assert!(string_number_operands::trailing(&ctx));",
            "    assert!(string_number_operands::boolTail(&ctx));",
            "    assert!(string_number_operands::leading(&ctx));",
            "    assert_eq!(string_number_operands::mixed(&ctx), 3.5);",
        ],
    );
}

#[test]
fn a_string_ordering_comparison_is_refused() {
    for (source, op) in [
        (
            "export function f(a: string, b: string): boolean { return a < b; }",
            "`<`",
        ),
        (
            "export function f(a: string, b: string): boolean { return a >= b; }",
            "`>=`",
        ),
    ] {
        assert_refused(source, op);
    }
    assert_runs(
        "string_equality",
        "export function same(): boolean { return \"a\" == \"a\"; }\n\
         export function different(): boolean { return \"a\" == \"b\"; }",
        &[
            "    assert!(string_equality::same(&ctx));",
            "    assert!(!string_equality::different(&ctx));",
        ],
    );
}

#[test]
fn an_object_operand_in_string_concatenation_is_refused() {
    for (source, op) in [
        (
            "class C { x: number = 0; }\n\
             export function f(b: C): string { return \"x\" + b; }",
            "`+`",
        ),
        (
            "export function f(b: number[]): string { return b + \"x\"; }",
            "`+`",
        ),
    ] {
        assert_refused(source, op);
    }
}

#[test]
fn an_object_equality_comparison_is_refused() {
    for (source, op) in [
        (
            "class C { x: number = 0; }\n\
             export function f(a: C, b: C): boolean { return a == b; }",
            "`==`",
        ),
        (
            "export function f(a: number[], b: number[]): boolean { return a === b; }",
            "`===`",
        ),
    ] {
        assert_refused(source, op);
    }
}
