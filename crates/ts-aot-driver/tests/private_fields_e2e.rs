use std::fmt::Write as _;

use ts_aot_driver::{CompileOptions, Driver, DriverOutput, EmitStage};
use ts_aot_test_support::run_generated_crate;

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

fn numbers_main(package: &str, expectations: &[f64]) -> String {
    let mut checks = String::new();
    for (index, value) in expectations.iter().enumerate() {
        let _ = writeln!(
            checks,
            "    assert_eq!({package}::got{index}(&ctx), {value}_f64);"
        );
    }
    format!(
        "use ts_aot_runtime::{{Bump, Ctx, Host}};\n\nstruct NoHost;\nimpl Host for NoHost {{}}\n\n\
         fn main() {{\n    let host = NoHost;\n    let bump = Bump::new();\n    \
         let ctx = Ctx {{ arena: &bump, host: &host }};\n{checks}    println!(\"ok\");\n}}\n"
    )
}

fn check(package: &str, source: &str, expectations: &[f64]) {
    let rust = rust_of(source);
    let main = numbers_main(package, expectations);
    let stdout = run_generated_crate(package, &rust, &main);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

fn assert_rejected(source: &str, fragment: &str) {
    let out = compile(source);
    assert!(
        out.has_errors(),
        "the access must be rejected; got {:?}",
        out.diagnostics
    );
    assert!(
        out.diagnostics
            .errors()
            .any(|d| d.code.as_str() == "E0500" && d.message.contains(fragment)),
        "expected a hard E0500 error mentioning `{fragment}`, got {:?}",
        out.diagnostics
    );
}

#[test]
fn private_instance_fields_are_read_written_and_updated() {
    const SOURCE: &str = r"
class C {
  #x: number = 1;
  #y: number = 0;
  constructor(y: number) { this.#y = y; }
  read(): number { return this.#x; }
  write(v: number): void { this.#x = v; }
  inc(): number { this.#x++; return this.#x; }
  total(): number { return this.#x * 100 + this.#y; }
}
export function got0(): number {
  const c = new C(5);
  c.write(7);
  c.inc();
  return c.total();
}
";
    check("priv_field_rw", SOURCE, &[805.0]);
}

#[test]
fn the_private_field_representation_is_an_ordinary_struct_field() {
    const SOURCE: &str = r"
class C {
  #x: number = 1;
  read(): number { return this.#x; }
}
export function got0(): number { return new C().read(); }
";
    let rust = rust_of(SOURCE);
    let flat: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        flat.contains("__ps_") && flat.contains(":f64"),
        "the private field must become a uniquely named f64 struct field; got:\n{flat}"
    );
    let stdout = run_generated_crate(
        "priv_field_repr",
        &rust,
        &numbers_main("priv_field_repr", &[1.0]),
    );
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_private_field_type_is_inferred_from_an_unannotated_literal_initializer() {
    const SOURCE: &str = r"
class C {
  #x = 41;
  read(): number { return this.#x + 1; }
}
export function got0(): number { return new C().read(); }
";
    check("priv_field_infer", SOURCE, &[42.0]);
}

#[test]
fn a_private_field_reads_from_a_captured_self_closure() {
    const SOURCE: &str = r"
class C {
  #x: number = 3;
  read(): number { return this.#x; }
  via(): number {
    const self = this;
    const f = (): number => self.read() * 10 + self.#x;
    return f();
  }
}
export function got0(): number { return new C().via(); }
";
    check("priv_field_closure", SOURCE, &[33.0]);
}

#[test]
fn a_private_method_is_called_like_an_ordinary_method() {
    const SOURCE: &str = r"
class C {
  #m(): number { return 5; }
  #n(v: number): number { return v + 1; }
  #read(): number { return this.#m() + this.#n(2); }
  call(): number { return this.#read(); }
}
export function got0(): number { return new C().call(); }
";
    check("priv_method_call", SOURCE, &[8.0]);
}

#[test]
fn private_static_fields_and_methods_live_in_class_globals() {
    const SOURCE: &str = r"
class C {
  static #s: number = 3;
  static #sm(): number { return 4; }
  static read(): number { return C.#s; }
  static write(v: number): void { C.#s = v; }
  static call(): number { return C.#sm(); }
  static both(): number { return this.#s * 10 + this.#sm(); }
}
export function got0(): number { C.write(9); return C.read() * 100 + C.call(); }
export function got1(): number { return C.both(); }
";
    check("priv_static_members", SOURCE, &[904.0, 94.0]);
}

#[test]
fn a_parent_private_field_survives_inheritance() {
    const SOURCE: &str = r"
class C {
  #x: number = 11;
  read(): number { return this.#x; }
}
class D extends C {
  extra(): number { return this.read() + 1; }
}
export function got0(): number {
  const d = new D();
  return d.read() * 10 + d.extra();
}
";
    check("priv_field_inherit", SOURCE, &[122.0]);
}

#[test]
fn a_subclass_may_declare_its_own_private_field_with_the_same_name() {
    const SOURCE: &str = r"
class C {
  #x: number = 1;
  from_c(): number { return this.#x; }
}
class D extends C {
  #x: number = 2;
  from_d(): number { return this.#x; }
}
export function got0(): number {
  const d = new D();
  return d.from_c() * 10 + d.from_d();
}
";
    check("priv_field_shadow", SOURCE, &[12.0]);
}

#[test]
fn a_local_class_private_field_is_lowered() {
    const SOURCE: &str = r"
export function got0(): number {
  class C {
    #x: number = 4;
    read(): number { return this.#x; }
    bump(): number { this.#x += 10; return this.#x; }
  }
  const c = new C();
  return c.read() * 100 + c.bump();
}
";
    check("priv_field_local_class", SOURCE, &[414.0]);
}

#[test]
fn private_accessors_are_lowered_like_public_accessors() {
    const SOURCE: &str = r"
class C {
  #v: number = 1;
  get #x(): number { return this.#v; }
  set #x(v: number) { this.#v = v; }
  read(): number { return this.#x; }
  write(v: number): void { this.#x = v; }
}
export function got0(): number {
  const c = new C();
  c.write(5);
  return c.read();
}
";
    check("priv_accessors", SOURCE, &[5.0]);
}

#[test]
fn the_ergonomic_brand_check_folds_to_the_declaring_class() {
    const SOURCE: &str = r"
class C {
  #x: number = 1;
  static has(o: C): boolean { return #x in o; }
  static size(): number { return 7; }
}
export function got0(): number {
  return C.has(new C()) ? C.size() : 0;
}
";
    check("priv_brand_check", SOURCE, &[7.0]);
}

#[test]
fn accessing_a_private_field_outside_its_class_is_rejected() {
    const SOURCE: &str = r"
class C {
  #x: number = 1;
  read(): number { return this.#x; }
}
export function got0(c: C): number { return c.#x; }
";
    assert_rejected(SOURCE, "#x");
}

#[test]
fn a_static_private_member_cannot_be_reached_through_an_instance() {
    const SOURCE: &str = r"
class C {
  static #s: number = 1;
  read(): number { return this.#s; }
}
export function got0(): number { return new C().read(); }
";
    assert_rejected(SOURCE, "#s");
}

#[test]
fn a_private_method_outside_its_class_is_rejected() {
    const SOURCE: &str = r"
class C {
  #m(): number { return 5; }
  call(): number { return this.#m(); }
}
export function got0(c: C): number { return c.#m(); }
";
    assert_rejected(SOURCE, "#m");
}

#[test]
fn a_parent_private_field_cannot_be_named_from_a_subclass_body() {
    const SOURCE: &str = r"
class C {
  #x: number = 1;
  read(): number { return this.#x; }
}
class D extends C {
  leak(): number { return this.#x; }
}
export function got0(): number { return new D().leak(); }
";
    assert_rejected(SOURCE, "#x");
}
