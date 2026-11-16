use ts_aot_driver::{CompileOptions, Driver, EmitStage};
use ts_aot_test_support::check_generated_crate;

const SHIM: &str = r#"
class Test262Error { message: string; constructor(message: string = "") { this.message = message; } }
function assert(mustBeTrue: i32, message: string = ""): i32 { return 0; }
function assertNotSameValue(actual: i32, unexpected: i32, message: string = ""): i32 { return 0; }
function assertSameValue(actual: i32, expected: i32, message: string = ""): i32 { return 0; }
function assertThrows(ctor: i32, fn: () => void, message: string = ""): i32 {
  try { fn(); } catch (e) { return 0; }
  return 1;
}
"#;

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

fn shim_source(body: &str) -> String {
    format!("{SHIM}{body}")
}

fn rust_of(source: &str) -> String {
    let out = compile_module(source);
    assert!(
        !out.has_errors(),
        "must compile through the full pipeline; got {:?}",
        out.diagnostics
    );
    out.rust_source
        .expect("emit-rust must populate rust_source")
}

#[test]
fn assert_throws_is_lowered_to_the_assert_throws_shim_helper() {
    let rust = rust_of(&shim_source(
        "export function run(): i32 { const ctor: i32 = 0; \
         assert.throws(ctor, function (): void {}); return 0; }",
    ));
    assert!(
        rust.contains("assertThrows"),
        "`assert.throws` must be lowered to the `assertThrows` shim helper; got:\n{rust}"
    );
}

#[test]
fn a_mapped_assert_throws_call_emits_a_valid_crate() {
    let rust = rust_of(&shim_source(
        "export function run(): i32 { const ctor: i32 = 0; \
         assert.throws(ctor, function (): void {}); \
         assert.throws(ctor, function (): void {}, \"boom\"); return 0; }",
    ));
    check_generated_crate("assert_throws_shim", &rust);
}

#[test]
fn the_test262_shim_helpers_accept_a_missing_message() {
    let out = compile_module(&shim_source(
        "export function run(): i32 { assert(0); assert.sameValue(1, 1); \
         assert.notSameValue(1, 2); return 0; }",
    ));
    assert!(!out.has_errors(), "got {:?}", out.diagnostics);
    assert!(
        !out.diagnostics.iter().any(|d| d.code.as_str() == "P0004"),
        "a shim helper with a defaulted `message` must accept two arguments; got {:?}",
        out.diagnostics
    );
}

#[test]
fn the_test262_error_constructor_accepts_a_missing_message() {
    let out = compile_module(&shim_source(
        "export function run(): i32 { new Test262Error(); return 0; }",
    ));
    assert!(!out.has_errors(), "got {:?}", out.diagnostics);
    assert!(
        !out.diagnostics.iter().any(|d| d.code.as_str() == "P0004"),
        "the shim error constructor must accept a missing message; got {:?}",
        out.diagnostics
    );
}

#[test]
fn a_locally_bound_assert_object_is_not_rewritten() {
    let out = compile_module(
        "export function run(): number { \
         const assert = { throws: function (): number { return 7; } }; \
         return assert.throws(); }",
    );
    assert!(!out.has_errors(), "got {:?}", out.diagnostics);
    let rust = out
        .rust_source
        .expect("emit-rust must populate rust_source");
    assert!(
        !rust.contains("assertThrows"),
        "a shadowed `assert` binding must keep its own `throws` member; got:\n{rust}"
    );
}

#[test]
fn a_missing_required_argument_is_still_reported() {
    let out = compile_module(
        "function need(a: number, b: number): number { return a + b; } \
         export function run(): number { return need(1); }",
    );
    assert!(out.has_errors(), "got {:?}", out.diagnostics);
    assert!(
        out.diagnostics.iter().any(|d| d.code.as_str() == "P0004"),
        "arity checking must stay enabled for non-shim functions; got {:?}",
        out.diagnostics
    );
}
