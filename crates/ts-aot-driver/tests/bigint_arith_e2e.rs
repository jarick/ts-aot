use ts_aot_driver::{CompileOptions, Driver, EmitStage};

mod common;

use common::normalize_rust;

fn compile_mir(source: &str) -> ts_aot_driver::DriverOutput {
    let opts = CompileOptions::with_emit(EmitStage::Mir);
    Driver::new().compile_source("test.ts", source, &opts)
}

fn assert_runtime_call(normalized: &str, op: &str, context: &str) {
    let pattern = format!("{op}(");
    normalized.find(&pattern).unwrap_or_else(|| {
        panic!(
            "MIR must contain a runtime call to `{op}(` ({context}); expected `{pattern}` not found in:\n{normalized}"
        );
    });
}

fn assert_no_errors(out: &ts_aot_driver::DriverOutput, context: &str) {
    assert!(
        !out.has_errors(),
        "{context} must compile without errors; got diagnostics: {:?}",
        out.diagnostics
    );
}

fn normalized_mir(source: &str) -> String {
    let out = compile_mir(source);
    assert_no_errors(&out, source);
    let mir = out
        .mir_text
        .expect("emit-mir must populate mir_text for bigint e2e");
    normalize_rust(&mir)
}

#[test]
fn bigint_literal_emits_bigint_handle() {
    let normalized = normalized_mir("function f(): bigint { return 42n; }");
    assert!(
        normalized.contains("bigint(\"42\")"),
        "MIR dump must preserve BigIntLiteral `42n` as `bigint(\"42\")` shape (lowered to \
         __ts_aot_bigint_new at backend emit); got:\n{normalized}"
    );
}

#[test]
fn bigint_addition_emits_bigint_add_runtime() {
    let normalized = normalized_mir("function f(a: bigint, b: bigint): bigint { return a + b; }");
    assert_runtime_call(&normalized, "bigint_add", "a + b on two BigInts");
}

#[test]
fn bigint_subtraction_emits_bigint_sub_runtime() {
    let normalized = normalized_mir("function f(a: bigint, b: bigint): bigint { return a - b; }");
    assert_runtime_call(&normalized, "bigint_sub", "a - b on two BigInts");
}

#[test]
fn bigint_multiplication_emits_bigint_mul_runtime() {
    let normalized = normalized_mir("function f(a: bigint, b: bigint): bigint { return a * b; }");
    assert_runtime_call(&normalized, "bigint_mul", "a * b on two BigInts");
}

#[test]
fn bigint_division_emits_bigint_div_runtime() {
    let normalized = normalized_mir("function f(a: bigint, b: bigint): bigint { return a / b; }");
    assert_runtime_call(&normalized, "bigint_div", "a / b on two BigInts");
}

#[test]
fn bigint_remainder_emits_bigint_rem_runtime() {
    let normalized = normalized_mir("function f(a: bigint, b: bigint): bigint { return a % b; }");
    assert_runtime_call(&normalized, "bigint_rem", "a % b on two BigInts");
}

#[test]
fn bigint_negation_emits_bigint_neg_runtime() {
    let normalized = normalized_mir("function f(a: bigint): bigint { return -a; }");
    assert_runtime_call(&normalized, "bigint_neg", "-a on a BigInt");
}

#[test]
fn bigint_bitnot_emits_bigint_not_runtime() {
    let normalized = normalized_mir("function f(a: bigint): bigint { return ~a; }");
    assert_runtime_call(&normalized, "bigint_not", "~a on a BigInt");
}

#[test]
fn bigint_lessthan_emits_bigint_lt_runtime() {
    let normalized = normalized_mir("function f(a: bigint, b: bigint): boolean { return a < b; }");
    assert_runtime_call(&normalized, "bigint_lt", "a < b on two BigInts");
}

#[test]
fn bigint_equality_emits_bigint_eq_runtime() {
    let normalized = normalized_mir("function f(a: bigint, b: bigint): boolean { return a == b; }");
    assert_runtime_call(&normalized, "bigint_eq", "a == b on two BigInts");
}

#[test]
fn bigint_bitand_emits_bigint_and_runtime() {
    let normalized = normalized_mir("function f(a: bigint, b: bigint): bigint { return a & b; }");
    assert_runtime_call(&normalized, "bigint_and", "a & b on two BigInts");
}

#[test]
fn bigint_bitor_emits_bigint_or_runtime() {
    let normalized = normalized_mir("function f(a: bigint, b: bigint): bigint { return a | b; }");
    assert_runtime_call(&normalized, "bigint_or", "a | b on two BigInts");
}

#[test]
fn bigint_bitxor_emits_bigint_xor_runtime() {
    let normalized = normalized_mir("function f(a: bigint, b: bigint): bigint { return a ^ b; }");
    assert_runtime_call(&normalized, "bigint_xor", "a ^ b on two BigInts");
}

#[test]
fn bigint_shl_emits_bigint_shl_runtime() {
    let normalized = normalized_mir("function f(a: bigint, b: bigint): bigint { return a << b; }");
    assert_runtime_call(&normalized, "bigint_shl", "a << b on two BigInts");
}

#[test]
fn bigint_shr_emits_bigint_shr_runtime() {
    let normalized = normalized_mir("function f(a: bigint, b: bigint): bigint { return a >> b; }");
    assert_runtime_call(&normalized, "bigint_shr", "a >> b on two BigInts");
}

#[test]
fn bigint_constructor_from_string_emits_bigint_new_runtime() {
    let normalized = normalized_mir("function f(s: string): bigint { return BigInt(s); }");
    assert_runtime_call(&normalized, "bigint_new", "BigInt(s) on a string");
}

#[test]
fn bigint_constructor_from_int_emits_p0005_diagnostic() {
    let out = compile_mir("function f(n: i64): bigint { return BigInt(n); }");
    assert!(
        out.has_errors(),
        "BigInt(i64) must emit a P0005 diagnostic (number arg currently unsupported); \
         got: {:?}",
        out.diagnostics
    );
    let diag_codes: Vec<&str> = out.diagnostics.iter().map(|d| d.code.as_str()).collect();
    assert!(
        diag_codes.contains(&"P0005"),
        "expected P0005 for BigInt(i64) unsupported, got codes: {diag_codes:?}"
    );
}

#[test]
fn bigint_arithmetic_with_known_type_chains() {
    let normalized = normalized_mir("function f(): bigint { return 1n + 2n * 3n; }");
    assert_runtime_call(&normalized, "bigint_add", "1n + (2n * 3n)");
    assert_runtime_call(&normalized, "bigint_mul", "2n * 3n");
}

#[test]
fn bigint_type_annotation_emits_class_field_with_bigint_typeid() {
    let normalized =
        normalized_mir("class C { x: bigint = 42n; } function f(c: C): bigint { return c.x; }");
    assert!(
        normalized.contains("struct C { x: 2,"),
        "MIR dump must contain the class with a typed field (the BigInt TypeId is the field's \
         declared type slot); got:\n{normalized}"
    );
    assert!(
        normalized.contains("local(0).0"),
        "MIR dump must contain `c.x` field access path `local(0).0`; got:\n{normalized}"
    );
}

#[test]
fn bigint_to_string_emits_bigint_to_string_runtime() {
    let normalized = normalized_mir("function f(a: bigint): string { return a.toString(); }");
    assert_runtime_call(&normalized, "bigint_to_string", "a.toString() on a BigInt");
}

#[test]
fn bigint_to_number_emits_bigint_to_number_runtime() {
    let normalized = normalized_mir("function f(a: bigint): number { return Number(a); }");
    assert_runtime_call(&normalized, "bigint_to_number", "Number(a) on a BigInt");
}
