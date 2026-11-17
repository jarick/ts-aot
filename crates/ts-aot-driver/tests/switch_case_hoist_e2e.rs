use ts_aot_driver::{CompileOptions, Driver, EmitStage};
use ts_aot_test_support::run_generated_crate;

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

fn codes(out: &ts_aot_driver::DriverOutput) -> Vec<&str> {
    out.diagnostics.iter().map(|d| d.code.as_str()).collect()
}

fn run(source: &str, name: &str, main: &str) -> String {
    let rust = rust_of(source);
    run_generated_crate(name, &rust, main)
}

#[test]
fn a_string_declared_in_one_case_is_hoisted_into_another() {
    const SOURCE: &str = "export function got(): number { \
         switch (1) { \
             case 1: const c: string = \"abc\"; \
             default: return c.length; \
         } \
     }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(hoisted_string::got(&ctx), 3.0);
    println!("ok");
}
"#;
    let stdout = run(SOURCE, "hoisted_string", MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn an_array_declared_in_one_case_is_hoisted_into_another() {
    const SOURCE: &str = "export function got(): number { \
         switch (1) { \
             case 1: const arr: number[] = [1, 2, 3]; \
             default: return arr.length; \
         } \
     }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(hoisted_array::got(&ctx), 3.0);
    println!("ok");
}
"#;
    let stdout = run(SOURCE, "hoisted_array", MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_closure_declared_in_one_case_is_hoisted_into_another() {
    const SOURCE: &str = "export function got(): number { \
         switch (1) { \
             case 1: const f = function (): number { return 5; }; \
             default: return f(); \
         } \
     }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(hoisted_closure::got(&ctx), 5.0);
    println!("ok");
}
"#;
    let stdout = run(SOURCE, "hoisted_closure", MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_struct_with_an_arena_field_is_hoisted_into_another_case() {
    const SOURCE: &str = "class Holder { C: string = \"xy\"; } \
         export function got(): number { \
             switch (1) { \
                 case 1: const h = new Holder(); \
                 default: return h.C.length; \
             } \
         }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(hoisted_struct::got(&ctx), 2.0);
    println!("ok");
}
"#;
    let stdout = run(SOURCE, "hoisted_struct", MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_var_string_declared_before_its_use_is_hoisted_into_another_case() {
    const SOURCE: &str = "export function got(): number { \
         switch (1) { \
             case 1: var s: string = \"abc\"; \
             default: return s.length; \
         } \
     }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(hoisted_var_string::got(&ctx), 3.0);
    println!("ok");
}
"#;
    let stdout = run(SOURCE, "hoisted_var_string", MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_var_without_an_initializer_is_hoisted_with_a_default() {
    const SOURCE: &str = "export function got(): number { \
         switch (1) { \
             case 1: var x: number; x = 5; \
             default: return x; \
         } \
     }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(hoisted_var_default::got(&ctx), 5.0);
    println!("ok");
}
"#;
    let stdout = run(SOURCE, "hoisted_var_default", MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_var_string_used_before_its_declaration_stays_refused() {
    const SOURCE: &str = "export function got(n: number): number { \
         switch (n) { \
             case 1: var s: string = \"abc\"; return 1; \
             default: return s.length; \
         } \
     }";
    let out = compile_module(SOURCE);
    assert!(
        codes(&out).contains(&"E0419"),
        "the `default` case can run before the declaration, where JavaScript reads `undefined`; a \
         hoisted `\"\"` would silently differ, so the read must be refused; got {:?}",
        out.diagnostics
    );
}
