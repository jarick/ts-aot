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

fn codes(out: &DriverOutput) -> Vec<String> {
    out.diagnostics
        .iter()
        .map(|d| d.code.as_str().to_owned())
        .collect()
}

fn assert_no_e0414(out: &DriverOutput) {
    assert!(
        !out.diagnostics.errors().any(|d| d.code.as_str() == "E0414"),
        "a local whose initializer has a statically known type must infer it; got {:?}",
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

fn run(package: &str, out: &DriverOutput) -> f64 {
    assert!(
        !out.has_errors(),
        "must compile through the full pipeline; got {:?}",
        out.diagnostics
    );
    let rust = out
        .rust_source
        .as_deref()
        .expect("emit-rust must populate rust_source");
    let main = CTX_MAIN.replace("PKG", package);
    let stdout = run_generated_crate(package, rust, &main);
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
fn an_extension_new_on_a_member_holding_an_instance_allocates_that_class() {
    const SOURCE: &str = r"
class Foo {
  bar: i32;
  constructor() {
    this.bar = 5;
  }
}
class Outer {
  static held: Foo = new Foo();
}
export function got(): number {
  Outer.held.bar = 1;
  const x = new Outer.held();
  return x.bar;
}
export function held(): number {
  return Outer.held.bar;
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
    println!("fresh={}", local_infer_new_static_field::got(&ctx));
    println!("held={}", local_infer_new_static_field::held(&ctx));
}
"#;
    let out = compile(SOURCE);
    assert!(
        codes(&out).is_empty(),
        "AOT extension: `new` on a member whose type is a class instance allocates a fresh \
         instance of that class. JavaScript would reject a non-constructor here, so this is an \
         intentional extension of the target rather than a JavaScript-conformant lowering; \
         the test name records the extension explicitly, and it must not report E0414 or any \
         other diagnostic. got {:?}",
        out.diagnostics
    );
    let rust = out
        .rust_source
        .as_deref()
        .expect("emit-rust must populate rust_source");
    let stdout = run_generated_crate("local_infer_new_static_field", rust, MAIN);
    assert_eq!(
        stdout.trim(),
        "fresh=5\nheld=1",
        "the extension must allocate a fresh instance whose `bar` is 5 while the instance the \
         member already held keeps the `bar = 1` written before the construction; got stdout:\n\
         {stdout}"
    );
}

#[test]
fn a_static_method_result_keeps_the_declared_return_type() {
    const SOURCE: &str = r"
class Desc {
  get: i32;
  constructor() {
    this.get = 7;
  }
}
class MyObj {
  static gopd(a1: i32, a2: i32): Desc {
    return new Desc();
  }
}
export function got(): number {
  const g = MyObj.gopd(1, 2).get;
  return g;
}
";
    let out = compile(SOURCE);
    assert_no_e0414(&out);
    assert!(
        !out.diagnostics.iter().any(|d| d.code.as_str() == "P0013"),
        "the declared return type of the callee must be used as the field owner type; got {:?}",
        codes(&out)
    );
    assert_exact_f64(run("local_infer_static_method_ret", &out), 7.0);
}

#[test]
fn a_method_on_a_static_field_class_value_keeps_the_declared_return_type() {
    const SOURCE: &str = r"
class Desc {
  get: i32;
  constructor() {
    this.get = 7;
  }
}
class Box {
  gopd(a1: i32, a2: i32): Desc {
    return new Desc();
  }
}
class Outer {
  static Box: Box = new Box();
}
export function got(): number {
  const g = Outer.Box.gopd(1, 2).get;
  return g;
}
";
    let out = compile(SOURCE);
    assert_no_e0414(&out);
    assert!(
        !out.diagnostics.iter().any(|d| d.code.as_str() == "P0013"),
        "the receiver type of the call must be recovered from the static field; got {:?}",
        codes(&out)
    );
    assert_exact_f64(run("local_infer_static_field_method", &out), 7.0);
}

#[test]
fn an_instance_method_result_feeds_a_field_access() {
    const SOURCE: &str = r"
class Desc {
  get: i32;
  constructor() {
    this.get = 7;
  }
}
class Box {
  desc(): Desc {
    return new Desc();
  }
}
export function got(): number {
  const box = new Box();
  const g = box.desc().get;
  return g;
}
";
    let out = compile(SOURCE);
    assert_no_e0414(&out);
    assert!(
        !out.diagnostics.iter().any(|d| d.code.as_str() == "P0013"),
        "the declared return type of the callee must be used as the field owner type; got {:?}",
        codes(&out)
    );
    assert_exact_f64(run("local_infer_instance_method_ret", &out), 7.0);
}
