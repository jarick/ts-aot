use std::fmt::Write as _;

use ts_aot_driver::{CompileOptions, Driver, DriverOutput, EmitStage};
use ts_aot_test_support::{check_generated_crate, run_generated_crate};

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

fn codes(out: &DriverOutput) -> Vec<String> {
    out.diagnostics
        .errors()
        .map(|d| d.code.as_str().to_owned())
        .collect()
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
    let out = compile(source);
    assert!(
        !out.has_errors(),
        "must compile through the full pipeline; got {:?}",
        out.diagnostics
    );
    let rust = out
        .rust_source
        .expect("emit-rust must populate rust_source");
    let main = numbers_main(package, expectations);
    let stdout = run_generated_crate(package, &rust, &main);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
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

#[test]
fn distinct_private_static_field_names_keep_their_own_values() {
    const SOURCE: &str = r"
export function got0(): number {
  class C {
    static #$: number = 1;
    static #_: number = 2;
    static #\u{6F}: number = 3;
    static #ZW_\u200C_NJ: number = 4;
    static read(): number {
      return this.#$ + this.#_ + this.#\u{6F} + this.#ZW_\u200C_NJ;
    }
  }
  return C.read();
}
";
    check("statics_exotic", SOURCE, &[10.0]);
}

#[test]
fn module_level_private_static_fields_emit_a_valid_crate() {
    const SOURCE: &str = r"
class C {
  static #$: number = 1;
  static #_: number = 2;
  static #\u2118: number = 3;
  static read(): number { return this.#$ + this.#_ + this.#\u2118; }
}
export function got0(): number { return C.read(); }
";
    let rust = rust_of(SOURCE);
    check_generated_crate("statics_exotic_module", &rust);
}

#[test]
fn distinct_private_instance_field_names_become_distinct_rust_fields() {
    const SOURCE: &str = r"
class C {
  #$: number = 1;
  #_: number = 2;
  #\u2118: number = 3;
  total(): number { return this.#$ + this.#_ + this.#\u2118; }
}
export function got0(): number { return new C().total(); }
";
    check("instance_exotic", SOURCE, &[6.0]);
}

#[test]
fn public_static_fields_with_exotic_names_do_not_collide() {
    const SOURCE: &str = r"
export function got0(): number {
  class C {
    static $: number = 1;
    static _: number = 2;
    static read(): number { return C.$ + C._; }
  }
  return C.read();
}
";
    check("statics_public_exotic", SOURCE, &[3.0]);
}

#[test]
fn a_global_and_a_function_sharing_a_name_still_collide() {
    let out = compile(
        "var counter: number = 0; \
         function counter(): number { return 0; } \
         export function got0(): number { counter = counter + 1; return counter; }",
    );
    assert!(out.has_errors(), "got {:?}", out.diagnostics);
    assert!(
        codes(&out).iter().any(|code| code == "E0503"),
        "a genuine name collision must still surface E0503, got {:?}",
        codes(&out)
    );
}

#[test]
fn a_local_read_only_by_a_constructor_may_be_assigned_in_the_enclosing_function() {
    const SOURCE: &str = r"
export function got0(): number {
  let resize = 0;
  class C {
    value: number = 0;
    constructor() { this.value = resize + 10; }
    read(): number { return this.value; }
  }
  const first = new C();
  resize = 5;
  const second = new C();
  return first.read() * 100 + second.read();
}
";
    check("ctor_capture_write", SOURCE, &[1015.0]);
}

#[test]
fn a_local_read_by_an_ordinary_method_and_assigned_later_is_still_rejected() {
    let out = compile(
        "export function got0(): number { \
           let total = 1; \
           class C { read(): number { return total; } } \
           const c = new C(); \
           total = 2; \
           return c.read(); \
         }",
    );
    assert!(
        codes(&out).iter().any(|code| code == "E0419"),
        "a stale snapshot read must stay rejected, got {:?}",
        codes(&out)
    );
}
