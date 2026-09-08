use ts_aot_driver::{CompileOptions, Driver, EmitStage};

mod common;

use common::normalize_rust;

fn compile_mir(source: &str) -> ts_aot_driver::DriverOutput {
    let opts = CompileOptions::with_emit(EmitStage::Mir);
    Driver::new().compile_source("test.ts", source, &opts)
}

fn skip_type_annotation(s: &str) -> &str {
    s.strip_prefix(':').map_or(s, |rest| {
        let end = rest
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(rest.len());
        &rest[end..]
    })
}

fn assert_node_with_op(normalized: &str, parent: &str, inner: &str, op: &str, context: &str) {
    let pattern = format!("{parent}({inner})");
    let pos = normalized.find(&pattern).unwrap_or_else(|| {
        panic!(
            "MIR must contain parent node `{parent}` wrapping `{inner}` directly ({context}); \
             expected `{pattern}` substring not found in:\n{normalized}"
        );
    });
    let after = skip_type_annotation(&normalized[pos + pattern.len()..]);
    assert!(
        after.starts_with(op),
        "MIR must apply `{op}` directly to `{parent}`'s result ({context}); \
         got `{after:?}` after `{pattern}` in:\n{normalized}"
    );
}

#[test]
fn e2e_optional_field_lowering_wires_owner_through_optional_chain() {
    let out =
        compile_mir("class C { x: i64 = 0; } function f(c: C | null): i64 | null { return c?.x; }");
    assert!(
        !out.has_errors(),
        "c?.x on a class field must compile through full pipeline; got {:?}",
        out.diagnostics
    );
    let mir = out
        .mir_text
        .expect("emit-mir must populate mir_text for optional chain e2e check");
    let normalized = normalize_rust(&mir);
    assert_node_with_op(
        &normalized,
        "opt",
        "local(0)",
        ".0",
        "c?.x: OptionalChain wraps the Local owner directly, and `.0` field access is applied \
         to the OptionalChain result (not to the Local alone, not nested deeper)",
    );
}

#[test]
fn e2e_optional_index_lowering_wires_owner_through_optional_chain() {
    let out = compile_mir(
        "class C { x: i64 = 0; } function f(c: C | null): i64 | null { return c?.[0]; }",
    );
    assert!(
        !out.has_errors(),
        "c?.[0] must compile through full pipeline; got {:?}",
        out.diagnostics
    );
    let mir = out
        .mir_text
        .expect("emit-mir must populate mir_text for optional index e2e check");
    let normalized = normalize_rust(&mir);
    assert_node_with_op(
        &normalized,
        "opt",
        "local(0)",
        "[0",
        "c?.[0]: OptionalChain wraps the Local owner directly, and `[0` index is applied \
         to the OptionalChain result (not to the Local alone, not wrapped around a completed index)",
    );
}

#[test]
fn e2e_optional_call_lowering_wires_callee_through_optional_chain() {
    let out = compile_mir("function f(g: (() => i64) | null): i64 | null { return g?.(); }");
    assert!(
        !out.has_errors(),
        "g?.() must compile through full pipeline; got {:?}",
        out.diagnostics
    );
    let mir = out
        .mir_text
        .expect("emit-mir must populate mir_text for optional call e2e check");
    let normalized = normalize_rust(&mir);
    assert_node_with_op(
        &normalized,
        "indirect_call",
        "opt(local(0)):6",
        "()",
        "g?.(): indirect_call's argument is the OptionalChain wrapping the Local callee directly, \
         and the call `()` is applied directly to indirect_call's result",
    );
}

#[test]
fn e2e_delete_with_optional_member_lowers_inner_to_optional_chain() {
    let out = compile_mir("class C { x: i64 = 0; } function f(c: C | null): void { delete c?.x; }");
    assert!(
        !out.has_errors(),
        "delete c?.x on a class field must compile through full pipeline; got {:?}",
        out.diagnostics
    );
    let mir = out
        .mir_text
        .expect("emit-mir must populate mir_text for delete + optional e2e check");
    let normalized = normalize_rust(&mir);
    assert_node_with_op(
        &normalized,
        "opt",
        "local(0)",
        ".0",
        "delete c?.x: OptionalChain wraps the Local owner directly, and `.0` field access is \
         applied to the OptionalChain result",
    );
}

#[test]
fn e2e_short_circuit_chain_wraps_each_question_dot_in_optional_chain() {
    let out = compile_mir(
        "class C { b: C | null = null; d: i64 = 0; } \
         function f(a: C | null): i64 | null { return a?.b?.d; }",
    );
    assert!(
        !out.has_errors(),
        "`a?.b?.d` must compile through full pipeline; got {:?}",
        out.diagnostics
    );
    let mir = out
        .mir_text
        .expect("emit-mir must populate mir_text for short-circuit chain e2e check");
    let normalized = normalize_rust(&mir);
    assert_node_with_op(
        &normalized,
        "opt",
        "local(0)",
        ".0",
        "a?.b?.d inner: first OptionalChain wraps the Local owner `a` directly, and `.0` (b) \
         field access is applied to the inner OptionalChain result",
    );
    assert_node_with_op(
        &normalized,
        "opt",
        "opt(local(0)):5.0:7",
        ".1",
        "a?.b?.d outer: second OptionalChain wraps the inner OptionalChain (with its `.0` field \
         access) directly, and `.1` (d) field access is applied to the outer OptionalChain result",
    );
}

#[test]
fn e2e_optional_method_call_wraps_owner_then_uses_indirect_call() {
    let out = compile_mir(
        "class C { m: () => i64 | null = () => 0; } \
         function f(c: C | null): i64 | null { return c?.m(); }",
    );
    assert!(
        !out.has_errors(),
        "method call `c?.m()` must compile through full pipeline; got {:?}",
        out.diagnostics
    );
    let mir = out
        .mir_text
        .expect("emit-mir must populate mir_text for method call e2e check");
    let normalized = normalize_rust(&mir);
    assert_node_with_op(
        &normalized,
        "indirect_call",
        "opt(local(0)):6.0:7",
        "()",
        "c?.m(): indirect_call's argument is the OptionalChain with its `.0` (m) field access, \
         and the call `()` is applied directly to indirect_call's result",
    );
}

#[test]
fn e2e_chained_field_access_inside_ternary_lowers_without_p0012() {
    let out = compile_mir(
        "class C { b: C | null = null; d: i64 = 0; } \
         function f(a: C | null): i64 | null { return a ? a.b?.d : null; }",
    );
    assert!(
        !out.has_errors(),
        "ternary with chained optional field access (a ? a.b?.d : null) must lower; got {:?}",
        out.diagnostics
    );
}

#[test]
fn e2e_optional_chain_inside_no_capture_closure_body_with_typed_param() {
    let out = compile_mir(
        "class C { x: i64 = 0; } \
         function f(c: C): (c: C) => i64 | null { return (c: C): i64 | null => c?.x; }",
    );
    assert!(
        !out.has_errors(),
        "no-capture closure over `c?.x` (typed param shadow) must compile through full pipeline; \
         got {:?}",
        out.diagnostics
    );
    let mir = out
        .mir_text
        .expect("emit-mir must populate mir_text for closure with typed param e2e check");
    let normalized = normalize_rust(&mir);
    assert_node_with_op(
        &normalized,
        "opt",
        "local(0)",
        ".0",
        "closure c?.x: OptionalChain wraps the Local owner directly inside the closure body, \
         and `.0` field access is applied to the OptionalChain result",
    );
}
