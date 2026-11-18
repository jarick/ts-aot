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
        .map(|d| format!("{}/{:?}", d.code.as_str(), d.severity))
        .collect()
}

fn assert_no_unwalked(out: &DriverOutput) {
    assert!(
        out.diagnostics.iter().all(|d| d.code.as_str() != "E0500"),
        "the form must be walked without E0500; got {:?}",
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

const ASYNC_MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    let promise = ts_aot_runtime::__ts_aot_run_or_poll(PKG::got(&ctx));
    let value = ts_aot_runtime::__ts_aot_await_value(&promise);
    assert_eq!(value, 7.0);
    println!("ok");
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
fn a_local_function_declaration_is_called_after_its_statement() {
    const SOURCE: &str = r"
export function got(): number {
  function g(): number {
    return 1;
  }
  return g() + g() + 1;
}
";
    let out = compile(SOURCE);
    assert_no_unwalked(&out);
    assert_exact_f64(run("body_local_fn_after", SOURCE), 3.0);
}

#[test]
fn a_local_function_declaration_captures_an_enclosing_local() {
    const SOURCE: &str = r"
export function got(): number {
  let total = 0;
  function add(n: number): number {
    total = total + n;
    return total;
  }
  add(3);
  add(4);
  return total;
}
";
    let out = compile(SOURCE);
    assert_no_unwalked(&out);
    assert_exact_f64(run("body_local_fn_capture", SOURCE), 7.0);
}

#[test]
fn nested_local_function_declarations_are_walked() {
    const SOURCE: &str = r"
export function got(): number {
  function outer(n: number): number {
    function inner(m: number): number {
      return m * 2;
    }
    return inner(n) + 1;
  }
  return outer(5);
}
";
    let out = compile(SOURCE);
    assert_no_unwalked(&out);
    assert_exact_f64(run("body_local_fn_nested", SOURCE), 11.0);
}

#[test]
fn a_local_function_declaration_inside_a_block_stays_inside_it() {
    const SOURCE: &str = r"
export function got(): number {
  let v = 0;
  if (v === 0) {
    function g(): number {
      return 6;
    }
    v = g();
  }
  return v;
}
";
    let out = compile(SOURCE);
    assert_no_unwalked(&out);
    assert_exact_f64(run("body_local_fn_block", SOURCE), 6.0);
}

#[test]
fn a_local_function_declaration_infers_its_return_type_from_the_body() {
    const SOURCE: &str = r"
export function got(): number {
  function ident(n: number) {
    return n;
  }
  return ident(9) + 1;
}
";
    let out = compile(SOURCE);
    assert_no_unwalked(&out);
    assert!(
        !out.diagnostics.errors().any(|d| d.code.as_str() == "E0415"),
        "the local function return type must be inferred; got {:?}",
        codes(&out)
    );
    assert_exact_f64(run("body_local_fn_infer", SOURCE), 10.0);
}

#[test]
fn a_local_function_declaration_inside_a_method_captures_this() {
    const SOURCE: &str = r"
class Box {
  x: number = 4;
  m(): number {
    const self = this;
    function read(): number {
      return self.x;
    }
    return read() + self.x;
  }
}
export function got(): number {
  return new Box().m();
}
";
    let out = compile(SOURCE);
    assert_no_unwalked(&out);
    assert_exact_f64(run("body_local_fn_method", SOURCE), 8.0);
}

#[test]
fn an_async_function_expression_result_is_refused_in_arithmetic() {
    const SOURCE: &str = r"
export function got(): number {
  const f = async function (n: number): Promise<number> {
    return n + 1;
  };
  return f(1) + 5;
}
";
    let out = compile(SOURCE);
    assert_no_unwalked(&out);
    assert!(
        out.diagnostics.errors().any(|d| d.code.as_str() == "E0419"),
        "using an async function expression's `Promise<number>` result in an arithmetic \
         operation must be a hard refusal (E0419), not a silent synchronous lowering into invalid \
         Rust; got {:?}",
        codes(&out)
    );
    assert!(
        out.rust_source.is_none(),
        "a refused async function expression must not emit Rust; got {} bytes",
        out.rust_source.as_deref().map_or(0, str::len)
    );
}

#[test]
fn an_async_function_expression_result_is_awaited_as_a_promise() {
    const SOURCE: &str = r"
export async function got(): Promise<number> {
  const f = async function (n: number): Promise<number> {
    return n + 1;
  };
  const v = await f(1);
  return v + 5;
}
";
    let out = compile(SOURCE);
    assert_no_unwalked(&out);
    assert!(
        !out.diagnostics.errors().any(|d| d.code.as_str() == "E0419"),
        "awaiting an async function expression's `Promise<number>` result must be accepted; \
         got {:?}",
        codes(&out)
    );
    let rust = out
        .rust_source
        .expect("emit-rust must populate rust_source");
    let main = ASYNC_MAIN.replace("PKG", "body_async_promise");
    let stdout = run_generated_crate("body_async_promise", &rust, &main);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn an_async_function_used_as_a_value_does_not_break_the_crate() {
    const SOURCE: &str = r"
export function got(): number {
  const f = async function (n: number): Promise<number> {
    return n + 1;
  };
  return 3;
}
";
    let out = compile(SOURCE);
    assert_no_unwalked(&out);
    assert_exact_f64(run("body_async_fn_value", SOURCE), 3.0);
}

#[test]
fn a_local_generator_function_declaration_is_lifted_and_can_be_iterated() {
    const SOURCE: &str = r"
export function got(): number {
  function* g(): number {
    yield 1;
    yield 2;
  }
  let sum = 0;
  for (const x of g()) {
    sum = sum + x;
  }
  return sum;
}
";
    let out = compile(SOURCE);
    assert_no_unwalked(&out);
    assert_exact_f64(run("body_local_generator", SOURCE), 3.0);
}

#[test]
fn a_super_method_call_reaches_an_inherited_parent_method() {
    const SOURCE: &str = r"
class Base {
  m(): number {
    return 1;
  }
  n(): number {
    return 10;
  }
}
class Derived extends Base {
  both(): number {
    return super.m() + super.n();
  }
}
export function got(): number {
  return new Derived().both() + 2;
}
";
    let out = compile(SOURCE);
    assert_no_unwalked(&out);
    assert_exact_f64(run("body_super_inherited", SOURCE), 13.0);
}

#[test]
fn a_super_call_to_an_overridden_method_reaches_the_parent_body() {
    const SOURCE: &str = r"
class Base {
  add(a: number, b: number): number {
    return a + b;
  }
}
class Derived extends Base {
  add(a: number, b: number): number {
    return super.add(a, b) + 1;
  }
}
export function got(): number {
  return new Derived().add(2, 3);
}
";
    let out = compile(SOURCE);
    assert_no_unwalked(&out);
    assert_exact_f64(run("body_super_overridden", SOURCE), 6.0);
}
