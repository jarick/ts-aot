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

#[test]
fn optional_string_global_accepts_a_parameter_write() {
    let out = compile_rust(
        "let g: string | null = null; \
         export function set(v: string | null): void { g = v; } \
         export function get(): string | null { return g; }",
    );
    assert!(
        !out.has_errors(),
        "a mutable `string | null` global must compile through the full pipeline; got {:?}",
        out.diagnostics
    );
    let source = out
        .rust_source
        .expect("emit-rust must populate rust_source for the optional-string-global check");
    let norm = common::normalize_rust(&source);
    assert!(
        norm.contains("Option<::std::string::String>"),
        "a `string | null` global must use the owned `Option<String>` representation so a write \
         from a `string | null` parameter is accepted (a borrowed `Option<JsString<'static>>` \
         would reject it); got:\n{source}"
    );
    assert!(
        !source.contains("'static"),
        "an optional-string global must not extend the arena borrow to `'static`; got:\n{source}"
    );
    check_generated_crate("optional_string_global", &source);
}

const OPTIONAL_ARENA_RESET_MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host, JsString};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let mut bump = Bump::new();

    {
        let ctx = Ctx { arena: &bump, host: &host };
        optional_string_global::set(&ctx, Some(JsString::from("hello")));
        assert_eq!(
            optional_string_global::get(&ctx).map(|s| s.to_string_lossy()),
            Some("hello".to_string()),
            "the optional greeting must be readable before the arena resets"
        );
    }

    bump.reset();

    {
        let ctx = Ctx { arena: &bump, host: &host };
        assert_eq!(
            optional_string_global::get(&ctx).map(|s| s.to_string_lossy()),
            Some("hello".to_string()),
            "an optional string global must keep its value after the arena resets"
        );
    }

    println!("OPTIONAL_STRING_GLOBAL_SURVIVED_ARENA_RESET");
}
"#;

#[test]
fn optional_string_global_survives_an_arena_reset_at_runtime() {
    let out = compile_script(
        "let g: string | null; \
         export function set(v: string | null): void { g = v; } \
         export function get(): string | null { return g; }",
    );
    assert!(
        !out.has_errors(),
        "a mutable `string | null` global must compile through the full pipeline; got {:?}",
        out.diagnostics
    );
    let source = out
        .rust_source
        .expect("emit-rust must populate rust_source for the optional arena-reset check");
    let stdout = run_generated_crate("optional_string_global", &source, OPTIONAL_ARENA_RESET_MAIN);
    assert!(
        stdout.contains("OPTIONAL_STRING_GLOBAL_SURVIVED_ARENA_RESET"),
        "the generated crate must store an optional string global outside the arena; got \
         stdout:\n{stdout}"
    );
}

const OPTIONAL_STRING_WRITE_SOURCE: &str = "let g: string | null = null; \
     export function setStr(v: string): void { g = v; } \
     export function setNull(): void { g = null; } \
     export function setLit(): void { g = \"lit\"; } \
     export function get(): string | null { return g; } \
     export function localOps(): void { \
         let a: string | null = null; \
         a = \"hi\"; \
         let b: string | null = a; \
         b = null; \
         let c: string | null = \"c\"; \
         c = a; \
     }";

#[test]
fn optional_string_global_accepts_string_null_literal_and_local_writes() {
    let out = compile_rust(OPTIONAL_STRING_WRITE_SOURCE);
    assert!(
        !out.has_errors(),
        "writes of every supported source shape (string, null, literal, local) into a \
         `string | null` global must compile through the full pipeline; got {:?}",
        out.diagnostics
    );
    let source = out
        .rust_source
        .expect("emit-rust must populate rust_source for the optional-write check");
    let norm = common::normalize_rust(&source);
    assert!(
        norm.contains("Option::Some"),
        "a plain-string source must be wrapped in `Option::Some(..)`, not mapped, when written \
         into a `string | null` global; got:\n{source}"
    );
    assert!(
        norm.contains("::core::option::Option::None"),
        "a `null` source must be written into a `string | null` global as `Option::None`; \
         got:\n{source}"
    );
    assert!(
        !source.contains("'static"),
        "an optional-string global must not extend the arena borrow to `'static`; got:\n{source}"
    );
    check_generated_crate("optional_string_writes", &source);
}

const OPTIONAL_STRING_WRITE_MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host, JsString};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let mut bump = Bump::new();

    {
        let ctx = Ctx { arena: &bump, host: &host };
        assert_eq!(
            optional_string_writes::get(&ctx).map(|s| s.to_string_lossy()),
            None,
            "the optional global must start as null"
        );
        optional_string_writes::setStr(&ctx, JsString::from("plain"));
        assert_eq!(
            optional_string_writes::get(&ctx).map(|s| s.to_string_lossy()),
            Some("plain".to_string()),
            "writing a plain `string` must set the optional global"
        );
        optional_string_writes::setLit(&ctx);
        assert_eq!(
            optional_string_writes::get(&ctx).map(|s| s.to_string_lossy()),
            Some("lit".to_string()),
            "writing a string literal must set the optional global"
        );
        optional_string_writes::setNull(&ctx);
        assert_eq!(
            optional_string_writes::get(&ctx).map(|s| s.to_string_lossy()),
            None,
            "writing `null` must clear the optional global"
        );
        optional_string_writes::localOps(&ctx);
    }

    bump.reset();

    {
        let ctx = Ctx { arena: &bump, host: &host };
        optional_string_writes::setStr(&ctx, JsString::from("after_reset"));
        assert_eq!(
            optional_string_writes::get(&ctx).map(|s| s.to_string_lossy()),
            Some("after_reset".to_string()),
            "an optional string global must accept a write and keep its value after the arena \
             resets"
        );
    }

    println!("OPTIONAL_STRING_WRITES_OK");
}
"#;

#[test]
fn optional_string_global_writes_compile_and_run() {
    let out = compile_script(OPTIONAL_STRING_WRITE_SOURCE);
    assert!(
        !out.has_errors(),
        "writes of every supported source shape into a `string | null` global must compile; \
         got {:?}",
        out.diagnostics
    );
    let source = out
        .rust_source
        .expect("emit-rust must populate rust_source for the optional-write run check");
    let stdout = run_generated_crate(
        "optional_string_writes",
        &source,
        OPTIONAL_STRING_WRITE_MAIN,
    );
    assert!(
        stdout.contains("OPTIONAL_STRING_WRITES_OK"),
        "the generated crate must accept string, literal, and null writes (and survive an arena \
         reset); got stdout:\n{stdout}"
    );
}
