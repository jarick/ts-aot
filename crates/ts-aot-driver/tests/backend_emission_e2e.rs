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

fn crate_rust_of(source: &str) -> String {
    let out = Driver::new().compile_source(
        "test.ts",
        source,
        &CompileOptions {
            emit: EmitStage::Crate,
            module: true,
        },
    );
    assert!(
        !out.has_errors(),
        "must compile through the full pipeline; got {:?}",
        out.diagnostics
    );
    out.rust_source
        .expect("emit-crate must populate rust_source")
}

fn normalized(rust: &str) -> String {
    rust.chars().filter(|c| !c.is_whitespace()).collect()
}

fn run(source: &str, name: &str, main: &str) -> String {
    let rust = crate_rust_of(source);
    check_generated_crate(name, &rust);
    run_generated_crate(name, &rust, main)
}

const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert!(backend_emission_static_method_value::is_function(&ctx));
    assert_eq!(backend_emission_static_method_value::called(&ctx), 42.0);
    assert_eq!(backend_emission_static_method_value::passed(&ctx), 42.0);
    println!("ok");
}
"#;

#[test]
fn a_static_method_read_as_a_value_is_a_function() {
    const SOURCE: &str = r#"
class Tools {
  static twice(x: number): number { return x * 2; }
}
function apply(f: (x: number) => number, v: number): number { return f(v); }
export function is_function(): boolean { return typeof Tools.twice === "function"; }
export function called(): number { const f = Tools.twice; return f(21); }
export function passed(): number { return apply(Tools.twice, 21); }
"#;
    let stdout = run(SOURCE, "backend_emission_static_method_value", MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn typeof_a_static_method_without_any_other_function_value_is_function() {
    const SOURCE: &str = r"
class Tools {
  static twice(x: number): number { return x * 2; }
}
export function is_function(): boolean { return typeof Tools.twice === 'function'; }
";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert!(backend_emission_typeof_static_method::is_function(&ctx));
    println!("ok");
}
"#;
    let rust = crate_rust_of(SOURCE);
    let flat = normalized(&rust);
    assert!(
        flat.contains(r#"JsString::from("function")"#),
        "`typeof` of a static method must lower to the `function` tag, got:\n{flat}"
    );
    check_generated_crate("backend_emission_typeof_static_method", &rust);
    let stdout = run_generated_crate("backend_emission_typeof_static_method", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn typeof_a_closure_local_is_function() {
    const SOURCE: &str = r"
export function is_function(): boolean {
  const f = (x: number): number => x + 1;
  return typeof f === 'function';
}
";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert!(backend_emission_typeof_closure::is_function(&ctx));
    println!("ok");
}
"#;
    let stdout = run(SOURCE, "backend_emission_typeof_closure", MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn equality_of_function_values_is_refused_rather_than_emitted_unbuildable() {
    const SOURCE: &str = r"
class Tools {
  static a(): number { return 1; }
  static b(): number { return 2; }
}
export function same(): boolean { return Tools.a === Tools.b; }
";
    let out = compile_module(SOURCE);
    assert!(
        out.diagnostics
            .iter()
            .any(|d| d.code.as_str() == "E0300" && d.message.contains("function value equality")),
        "comparing function values must report a diagnostic instead of emitting an enum \
         comparison the runtime crate cannot compile; got {:?}",
        out.diagnostics
    );
}
