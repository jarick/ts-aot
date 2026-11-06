use ts_aot_driver::{CompileOptions, Driver, EmitStage};
use ts_aot_test_support::{check_generated_crate, run_generated_crate};

mod common;

fn compile_rust(source: &str) -> ts_aot_driver::DriverOutput {
    Driver::new().compile_source(
        "test.ts",
        source,
        &CompileOptions {
            emit: EmitStage::Rust,
            module: true,
        },
    )
}

fn compile_script(source: &str) -> ts_aot_driver::DriverOutput {
    Driver::new().compile_source(
        "test.ts",
        source,
        &CompileOptions {
            emit: EmitStage::Rust,
            module: false,
        },
    )
}

#[test]
fn mutable_global_read_write_is_wrapped_in_unsafe() {
    let out = compile_rust(
        "var counter: i64 = 0; \
         export function bump(): i64 { counter = counter + 1; return counter; }",
    );
    assert!(
        !out.has_errors(),
        "mutable global assignment must compile through the full pipeline; got {:?}",
        out.diagnostics
    );
    let source = out
        .rust_source
        .expect("emit-rust must populate rust_source for the global check");
    assert!(
        source.contains("unsafe"),
        "reads/writes of a mutable `static mut` global must be wrapped in `unsafe`; got:\n{source}"
    );
    assert!(
        source.contains("static mut counter"),
        "a reassigned global must be emitted as a mutable static; got:\n{source}"
    );
}

#[test]
fn immutable_global_is_not_wrapped_in_unsafe() {
    let out = compile_rust("const answer: i64 = 42; export function get(): i64 { return answer; }");
    assert!(!out.has_errors(), "got {:?}", out.diagnostics);
    let source = out.rust_source.expect("rust_source");
    assert!(
        source.contains("static answer"),
        "an immutable global must be emitted as a plain `static`; got:\n{source}"
    );
    assert!(
        !source.contains("unsafe"),
        "reads of an immutable global must not require `unsafe`; got:\n{source}"
    );
}

#[test]
fn mutable_string_global_uses_owned_storage_that_outlives_the_arena() {
    let out = compile_rust(
        "let greeting: string; \
         export function setGreeting(s: string): void { greeting = s; } \
         export function getGreeting(): string { return greeting; }",
    );
    assert!(
        !out.has_errors(),
        "a mutable string global must compile through the full pipeline; got {:?}",
        out.diagnostics
    );
    let source = out
        .rust_source
        .expect("emit-rust must populate rust_source for the string-global check");
    let norm = common::normalize_rust(&source);
    assert!(
        source.contains("static mut greeting"),
        "a reassigned string global must be a mutable static; got:\n{source}"
    );
    assert!(
        norm.contains("::std::string::String"),
        "a string global must use the owned `String` representation (the arena-borrowing \
         `JsString<'static>` cannot store a value past the call's arena); got:\n{source}"
    );
    assert!(
        !source.contains("'static"),
        "a string global must not extend the arena borrow to `'static`; got:\n{source}"
    );
    assert!(
        source.contains("to_string_lossy ()"),
        "assigning a borrowed string to the global must convert through `to_string_lossy`; \
         got:\n{source}"
    );
    check_generated_crate("mutable_string_global", &source);
}

const ARENA_RESET_MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host, JsString};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let mut bump = Bump::new();

    {
        let ctx = Ctx { arena: &bump, host: &host };
        arena_string_global::setGreeting(&ctx, JsString::from("hello"));
        assert_eq!(
            arena_string_global::getGreeting(&ctx).to_string_lossy(),
            "hello",
            "the greeting must be readable before the arena resets"
        );
    }

    bump.reset();

    {
        let ctx = Ctx { arena: &bump, host: &host };
        assert_eq!(
            arena_string_global::getGreeting(&ctx).to_string_lossy(),
            "hello",
            "a string global must keep its value after the arena resets"
        );
    }

    println!("STRING_GLOBAL_SURVIVED_ARENA_RESET");
}
"#;

#[test]
fn mutable_string_global_survives_an_arena_reset_at_runtime() {
    let out = compile_script(
        "let greeting: string; \
         export function setGreeting(s: string): void { greeting = s; } \
         export function getGreeting(): string { return greeting; }",
    );
    assert!(
        !out.has_errors(),
        "a mutable string global must compile through the full pipeline; got {:?}",
        out.diagnostics
    );
    let source = out
        .rust_source
        .expect("emit-rust must populate rust_source for the arena-reset check");
    let stdout = run_generated_crate("arena_string_global", &source, ARENA_RESET_MAIN);
    assert!(
        stdout.contains("STRING_GLOBAL_SURVIVED_ARENA_RESET"),
        "the generated crate must store a string global outside the arena; got stdout:\n{stdout}"
    );
}
