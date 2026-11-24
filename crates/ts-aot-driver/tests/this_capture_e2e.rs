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

#[test]
fn an_arrow_in_a_method_reads_a_public_field_through_this() {
    const SOURCE: &str = r"
class C {
  x: number = 1;
  m(): number {
    const f = (): number => this.x;
    return f();
  }
}
export function got0(): number { return new C().m(); }
";
    check("this_arrow_public", SOURCE, &[1.0]);
}

#[test]
fn an_arrow_in_a_method_reads_a_private_field_through_this() {
    const SOURCE: &str = r"
class C {
  #x: number = 7;
  m(): number {
    const f = (): number => this.#x;
    return f();
  }
}
export function got0(): number { return new C().m(); }
";
    check("this_arrow_private", SOURCE, &[7.0]);
}

#[test]
fn an_arrow_in_a_constructor_reads_this_before_the_constructor_writes_it() {
    const SOURCE: &str = r"
class C {
  x: number = 0;
  constructor() {
    const f = (): number => this.x;
    this.x = f() + 5;
  }
}
export function got0(): number { return new C().x; }
";
    check("this_arrow_ctor", SOURCE, &[5.0]);
}

#[test]
fn an_arrow_captures_both_this_and_a_local() {
    const SOURCE: &str = r"
class C {
  x: number = 2;
  m(): number {
    const k = 40;
    const f = (): number => this.x + k;
    return f();
  }
}
export function got0(): number { return new C().m(); }
";
    check("this_arrow_two_captures", SOURCE, &[42.0]);
}

#[test]
fn a_mutating_method_observes_its_own_field_writes_through_an_arrow() {
    const SOURCE: &str = r"
class C {
  x: number = 1;
  m(): number {
    const f = (): number => this.x;
    this.x = 5;
    return f();
  }
}
export function got0(): number { return new C().m(); }
";
    check("this_arrow_mutating_read", SOURCE, &[5.0]);
}

#[test]
fn an_arrow_writes_the_receiver_field_and_the_caller_observes_it() {
    const SOURCE: &str = r"
class C {
  x: number = 1;
  inc(): number {
    const f = (): void => { this.x = this.x + 10; };
    f();
    return this.x;
  }
}
export function got0(): number {
  const c = new C();
  c.inc();
  return c.x;
}
";
    check("this_arrow_write", SOURCE, &[11.0]);
}

#[test]
fn an_arrow_can_mutate_the_receiver_through_a_mutating_method_call() {
    const SOURCE: &str = r"
class C {
  x: number = 1;
  m(): number {
    const f = (): void => { this.inc(); };
    f();
    return this.x;
  }
  inc(): void { this.x = this.x + 10; }
}
export function got0(): number { return new C().m(); }
";
    check("this_arrow_mutating_method", SOURCE, &[11.0]);
}

#[test]
fn an_argument_reading_the_receiver_field_does_not_conflict_with_the_mutating_call() {
    const SOURCE: &str = r"
class C {
  v: number = 0;
  setWith(x: number): void { this.v = x; }
  m(n: number): number {
    this.setWith(this.v + n);
    return this.v;
  }
}
export function got0(): number { return new C().m(1); }
export function got1(): number {
  const c = new C();
  c.v = 7;
  c.setWith(c.v + 3);
  return c.v;
}
";
    check("this_receiver_staged_arg", SOURCE, &[1.0, 10.0]);
}

#[test]
fn an_arrow_reads_the_receiver_after_an_inner_mutating_method_call() {
    const SOURCE: &str = r"
class C {
  c: number = 0;
  grab(): void { this.c = 5; }
  get0(): number {
    this.c = 0;
    const f = (): number => this.c;
    this.grab();
    return f();
  }
}
export function got0(): number { return new C().get0(); }
";
    check("this_arrow_inner_mutation", SOURCE, &[5.0]);
}

#[test]
fn an_escaping_this_capture_is_refused_by_the_frontend() {
    const SOURCE: &str = r"
class C {
  x: number = 3;
  grab(): () => number {
    return (): number => this.x;
  }
}
export function got0(): number {
  const c = new C();
  const f = c.grab();
  c.x = 9;
  return f();
}
";
    let out = compile(SOURCE);
    assert!(
        out.has_errors(),
        "an escaping `this` capture must be rejected so it cannot observe a stale receiver snapshot; got {:?}",
        out.diagnostics
    );
    assert!(
        out.rust_source.is_none(),
        "a refused source must not emit rust; got:\n{}",
        out.rust_source.unwrap_or_default()
    );
    assert!(
        out.diagnostics.iter().any(|d| d.code.as_str() == "E0513"),
        "expected the E0513 escaping-`this`-capture refusal; got {:?}",
        out.diagnostics
    );
}

#[test]
fn an_arrow_inside_a_local_function_does_not_capture_the_method_receiver() {
    const SOURCE: &str = r"
class C {
  x: number = 9;
  m(): number {
    const g = function (): number {
      const f = (): number => (this === undefined ? 1 : 2);
      return f();
    };
    return g();
  }
}
export function got0(): number { return new C().m(); }
";
    check("this_arrow_local_fn", SOURCE, &[1.0]);
}

#[test]
fn a_compound_receiver_mutating_call_reads_its_own_field_without_conflicting() {
    const SOURCE: &str = r"
class C {
  v: number = 0;
  setWith(x: number): void { this.v = x; }
}
export function got0(): number {
  const list: C[] = [new C()];
  list[0].setWith(list[0].v + 3);
  return list[0].v;
}
";
    check("compound_receiver_staged_arg", SOURCE, &[3.0]);
}

#[test]
fn a_mutating_call_with_the_receiver_as_its_own_argument_is_staged() {
    const SOURCE: &str = r"
class SelfArg {
  n: number = 5;
  add(o: SelfArg): void { this.n = this.n + o.n; }
  run(): number {
    const f = (): number => this.n;
    this.add(this);
    return f();
  }
}
class BareSelf {
  v: number = 2;
  merge(o: BareSelf): void { this.v = this.v + o.v; }
  run(): number { this.merge(this); return this.v; }
}
class SharedLocal {
  v: number = 3;
  merge(o: SharedLocal): void { this.v = this.v + o.v; }
}
export function got0(): number { return new SelfArg().run(); }
export function got1(): number { return new BareSelf().run(); }
export function got2(): number {
  const c = new SharedLocal();
  const f = (): number => c.v;
  c.merge(c);
  return f() + c.v;
}
";
    check("staged_self_args", SOURCE, &[10.0, 4.0, 12.0]);
}

#[test]
fn an_aliased_escaping_this_capture_is_refused_by_the_backend() {
    const SOURCE: &str = r"
class C {
  x: number = 3;
  grab(): () => number {
    const a = (): number => this.x;
    const b = a;
    this.x = 5;
    return b;
  }
}
export function got0(): number { return new C().grab()(); }
";
    let out = compile(SOURCE);
    assert!(
        out.has_errors(),
        "an escaping `this` capture through an alias must be refused; got {:?}",
        out.diagnostics
    );
    assert!(
        out.rust_source.is_none(),
        "a refused source must not emit rust; got:\n{}",
        out.rust_source.unwrap_or_default()
    );
    assert!(
        out.diagnostics.iter().any(|d| d.code.as_str() == "E0513"),
        "expected the E0513 escaping-`this`-capture refusal; got {:?}",
        out.diagnostics
    );
}

#[test]
fn an_array_literal_escaping_this_capture_is_refused_by_the_backend() {
    const SOURCE: &str = r"
class C {
  x: number = 3;
  grab(): (() => number)[] {
    const f = (): number => this.x;
    this.x = 5;
    return [f];
  }
}
export function got0(): number { return new C().grab()[0](); }
";
    let out = compile(SOURCE);
    assert!(
        out.has_errors(),
        "an escaping `this` capture through an array literal must be refused; got {:?}",
        out.diagnostics
    );
    assert!(
        out.rust_source.is_none(),
        "a refused source must not emit rust; got:\n{}",
        out.rust_source.unwrap_or_default()
    );
    assert!(
        out.diagnostics.iter().any(|d| d.code.as_str() == "E0513"),
        "expected the E0513 escaping-`this`-capture refusal; got {:?}",
        out.diagnostics
    );
}

fn refuses_escaping_this_capture(source: &str) {
    let out = compile(source);
    assert!(
        out.has_errors(),
        "an escaping `this` capture must be refused; got {:?}",
        out.diagnostics
    );
    assert!(
        out.rust_source.is_none(),
        "a refused source must not emit rust; got:\n{}",
        out.rust_source.unwrap_or_default()
    );
    assert!(
        out.diagnostics.iter().any(|d| d.code.as_str() == "E0513"),
        "expected the E0513 escaping-`this`-capture refusal; got {:?}",
        out.diagnostics
    );
}

#[test]
fn a_pushed_this_capture_escaping_in_its_array_is_refused_by_the_backend() {
    const SOURCE: &str = r"
class C {
  x: number = 3;
  grab(): (() => number)[] {
    const a: (() => number)[] = [];
    a.push((): number => this.x);
    return a;
  }
}
export function got0(): number { return new C().grab().length; }
";
    refuses_escaping_this_capture(SOURCE);
}

#[test]
fn an_element_assigned_this_capture_escaping_in_its_array_is_refused_by_the_backend() {
    const SOURCE: &str = r"
class C {
  x: number = 3;
  grab(): (() => number)[] {
    const a: (() => number)[] = [];
    const f = (): number => this.x;
    a[0] = f;
    return a;
  }
}
export function got0(): number { return new C().grab().length; }
";
    refuses_escaping_this_capture(SOURCE);
}

#[test]
fn a_field_assigned_this_capture_escaping_in_its_object_is_refused_by_the_backend() {
    const SOURCE: &str = r"
class Box {
  n: number = 1;
  f: (() => number) | null = null;
}
class C {
  x: number = 3;
  grab(): Box {
    const f = (): number => this.x;
    const o = new Box();
    o.f = f;
    return o;
  }
}
export function got0(): number { return new C().grab().n; }
";
    refuses_escaping_this_capture(SOURCE);
}

#[test]
fn a_struct_literal_this_capture_read_through_its_field_is_refused_by_the_backend() {
    const SOURCE: &str = r"
class C {
  x: number = 3;
  grab(): () => number {
    const f = (): number => this.x;
    const b = { f };
    return b.f;
  }
}
export function got0(): number { return new C().grab()(); }
";
    refuses_escaping_this_capture(SOURCE);
}

#[test]
fn a_pushed_this_capture_escaping_through_this_field_is_refused_by_the_backend() {
    const SOURCE: &str = r"
class C {
  xs: (() => number)[] = [];
  x: number = 3;
  m(): void { this.xs.push((): number => this.x); }
}
export function got0(): number { const c = new C(); c.m(); c.x = 100; return c.xs[0](); }
";
    refuses_escaping_this_capture(SOURCE);
}

#[test]
fn a_pushed_this_capture_escaping_through_a_global_container_is_refused_by_the_backend() {
    const SOURCE: &str = r"
const g: number[] = [];
class C {
  x: number = 3;
  m(): void { g.push(((): number => this.x) as number); }
}
export function got0(): number { const c = new C(); c.m(); return (g[0] as () => number)(); }
";
    refuses_escaping_this_capture(SOURCE);
}

#[test]
fn a_thrown_this_capture_is_refused_by_the_backend() {
    const SOURCE: &str = r"
class C {
  x: number = 3;
  m(): void { throw ((): number => this.x); }
}
";
    refuses_escaping_this_capture(SOURCE);
}

#[test]
fn this_container_writes_without_an_escaping_this_capture_are_accepted() {
    const SOURCE: &str = r"
const g: number[] = [];
class C {
  xs: number[] = [];
  x: number = 3;
  pushNumber(): void { this.xs.push(1); }
  pushGlobal(): void { g.push(4); }
  lenGlobal(): number { return g.length; }
  callLocal(): number {
    const a: (() => number)[] = [];
    a.push((): number => this.x);
    const f = a[0];
    return f();
  }
  readX(): number { return this.x; }
}
export function got0(): number { const c = new C(); c.pushNumber(); c.pushGlobal(); return c.lenGlobal(); }
export function got1(): number { return new C().callLocal(); }
export function got2(): number { return new C().readX(); }
";
    check("this_container_no_escape", SOURCE, &[1.0, 3.0, 3.0]);
}

#[test]
fn a_concatenated_this_capture_is_refused_by_the_backend() {
    const SOURCE: &str = r"
class C {
  x: number = 3;
  grab(): (() => number)[] {
    const a: (() => number)[] = [];
    return a.concat([(): number => this.x]);
  }
}
export function got0(): number {
  const c = new C();
  const f = c.grab()[0];
  c.x = 9;
  return f();
}
";
    refuses_escaping_this_capture(SOURCE);
}

#[test]
fn a_sliced_this_capture_is_refused_by_the_backend() {
    const SOURCE: &str = r"
class C {
  x: number = 3;
  grab(): (() => number)[] {
    const a: (() => number)[] = [(): number => this.x];
    return a.slice();
  }
}
export function got0(): number {
  const c = new C();
  const f = c.grab()[0];
  c.x = 9;
  return f();
}
";
    refuses_escaping_this_capture(SOURCE);
}

#[test]
fn an_array_from_mapped_identity_this_capture_is_refused_by_the_backend() {
    const SOURCE: &str = r"
function passthrough(x: () => number): () => number { return x; }
class C {
  x: number = 3;
  grab(): (() => number)[] {
    const a: (() => number)[] = [(): number => this.x];
    return Array.from(a, passthrough);
  }
}
export function got0(): number {
  const c = new C();
  const g = c.grab()[0];
  c.x = 9;
  return g();
}
";
    refuses_escaping_this_capture(SOURCE);
}

#[test]
fn a_destructured_this_capture_element_is_refused_by_the_backend() {
    const SOURCE: &str = r"
class C {
  x: number = 3;
  grab(): (() => number) | null {
    const a: ((() => number) | null)[] = [(): number => this.x];
    const [f] = a;
    return f;
  }
}
export function got0(): number {
  const c = new C();
  const f = c.grab() as () => number;
  c.x = 9;
  return f();
}
";
    refuses_escaping_this_capture(SOURCE);
}

#[test]
fn a_mapped_array_of_numbers_and_a_destructured_number_are_accepted() {
    const SOURCE: &str = r"
function toNumber(x: () => number): number { return x(); }
class C {
  x: number = 3;
  sumMapped(): number {
    const a: (() => number)[] = [(): number => this.x];
    const nums: number[] = Array.from(a, toNumber);
    const [n] = nums;
    return n + nums[0];
  }
  firstLocal(): number {
    const numbers: number[] = [1, 2];
    const [n] = numbers;
    return n;
  }
}
export function got0(): number {
  const c = new C();
  return c.sumMapped() + c.firstLocal();
}
";
    check("this_capture_mapped_numbers", SOURCE, &[7.0]);
}

#[test]
fn an_array_from_mapped_struct_wrapping_this_capture_is_refused_by_the_backend() {
    const SOURCE: &str = r"
class Box { f: (() => number) | null = null; n: number = 5; }
function boxIt(f: () => number): Box { const b = new Box(); b.f = f; return b; }
class C {
  x: number = 3;
  grab(): Box[] {
    const a: (() => number)[] = [(): number => this.x];
    return Array.from(a, boxIt);
  }
}
export function got0(): number {
  const c = new C();
  const box = c.grab()[0];
  c.x = 9;
  const g = box.f as () => number;
  return g();
}
";
    refuses_escaping_this_capture(SOURCE);
}

#[test]
fn a_destructured_struct_field_that_cannot_hold_a_closure_is_accepted_by_the_backend() {
    const SOURCE: &str = r"
class Box { f: (() => number) | null = null; n: number = 5; }
class C {
  x: number = 3;
  m(): number {
    const b = new Box();
    b.f = (): number => this.x;
    const arr: Box[] = [b];
    const [o] = arr;
    return o.n;
  }
}
export function got0(): number { return new C().m(); }
";
    let out = compile(SOURCE);
    assert!(
        !out.has_errors(),
        "destructuring an array of structs and reading a numeric field must not look like an \
         escaping `this` capture; got {:?}",
        out.diagnostics
    );
    assert!(
        out.rust_source.is_some(),
        "an accepted source must emit rust"
    );
}

#[test]
fn a_destructured_nullable_struct_field_that_cannot_hold_a_closure_is_accepted() {
    const SOURCE: &str = r"
class Box { f: (() => number) | null = null; n: number = 5; }
class C {
  x: number = 3;
  m(): number {
    const b = new Box();
    b.f = (): number => this.x;
    const arr: (Box | null)[] = [b];
    const [o] = arr;
    return o!.n;
  }
}
export function got0(): number { return new C().m(); }
";
    check("this_capture_destructured_nullable_field", SOURCE, &[5.0]);
}

#[test]
fn an_indexed_struct_field_that_cannot_hold_a_closure_is_accepted() {
    const SOURCE: &str = r"
class Box { f: (() => number) | null = null; n: number = 7; }
class C {
  x: number = 3;
  m(): number {
    const b = new Box();
    b.f = (): number => this.x;
    const arr: Box[] = [b];
    return arr[0].n;
  }
}
export function got0(): number { return new C().m(); }
";
    check("this_capture_indexed_number_field", SOURCE, &[7.0]);
}
