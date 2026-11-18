use ts_aot_driver::{CompileOptions, Driver, DriverOutput, EmitStage};
use ts_aot_test_support::run_generated_crate;

fn compile(source: &str) -> DriverOutput {
    Driver::new().compile_source(
        "test.ts",
        source,
        &CompileOptions {
            emit: EmitStage::Crate,
            module: true,
        },
    )
}

fn rust_of(source: &str) -> String {
    let out = compile(source);
    assert!(
        !out.has_errors(),
        "must compile through the full pipeline; got {:?}",
        out.diagnostics
    );
    out.rust_source
        .expect("emit-crate must populate rust_source")
}

const CTX_MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    PKG::__ts_aot_init();
    println!("got={}", PKG::got(&ctx));
}
"#;

fn run(package: &str, source: &str) -> i32 {
    let rust = rust_of(source);
    let main = CTX_MAIN.replace("PKG", package);
    let stdout = run_generated_crate(package, &rust, &main);
    let line = stdout
        .lines()
        .find(|line| line.starts_with("got="))
        .unwrap_or_else(|| panic!("missing `got=` line in stdout:\n{stdout}"));
    line["got=".len()..]
        .trim()
        .parse::<i32>()
        .unwrap_or_else(|_| panic!("`{}` is not a number:\n{stdout}", &line["got=".len()..]))
}

#[test]
fn a_field_write_through_a_module_global_class_instance_is_emitted() {
    let value = run(
        "global_field_write",
        "class C { x: i32 = 1; }\n\
         let c = new C();\n\
         export function got(): i32 { c.x = 5; return c.x; }\n",
    );
    assert_eq!(
        value, 5,
        "assigning a field of a module-global class instance must be emitted and read back"
    );
}

#[test]
fn a_field_write_through_a_module_global_from_a_function_is_emitted() {
    let value = run(
        "global_field_write_fn",
        "class C { x: i32 = 1; }\n\
         let c = new C();\n\
         function set(v: i32): void { c.x = v; }\n\
         export function got(): i32 { set(7); return c.x; }\n",
    );
    assert_eq!(
        value, 7,
        "a function must be able to write a field of a module-global class instance"
    );
}

#[test]
fn a_field_write_through_a_nested_module_global_is_emitted() {
    let value = run(
        "global_nested_field_write",
        "class Inner { n: i32 = 0; }\n\
         class Outer { i: Inner = new Inner(); }\n\
         let g = new Outer();\n\
         export function got(): i32 { g.i.n = 5; return g.i.n; }\n",
    );
    assert_eq!(
        value, 5,
        "a nested field of a module-global class instance must be writable"
    );
}

#[test]
fn a_compound_write_through_a_module_global_class_instance_is_emitted() {
    let value = run(
        "global_compound_write",
        "class C { x: i32 = 1; }\n\
         let c = new C();\n\
         export function got(): i32 { c.x += 4; return c.x; }\n",
    );
    assert_eq!(
        value, 5,
        "a compound assignment to a field of a module-global class instance must be emitted"
    );
}
