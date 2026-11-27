use ts_aot_driver::{CompileOptions, Driver, DriverOutput, EmitStage};
use ts_aot_test_support::{assert_exact_f64, run_generated_crate};

fn compile(source: &str) -> DriverOutput {
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
    let out = compile(source);
    assert!(
        !out.has_errors(),
        "must compile through the full pipeline; got {:?}",
        out.diagnostics
    );
    out.rust_source
        .expect("emit-rust must populate rust_source")
}

fn codes(out: &DriverOutput) -> Vec<String> {
    out.diagnostics
        .iter()
        .map(|d| d.code.as_str().to_owned())
        .collect()
}

fn assert_no_inference_errors(out: &DriverOutput) {
    assert!(
        !out.diagnostics
            .errors()
            .any(|d| d.code.as_str() == "E0414" || d.code.as_str() == "E0415"),
        "the local and return types must be inferred; got {:?}",
        codes(out)
    );
}

const CTX_MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    println!("got={}", PKG::got(&ctx));
}
"#;

fn run(package: &str, source: &str) -> f64 {
    let rust = rust_of(source);
    let main = CTX_MAIN.replace("PKG", package);
    let stdout = run_generated_crate(package, &rust, &main);
    let line = stdout
        .lines()
        .find(|line| line.starts_with("got="))
        .unwrap_or_else(|| panic!("missing `got=` line in stdout:\n{stdout}"));
    line["got=".len()..]
        .trim()
        .parse::<f64>()
        .unwrap_or_else(|_| panic!("`{}` is not a number:\n{stdout}", &line["got=".len()..]))
}

#[test]
fn a_call_through_a_local_closure_types_the_local() {
    const SOURCE: &str = r"
export function got(): number {
  const g = function () { return 7; };
  const it = g();
  return it;
}
";
    let out = compile(SOURCE);
    assert_no_inference_errors(&out);
    assert_exact_f64(run("loc_ret_closure_call", SOURCE), 7.0);
}

#[test]
fn a_call_through_a_local_closure_returning_a_new_instance_types_the_local() {
    const SOURCE: &str = r"
class Foo { x: i32; constructor() { this.x = 5; } }
export function got(): number {
  const mk = function () { return new Foo(); };
  const f = mk();
  return f.x;
}
";
    let out = compile(SOURCE);
    assert_no_inference_errors(&out);
    assert_exact_f64(run("loc_ret_closure_new", SOURCE), 5.0);
}

#[test]
fn an_instance_method_call_result_types_the_local() {
    const SOURCE: &str = r"
class Desc { get: i32; constructor() { this.get = 7; } }
class Box { desc(): Desc { return new Desc(); } }
export function got(): number {
  const box = new Box();
  const d = box.desc();
  return d.get;
}
";
    let out = compile(SOURCE);
    assert_no_inference_errors(&out);
    assert_exact_f64(run("loc_ret_method_call", SOURCE), 7.0);
}

#[test]
fn an_unannotated_method_returning_a_this_method_call_infers_the_return_type() {
    const SOURCE: &str = r"
class Box { v(): i32 { return 4; } m() { return this.v(); } }
export function got(): number { return new Box().m(); }
";
    let out = compile(SOURCE);
    assert_no_inference_errors(&out);
    assert_exact_f64(run("loc_ret_this_method", SOURCE), 4.0);
}

#[test]
fn an_unannotated_method_returning_an_object_literal_is_refused() {
    const SOURCE: &str = r"
export function got(): number {
  class T { next() { return { done: false, value: 1 }; } }
  const t = new T();
  t.next();
  return 1;
}
";
    let out = compile(SOURCE);
    assert!(
        out.diagnostics.errors().any(|d| d.code.as_str() == "E0415"),
        "an object-literal return has no inferable type yet, so the method must be refused with \
         E0415 instead of being typed as void; got {:?}",
        codes(&out)
    );
    assert!(
        out.rust_source.is_none(),
        "no Rust may be emitted while the return type is unknown"
    );
}

#[test]
fn an_empty_array_local_infers_its_element_type_from_a_push() {
    const SOURCE: &str = r"
export function got(): number {
  const a = [];
  a.push(7);
  return a.length;
}
";
    let out = compile(SOURCE);
    assert_no_inference_errors(&out);
    assert_exact_f64(run("loc_ret_empty_array_push", SOURCE), 1.0);
}

#[test]
fn an_empty_array_local_infers_a_string_element_type_from_a_push() {
    const SOURCE: &str = r#"
export function got(): number {
  const a = [];
  a.push("x");
  a.push("y");
  return a.length;
}
"#;
    let out = compile(SOURCE);
    assert_no_inference_errors(&out);
    assert_exact_f64(run("loc_ret_empty_array_string", SOURCE), 2.0);
}

#[test]
fn a_static_method_result_carries_the_fields_of_the_returned_class() {
    const SOURCE: &str = r"
class A { x: i32; constructor() { this.x = 0; } }
class B { static m(): A { return new A(); } }
export function got(): number {
  const z = B.m();
  const zx = z.x;
  return zx;
}
";
    let out = compile(SOURCE);
    assert_no_inference_errors(&out);
    assert_exact_f64(run("loc_ret_static_chain", SOURCE), 0.0);
}

#[test]
fn an_empty_array_local_infers_its_element_type_from_an_index_write() {
    const SOURCE: &str = r"
export function got(): number {
  const a = [];
  if (a.length > 0) {
    a[0] = 3;
  }
  return a.length;
}
";
    let out = compile(SOURCE);
    assert_no_inference_errors(&out);
    assert_exact_f64(run("loc_ret_empty_array_index", SOURCE), 0.0);
}
