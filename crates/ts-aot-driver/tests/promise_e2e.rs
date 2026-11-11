use ts_aot_driver::{CompileOptions, Driver};
use ts_aot_test_support::check_generated_crate;

mod common;

#[test]
fn e2e_promise_all_resolving_to_arena_array_is_rejected() {
    let opts = CompileOptions {
        module: false,
        ..CompileOptions::default()
    };
    let out = Driver::new().compile_source(
        "test.ts",
        "function f(promises: Array<Promise<number>>): Promise<Array<number>> { \
            return Promise.all(promises); \
         }",
        &opts,
    );
    assert!(
        out.has_errors(),
        "Promise.all resolving to an arena-backed array must be rejected instead of emitting \
         Rust that does not compile; got {:?}",
        out.diagnostics
    );
    assert!(
        out.diagnostics
            .iter()
            .any(|d| d.code.as_str() == "E0300" && d.message.contains("borrows the arena")),
        "the rejection must explain that the resolved value borrows the arena; got: {:?}",
        out.diagnostics
    );
}

#[test]
fn e2e_promise_race_is_rejected_with_combinator_diagnostic() {
    let opts = CompileOptions {
        module: false,
        ..CompileOptions::default()
    };
    let out = Driver::new().compile_source(
        "test.ts",
        "function f(promises: Array<Promise<number>>): Promise<number> { \
            return Promise.race(promises); \
         }",
        &opts,
    );
    assert!(
        out.has_errors(),
        "Promise.race over an arena-backed array must be rejected instead of emitting Rust \
         that does not compile; got {:?}",
        out.diagnostics
    );
    assert!(
        out.diagnostics
            .iter()
            .any(|d| d.code.as_str() == "E0300" && d.message.contains("promise combinator")),
        "the rejection must name the promise combinator; got: {:?}",
        out.diagnostics
    );
}

#[test]
fn e2e_promise_all_settled_resolving_to_arena_array_is_rejected() {
    let opts = CompileOptions {
        module: false,
        ..CompileOptions::default()
    };
    let out = Driver::new().compile_source(
        "test.ts",
        "function f(promises: Array<Promise<number>>): Promise<Array<{ status: string; value?: number; reason?: string }>> { \
            return Promise.allSettled(promises); \
         }",
        &opts,
    );
    assert!(
        out.has_errors(),
        "Promise.allSettled resolving to an arena-backed array must be rejected instead of \
         emitting Rust that does not compile; got {:?}",
        out.diagnostics
    );
    assert!(
        out.diagnostics
            .iter()
            .any(|d| d.code.as_str() == "E0300" && d.message.contains("borrows the arena")),
        "the rejection must explain that the resolved value borrows the arena; got: {:?}",
        out.diagnostics
    );
}

#[test]
fn e2e_promise_any_is_rejected_with_combinator_diagnostic() {
    let opts = CompileOptions {
        module: false,
        ..CompileOptions::default()
    };
    let out = Driver::new().compile_source(
        "test.ts",
        "function f(promises: Array<Promise<number>>): Promise<number> { \
            return Promise.any(promises); \
         }",
        &opts,
    );
    assert!(
        out.has_errors(),
        "Promise.any over an arena-backed array must be rejected instead of emitting Rust \
         that does not compile; got {:?}",
        out.diagnostics
    );
    assert!(
        out.diagnostics
            .iter()
            .any(|d| d.code.as_str() == "E0300" && d.message.contains("promise combinator")),
        "the rejection must name the promise combinator; got: {:?}",
        out.diagnostics
    );
}

#[test]
fn e2e_promise_resolve_emits_runtime_call_with_value_ty() {
    let opts = CompileOptions {
        module: false,
        ..CompileOptions::default()
    };
    let out = Driver::new().compile_source(
        "test.ts",
        "function f(): Promise<number> { return Promise.resolve(42); }",
        &opts,
    );
    assert!(!out.has_errors(), "must compile; got {:?}", out.diagnostics);
    let rust_raw = out
        .rust_source
        .expect("emit-rust must populate rust_source");
    check_generated_crate("promise_resolve_value", &rust_raw);
    let rust = common::normalize_rust(&rust_raw);
    assert!(
        rust.contains("__ts_aot_promise_resolve_value"),
        "Promise.resolve must lower to __ts_aot_promise_resolve_value runtime call, got:\n{rust}"
    );
    assert!(
        rust.contains("__ts_aot_promise_resolve_value::<f64>(42.0)"),
        "Promise.resolve must coerce the numeric literal to the resolved value type and pass it \
         as the first generic argument; got:\n{rust}"
    );
    assert!(
        rust.contains("__tmp0 : ts_aot_runtime::Promise<f64>"),
        "the resolve temp must be bound to the concrete promise type (regression: the call's \
         frontend type is `Type::Error`, so the temp used to be `()` and the output did not \
         compile); got:\n{rust}"
    );
}

#[test]
fn e2e_promise_resolve_of_call_uses_callee_return_type() {
    let opts = CompileOptions {
        module: false,
        ..CompileOptions::default()
    };
    let out = Driver::new().compile_source(
        "test.ts",
        "function g(): number { return 1; } \
         function f(): Promise<number> { return Promise.resolve(g()); }",
        &opts,
    );
    assert!(!out.has_errors(), "must compile; got {:?}", out.diagnostics);
    let rust_raw = out
        .rust_source
        .expect("emit-rust must populate rust_source");
    check_generated_crate("promise_resolve_call_ret", &rust_raw);
    let rust = common::normalize_rust(&rust_raw);
    assert!(
        rust.contains("__ts_aot_promise_resolve_value::<f64>(g (__ctx))"),
        "Promise.resolve of a call must resolve with the callee's return type, not the frontend \
         `Type::Error` placeholder (which used to emit `<()>` and fail to compile); got:\n{rust}"
    );
    assert!(
        rust.contains("__tmp0 : ts_aot_runtime::Promise<f64>"),
        "the resolve temp of a call argument must be bound to the concrete promise type; \
         got:\n{rust}"
    );
}

#[test]
fn e2e_promise_resolve_of_promise_adopts_argument() {
    let opts = CompileOptions {
        module: false,
        ..CompileOptions::default()
    };
    let out = Driver::new().compile_source(
        "test.ts",
        "function f(p: Promise<number>): Promise<number> { return Promise.resolve(p); }",
        &opts,
    );
    assert!(!out.has_errors(), "must compile; got {:?}", out.diagnostics);
    let rust_raw = out
        .rust_source
        .expect("emit-rust must populate rust_source");
    check_generated_crate("promise_resolve_adopt", &rust_raw);
    let rust = common::normalize_rust(&rust_raw);
    assert!(
        !rust.contains("__ts_aot_promise_resolve_value"),
        "`Promise.resolve(promise)` adopts the argument; it must not re-resolve a `Promise<T>` \
         into `Promise<Promise<T>>` (which does not compile); got:\n{rust}"
    );
    assert!(
        rust.contains("return p"),
        "the adopted promise must be returned unchanged; got:\n{rust}"
    );
}

#[test]
fn e2e_promise_resolve_of_call_returning_promise_adopts_argument() {
    let opts = CompileOptions {
        module: false,
        ..CompileOptions::default()
    };
    let out = Driver::new().compile_source(
        "test.ts",
        "function g(): Promise<number> { return Promise.resolve(1); } \
         function f(): Promise<number> { return Promise.resolve(g()); }",
        &opts,
    );
    assert!(!out.has_errors(), "must compile; got {:?}", out.diagnostics);
    let rust_raw = out
        .rust_source
        .expect("emit-rust must populate rust_source");
    check_generated_crate("promise_resolve_adopt_call", &rust_raw);
    let rust = common::normalize_rust(&rust_raw);
    assert!(
        !rust.contains("Promise<Promise"),
        "`Promise.resolve(<call returning Promise<T>>)` adopts the callee's promise; it must not \
         wrap it into `Promise<Promise<T>>` (which does not compile); got:\n{rust}"
    );
    assert!(
        rust.contains("return g (__ctx)"),
        "the adopted callee result must be returned unchanged; got:\n{rust}"
    );
}

#[test]
fn e2e_promise_resolve_ok_type_is_interning_order_independent() {
    let opts = CompileOptions {
        module: false,
        ..CompileOptions::default()
    };
    let out = Driver::new().compile_source(
        "test.ts",
        "function first(s: string): string { return s; } \
         function f(): Promise<number> { return Promise.resolve(42); }",
        &opts,
    );
    assert!(!out.has_errors(), "must compile; got {:?}", out.diagnostics);
    let rust = out
        .rust_source
        .expect("emit-rust must populate rust_source");
    let rust = common::normalize_rust(&rust);
    assert!(
        rust.contains("__ts_aot_promise_resolve_value::<f64>(42.0)"),
        "the resolved literal type must not depend on `TypeId(0)` resolving to the first interned \
         type (`string` here); the integer literal in a `Promise<number>` context must be \
         `f64`; got:\n{rust}"
    );
}

#[test]
fn e2e_promise_reject_emits_runtime_call() {
    let opts = CompileOptions {
        module: false,
        ..CompileOptions::default()
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
    assert!(
        rust.contains("__ts_aot_promise_reject_value::<f64>"),
        "Promise.reject must use the enclosing function's promise ok type as the turbofish \
         (regression: the call's frontend type is `Type::Error`, so it used to emit `<()>` and \
         the output did not compile); got:\n{rust}"
    );
    assert!(
        !rust.contains("into_owned"),
        "`JsString::to_string_lossy` already returns `String`, so `.into_owned()` must not be \
         emitted (it does not compile); got:\n{rust}"
    );
}

#[test]
fn e2e_promise_passthrough_of_arena_array_is_allowed() {
    let opts = CompileOptions {
        module: false,
        ..CompileOptions::default()
    };
    let out = Driver::new().compile_source(
        "test.ts",
        "function f(p: Promise<number[]>): Promise<number[]> { return p; }",
        &opts,
    );
    assert!(
        !out.has_errors(),
        "returning an existing `Promise<number[]>` unchanged never resolves an arena value into \
         a promise, so it must compile instead of being rejected by an over-broad signature \
         check; got {:?}",
        out.diagnostics
    );
    let rust = out
        .rust_source
        .expect("emit-rust must populate rust_source");
    let rust = common::normalize_rust(&rust);
    assert!(
        rust.contains("Promise<ts_aot_runtime::bumpalo::collections::Vec<'a, f64>>"),
        "the promise signature must keep the arena-backed element type; got:\n{rust}"
    );
    assert!(
        !rust.contains("__ts_aot_promise_resolve_value"),
        "a passthrough must not resolve the value into a new promise; got:\n{rust}"
    );
}

#[test]
fn e2e_promise_then_with_named_handler_emits_instance_call() {
    let opts = CompileOptions {
        module: false,
        ..CompileOptions::default()
    };
    let out = Driver::new().compile_source(
        "test.ts",
        "function handler(v: number): number { return v; }\
         function f(p: Promise<number>): Promise<number> { return p.then(handler); }",
        &opts,
    );
    assert!(
        out.diagnostics
            .iter()
            .any(|d| d.code.as_str() == "E0507" && d.severity == ts_aot_core::Severity::Error),
        "Promise.prototype.then is not lowerable yet and must be a hard E0507 error until \
         P3; the previous assertion only checked token presence while the emitted Rust did not \
         compile; got {:?}",
        out.diagnostics
    );
    assert!(
        out.rust_source.is_none()
            || !out
                .rust_source
                .as_deref()
                .unwrap_or("")
                .contains("__ts_aot_promise_then_value"),
        "no uncompilable `then` call may be emitted; got {:?}",
        out.rust_source
    );
}

#[test]
fn e2e_promise_catch_with_named_handler_emits_instance_call() {
    let opts = CompileOptions {
        module: false,
        ..CompileOptions::default()
    };
    let out = Driver::new().compile_source(
        "test.ts",
        "function err_handler(e: string): number { return 0; }\
         function f(p: Promise<number>): void { p.catch(err_handler); }",
        &opts,
    );
    assert!(
        out.diagnostics
            .iter()
            .any(|d| d.code.as_str() == "E0507" && d.severity == ts_aot_core::Severity::Error),
        "Promise.prototype.catch is not lowerable yet and must be a hard E0507 error until \
         P3; the previous assertion only checked token presence while the emitted Rust did not \
         compile; got {:?}",
        out.diagnostics
    );
    assert!(
        out.rust_source.is_none()
            || !out
                .rust_source
                .as_deref()
                .unwrap_or("")
                .contains("__ts_aot_promise_catch_value"),
        "no uncompilable `catch` call may be emitted; got {:?}",
        out.rust_source
    );
}

#[test]
fn e2e_promise_finally_with_named_handler_emits_instance_call() {
    let opts = CompileOptions {
        module: false,
        ..CompileOptions::default()
    };
    let out = Driver::new().compile_source(
        "test.ts",
        "function cleanup(): void {}\
         function f(p: Promise<number>): void { p.finally(cleanup); }",
        &opts,
    );
    assert!(
        out.diagnostics
            .iter()
            .any(|d| d.code.as_str() == "E0507" && d.severity == ts_aot_core::Severity::Error),
        "Promise.prototype.finally is not lowerable yet and must be a hard E0507 error until \
         P3; the previous assertion only checked token presence while the emitted Rust did not \
         compile; got {:?}",
        out.diagnostics
    );
    assert!(
        out.rust_source.is_none()
            || !out
                .rust_source
                .as_deref()
                .unwrap_or("")
                .contains("__ts_aot_promise_finally_value"),
        "no uncompilable `finally` call may be emitted; got {:?}",
        out.rust_source
    );
}

#[test]
fn e2e_promise_type_annotation_resolves_to_promise_emit_with_inner_type() {
    let opts = CompileOptions {
        module: false,
        ..CompileOptions::default()
    };
    let out = Driver::new().compile_source(
        "test.ts",
        "function f(p: Promise<number>): Promise<number> { return p; }",
        &opts,
    );
    assert!(!out.has_errors(), "must compile; got {:?}", out.diagnostics);
    let rust = out
        .rust_source
        .expect("emit-rust must populate rust_source");
    let rust = common::normalize_rust(&rust);
    assert!(
        rust.contains("ts_aot_runtime::Promise<f64>"),
        "Promise<number> must lower to ts_aot_runtime::Promise<f64> (regression: \
         Promise<T> was missing from BUILTIN_GENERICS, so T-anon fell back to Type::Error \
         and emitted as `()`); got:\n{rust}"
    );
}

#[test]
fn e2e_promise_reject_with_string_local_emits_jsstring_conversion_not_bare_tostring() {
    let opts = CompileOptions {
        module: false,
        ..CompileOptions::default()
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
        module: false,
        ..CompileOptions::default()
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
        module: false,
        ..CompileOptions::default()
    };
    let out = Driver::new().compile_source(
        "test.ts",
        "function f(p: Promise<number>): Promise<number> { \
            return p.then((v: number): number => v); \
         }",
        &opts,
    );
    assert!(
        out.diagnostics
            .iter()
            .any(|d| d.code.as_str() == "E0507" && d.severity == ts_aot_core::Severity::Error),
        "Promise.prototype.then is not lowerable yet and must be a hard E0507 error until \
         P3; the previous assertion only checked token presence while the emitted Rust did not \
         compile; got {:?}",
        out.diagnostics
    );
    assert!(
        out.rust_source.is_none()
            || !out
                .rust_source
                .as_deref()
                .unwrap_or("")
                .contains("__ts_aot_promise_then_value"),
        "no uncompilable `then` call may be emitted; got {:?}",
        out.rust_source
    );
}

#[test]
fn e2e_promise_resolve_no_arg_is_rejected_in_non_void_context() {
    let opts = CompileOptions {
        module: false,
        ..CompileOptions::default()
    };
    let out = Driver::new().compile_source(
        "test.ts",
        "export function f(): Promise<number> { return Promise.resolve(); }",
        &opts,
    );
    assert!(
        out.has_errors(),
        "Promise.resolve() is Promise<void>; using it as Promise<number> must be a hard error, \
         got diagnostics {:?}",
        out.diagnostics
    );
    assert!(
        out.diagnostics
            .iter()
            .any(|d| d.code.as_str() == "E0506" && d.message.contains("Promise<void>")),
        "the diagnostic must explain the void mismatch; got {:?}",
        out.diagnostics
    );
}

#[test]
fn e2e_promise_resolve_no_arg_still_compiles_in_void_context() {
    let opts = CompileOptions {
        module: false,
        ..CompileOptions::default()
    };
    let out = Driver::new().compile_source(
        "test.ts",
        "export function f(): Promise<void> { return Promise.resolve(); }",
        &opts,
    );
    assert!(!out.has_errors(), "must compile; got {:?}", out.diagnostics);
    let rust_raw = out.rust_source.expect("rust_source");
    check_generated_crate("promise_resolve_void", &rust_raw);
    let rust = common::normalize_rust(&rust_raw);
    assert!(
        rust.contains("__ts_aot_promise_resolve_value::<()>(())"),
        "Promise.resolve() must resolve unit into Promise<void>; got:\n{rust}"
    );
}

#[test]
fn e2e_promise_resolve_undefined_in_non_void_context_is_rejected() {
    let opts = CompileOptions {
        module: false,
        ..CompileOptions::default()
    };
    let out = Driver::new().compile_source(
        "test.ts",
        "export function f(): Promise<number> { return Promise.resolve(undefined); }",
        &opts,
    );
    assert!(
        out.has_errors(),
        "Promise.resolve(undefined) is Promise<void>; using it as Promise<number> must be a hard \
         error, got diagnostics {:?}",
        out.diagnostics
    );
    assert!(
        out.diagnostics
            .iter()
            .any(|d| d.code.as_str() == "E0506" && d.message.contains("Promise<void>")),
        "the diagnostic must explain the void mismatch; got {:?}",
        out.diagnostics
    );
}

#[test]
fn e2e_promise_resolve_null_returns_promise_null() {
    let opts = CompileOptions {
        module: false,
        ..CompileOptions::default()
    };
    let out = Driver::new().compile_source(
        "test.ts",
        "export function f(): Promise<null> { return Promise.resolve(null); }",
        &opts,
    );
    assert!(
        !out.has_errors(),
        "`Promise.resolve(null)` is `Promise<null>`, not `Promise<void>`, so a function declared \
         `Promise<null>` must accept it; got {:?}",
        out.diagnostics
    );
    let rust_raw = out.rust_source.expect("rust_source");
    check_generated_crate("promise_resolve_null", &rust_raw);
}

#[test]
fn e2e_promise_resolve_null_in_non_nullable_context_is_rejected() {
    let opts = CompileOptions {
        module: false,
        ..CompileOptions::default()
    };
    let out = Driver::new().compile_source(
        "test.ts",
        "export function f(): Promise<number> { return Promise.resolve(null); }",
        &opts,
    );
    assert!(
        out.has_errors(),
        "`Promise<null>` cannot satisfy `Promise<number>`: null lowers to `()` but number lowers \
         to `f64`, so the generated crate would fail `cargo check`; it must be rejected with a \
         diagnostic instead; got {:?}",
        out.diagnostics
    );
    assert!(
        out.diagnostics
            .iter()
            .any(|d| d.code.as_str() == "E0506" && d.message.contains("null")),
        "the diagnostic must explain the null mismatch; got {:?}",
        out.diagnostics
    );
}

#[test]
fn e2e_promise_resolve_null_in_optional_context_is_rejected_not_broken_code() {
    let opts = CompileOptions {
        module: false,
        ..CompileOptions::default()
    };
    let out = Driver::new().compile_source(
        "test.ts",
        "export function f(): Promise<number | null> { return Promise.resolve(null); }",
        &opts,
    );
    assert!(
        out.has_errors(),
        "`number | null` lowers to `Option<f64>` while `null` lowers to `()`, and the returned \
         value is a promise rather than a bare optional, so the generated crate would fail \
         `cargo check` with E0308; keep the diagnostic instead of emitting broken code; got {:?}",
        out.diagnostics
    );
    assert!(
        out.diagnostics
            .iter()
            .any(|d| d.code.as_str() == "E0506" && d.message.contains("null")),
        "the diagnostic must explain the null mismatch; got {:?}",
        out.diagnostics
    );
}

#[test]
fn e2e_promise_reject_uses_the_declared_local_type_not_the_function_return_type() {
    let opts = CompileOptions {
        module: false,
        ..CompileOptions::default()
    };
    let out = Driver::new().compile_source(
        "test.ts",
        "export function f(): Promise<number> { \
            const p: Promise<i32> = Promise.reject(\"boom\"); \
            p; \
            return Promise.resolve(1); \
         }",
        &opts,
    );
    assert!(
        !out.has_errors(),
        "`Promise.reject(\"boom\")` must be typed from the declared local `Promise<i32>`, not \
         from the enclosing function's `Promise<number>`; using the function's return type made \
         the initializer `Promise<f64>` and the generated crate failed `cargo check` with \
         E0308; got {:?}",
        out.diagnostics
    );
    let rust_raw = out.rust_source.expect("rust_source");
    check_generated_crate("promise_reject_declared_local_ty", &rust_raw);
}

#[test]
fn e2e_discarded_promise_resolve_is_not_checked_against_the_function_return_type() {
    let opts = CompileOptions {
        module: false,
        ..CompileOptions::default()
    };
    let out = Driver::new().compile_source(
        "test.ts",
        "export function f(): Promise<number> { Promise.resolve(); return Promise.resolve(1); }",
        &opts,
    );
    assert!(
        !out.has_errors(),
        "a discarded `Promise.resolve()` statement has no expected type, so it must not be \
         checked against the enclosing function's `Promise<number>`; the old code used the \
         function return type and reported a spurious E0506; got {:?}",
        out.diagnostics
    );
    let rust_raw = out.rust_source.expect("rust_source");
    check_generated_crate("discarded_promise_resolve", &rust_raw);
}

#[test]
fn e2e_unannotated_promise_reject_local_compiles() {
    let opts = CompileOptions {
        module: false,
        ..CompileOptions::default()
    };
    let out = Driver::new().compile_source(
        "test.ts",
        "export function f(): Promise<number> { \
            const p = Promise.reject(1); \
            p; \
            return Promise.resolve(1); \
         }",
        &opts,
    );
    assert!(
        !out.has_errors(),
        "an unannotated `Promise.reject(1)` local must recover the `Promise<Error>` value type \
         that lowering emits for the reject runtime op; otherwise the local's recovered type \
         disagrees with its initializer and the generated crate fails `cargo check`; got {:?}",
        out.diagnostics
    );
    let rust_raw = out.rust_source.expect("rust_source");
    check_generated_crate("unannotated_promise_reject_local", &rust_raw);
}

#[test]
fn e2e_returned_call_arguments_do_not_inherit_the_enclosing_return_type() {
    let opts = CompileOptions {
        module: false,
        ..CompileOptions::default()
    };
    let out = Driver::new().compile_source(
        "test.ts",
        "function id(x: Promise<void>): Promise<number> { return Promise.resolve(1); } \
         export function f(): Promise<number> { return id(Promise.resolve()); }",
        &opts,
    );
    assert!(
        !out.has_errors(),
        "a returned call's `Promise.resolve()` argument must be checked against the callee's \
         parameter type (`Promise<void>`), not the enclosing function's return type \
         (`Promise<number>`); inheriting the return type reported a spurious E0506; got {:?}",
        out.diagnostics
    );
    let rust_raw = out.rust_source.expect("rust_source");
    check_generated_crate("returned_call_arg_context", &rust_raw);
}

#[test]
fn e2e_capturing_closure_body_uses_its_own_return_type() {
    let opts = CompileOptions {
        module: false,
        ..CompileOptions::default()
    };
    let out = Driver::new().compile_source(
        "test.ts",
        "export function f(): Promise<number> { \
            const flag: boolean = true; \
            const g: () => Promise<i32> = (): Promise<i32> => { flag; return Promise.resolve(1); }; \
            g; \
            return Promise.resolve(1); \
         }",
        &opts,
    );
    assert!(
        !out.has_errors(),
        "a capturing closure declared `() => Promise<i32>` must convert its body against \
         `Promise<i32>`, not the enclosing function's `Promise<number>`; inheriting the enclosing \
         return type made the body produce `Promise<f64>` and the generated crate failed `cargo \
         check`; got {:?}",
        out.diagnostics
    );
    let rust_raw = out.rust_source.expect("rust_source");
    check_generated_crate("capturing_closure_return_ty", &rust_raw);
}

#[test]
fn e2e_promise_resolve_in_a_closure_body_keeps_its_diagnostic() {
    let opts = CompileOptions {
        module: false,
        ..CompileOptions::default()
    };
    let out = Driver::new().compile_source(
        "test.ts",
        "export function f(): Promise<number> { \
            const n: number = 1; \
            const g: () => Promise<number> = () => { n; return Promise.resolve(); }; \
            g; \
            return Promise.resolve(1); \
         }",
        &opts,
    );
    assert!(
        out.diagnostics
            .iter()
            .any(|d| d.code.as_str() == "E0506" && d.message.contains("Promise<void>")),
        "a closure declared `() => Promise<number>` whose body returns `Promise.resolve()` must \
         still be rejected; threading the expected type per statement must not silence promise \
         checks inside a closure body; got {:?}",
        out.diagnostics
    );
}

#[test]
fn e2e_discarded_promise_resolve_inside_a_closure_is_not_a_false_positive() {
    let opts = CompileOptions {
        module: false,
        ..CompileOptions::default()
    };
    let out = Driver::new().compile_source(
        "test.ts",
        "export function f(): Promise<number> { \
            const n: number = 1; \
            const g: () => Promise<number> = () => { n; Promise.resolve(); return Promise.resolve(1); }; \
            g; \
            return Promise.resolve(1); \
         }",
        &opts,
    );
    assert!(
        !out.has_errors(),
        "a discarded `Promise.resolve()` inside a closure body has no expected type, so it must \
         not be checked against the enclosing function's `Promise<number>`; got {:?}",
        out.diagnostics
    );
}

#[test]
fn e2e_promise_reject_zero_args_keeps_arity_warning() {
    let opts = CompileOptions {
        module: false,
        ..CompileOptions::default()
    };
    let out = Driver::new().compile_source(
        "test.ts",
        "export function f(): Promise<number> { return Promise.reject(); }",
        &opts,
    );
    assert!(
        out.diagnostics
            .iter()
            .any(|d| d.code.as_str() == "E0406" && d.message.contains("Promise.reject")),
        "Promise.reject() must keep today's arity warning (it is not re-routed as a void \
         resolve); got {:?}",
        out.diagnostics
    );
}

#[test]
fn e2e_unannotated_local_from_call_compiles() {
    let opts = CompileOptions {
        module: false,
        ..CompileOptions::default()
    };
    let out = Driver::new().compile_source(
        "test.ts",
        "function g(): number { return 1; } \
         function f(): number { const x = g(); return x; }",
        &opts,
    );
    assert!(!out.has_errors(), "must compile; got {:?}", out.diagnostics);
    let rust_raw = out.rust_source.expect("rust_source");
    check_generated_crate("type_recovery_local_from_call", &rust_raw);
    let rust = common::normalize_rust(&rust_raw);
    assert!(
        rust.contains("let x : f64 = g (__ctx)"),
        "an unannotated local initialised from a call must be typed from the callee's return \
         type; the frontend leaves the call as `Type::Error`, so it used to emit \
         `let x : () = g (__ctx)`; got:\n{rust}"
    );
}

#[test]
fn e2e_unannotated_local_from_await_compiles() {
    let opts = CompileOptions {
        module: false,
        ..CompileOptions::default()
    };
    let out = Driver::new().compile_source(
        "test.ts",
        "async function g(): Promise<number> { return Promise.resolve(1); } \
         async function f(): Promise<number> { const v = await g(); return v; }",
        &opts,
    );
    assert!(!out.has_errors(), "must compile; got {:?}", out.diagnostics);
    let rust_raw = out.rust_source.expect("rust_source");
    check_generated_crate("type_recovery_local_from_await", &rust_raw);
    let rust = common::normalize_rust(&rust_raw);
    assert!(
        rust.contains("let v : f64 ="),
        "`const v = await g()` must type the local from the awaited promise's ok type; it used \
         to emit `let v : () = __ts_aot_await_value::<f64>(…)`; got:\n{rust}"
    );
}

#[test]
fn e2e_ternary_of_promises_compiles() {
    let opts = CompileOptions {
        module: false,
        ..CompileOptions::default()
    };
    let out = Driver::new().compile_source(
        "test.ts",
        "function f(b: boolean): Promise<number> { \
            return b ? Promise.resolve(1) : Promise.resolve(2); \
         }",
        &opts,
    );
    assert!(!out.has_errors(), "must compile; got {:?}", out.diagnostics);
    let rust_raw = out.rust_source.expect("rust_source");
    check_generated_crate("type_recovery_ternary", &rust_raw);
    let rust = common::normalize_rust(&rust_raw);
    assert!(
        rust.contains("__tmp3 : ts_aot_runtime::Promise<f64>"),
        "the ternary temp must be typed from its branches; it used to be `let mut __tmp3 : ()` \
         and then assigned `Promise<f64>` into it, which does not compile; got:\n{rust}"
    );
}

#[test]
fn e2e_ternary_numeric_branch_is_cast_to_the_joined_type() {
    let opts = CompileOptions {
        module: false,
        ..CompileOptions::default()
    };
    let out = Driver::new().compile_source(
        "test.ts",
        "export function f(c: boolean, x: i32): f64 { return c ? x : 1.5; }",
        &opts,
    );
    assert!(!out.has_errors(), "must compile; got {:?}", out.diagnostics);
    let rust_raw = out.rust_source.expect("rust_source");
    check_generated_crate("ternary_numeric_branch_cast", &rust_raw);
    let rust = common::normalize_rust(&rust_raw);
    assert!(
        rust.contains("as f64"),
        "an i32 branch of an f64 conditional must be cast to f64; without the cast the then \
         branch assigns an i32 into the f64 dest temp, which fails `cargo check` with E0308; \
         got:\n{rust}"
    );
}

#[test]
fn e2e_promise_then_on_a_resolved_promise_is_rejected_not_a_field_access() {
    let opts = CompileOptions {
        module: false,
        ..CompileOptions::default()
    };
    let out = Driver::new().compile_source(
        "test.ts",
        "function handler(v: number): number { return v; } \
         function f(): Promise<number> { return Promise.resolve(1).then(handler); }",
        &opts,
    );
    assert!(
        out.diagnostics
            .iter()
            .any(|d| d.code.as_str() == "E0507" && d.severity == ts_aot_core::Severity::Error),
        "a `.then` chain on `Promise.resolve(...)` must be rejected with E0507 rather than \
         emitting uncompilable Rust or a bogus E0300 field access; got {:?}",
        out.diagnostics
    );
}

#[test]
fn e2e_unrecoverable_local_type_is_a_hard_error() {
    let opts = CompileOptions {
        module: false,
        ..CompileOptions::default()
    };
    let out = Driver::new().compile_source("test.ts", "function f(): void { let x; }", &opts);
    assert!(
        out.diagnostics
            .iter()
            .any(|d| d.code.as_str() == "E0414" && d.severity == ts_aot_core::Severity::Error),
        "a local with no annotation and no inferable initializer must be a hard E0414 error \
         instead of emitting `let x : ()`; got {:?}",
        out.diagnostics
    );
}

#[test]
fn e2e_ternary_with_ununifiable_branch_types_is_a_hard_error() {
    let opts = CompileOptions {
        module: false,
        ..CompileOptions::default()
    };
    let out = Driver::new().compile_source(
        "test.ts",
        "function f(b: boolean): i64 { return b ? 1 : \"x\"; }",
        &opts,
    );
    assert!(
        out.diagnostics
            .iter()
            .any(|d| d.code.as_str() == "E0414" && d.severity == ts_aot_core::Severity::Error),
        "a conditional whose branches are individually known but cannot be unified (i64 vs \
         string) must be a hard E0414 error instead of emitting `let mut __tmp : ()`; got {:?}",
        out.diagnostics
    );
}

#[test]
fn e2e_nested_promise_resolve_adopts_without_double_wrapping() {
    let out = Driver::new().compile_source(
        "test.ts",
        "export function f(n: number): Promise<number> { \
            const p = Promise.resolve(Promise.resolve(Promise.resolve(n))); return p; }",
        &CompileOptions {
            module: false,
            ..CompileOptions::default()
        },
    );
    assert!(!out.has_errors(), "got {:?}", out.diagnostics);
    let rust = out.rust_source.expect("rust_source");
    assert_eq!(
        rust.matches("__ts_aot_promise_resolve_value").count(),
        1,
        "{rust}"
    );
    check_generated_crate("nested_resolve_adoption", &rust);
}
