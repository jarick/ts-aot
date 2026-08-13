use ts_aot_driver::{CompileOptions, Driver, DriverOutput, EmitStage};

mod common;

fn compile(src: &str) -> DriverOutput {
    let opts = CompileOptions {
        emit: EmitStage::Rust,
    };
    Driver::new().compile_source("test.ts", src, &opts)
}

#[test]
fn e2e_array_spread_emits_concat_call() {
    let out = compile(
        r"
        function f(a: i64[]): i64[] {
            return [...a, 1];
        }
        ",
    );
    assert!(!out.has_errors(), "must compile; got {:?}", out.diagnostics);
    let rust = out.rust_source.clone().expect("emit");
    let rust = common::normalize_rust(&rust);
    assert!(
        rust.contains("__ts_aot_array_concat"),
        "spread must lower to __ts_aot_array_concat, got:\n{rust}"
    );
    assert!(
        rust.contains("::<"),
        "concat must be monomorphized with turbofish, got:\n{rust}"
    );
}

#[test]
fn e2e_array_spread_with_multiple_sources_preserves_operand_sequence() {
    let out = compile(
        r"
        function f(a: i64[], b: i64[]): i64[] {
            return [0, ...a, 1, ...b, 2];
        }
        ",
    );
    assert!(!out.has_errors(), "must compile; got {:?}", out.diagnostics);
    let rust = out.rust_source.clone().expect("emit");
    let rust = common::normalize_rust(&rust);
    let call_marker = "__ts_aot_array_concat::<i64>(vec !";
    let call_start = rust.find(call_marker).expect("concat call must be present");
    let after = &rust[call_start + call_marker.len()..];
    let inner_end = after
        .find("])")
        .expect("concat call must terminate with ])");
    let inner = &after[..inner_end];
    let inner = inner.trim_start().trim_start_matches('[').trim_start();
    let operands: Vec<&str> = inner.split(", ").collect();
    assert_eq!(
        operands,
        vec!["_", "a", "_", "b", "_"],
        "[0, ...a, 1, ...b, 2] must lower to [_ (literal 0), a (spread), _ (literal 1), b (spread), _ (literal 2)] in source order, got:\n{rust}"
    );
}

#[test]
fn e2e_array_without_spread_uses_array_create() {
    let out = compile(
        r"
        function f(): i64[] {
            return [1, 2, 3];
        }
        ",
    );
    assert!(!out.has_errors(), "must compile; got {:?}", out.diagnostics);
    let rust = out.rust_source.clone().expect("emit");
    let rust = common::normalize_rust(&rust);
    assert!(
        rust.contains("__ts_aot_array_create"),
        "plain literal must lower to __ts_aot_array_create, got:\n{rust}"
    );
    assert!(
        !rust.contains("__ts_aot_array_concat"),
        "plain literal must NOT use concat, got:\n{rust}"
    );
}
