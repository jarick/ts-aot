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
        codes(&out)
    );
    out.rust_source
        .expect("emit-rust must populate rust_source")
}

fn codes(out: &DriverOutput) -> Vec<String> {
    out.diagnostics
        .iter()
        .map(|d| format!("{}/{:?}/{}", d.code.as_str(), d.severity, d.message))
        .collect()
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
fn generator_function_expression_with_annotation_feeds_for_of() {
    const SOURCE: &str = r"
export function got(): number {
  const g = function* (): number { yield 1; yield 2; };
  let s: number = 0;
  for (const x of g()) { s = s + x; }
  return s;
}
";
    let out = compile(SOURCE);
    assert!(
        out.diagnostics.iter().all(|d| d.code.as_str() != "E0500"),
        "a generator function expression must be walked; got {:?}",
        codes(&out)
    );
    assert_exact_f64(run("gen_expr_annotated", SOURCE), 3.0);
}

#[test]
fn generator_function_expression_infers_element_type_from_yields() {
    const SOURCE: &str = r"
export function got(): number {
  const g = function* () { yield 4; yield 5; };
  let s: number = 0;
  for (const x of g()) { s = s + x; }
  return s;
}
";
    let out = compile(SOURCE);
    assert!(
        !out.has_errors(),
        "a generator function expression with literal yields must compile; got {:?}",
        codes(&out)
    );
    assert_exact_f64(run("gen_expr_inferred", SOURCE), 9.0);
}

#[test]
fn immediately_invoked_generator_expression_returns_a_generator() {
    const SOURCE: &str = r"
export function got(): number {
  const it = (function* (): number { yield 6; yield 7; })();
  let s: number = 0;
  for (const x of it) { s = s + x; }
  return s;
}
";
    assert_exact_f64(run("gen_expr_iife", SOURCE), 13.0);
}

#[test]
fn generator_expression_next_call_is_lowered_to_a_method_call() {
    const SOURCE: &str = r"
export function got(): number {
  const g = function* (): number { yield 8; };
  const it = g();
  it.next();
  return 1;
}
";
    let rust = rust_of(SOURCE);
    assert!(
        rust.contains("next ()") || rust.contains("next()"),
        "`next()` on a generator returned by a generator expression must stay a method call, \
         got:\n{rust}"
    );
    assert_exact_f64(run("gen_expr_next_call", SOURCE), 1.0);
}

#[test]
fn generator_expression_with_a_parameter_is_callable() {
    const SOURCE: &str = r"
export function got(): number {
  const g = function* (n: number): number { yield n + 1; yield n + 2; };
  let s: number = 0;
  for (const x of g(10)) { s = s + x; }
  return s;
}
";
    assert_exact_f64(run("gen_expr_param", SOURCE), 23.0);
}

#[test]
fn immediately_invoked_generator_expression_forwards_its_arguments() {
    const SOURCE: &str = r"
export function got(): number {
  let s: number = 0;
  for (const x of (function* (n: number): number { yield n; yield n * 2; })(4)) {
    s = s + x;
  }
  return s;
}
";
    assert_exact_f64(run("gen_expr_iife_args", SOURCE), 12.0);
}

#[test]
fn generator_expression_with_a_capture_is_refused_with_e0500() {
    const SOURCE: &str = r"
export function got(): number {
  const n: number = 3;
  const g = function* (): number { yield n; };
  let s: number = 0;
  for (const x of g()) { s = s + x; }
  return s;
}
";
    let out = compile(SOURCE);
    assert!(
        out.diagnostics
            .errors()
            .any(|d| d.code.as_str() == "E0500" && d.message.contains("enclosing function")),
        "a capturing generator expression must be refused with a hard E0500; got {:?}",
        codes(&out)
    );
}

#[test]
fn super_call_reaches_an_overridden_parent_method() {
    const SOURCE: &str = r"
class Base {
  m(): number {
    return 1;
  }
}
class Derived extends Base {
  m(): number {
    return super.m() + 10;
  }
}
export function got(): number {
  return new Derived().m();
}
";
    assert_exact_f64(run("super_override_one", SOURCE), 11.0);
}

#[test]
fn super_call_walks_two_override_levels() {
    const SOURCE: &str = r"
class A {
  m(): number {
    return 1;
  }
}
class B extends A {
  m(): number {
    return super.m() + 1;
  }
}
class C extends B {
  m(): number {
    return super.m() + 1;
  }
}
export function got(): number {
  return new C().m();
}
";
    assert_exact_f64(run("super_override_two", SOURCE), 3.0);
}

#[test]
fn super_call_skips_a_parent_that_does_not_declare_the_method() {
    const SOURCE: &str = r"
class A {
  m(): number {
    return 1;
  }
}
class B extends A {}
class C extends B {
  m(): number {
    return super.m() + 5;
  }
}
export function got(): number {
  return new C().m();
}
";
    assert_exact_f64(run("super_override_skip", SOURCE), 6.0);
}

#[test]
fn super_call_to_an_overridden_method_dispatches_virtually_on_this() {
    const SOURCE: &str = r"
class Base {
  m(): number {
    return this.tag() + 1;
  }
  tag(): number {
    return 1;
  }
}
class Derived extends Base {
  m(): number {
    return super.m() * 10;
  }
  tag(): number {
    return 2;
  }
}
export function got(): number {
  return new Derived().m();
}
";
    assert_exact_f64(run("super_override_virtual", SOURCE), 30.0);
}

#[test]
fn super_call_into_a_parent_constructor_runs_its_body() {
    const SOURCE: &str = r"
class Base {
  x: number = 0;
  constructor(v: number) {
    this.x = v;
  }
}
class Derived extends Base {
  constructor(v: number) {
    super(v);
  }
}
export function got(): number {
  return new Derived(7).x;
}
";
    assert_exact_f64(run("super_ctor_body", SOURCE), 7.0);
}

#[test]
fn super_field_read_of_an_instance_field_is_refused_with_e0500() {
    const SOURCE: &str = r"
class Base {
  x: number = 1;
}
class Derived extends Base {
  m(): number {
    return super.x;
  }
}
export function got(): number {
  return new Derived().m();
}
";
    let out = compile(SOURCE);
    assert!(
        out.diagnostics.errors().any(|d| d.code.as_str() == "E0500"),
        "`super.<field>` must stay a hard E0500 (prototype lookup is not modelled); got {:?}",
        codes(&out)
    );
}
