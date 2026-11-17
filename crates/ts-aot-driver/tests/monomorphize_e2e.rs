mod common;

use common::normalize_rust;
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

#[test]
fn generic_identity_infers_the_return_type_of_a_call_argument() {
    const SOURCE: &str = "function id<T>(x: T): T { return x; } \
         function n(): number { return 5; } \
         export function got(): number { return id(n()); }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(mono_call_argument::got(&ctx), 5.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let norm = normalize_rust(&rust);
    assert!(
        norm.contains("fn id_mono_") && norm.contains("x : f64"),
        "the call argument's declared return type must specialize `T` to number; got:\n{rust}"
    );
    check_generated_crate("mono_call_argument", &rust);
    let stdout = run_generated_crate("mono_call_argument", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_method_call_on_a_call_receiver_resolves_by_the_declared_return_type() {
    const SOURCE: &str = "class C { m(x: number): number { return x + 1; } } \
         function make(): C { return new C(); } \
         export function got(): number { return make().m(4); }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(mono_call_receiver::got(&ctx), 5.0);
    println!("ok");
}
"#;
    let out = compile_module(SOURCE);
    assert!(!out.has_errors(), "got {:?}", out.diagnostics);
    assert!(
        !out.diagnostics.iter().any(|d| d.code.as_str() == "P0013"),
        "the receiver type must resolve instead of being skipped; got {:?}",
        out.diagnostics
    );
    let rust = out
        .rust_source
        .expect("emit-rust must populate rust_source");
    let norm = normalize_rust(&rust);
    assert!(
        norm.contains("C::C__m (make (__ctx), __ctx, 4.0)"),
        "the method must be dispatched through the receiver's declared class; got:\n{rust}"
    );
    assert!(
        !norm.contains("__field0"),
        "no placeholder field access may be emitted; got:\n{rust}"
    );
    check_generated_crate("mono_call_receiver", &rust);
    let stdout = run_generated_crate("mono_call_receiver", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_specialized_call_pads_its_omitted_optional_parameter() {
    const SOURCE: &str = "function f<T>(x: T, y?: number): number { \
             if (y === undefined) { return 1; } return 2; \
         } \
         export function got(): number { return f(7); }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(mono_optional_padding::got(&ctx), 1.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let norm = normalize_rust(&rust);
    assert!(
        norm.contains("f_mono_") && norm.contains("Option::None"),
        "a specialized call must pad the omitted optional argument; got:\n{rust}"
    );
    check_generated_crate("mono_optional_padding", &rust);
    let stdout = run_generated_crate("mono_optional_padding", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}
