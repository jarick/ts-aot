use ts_aot_driver::{CompileOptions, Driver, DriverOutput, EmitStage};

fn compile(source: &str) -> DriverOutput {
    Driver.compile_source(
        "test.ts",
        source,
        &CompileOptions {
            emit: EmitStage::Mir,
        },
    )
}

#[test]
fn driver_compiles_simple_generator_function() {
    let out = compile("function* gen(): i64 { yield 1; return 2; }");
    assert!(
        !out.diagnostics.has_errors(),
        "generator compile failed: {:?}",
        out.diagnostics
    );
    let mir = out.mir_text.expect("Mir text must be set");
    assert!(
        mir.contains("__gen_dispatch_gen"),
        "dispatch function must be emitted, got MIR:\n{mir}"
    );
    assert!(
        mir.contains("ts_aot_runtime_Generator_new"),
        "runtime call to Generator::new must be emitted, got MIR:\n{mir}"
    );
}

#[test]
fn driver_compiles_multi_yield_generator() {
    let out = compile("function* gen(): i64 { yield 1; yield 2; return 3; }");
    assert!(
        !out.diagnostics.has_errors(),
        "multi-yield generator compile failed: {:?}",
        out.diagnostics
    );
    let mir = out.mir_text.expect("Mir text");
    let state_branches = mir.matches("indirect_call(").count();
    assert!(
        state_branches >= 3,
        "expected at least 3 indirect_calls (one per state branch helper), got {state_branches}, MIR:\n{mir}"
    );
}

#[test]
fn driver_lower_generators_splits_function_and_emits_dispatch() {
    let out = compile("function* gen(): i64 { yield 1; return 2; }");
    assert!(
        !out.diagnostics.has_errors(),
        "compile failed: {:?}",
        out.diagnostics
    );
    let mir = out.mir_text.expect("Mir text");
    assert!(
        mir.contains("fn #0 gen() -> "),
        "original `gen` function must still be present as a Plain function, got MIR:\n{mir}"
    );
    assert!(
        mir.contains("fn #1 __gen_dispatch_gen("),
        "dispatch function `__gen_dispatch_gen` must be added by lower_generators, got MIR:\n{mir}"
    );
    let gen_body_end = mir
        .find("fn #1 __gen_dispatch_gen(")
        .expect("dispatch fn must be present");
    let gen_body = &mir[..gen_body_end];
    assert!(
        gen_body.contains("return indirect_call(ts_aot_runtime_Generator_new)(__gen_dispatch_gen)"),
        "original `gen` body must be the constructor returning Generator::new(dispatch), got MIR:\n{gen_body}"
    );
    assert!(
        !gen_body.contains("__ts_aot_generator_get_state"),
        "state machine code (get_state calls) must NOT remain in the original `gen` function body, got MIR:\n{gen_body}"
    );
    let get_state_count = gen_body.matches("__ts_aot_generator_get_state").count();
    assert_eq!(
        get_state_count, 0,
        "original `gen` function body must have zero get_state calls, got {get_state_count}, MIR:\n{gen_body}"
    );
}

#[test]
fn driver_dispatch_uses_yielded_helper_not_struct_literal_with_get_state() {
    let out = compile("function* gen(): i64 { yield 42; return 7; }");
    assert!(
        !out.diagnostics.has_errors(),
        "compile failed: {:?}",
        out.diagnostics
    );
    let mir = out.mir_text.expect("Mir text");
    assert!(
        mir.contains("ts_aot_runtime___ts_aot_generator_yielded"),
        "Yielded branch must call the yielded runtime helper, not construct a struct literal with get_state(), got MIR:\n{mir}"
    );
    assert!(
        mir.contains("ts_aot_runtime___ts_aot_generator_done"),
        "Done branch must call the done runtime helper, got MIR:\n{mir}"
    );
    assert!(
        !mir.contains("struct(0){0:indirect_call(ts_aot_runtime___ts_aot_generator_get_state"),
        "must NOT emit struct literal with get_state field for GeneratorResult::Yielded (was returning state instead of stored value), got MIR:\n{mir}"
    );
}
