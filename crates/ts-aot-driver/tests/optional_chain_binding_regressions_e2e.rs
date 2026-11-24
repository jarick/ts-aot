use ts_aot_driver::{CompileOptions, Driver, EmitStage};
use ts_aot_test_support::{check_generated_crate, run_generated_crate};

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

fn compact(rust: &str) -> String {
    rust.chars().filter(|c| !c.is_whitespace()).collect()
}

#[test]
fn a_non_generic_optional_chain_call_binding_is_option() {
    const SOURCE: &str = "class K { plain(): number { return 7; } } \
         function f(k: K | undefined): number { const z = k?.plain(); return z === undefined ? 1 : 2; } \
         export function got(): number { \
             const p: K | undefined = new K(); const n: K | undefined = undefined; \
             return f(p) * 10 + f(n); \
         }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(non_generic_chain::got(&ctx), 21.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let norm = compact(&rust);
    assert!(
        norm.contains("letz:Option<f64>"),
        "a non-generic call through an optional chain yields `number | undefined`, so the local \
         must be `Option<f64>`; got:\n{rust}"
    );
    check_generated_crate("non_generic_chain", &rust);
    let stdout = run_generated_crate("non_generic_chain", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_non_generic_optional_chain_call_with_a_null_result_keeps_both_levels() {
    const SOURCE: &str = "class K { nul(): number | null { return null; } } \
         function f(k: K | undefined): number { const z = k?.nul(); return z === undefined ? 1 : 2; } \
         export function got(): number { \
             const p: K | undefined = new K(); const n: K | undefined = undefined; \
             return f(p) * 10 + f(n); \
         }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(non_generic_chain_null::got(&ctx), 21.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let norm = compact(&rust);
    assert!(
        norm.contains("letz:Option<Option<f64>>"),
        "`null` must keep its own level under the chain's `undefined`, because `null === \
         undefined` is false; got:\n{rust}"
    );
    assert!(
        !norm.contains("and_then"),
        "the nested result must not be flattened into one `Option`; got:\n{rust}"
    );
    check_generated_crate("non_generic_chain_null", &rust);
    let stdout = run_generated_crate("non_generic_chain_null", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_generic_optional_chain_call_binding_control_stays_option() {
    const SOURCE: &str = "class K { m<T>(x: T): T { return x; } } \
         function f(k: K | undefined): number { const z = k?.m(5); return z === undefined ? 1 : 2; } \
         export function got(): number { \
             const p: K | undefined = new K(); const n: K | undefined = undefined; \
             return f(p) * 10 + f(n); \
         }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(generic_chain_literal::got(&ctx), 21.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let norm = compact(&rust);
    assert!(
        norm.contains("letz:Option<i64>"),
        "the specialized `k?.m(5)` keeps the `undefined` alternative; got:\n{rust}"
    );
    check_generated_crate("generic_chain_literal", &rust);
    let stdout = run_generated_crate("generic_chain_literal", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_generic_optional_chain_call_with_an_undefined_argument_control_stays_single_option() {
    const SOURCE: &str = "class K { m<T>(x: T): T { return x; } } \
         function g(k: K | undefined, a: number | undefined): number { \
             const z = k?.m(a); return z === undefined ? 1 : 3; \
         } \
         export function got(): number { \
             const p: K | undefined = new K(); const n: K | undefined = undefined; \
             return g(p, undefined) * 100 + g(p, 4) * 10 + g(n, 4); \
         }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(generic_chain_undefined_arg::got(&ctx), 131.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let norm = compact(&rust);
    assert!(
        norm.contains("letz:Option<f64>") && !norm.contains("Option<Option<f64>>"),
        "an `undefined`-typed value type merges into the chain's `undefined`, so `z` stays a \
         single `Option<f64>`; got:\n{rust}"
    );
    check_generated_crate("generic_chain_undefined_arg", &rust);
    let stdout = run_generated_crate("generic_chain_undefined_arg", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_module_level_optional_chain_receiver_is_not_narrowed() {
    const SOURCE: &str = "class K { m<T>(x: T): T { return x; } } \
         let g: K | undefined = undefined; \
         export function absent(): number { return g?.m(g) === undefined ? 1 : 2; } \
         export function present(): number { g = new K(); return g?.m(g) === undefined ? 1 : 2; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(module_receiver_chain::absent(&ctx), 1.0);
    assert_eq!(module_receiver_chain::present(&ctx), 2.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let norm = compact(&rust);
    assert!(
        !norm.contains("(unsafe{g}).clone().unwrap()"),
        "a module-level receiver is a `static mut`, so the argument must not be narrowed with an \
         `unwrap()` that moves out of it; got:\n{rust}"
    );
    assert!(
        norm.contains("x:Option<K>"),
        "without narrowing, `T` is instantiated with the argument's own `K | undefined`; got:\n{rust}"
    );
    check_generated_crate("module_receiver_chain", &rust);
    let stdout = run_generated_crate("module_receiver_chain", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_local_optional_chain_receiver_is_still_narrowed_to_a_single_option() {
    const SOURCE: &str = "class K { m<T>(x: T): T { return x; } } \
         export function got(): number { \
             let l: K | undefined = new K(); \
             const r = l?.m(l); \
             return r === undefined ? 1 : 2; \
         }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(local_receiver_chain::got(&ctx), 2.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let norm = compact(&rust);
    assert!(
        !norm.contains("Option<Option<K>>"),
        "a local receiver is still narrowed to `K` inside the chain, so the result stays a single \
         `Option<K>`; got:\n{rust}"
    );
    check_generated_crate("local_receiver_chain", &rust);
    let stdout = run_generated_crate("local_receiver_chain", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_super_method_call_reaches_the_parent_method() {
    const SOURCE: &str = "class A { m(): number { return 1; } } \
         class B extends A { m(): number { return super.m(); } } \
         export function got(): number { return new B().m(); }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(super_method_call::got(&ctx), 1.0);
    println!("ok");
}
"#;
    let out = compile_module(SOURCE);
    assert!(
        !out.has_errors(),
        "`super.m()` must compile; got {:?}",
        out.diagnostics
    );
    assert!(
        !out.diagnostics.iter().any(|d| d.code.as_str() == "P0010"),
        "`super.m()` must resolve to the parent method, not a placeholder field; got {:?}",
        out.diagnostics
    );
    let rust = out
        .rust_source
        .expect("emit-rust must populate rust_source");
    check_generated_crate("super_method_call", &rust);
    let stdout = run_generated_crate("super_method_call", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_super_method_call_runs_the_parent_body_on_the_subclass_receiver() {
    const SOURCE: &str = "class A { m(x: number): number { return x + 1; } } \
         class B extends A { m(): number { return super.m(4) + 10; } } \
         export function got(): number { return new B().m(); }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(super_method_args::got(&ctx), 15.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    check_generated_crate("super_method_args", &rust);
    let stdout = run_generated_crate("super_method_args", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}
