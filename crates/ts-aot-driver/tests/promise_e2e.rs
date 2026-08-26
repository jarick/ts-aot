use ts_aot_driver::{CompileOptions, Driver, EmitStage};

mod common;

fn has_closure_ref(haystack: &str) -> bool {
    const PREFIX: &str = "__ts_aot_closure_";
    haystack.match_indices(PREFIX).any(|(i, _)| {
        haystack
            .as_bytes()
            .get(i + PREFIX.len())
            .is_some_and(u8::is_ascii_digit)
    })
}

#[test]
fn e2e_promise_all_emits_runtime_call_with_element_turbofish() {
    let opts = CompileOptions {
        emit: EmitStage::Rust,
    };
    let out = Driver::new().compile_source(
        "test.ts",
        "function f(promises: Array<Promise<number>>): Promise<Array<number>> { \
            return Promise.all(promises); \
         }",
        &opts,
    );
    assert!(!out.has_errors(), "must compile; got {:?}", out.diagnostics);
    let rust = out
        .rust_source
        .expect("emit-rust must populate rust_source");
    let rust = common::normalize_rust(&rust);
    assert!(
        rust.contains("__ts_aot_promise_all"),
        "Promise.all must lower to __ts_aot_promise_all runtime call, got:\n{rust}"
    );
}

#[test]
fn e2e_promise_race_emits_runtime_call() {
    let opts = CompileOptions {
        emit: EmitStage::Rust,
    };
    let out = Driver::new().compile_source(
        "test.ts",
        "function f(promises: Array<Promise<number>>): Promise<number> { \
            return Promise.race(promises); \
         }",
        &opts,
    );
    assert!(!out.has_errors(), "must compile; got {:?}", out.diagnostics);
    let rust = out
        .rust_source
        .expect("emit-rust must populate rust_source");
    let rust = common::normalize_rust(&rust);
    assert!(
        rust.contains("__ts_aot_promise_race"),
        "Promise.race must lower to runtime call, got:\n{rust}"
    );
}

#[test]
fn e2e_promise_all_settled_emits_runtime_call() {
    let opts = CompileOptions {
        emit: EmitStage::Rust,
    };
    let out = Driver::new().compile_source(
        "test.ts",
        "function f(promises: Array<Promise<number>>): Promise<Array<{ status: string; value?: number; reason?: string }>> { \
            return Promise.allSettled(promises); \
         }",
        &opts,
    );
    assert!(!out.has_errors(), "must compile; got {:?}", out.diagnostics);
    let rust = out
        .rust_source
        .expect("emit-rust must populate rust_source");
    let rust = common::normalize_rust(&rust);
    assert!(
        rust.contains("__ts_aot_promise_all_settled"),
        "Promise.allSettled must lower to runtime call, got:\n{rust}"
    );
}

#[test]
fn e2e_promise_any_emits_runtime_call() {
    let opts = CompileOptions {
        emit: EmitStage::Rust,
    };
    let out = Driver::new().compile_source(
        "test.ts",
        "function f(promises: Array<Promise<number>>): Promise<number> { \
            return Promise.any(promises); \
         }",
        &opts,
    );
    assert!(!out.has_errors(), "must compile; got {:?}", out.diagnostics);
    let rust = out
        .rust_source
        .expect("emit-rust must populate rust_source");
    let rust = common::normalize_rust(&rust);
    assert!(
        rust.contains("__ts_aot_promise_any"),
        "Promise.any must lower to runtime call, got:\n{rust}"
    );
}

#[test]
fn e2e_promise_resolve_emits_runtime_call_with_value_ty() {
    let opts = CompileOptions {
        emit: EmitStage::Rust,
    };
    let out = Driver::new().compile_source(
        "test.ts",
        "function f(): Promise<number> { return Promise.resolve(42); }",
        &opts,
    );
    assert!(!out.has_errors(), "must compile; got {:?}", out.diagnostics);
    let rust = out
        .rust_source
        .expect("emit-rust must populate rust_source");
    let rust = common::normalize_rust(&rust);
    assert!(
        rust.contains("__ts_aot_promise_resolve_value"),
        "Promise.resolve must lower to __ts_aot_promise_resolve_value runtime call, got:\n{rust}"
    );
    assert!(
        rust.contains("__ts_aot_promise_resolve_value::<i32>(42)"),
        "Promise.resolve must include the concrete numeric type as the first generic argument; \
         got:\n{rust}"
    );
}

#[test]
fn e2e_promise_reject_emits_runtime_call() {
    let opts = CompileOptions {
        emit: EmitStage::Rust,
    };
    let out = Driver::new().compile_source(
        "test.ts",
        "function f(): Promise<number> { return Promise.reject('boom'); }",
        &opts,
    );
    assert!(!out.has_errors(), "must compile; got {:?}", out.diagnostics);
    let rust = out
        .rust_source
        .expect("emit-rust must populate rust_source");
    let rust = common::normalize_rust(&rust);
    assert!(
        rust.contains("__ts_aot_promise_reject_value"),
        "Promise.reject must lower to __ts_aot_promise_reject_value runtime call, got:\n{rust}"
    );
}

#[test]
fn e2e_promise_then_with_named_handler_emits_instance_call() {
    let opts = CompileOptions {
        emit: EmitStage::Rust,
    };
    let out = Driver::new().compile_source(
        "test.ts",
        "function handler(v: number): number { return v; }\
         function f(p: Promise<number>): Promise<number> { return p.then(handler); }",
        &opts,
    );
    assert!(!out.has_errors(), "must compile; got {:?}", out.diagnostics);
    let rust = out
        .rust_source
        .expect("emit-rust must populate rust_source");
    let rust = common::normalize_rust(&rust);
    assert!(
        rust.contains("ts_aot_runtime::Promise<i32>"),
        "p.then(handler) must reference the full Promise type in its dest binding; got:\n{rust}"
    );
    assert!(
        rust.contains("__ts_aot_promise_then_value::<i32,"),
        "p.then(handler) must lower to instance runtime call with the inner promise \
         element type as the turbofish; got:\n{rust}"
    );
    assert!(
        rust.contains("handler"),
        "handler must be referenced in emit, got:\n{rust}"
    );
}

#[test]
fn e2e_promise_catch_with_named_handler_emits_instance_call() {
    let opts = CompileOptions {
        emit: EmitStage::Rust,
    };
    let out = Driver::new().compile_source(
        "test.ts",
        "function err_handler(e: string): number { return 0; }\
         function f(p: Promise<number>): void { p.catch(err_handler); }",
        &opts,
    );
    assert!(!out.has_errors(), "must compile; got {:?}", out.diagnostics);
    let rust = out
        .rust_source
        .expect("emit-rust must populate rust_source");
    let rust = common::normalize_rust(&rust);
    assert!(
        rust.contains("__ts_aot_promise_catch_value"),
        "p.catch(handler) must lower to instance runtime call, got:\n{rust}"
    );
}

#[test]
fn e2e_promise_finally_with_named_handler_emits_instance_call() {
    let opts = CompileOptions {
        emit: EmitStage::Rust,
    };
    let out = Driver::new().compile_source(
        "test.ts",
        "function cleanup(): void {}\
         function f(p: Promise<number>): void { p.finally(cleanup); }",
        &opts,
    );
    assert!(!out.has_errors(), "must compile; got {:?}", out.diagnostics);
    let rust = out
        .rust_source
        .expect("emit-rust must populate rust_source");
    let rust = common::normalize_rust(&rust);
    assert!(
        rust.contains("__ts_aot_promise_finally_value"),
        "p.finally(handler) must lower to instance runtime call, got:\n{rust}"
    );
}

#[test]
fn e2e_promise_type_annotation_resolves_to_promise_emit_with_inner_type() {
    let opts = CompileOptions {
        emit: EmitStage::Rust,
    };
    let out = Driver::new().compile_source(
        "test.ts",
        "function f(p: Promise<number>): Promise<Array<number>> { \
            return Promise.all([p]); \
         }",
        &opts,
    );
    assert!(!out.has_errors(), "must compile; got {:?}", out.diagnostics);
    let rust = out
        .rust_source
        .expect("emit-rust must populate rust_source");
    let rust = common::normalize_rust(&rust);
    assert!(
        rust.contains("ts_aot_runtime::Promise<i32>"),
        "Promise<number> must lower to ts_aot_runtime::Promise<i32> (regression: \
         Promise<T> was missing from BUILTIN_GENERICS, so T-anon fell back to Type::Error \
         and emitted as `()`); got:\n{rust}"
    );
}

#[test]
fn e2e_promise_reject_with_string_local_emits_jsstring_conversion_not_bare_tostring() {
    let opts = CompileOptions {
        emit: EmitStage::Rust,
    };
    let out = Driver::new().compile_source(
        "test.ts",
        "function f(s: string): Promise<void> { \
            return Promise.reject(s); \
         }",
        &opts,
    );
    assert!(!out.has_errors(), "must compile; got {:?}", out.diagnostics);
    let rust = out
        .rust_source
        .expect("emit-rust must populate rust_source");
    let rust = common::normalize_rust(&rust);
    assert!(
        rust.contains("__ts_aot_promise_reject"),
        "Promise.reject must lower to runtime call, got:\n{rust}"
    );
    assert!(
        rust.contains("to_string_lossy"),
        "Promise.reject with a string-typed Local must convert via JsString::to_string_lossy \
         (regression: shape-based guard matched only MirExpr::Local, missing Field/Index; \
         bare `.to_string()` on a JsString does not compile); got:\n{rust}"
    );
    assert!(
        !rust.contains("s . to_string ()"),
        "Promise.reject must not emit bare `.to_string()` on a JsString-backed local; got:\n{rust}"
    );
}

#[test]
fn e2e_promise_reject_with_string_index_access_emits_jsstring_conversion() {
    let opts = CompileOptions {
        emit: EmitStage::Rust,
    };
    let out = Driver::new().compile_source(
        "test.ts",
        "function f(arr: string[]): Promise<void> { \
            return Promise.reject(arr[0]); \
         }",
        &opts,
    );
    assert!(!out.has_errors(), "must compile; got {:?}", out.diagnostics);
    let rust = out
        .rust_source
        .expect("emit-rust must populate rust_source");
    let rust = common::normalize_rust(&rust);
    assert!(
        rust.contains("to_string_lossy"),
        "Promise.reject with a string-typed Index access must convert via JsString::to_string_lossy \
         (regression: HIR Index ty was Type::Error placeholder, never resolved; \
         guard never matched, emitted bare `.to_string()` on JsString which doesn't compile); \
         got:\n{rust}"
    );
    assert!(
        !rust.contains("[0].to_string()"),
        "Promise.reject must not emit bare `.to_string()` on a JsString-backed indexed value; \
         got:\n{rust}"
    );
}

#[test]
fn e2e_promise_then_with_captureless_arrow_handler_emits_instance_call() {
    let opts = CompileOptions {
        emit: EmitStage::Rust,
    };
    let out = Driver::new().compile_source(
        "test.ts",
        "function f(p: Promise<number>): Promise<number> { \
            return p.then((v: number): number => v); \
         }",
        &opts,
    );
    assert!(!out.has_errors(), "must compile; got {:?}", out.diagnostics);
    let rust = out
        .rust_source
        .expect("emit-rust must populate rust_source");
    let rust = common::normalize_rust(&rust);
    assert!(
        rust.contains("ts_aot_runtime::Promise<i32>"),
        "p.then(arrow) must reference the full Promise type in its dest binding; got:\n{rust}"
    );
    assert!(
        rust.contains("__ts_aot_promise_then_value::<i32,"),
        "p.then(arrow) must lower to instance runtime call with the inner promise element \
         type as the turbofish; got:\n{rust}"
    );
    assert!(
        has_closure_ref(&rust),
        "p.then(arrow) must reference the hoisted closure fn by name; got:\n{rust}"
    );
    assert!(
        rust.contains("fn __ts_aot_closure_"),
        "p.then(arrow) must emit a `fn __ts_aot_closure_<n>` definition; got:\n{rust}"
    );
    assert!(
        rust.contains("(v : i32)"),
        "p.then(arrow) hoisted closure must declare the captured parameter typed i32; \
         got:\n{rust}"
    );
    assert!(
        rust.contains("->i32"),
        "p.then(arrow) hoist must set HirFunction.ret to the arrow's return type annotation \
         (i32), not the closure's Type::Fn (regression: lower_closures was setting \
         ret=Type::Fn which made handler_ret incompatible with Promise ok type); \
         got:\n{rust}"
    );
}
