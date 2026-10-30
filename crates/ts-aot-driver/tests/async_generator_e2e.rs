use ts_aot_driver::{CompileOptions, Driver, EmitStage};

fn compile_rust(source: &str) -> String {
    let out = Driver::new().compile_source(
        "test.ts",
        source,
        &CompileOptions {
            emit: EmitStage::Rust,
            module: true,
        },
    );
    assert!(!out.has_errors(), "must compile; got {:?}", out.diagnostics);
    out.rust_source
        .expect("emit-rust must populate rust_source")
}

#[test]
fn throwing_async_generator_compiles() {
    let rust = compile_rust("async function* g(): i64 { throw 1; }");
    assert!(
        rust.contains("__ts_aot_async_generator_throw"),
        "throwing async generator must lower to the runtime throw helper, got:\n{rust}"
    );
}

#[test]
fn conditional_return_in_catch_compiles() {
    let rust = compile_rust(
        "function f(c: boolean): i64 { try { throw 1; } catch (e) { if (c) { return 1; } } return 2; }",
    );
    assert!(
        rust.contains("catch_unwind"),
        "a catch with a conditional return must still emit a catch_unwind loop, got:\n{rust}"
    );
}

#[test]
fn continue_inside_switch_rechecks_loop_condition_compiles() {
    let _ = compile_rust(
        "function f(): i64 { let i: i64 = 0; while (i < 5) { switch (1) { case 1: i = i + 1; continue; } } return i; }",
    );
}

#[test]
fn async_explicit_return_compiles() {
    let _ = compile_rust("async function f(): Promise<i64> { return 5; }");
    let _ = compile_rust("async function f(): Promise<void> { return; }");
    let _ = compile_rust(
        "async function f(): Promise<i64> { try { return 5; } catch (e) { return 6; } }",
    );
}

#[test]
fn async_void_fallthrough_compiles() {
    let _ = compile_rust("async function f(): Promise<void> { const x: i64 = 1; }");
}

#[test]
fn function_ending_in_try_compiles() {
    let _ = compile_rust("function f(): i64 { try { return 1; } catch (e) { return 2; } }");
}

#[test]
fn throwing_closure_value_compiles() {
    let _ = compile_rust(
        "function f(): i64 { const g: () => i64 = (): i64 => { try { throw 1; } catch (e) { return 5; } return 0; }; return g(); }",
    );
}

#[test]
fn closure_with_try_compiles() {
    let _ = compile_rust(
        "function f(): i64 { const g: () => i64 = (): i64 => { let i: i64 = 0; while (true) { try { i = i + 1; if (i == 3) { break; } } catch (e) { } } return i; }; return g(); }",
    );
}

#[test]
fn async_throw_in_try_compiles() {
    let rust = compile_rust(
        "async function f(): Promise<i64> { try { throw 1; } catch (e) { if (e === 1) { return 5; } } return 0; }",
    );
    assert!(
        rust.contains("catch_unwind"),
        "an async function with a throwing try must lower to a catch_unwind loop, got:\n{rust}"
    );
}

#[test]
fn async_call_returns_promise_compiles() {
    let rust = compile_rust(
        "async function h(): Promise<i64> { return 1; } async function f(): Promise<i64> { return h(); }",
    );
    assert!(
        rust.contains("__ts_aot_run_or_poll"),
        "a call to an async function must be driven to its Promise via run_or_poll, got:\n{rust}"
    );
}

#[test]
fn async_function_as_value_compiles() {
    let rust = compile_rust(
        "async function h(): Promise<i64> { return 1; } \
         function f(g: () => Promise<i64>): Promise<i64> { return g(); } \
         function m(): Promise<i64> { return f(h); }",
    );
    assert!(
        rust.contains("__ts_aot_run_or_poll"),
        "an async function used as a value must be driven to its Promise via run_or_poll, \
         got:\n{rust}"
    );
}

#[test]
fn try_catch_async_generator_compiles() {
    let rust = compile_rust(
        "async function* g(p: Promise<i64>): i64 { try { const v: i64 = await p; } catch (e) { } return 1; }",
    );
    assert!(
        rust.contains("catch_unwind") && rust.contains("__ts_aot_await_value_raw"),
        "a caught await inside an async generator must lower to a catch_unwind loop using the raw await helper, got:\n{rust}"
    );
}
