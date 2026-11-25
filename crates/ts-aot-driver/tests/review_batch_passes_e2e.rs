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
fn an_optional_chain_argument_is_read_before_a_later_argument_reassigns_it() {
    const SOURCE: &str = "class K { m(a: number, b: number): number { received = a * 10 + b; return 0; } } \
         var received: number = 0; \
         export function run(k: K | undefined, x: number): number { \
             received = 0; \
             const r = k?.m(x, x = 9); \
             return received * 100 + (r === undefined ? 1 : 0) * 10 + x; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    use ::core::option::Option::{None, Some};
    assert_eq!(
        chain_argument_order::run(&ctx, Some(chain_argument_order::K {}), 1.0),
        1909.0,
        "the callee must receive the argument value read before the later argument ran: \
         m(1, 9) records 19, so 19 * 100 + 0 * 10 + 9 = 1909; reading the reassigned source \
         would pass m(9, 9), record 99 and give 9909"
    );
    assert_eq!(
        chain_argument_order::run(&ctx, None, 1.0),
        11.0,
        "an absent receiver must skip both the call and the whole argument list, so the \
         assignment must not run: received stays 0 and x stays 1"
    );
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    check_generated_crate("chain_argument_order", &rust);
    let stdout = run_generated_crate("chain_argument_order", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
    let norm = compact(&rust);
    assert!(
        norm.contains(":f64=x;"),
        "the earlier argument must be captured into its own local inside the receiver guard, \
         before the later argument assigns to its source; got:\n{rust}"
    );
}

#[test]
fn a_local_explicitly_typed_undefined_is_accepted_when_the_parameter_admits_undefined() {
    const SOURCE: &str = "function f(x: number | undefined): number { return x ?? 1; } \
         export function run(): number { const u: undefined = undefined; return f(u); }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(undefined_argument::run(&ctx), 1.0);
    println!("ok");
}
"#;
    let out = compile_module(SOURCE);
    assert!(
        !out.has_errors(),
        "a local typed `undefined` is an `undefined` value, so passing it to a parameter that \
         includes `undefined` must be accepted; got {:?}",
        out.diagnostics
    );
    let rust = out
        .rust_source
        .expect("emit-rust must populate rust_source");
    check_generated_crate("undefined_argument", &rust);
    let stdout = run_generated_crate("undefined_argument", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_local_explicitly_typed_undefined_is_still_rejected_when_the_parameter_excludes_it() {
    const SOURCE: &str = "function f(x: number): number { return x; } \
         export function run(): number { const u: undefined = undefined; return f(u); }";
    let out = compile_module(SOURCE);
    assert!(
        out.has_errors(),
        "a local typed `undefined` cannot satisfy a parameter that excludes `undefined`; got {:?}",
        out.diagnostics
    );
    assert!(
        out.rust_source.is_none(),
        "a rejected call must not produce a Rust source; got:\n{}",
        out.rust_source.unwrap_or_default()
    );
    let message = "a `void` value cannot be passed to a parameter that is not `void`; this AOT \
                   target has no implicit Unit-to-value coercion";
    let diagnostic = out
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code.as_str() == "E0410")
        .expect("the rejected call must report E0410");
    assert_eq!(
        diagnostic.message, message,
        "the refusal must keep its exact diagnostic; got {:?}",
        out.diagnostics
    );
}
