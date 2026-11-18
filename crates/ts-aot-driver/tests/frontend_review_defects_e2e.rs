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

fn codes(out: &DriverOutput) -> Vec<String> {
    out.diagnostics
        .iter()
        .map(|d| d.code.as_str().to_owned())
        .collect()
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

fn refuses_with(source: &str, code: &str, context: &str) {
    let out = compile(source);
    assert!(
        out.has_errors(),
        "{context} must be refused; got {:?}",
        codes(&out)
    );
    assert!(
        out.diagnostics.errors().any(|d| d.code.as_str() == code),
        "{context} must be refused with {code}; got {:?}",
        codes(&out)
    );
}

#[test]
fn an_empty_array_pushed_a_fraction_keeps_the_fraction() {
    const SOURCE: &str = r"
function f(): number { const a = []; a.push(1.5); return a[0]; }
export function got(): number { return f(); }
";
    runs_to("fd_a_empty_array_fraction", SOURCE, 1.5);
}

#[test]
fn an_empty_array_pushed_an_integer_then_a_fraction_keeps_the_fraction() {
    const SOURCE: &str = r"
function f(): number { const a = []; a.push(1); a.push(2.5); return a[1]; }
export function got(): number { return f(); }
";
    runs_to("fd_a_empty_array_mixed", SOURCE, 2.5);
}

#[test]
fn an_empty_array_pushed_an_integer_still_counts_its_length() {
    const SOURCE: &str = r"
export function got(): number {
  const a = [];
  a.push(7);
  return a.length;
}
";
    runs_to("fd_a_empty_array_integer", SOURCE, 1.0);
}

#[test]
fn a_shadowed_function_name_does_not_type_the_parameter_of_the_call() {
    const SOURCE: &str = r#"
function g(a: number): number { return a + 1; }
function f(x) { const g = (a: string): string => a + "!"; return g(x); }
export function got(): string { return `r=${f("abc")}`; }
"#;
    refuses_with(
        SOURCE,
        "E0405",
        "a call through a local that shadows a global function",
    );
}

#[test]
fn a_shadowed_global_object_does_not_type_a_field_write() {
    const SOURCE: &str = r#"
class A { f: number = 0; }
class B { f: string = ""; }
const a = new A();
function h(x) { const a = new B(); a.f = x; return a.f; }
export function got(): string { return `r=${h("s")}`; }
"#;
    refuses_with(
        SOURCE,
        "E0405",
        "a field write through a local that shadows a global object",
    );
}

#[test]
fn a_static_method_of_a_local_class_is_callable_from_a_closure() {
    const SOURCE: &str = r"
function f(): number {
  class C { static get(): number { return 6; } }
  const h = (): number => C.get();
  return h();
}
export function got(): number { return f(); }
";
    runs_to("fd_a_local_class_static_in_closure", SOURCE, 6.0);
}

#[test]
fn a_recursive_local_function_in_a_ternary_types_itself() {
    const SOURCE: &str = r"
function fact(n: number): number {
  function go(m: number): number { return m <= 1 ? 1 : m * go(m - 1); }
  return go(n);
}
export function got(): number { return fact(5); }
";
    runs_to("fd_a_recursive_local_ternary", SOURCE, 120.0);
}

#[test]
fn a_recursive_local_function_with_if_returns_still_runs() {
    const SOURCE: &str = r"
function fact(n: number): number {
  function go(m: number): number { if (m <= 1) { return 1; } return m * go(m - 1); }
  return go(n);
}
export function got(): number { return fact(5); }
";
    runs_to("fd_a_recursive_local_if", SOURCE, 120.0);
}

#[test]
fn a_shadowing_local_binding_refuses_to_be_a_parent_class() {
    const SOURCE: &str = r"
class A { }
export function got(): number {
  const A = 5;
  class B extends A { }
  return 1;
}
";
    refuses_with(
        SOURCE,
        "E0300",
        "a class extending a local binding that shadows the outer class",
    );
}

#[test]
fn a_nested_class_keeps_its_own_binding_in_a_member_annotation() {
    const SOURCE: &str = r"
export function outer(): number {
  class A { v(): number { return 1; } }
  function inner(): number {
    class A { w(): number { return 2; } pick(o: A): number { return o.w(); } }
    const a = new A();
    return a.pick(a);
  }
  return inner() + new A().v();
}
export function got(): number { return outer(); }
";
    runs_to("fd_nested_class_binding", SOURCE, 3.0);
}

#[test]
fn a_declaration_annotation_prefers_a_lexically_scoped_local_class() {
    const SOURCE: &str = r"
class A {
  v(): number { return 1; }
  m(): number {
    class A { w(): number { return 2; } }
    const x: A = new A();
    return x.w();
  }
}
export function got(): number { return new A().m(); }
";
    runs_to("fd_decl_local_class_priority", SOURCE, 2.0);
}

#[test]
fn a_synthesized_capture_field_does_not_collide_with_a_declared_field() {
    const SOURCE: &str = r"
export function got(): number {
  const x: number = 5;
  class C {
    __capture_x: number = 99;
    m(): number { return x; }
  }
  return new C().m();
}
";
    runs_to("fd_capture_field_collision", SOURCE, 5.0);
}

#[test]
fn an_earlier_closure_construction_stops_a_later_one_from_exempting_a_write() {
    const SOURCE: &str = r"
export function got(): number {
  let v = 1;
  class C { m(): number { return v; } }
  const g = (): number => new C().m();
  v = 2;
  const c = new C();
  return g() + c.m();
}
";
    refuses_with(
        SOURCE,
        "E0419",
        "a write after a closure that may already have constructed the class",
    );
}

#[test]
fn a_derived_construction_counts_for_a_capture_declared_by_its_base() {
    const SOURCE: &str = r"
export function got(): number {
  let v = 1;
  class Base { m(): number { return v; } }
  class Child extends Base { }
  new Child();
  v = 2;
  new Base();
  return new Base().m();
}
";
    refuses_with(SOURCE, "E0419", "a write after a derived construction");
}

#[test]
fn a_lowered_private_field_receives_its_non_literal_initializer() {
    const SOURCE: &str = r"
function side(): number { return 7; }
export function got(): number {
  class C {
    #n: number = side();
    read(): number { return this.#n; }
  }
  return new C().read();
}
";
    runs_to("fd_private_nonliteral_init", SOURCE, 7.0);
}

#[test]
fn a_field_initializer_that_reads_the_instance_is_refused_explicitly() {
    const SOURCE: &str = r"
export function got(): number {
  class C {
    #n: number = 7;
    #m: number = this.#n + 1;
    m(): number { return this.#m; }
  }
  return new C().m();
}
";
    let out = compile(SOURCE);
    assert!(
        out.diagnostics
            .errors()
            .any(|d| d.code.as_str() == "E0419" && d.message.contains("reads the instance")),
        "a field initializer that reads its own instance must be refused with an explicit \
         diagnostic; got {:?}",
        codes(&out)
    );
    assert!(
        !out.diagnostics.iter().any(|d| d.code.as_str() == "E0500"),
        "the private name is in scope while the initializer is lowered, so the old \
         missing-private-name error must be gone; got {:?}",
        codes(&out)
    );
}

#[test]
fn a_hoisted_local_function_shadows_the_global_of_the_same_name() {
    const SOURCE: &str = r#"
function g(a: number): number { return a + 1; }
export function got(): string {
  function f(x) { const r = g(x); function g(a: string): string { return a + "!"; } return r; }
  return `r=${f("abc")}`;
}
"#;
    refuses_with(
        SOURCE,
        "E0405",
        "a call resolved before a later hoisted local function of the same name",
    );
}

#[test]
fn a_local_assert_object_does_not_lend_the_shim_signature() {
    const SOURCE: &str = r#"
function assertSameValue(actual: number, expected: number, message: string = ""): number { return 0; }
export function check(x) {
  const assert = { sameValue: (a: string, b: string): string => a };
  return assert.sameValue(x, "s");
}
export function got(): string { return check("s"); }
"#;
    let out = compile(SOURCE);
    assert!(
        out.rust_source.is_none(),
        "a refused program must emit no source; got {:?}",
        codes(&out)
    );
    assert!(
        out.diagnostics.errors().any(|d| d.code.as_str() == "E0405"),
        "a local `assert` object shadows the shim, so `x` must not take the shim's parameter \
         type; got {:?}",
        codes(&out)
    );
}
#[test]
fn a_subclass_constructed_in_a_closure_forwards_its_inherited_captures() {
    const SOURCE: &str = r"
export function got(): number {
  const x: number = 3;
  class Base {
    m(): number {
      return x;
    }
  }
  class Derived extends Base {}
  const build = (): number => new Derived().m();
  return build();
}
";
    runs_to("inherited_capture_through_closure", SOURCE, 3.0);
}

#[test]
fn a_subclass_of_a_capturing_local_class_is_refused_in_a_doubly_nested_closure() {
    const SOURCE: &str = r"
export function got(): number {
  const x: number = 2;
  class Base {
    m(): number {
      return x;
    }
  }
  class Derived extends Base {}
  const outer = (): number => {
    const inner = (): number => new Derived().m();
    return inner();
  };
  return outer();
}
";
    refuses_with(
        SOURCE,
        "E0419",
        "a subclass whose parent captures a local of its declaring body",
    );
}

#[test]
fn a_field_initializer_that_reads_an_enclosing_local_is_refused_in_a_nested_closure() {
    const SOURCE: &str = r"
export function got(): number {
  const base: number = 6;
  class A {
    v: number = base;
  }
  const outer = (): number => {
    const inner = (): number => new A().v;
    return inner();
  };
  return outer();
}
";
    refuses_with(
        SOURCE,
        "E0419",
        "a field initializer reading an enclosing local of another body",
    );
}
#[test]
fn a_mutation_through_an_alias_of_a_captured_local_is_refused() {
    const SOURCE: &str = r"
export function got(): number {
  const items: number[] = [1];
  class A {
    m(): number {
      return items[0];
    }
  }
  const a = new A();
  const alias = items;
  alias.push(2);
  return a.m();
}
";
    refuses_with(
        SOURCE,
        "E0419",
        "a mutation through an alias after construction",
    );
}

#[test]
fn a_local_class_method_parameter_hint_uses_a_visible_global_function() {
    const SOURCE: &str = r"
function g(x: number): number { return x + 1; }
export function got(): number {
  class A {
    f(v) {
      return g(v);
    }
  }
  return new A().f(1);
}
";
    runs_to("local_class_hint_global", SOURCE, 2.0);
}

#[test]
fn a_local_class_method_hint_does_not_resolve_through_a_shadowing_local() {
    const SOURCE: &str = r#"
function g(x: number): number { return x + 1; }
export function got(): number {
  const g = (x: string): string => x;
  class A {
    f(v) {
      return g(v);
    }
  }
  return new A().f("s") as unknown as number;
}
"#;
    let out = compile(SOURCE);
    assert!(
        out.diagnostics
            .iter()
            .any(|d| d.code.as_str() == "E0405" && d.message.contains("has no type annotation")),
        "the enclosing local `g` shadows the module function, so the method parameter hint must \
         not borrow the module signature; got {:?}",
        out.diagnostics
    );
}

#[test]
fn a_field_initializer_capture_does_not_shift_another_class_construction() {
    const SOURCE: &str = r"
export function got(): number {
  let x: number = 1;
  class A { f: number = x; }
  class B { m(): number { return x; } }
  const a = new A();
  x = 5;
  const b = new B();
  return b.m();
}
";
    runs_to("fd_field_capture_shift", SOURCE, 5.0);
}

#[test]
fn an_arrow_parameter_annotation_prefers_a_lexically_scoped_local_class() {
    const SOURCE: &str = r"
class A {
  v(): number { return 1; }
  m(): number {
    class A {
      w(): number { return 2; }
      pick(o: A): number { return o.w(); }
    }
    const f = (p: A): number => p.pick(p);
    return f(new A());
  }
}
export function got(): number { return new A().m(); }
";
    runs_to("fd_arrow_param_local_class", SOURCE, 2.0);
}

#[test]
fn a_var_declared_in_a_block_stays_visible_after_it() {
    const SOURCE: &str = r"
function g(x: string): string { return x; }
export function got(): number {
  function f(a) {
    { var g = (n: number): number => n + 1; }
    return g(a);
  }
  return f(1);
}
";
    refuses_with(
        SOURCE,
        "E0405",
        "a var declared in a block is function scoped, so it shadows the module function after the \
         block",
    );
}

#[test]
fn a_let_declared_in_a_block_is_gone_after_it() {
    const SOURCE: &str = r"
function g(x: string): string { return x; }
export function got(): number {
  function f(a) {
    { let g = (n: number): number => n + 1; }
    return g(a);
  }
  return f(1);
}
";
    let out = compile(SOURCE);
    assert!(
        !out.diagnostics.iter().any(|d| d.code.as_str() == "E0405"),
        "a let declared in a block must not stay visible after it, so the module function may \
         still type the parameter; got {:?}",
        out.diagnostics
    );
}

#[test]
fn a_block_local_shadowing_a_parameter_keeps_its_own_storage() {
    const SOURCE: &str = r"
function takes(a: number): number { return a; }
export function got(): number {
  function f(x: number): number {
    { const x: string = 's'; }
    return takes(x);
  }
  return f(7);
}
";
    runs_to("fd_block_shadow_param", SOURCE, 7.0);
}

#[test]
fn a_block_shadowing_a_parameter_does_not_stop_its_inference_after_it() {
    const SOURCE: &str = r"
function takes(a: number): number { return a; }
export function got(): number {
  function f(x) {
    { const x: string = 's'; }
    return takes(x);
  }
  return f(7);
}
";
    runs_to("fd_block_shadow_infer", SOURCE, 7.0);
}

#[test]
fn a_switch_case_shadowing_a_parameter_does_not_stop_its_inference_after_it() {
    const SOURCE: &str = r"
function takes(a: number): number { return a; }
export function got(): number {
  function f(x, k: number) {
    switch (k) {
      case 1: { const x: string = 's'; return takes(x.length); }
      default: return takes(x);
    }
  }
  return f(7, 0);
}
";
    runs_to("fd_switch_shadow_infer", SOURCE, 7.0);
}

#[test]
fn a_read_inside_a_shadowing_block_sees_the_block_binding() {
    const SOURCE: &str = r"
export function got(): number {
  const x: number = 1;
  {
    const x: string = 'abc';
    const len: number = x.length;
    return len;
  }
}
";
    runs_to("fd_block_shadow_read", SOURCE, 3.0);
}

#[test]
fn a_closure_captures_the_shadowing_block_binding() {
    const SOURCE: &str = r"
export function got(): number {
  const x: number = 1;
  {
    const x: string = 'abcd';
    const f = (): number => x.length;
    return f();
  }
}
";
    runs_to("fd_block_shadow_closure", SOURCE, 4.0);
}

#[test]
fn a_shadowing_local_does_not_capture_a_module_binding() {
    const SOURCE: &str = r"
let count_ = 100;
export function got(): number {
  function f(count: number): number {
    let r: number = 0;
    { const count: number = 7; r = count; }
    return count_;
  }
  return f(1);
}
";
    runs_to("fd_shadow_module_ident", SOURCE, 100.0);
}

#[test]
fn a_shadowing_local_inside_a_closure_keeps_the_outer_binding() {
    const SOURCE: &str = r"
export function got(): number {
  const n: number = 10;
  const f = (): number => {
    const x: number = n;
    { const x: number = n + 1; }
    return x;
  };
  return f();
}
";
    runs_to("fd_closure_shadow_local", SOURCE, 10.0);
}

#[test]
fn a_closure_parameter_captured_by_an_inner_closure_keeps_its_value() {
    const SOURCE: &str = r"
const p: number = 100;
export function got(): number {
  const outer: number = 3;
  const f = (p: number): number => { const g = (): number => p + outer; return g(); };
  return f(4);
}
";
    runs_to("fd_closure_param_captured", SOURCE, 7.0);
}

#[test]
fn a_local_named_like_the_try_return_slot_does_not_shadow_it() {
    const SOURCE: &str = r"
function side(v: number): number { return v; }
export function got(): number {
  try { const __return_slot_0: number = 1; return side(__return_slot_0) + 1; }
  finally { side(0); }
}
";
    runs_to("fd_try_slot_name", SOURCE, 2.0);
}

#[test]
fn a_local_named_like_the_loop_signal_does_not_shadow_it() {
    const SOURCE: &str = r"
export function got(): number {
  const __pending_throw: number = 1;
  try { return __pending_throw + 1; } catch { return 0; }
}
";
    runs_to("fd_loop_signal_name", SOURCE, 2.0);
}

#[test]
fn a_parameter_named_like_the_dispatch_slice_keeps_its_value() {
    const SOURCE: &str = r"
function sum(_args: number, b: number): number { return _args + b; }
export function got(): number { return sum(1, 2); }
";
    runs_to("fd_dispatch_slice_name", SOURCE, 3.0);
}

#[test]
fn a_discarded_parameter_captured_by_a_closure_keeps_its_value() {
    const SOURCE: &str = r"
function inner(_: number): number {
  const g = (): number => _ + 1;
  return g();
}
export function got(): number { return inner(1); }
";
    runs_to("fd_discarded_param", SOURCE, 2.0);
}

#[test]
fn a_parameter_named_like_a_dispatch_slot_keeps_its_value() {
    const SOURCE: &str = r"
function f(__slot_1: number, b: number): number { return __slot_1 + b; }
export function got(): number { return f(1, 2); }
";
    runs_to("fd_dispatch_slot_name", SOURCE, 3.0);
}

#[test]
fn a_local_named_like_the_index_of_needle_keeps_its_value() {
    const SOURCE: &str = r"
export function got(): number {
  const a: i64[] = [1, 2, 3];
  const __arr: number = 2;
  return a.indexOf(__arr);
}
";
    runs_to("fd_index_of_names", SOURCE, 1.0);
}

#[test]
fn a_captured_local_shadowing_a_module_binding_is_read_through_the_env() {
    const SOURCE: &str = r"
const x: number = 100;
export function got(): number {
  const x: number = 3;
  const f = (): number => { const g = (): number => x; return g(); };
  return f();
}
";
    runs_to("fd_captured_shadow_module", SOURCE, 3.0);
}

#[test]
fn a_local_named_like_the_runtime_context_does_not_shadow_it() {
    const SOURCE: &str = r"
function side(v: number): number { return v; }
export function got(): number {
  const __ctx: number = 1;
  return side(__ctx);
}
";
    runs_to("fd_reserved_ctx_name", SOURCE, 1.0);
}
