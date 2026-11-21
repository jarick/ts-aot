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

fn refuses_with_e0419(source: &str, context: &str) {
    let out = compile_module(source);
    assert!(
        out.has_errors(),
        "{context} must be refused; got {:?}",
        out.diagnostics
    );
    assert!(
        out.diagnostics.errors().any(|d| d.code.as_str() == "E0419"),
        "{context} must be refused with E0419; got {:?}",
        out.diagnostics
    );
}

fn module_main(package: &str, expected: f64) -> String {
    let literal = format!("{expected:?}");
    format!(
        r#"
use ts_aot_runtime::{{Bump, Ctx, Host}};

struct NoHost;
impl Host for NoHost {{}}

fn main() {{
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx {{ arena: &bump, host: &host }};
    assert_eq!({package}::got(&ctx), {literal});
    println!("ok");
}}
"#
    )
}

fn runs_to(package: &str, source: &str, expected: f64) {
    let rust = rust_of(source);
    let stdout = run_generated_crate(package, &rust, &module_main(package, expected));
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_method_that_mutates_a_captured_object_is_refused() {
    const SOURCE: &str = r"
class Box { n: number = 0; }
function f(): number {
  const b = new Box();
  class C { m(): number { b.n = 5; return 0; } }
  new C().m();
  return b.n;
}
export function got(): number { return f(); }
";
    refuses_with_e0419(SOURCE, "a method mutating a captured object");
}

#[test]
fn a_method_that_mutates_a_captured_array_is_refused() {
    const SOURCE: &str = r"
function f(): number {
  const a = [1];
  class C { m(): number { a.push(2); return a.length; } }
  return new C().m();
}
export function got(): number { return f(); }
";
    refuses_with_e0419(SOURCE, "a method mutating a captured array");
}

#[test]
fn mutating_a_captured_array_after_construction_is_refused() {
    const SOURCE: &str = r"
function f(): number {
  const a = [1];
  class C { m(): number { return a.length; } }
  const c = new C();
  a.push(2);
  return c.m();
}
export function got(): number { return f(); }
";
    refuses_with_e0419(SOURCE, "mutating a captured array after construction");
}

#[test]
fn mutating_a_captured_array_before_construction_is_accepted() {
    const SOURCE: &str = r"
function f(): number {
  const a = [1];
  class C { m(): number { return a.length; } }
  a.push(2);
  const c = new C();
  return c.m();
}
export function got(): number { return f(); }
";
    runs_to("local_capture_mutate_before_new", SOURCE, 2.0);
}

#[test]
fn a_captured_object_read_without_mutation_is_accepted() {
    const SOURCE: &str = r"
class Box { n: number = 3; }
function f(): number {
  const b = new Box();
  class C { m(): number { return b.n; } }
  return new C().m();
}
export function got(): number { return f(); }
";
    runs_to("local_capture_read_only_object", SOURCE, 3.0);
}

#[test]
fn a_captured_array_read_without_mutation_is_accepted() {
    const SOURCE: &str = r"
function f(): number {
  const a = [1, 2];
  class C { m(): number { return a.length; } }
  return new C().m();
}
export function got(): number { return f(); }
";
    runs_to("local_capture_read_only_array", SOURCE, 2.0);
}

#[test]
fn a_class_declared_in_a_closure_reads_an_outer_local() {
    const SOURCE: &str = r"
function f(): number {
  let v = 7;
  const g = (): number => { class C { m(): number { return v; } } return new C().m(); };
  return g();
}
export function got(): number { return f(); }
";
    runs_to("local_capture_in_closure", SOURCE, 7.0);
}

#[test]
fn a_class_declared_in_a_closure_reads_an_outer_parameter() {
    const SOURCE: &str = r"
function outer(v: number): number {
  const g = (): number => { class C { m(): number { return v; } } return new C().m(); };
  return g();
}
export function got(): number { return outer(7); }
";
    runs_to("local_capture_in_closure_param", SOURCE, 7.0);
}

#[test]
fn a_class_declared_in_another_method_reading_an_outer_local_is_refused() {
    const SOURCE: &str = r"
function f(): number {
  let v = 7;
  class A { m(): number { class B { n(): number { return v; } } return new B().n(); } }
  return new A().m();
}
export function got(): number { return f(); }
";
    refuses_with_e0419(
        SOURCE,
        "a class declared in a method reading an outer local",
    );
}

#[test]
fn a_class_with_captures_constructed_in_another_method_is_refused() {
    const SOURCE: &str = r"
function f(): number {
  const v = 4;
  class A { m(): number { return v; } }
  class B { n(): number { return new A().m() + 1; } }
  return new B().n();
}
export function got(): number { return f(); }
";
    refuses_with_e0419(
        SOURCE,
        "a class with captures constructed in another method",
    );
}

#[test]
fn a_class_with_captures_constructed_in_a_closure_reads_the_outer_local() {
    const SOURCE: &str = r"
function f(): number {
  const v = 4;
  class A { m(): number { return v; } }
  const g = (): number => new A().m();
  return g();
}
export function got(): number { return f(); }
";
    runs_to("local_capture_new_in_closure", SOURCE, 4.0);
}

#[test]
fn a_class_with_captures_constructed_in_a_closure_that_also_reads_the_local() {
    const SOURCE: &str = r"
function f(): number {
  const v = 4;
  class A { m(): number { return v; } }
  const g = (): number => new A().m() + v;
  return g();
}
export function got(): number { return f(); }
";
    runs_to("local_capture_new_in_closure_shared", SOURCE, 8.0);
}

#[test]
fn a_write_before_the_construction_is_accepted() {
    const SOURCE: &str = r"
function f(flag: boolean): number {
  let v = 1;
  if (flag) { v = 2; }
  class C { m(): number { return v; } }
  return new C().m();
}
export function got(): number { return f(true); }
";
    runs_to("local_capture_write_before_new", SOURCE, 2.0);
}

#[test]
fn a_write_after_the_construction_is_still_refused() {
    const SOURCE: &str = r"
function f(flag: boolean): number {
  let v = 1;
  class C { m(): number { return v; } }
  const c = new C();
  if (flag) { v = 2; }
  return c.m();
}
export function got(): number { return f(true); }
";
    refuses_with_e0419(SOURCE, "a write after construction");
}

#[test]
fn a_write_inside_the_construction_loop_is_refused() {
    const SOURCE: &str = r"
function f(): number {
  let v = 0;
  class C { m(): number { return v; } }
  const kept: C[] = [];
  for (let i = 0; i < 2; i++) { v = i; kept.push(new C()); }
  return kept[0].m() * 10 + kept[1].m();
}
export function got(): number { return f(); }
";
    refuses_with_e0419(
        SOURCE,
        "a write repeating around a construction in the same loop",
    );
}

#[test]
fn a_for_of_write_inside_the_construction_loop_is_refused() {
    const SOURCE: &str = r"
function f(): number {
  let v = 0;
  class C { m(): number { return v; } }
  const kept: C[] = [];
  for (const i of [0, 1]) { v = i; kept.push(new C()); }
  return kept[0].m() + kept[1].m();
}
export function got(): number { return f(); }
";
    refuses_with_e0419(SOURCE, "a for-of write repeating around a construction");
}

#[test]
fn a_content_mutation_inside_the_construction_loop_is_refused() {
    const SOURCE: &str = r"
function f(): number {
  const a: number[] = [1];
  class C { m(): number { return a.length; } }
  const kept: C[] = [];
  for (let i = 0; i < 2; i++) { a.push(i); kept.push(new C()); }
  return kept[0].m() * 10 + kept[1].m();
}
export function got(): number { return f(); }
";
    refuses_with_e0419(SOURCE, "a content mutation repeating around a construction");
}

#[test]
fn a_write_in_a_separate_loop_before_the_construction_is_accepted() {
    const SOURCE: &str = r"
function f(): number {
  let v = 0;
  for (let i = 0; i < 2; i++) { v = i + 1; }
  class C { m(): number { return v; } }
  return new C().m();
}
export function got(): number { return f(); }
";
    runs_to("local_capture_write_loop_before_new", SOURCE, 2.0);
}

#[test]
fn a_construction_loop_without_writes_is_accepted() {
    const SOURCE: &str = r"
function f(): number {
  let v = 0;
  class C { m(): number { return v; } }
  for (let i = 0; i < 2; i++) { new C(); }
  return v;
}
export function got(): number { return f(); }
";
    runs_to("local_capture_loop_no_write", SOURCE, 0.0);
}
