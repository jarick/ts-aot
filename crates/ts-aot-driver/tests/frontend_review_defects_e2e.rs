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

fn assert_refused_with(out: &DriverOutput, code: &str, context: &str) {
    assert!(
        out.has_errors(),
        "{context} must be refused; got {:?}",
        codes(out)
    );
    assert!(
        out.diagnostics.errors().any(|d| d.code.as_str() == code),
        "{context} must be refused with {code}; got {:?}",
        codes(out)
    );
}

fn refuses_with(source: &str, code: &str, context: &str) {
    let out = compile(source);
    assert_refused_with(&out, code, context);
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
    const SOURCE: &str = r#"
function g(x: string): string { return x; }
export function got(): number {
  function f(a): string {
    { let g = (n: number): number => n + 1; }
    return g(a);
  }
  return f("abc").length;
}
"#;
    runs_to("fd_block_let_scope", SOURCE, 3.0);
}

#[test]
fn a_local_generic_generator_declaration_is_refused_by_the_body_walker() {
    const SOURCE: &str = r"
export function got(): number {
  function* g<T>(x: T): Generator<T> { yield x; }
  return 0;
}
";
    let out = compile(SOURCE);
    assert!(
        out.diagnostics.errors().any(|d| {
            d.code.as_str() == "E0500"
                && d.message.contains(
                    "a generic generator declaration inside a function body is not supported by \
                     the body walker",
                )
        }),
        "a local generic generator declaration must take the generic-generator guard instead of \
         being lifted without its type parameters; got {:?}",
        codes(&out)
    );
    assert!(
        out.rust_source.is_none(),
        "a local generic generator declaration must not emit Rust; got {:?}",
        out.rust_source
    );
}

#[test]
fn an_inherited_field_types_a_derived_method_return() {
    const SOURCE: &str = r"
class A { n: number; constructor() { this.n = 4; } }
class B extends A { read() { return this.n; } }
export function got(): number { return new B().read(); }
";
    runs_to("fd_inherited_field_return", SOURCE, 4.0);
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

#[test]
fn a_class_expression_field_initializer_resolves_the_enclosing_parameter() {
    const SOURCE: &str = r"
function f(k: number): number {
  class H { C = class { static s(): number { return k; } }; }
  return new H().C.s();
}
export function got(): number { return f(5); }
";
    runs_to("fd_class_value_field_param", SOURCE, 5.0);
}

#[test]
fn a_class_expression_field_initializer_resolves_the_enclosing_class() {
    const SOURCE: &str = r"
class G { static v(): number { return 4; } }
function f(): number {
  class H { C = class { static s(): number { return G.v(); } }; }
  return new H().C.s();
}
export function got(): number { return f(); }
";
    runs_to("fd_class_value_field_class", SOURCE, 4.0);
}

#[test]
fn a_class_expression_field_initializer_resolves_the_enclosing_function() {
    const SOURCE: &str = r"
function k(): number { return 6; }
function f(): number {
  class H { C = class { static s(): number { return k(); } }; }
  return new H().C.s();
}
export function got(): number { return f(); }
";
    runs_to("fd_class_value_field_fn", SOURCE, 6.0);
}

#[test]
fn a_class_expression_field_initializer_resolves_the_enclosing_global() {
    const SOURCE: &str = r"
const k: number = 5;
function f(): number {
  class H { C = class { static s(): number { return k; } }; }
  return new H().C.s();
}
export function got(): number { return f(); }
";
    runs_to("fd_class_value_field_global", SOURCE, 5.0);
}

#[test]
fn a_class_expression_field_initializer_resolves_the_enclosing_local() {
    const SOURCE: &str = r"
export function got(): number {
  const k = 5;
  class H { C = class { static s(): number { return k; } }; }
  return new H().C.s();
}
";
    runs_to("fd_class_value_field_capture", SOURCE, 5.0);
}

#[test]
fn a_local_class_static_field_initializer_reading_an_enclosing_local_is_refused() {
    const SOURCE: &str = r"
export function got(): number {
  const k = 5;
  class H { static kk = k; }
  return H.kk;
}
";
    refuses_without_rust(
        SOURCE,
        "E0419",
        "a local-class static initializer reading an enclosing local",
    );
}

fn refuses_without_rust(source: &str, code: &str, context: &str) {
    let out = compile(source);
    assert_refused_with(&out, code, context);
    assert!(
        out.rust_source.is_none(),
        "{context} must be refused without emitting Rust; got {:?}",
        codes(&out)
    );
}

#[test]
fn a_static_method_capture_uses_the_recorded_binding_not_a_shadow() {
    const SOURCE: &str = r"
export function got(): number {
  const x = 3;
  class C { static read(): number { return x; } }
  let r = 0;
  { const x = 9; r = C.read(); }
  return r;
}
";
    runs_to("fd_static_capture_shadow", SOURCE, 3.0);
}

#[test]
fn a_void_call_result_is_not_typed_as_a_function() {
    const SOURCE: &str = r"
function nothing(): void {}
export function got(): number {
  const x = nothing() ?? 1;
  return x;
}
";
    runs_to("fd_void_call_result", SOURCE, 1.0);
}

#[test]
fn a_local_class_named_date_shadows_the_builtin() {
    const SOURCE: &str = r"
export function got(): number {
  class Date { value(): number { return 7; } }
  const f = (): number => new Date().value();
  return f();
}
";
    runs_to("fd_local_date_shadow", SOURCE, 7.0);
}

#[test]
fn a_brand_check_against_the_declaring_class_still_folds_true() {
    const SOURCE: &str = r"
class Box {
  #x: number = 1;
  static has(o: Box): boolean { return #x in o; }
}
export function got(): number { return Box.has(new Box()) ? 3 : 0; }
";
    runs_to("fd_brand_declaring", SOURCE, 3.0);
}

#[test]
fn a_brand_check_against_a_later_class_is_refused_instead_of_folding_false() {
    const SOURCE: &str = r"
class Base {
  #x: number = 1;
  static has(o: Derived): boolean { return #x in o; }
}
class Derived extends Base {}
export function got(): number { return Base.has(new Derived()) ? 3 : 0; }
";
    refuses_without_rust(
        SOURCE,
        "E0500",
        "a brand check whose owner class is declared later",
    );
}

#[test]
fn a_static_method_call_keeps_the_owner_side_effect() {
    const SOURCE: &str = r"
class Holder { C = class { static run(): number { return 1; } }; }
let n = 0;
export function got(): number {
  const h = new Holder();
  const f = (): Holder => { n = n + 1; return h; };
  return f().C.run() * 10 + n;
}
";
    runs_to("fd_static_owner_effect", SOURCE, 11.0);
}

#[test]
fn a_mutation_through_an_intermediate_alias_is_refused() {
    const SOURCE: &str = r"
function f(): number {
  const base: number[] = [1, 2];
  const items = base;
  const alias = items;
  class C { read(): number { return items[0]; } }
  const c = new C();
  alias[0] = 2;
  return c.read();
}
export function got(): number { return f(); }
";
    refuses_without_rust(SOURCE, "E0419", "a write through an intermediate alias");
}

#[test]
fn a_mutation_through_a_reassigned_alias_is_refused() {
    const SOURCE: &str = r"
function f(): number {
  const items: number[] = [1, 2];
  let alias: number[] = [];
  alias = items;
  class C { read(): number { return items[0]; } }
  const c = new C();
  alias[0] = 2;
  return c.read();
}
export function got(): number { return f(); }
";
    refuses_without_rust(SOURCE, "E0419", "a write through a reassigned alias");
}

#[test]
fn storing_a_tracked_this_capturing_local_on_this_is_refused() {
    const SOURCE: &str = r"
class Box {
  x: number = 1;
  callback: number = 0;
  install(): number {
    const f = (): number => this.x;
    this.callback = f;
    return 1;
  }
}
export function got(): number { return new Box().install(); }
";
    refuses_without_rust(SOURCE, "E0513", "storing a this-capturing local on this");
}

#[test]
fn storing_a_tracked_this_capturing_local_from_an_inner_closure_is_refused() {
    const SOURCE: &str = r"
class Box {
  x: number = 1;
  callback: number = 0;
  install(): number {
    const f = (): number => this.x;
    const set = (): number => { this.callback = f; return 1; };
    return set();
  }
}
export function got(): number { return new Box().install(); }
";
    refuses_without_rust(
        SOURCE,
        "E0513",
        "storing a this-capturing local from an inner closure on this",
    );
}

#[test]
fn reassigning_a_this_capturing_local_clears_the_fact() {
    const SOURCE: &str = r"
class Counter {
  x: number = 3;
  pick(): () => number {
    let f = (): number => this.x;
    f = (): number => 0;
    return f;
  }
}
export function got(): number { return new Counter().pick()(); }
";
    runs_to("fd_this_fact_cleared", SOURCE, 0.0);
}

#[test]
fn a_constructor_only_capture_does_not_pin_the_first_construction() {
    const SOURCE: &str = r"
function f(): number {
  let x: number = 1;
  class A { constructor() { const y: number = x; } }
  const a = new A();
  x = 5;
  class B { read(): number { return x; } }
  const b = new B();
  return b.read();
}
export function got(): number { return f(); }
";
    runs_to("fd_ctor_only_capture", SOURCE, 5.0);
}

#[test]
fn writing_a_local_class_static_field_is_refused() {
    const SOURCE: &str = r"
function f(): number {
  class C { static n: number = 0; }
  return ++C.n;
}
export function got(): number {
  const a = f();
  const b = f();
  return a * 10 + b;
}
";
    refuses_without_rust(
        SOURCE,
        "E0419",
        "incrementing a local-class static field shared across evaluations",
    );
}

#[test]
fn assigning_a_local_class_static_field_is_refused() {
    const SOURCE: &str = r"
function f(): number {
  class C { static n: number = 0; }
  C.n = 3;
  return C.n;
}
export function got(): number { return f(); }
";
    refuses_without_rust(
        SOURCE,
        "E0419",
        "assigning a local-class static field shared across evaluations",
    );
}

#[test]
fn an_annotated_coalesce_binding_may_not_drop_a_literal_null_fallback() {
    const SOURCE: &str = r"
class K { m(): number | undefined { return 3; } }
export function got(k: K | null): number { let r: number = k?.m() ?? null; return r ? 1 : 0; }
";
    let out = compile(SOURCE);
    assert!(
        out.diagnostics
            .errors()
            .any(|d| d.code.as_str() == "P0018" && d.message.contains("the annotation of `r`")),
        "`k?.m() ?? null` can yield the fallback's `null`, so the `number` annotation of `r` must \
         be reported instead of silently widened; got {:?}",
        codes(&out)
    );
    assert!(
        out.rust_source.is_none(),
        "a rejected annotated coalesce binding must not emit Rust; got {:?}",
        out.rust_source
    );
}

#[test]
fn an_unannotated_coalesce_binding_keeps_the_optional_type() {
    const SOURCE: &str = r"
class K { m(): number | undefined { return 3; } }
function inner(k: K | null): number { const r = k?.m() ?? null; return r ? 1 : 0; }
export function got(): number { return inner(new K()) * 10 + inner(null); }
";
    let rust = rust_of(SOURCE);
    let norm: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        norm.contains("letr:Option<f64>"),
        "`const r = k?.m() ?? null` is `number | null` and must declare an `Option<f64>`; got:\n\
         {rust}"
    );
    let stdout = run_generated_crate(
        "fd_coalesce_literal_null",
        &rust,
        &module_main("fd_coalesce_literal_null", 10.0),
    );
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn an_explicitly_optional_annotation_accepts_the_literal_null_fallback() {
    const SOURCE: &str = r"
class K { m(): number | undefined { return 3; } }
function pick(k: K | null): number | null { const r: number | null = k?.m() ?? null; return r; }
export function got(): number { return pick(new K()) ?? 0; }
";
    runs_to("fd_coalesce_literal_null_annotated", SOURCE, 3.0);
}

#[test]
fn an_unannotated_void_fallback_is_undefined_and_runs() {
    const SOURCE: &str = r"
let hits: number = 0;
function bump(): number { hits = hits + 1; return hits; }
class K { m(): number | undefined { return 3; } }
export function viaZero(present: boolean): number | undefined {
  const k: K | null = present ? new K() : null;
  const r = k?.m() ?? void 0;
  return r;
}
export function viaCall(present: boolean): number | undefined {
  const k: K | null = present ? new K() : null;
  const r = k?.m() ?? void bump();
  return r;
}
export function viaParen(present: boolean): number | undefined {
  const k: K | null = present ? new K() : null;
  const r = k?.m() ?? void (hits = hits + 10);
  return r;
}
export function hitsNow(): number { return hits; }
";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    let some = ::core::option::Option::Some;
    let none = ::core::option::Option::None;
    // node: `new K().m()` is 3, so a present chain yields 3 and the fallback is
    // never taken; an absent chain is `undefined`, which the fallback repeats.
    assert_eq!(fd_void_fallback::viaZero(&ctx, true), some(3.0));
    assert_eq!(fd_void_fallback::viaZero(&ctx, false), none);
    assert_eq!(fd_void_fallback::viaCall(&ctx, true), some(3.0));
    assert_eq!(
        fd_void_fallback::hitsNow(&ctx),
        0.0,
        "the fallback must not run when the left operand holds a value"
    );
    assert_eq!(fd_void_fallback::viaCall(&ctx, false), none);
    assert_eq!(
        fd_void_fallback::hitsNow(&ctx),
        1.0,
        "`void bump()` must still evaluate `bump()` when the fallback is taken"
    );
    assert_eq!(fd_void_fallback::viaParen(&ctx, true), some(3.0));
    assert_eq!(fd_void_fallback::hitsNow(&ctx), 1.0);
    assert_eq!(fd_void_fallback::viaParen(&ctx, false), none);
    assert_eq!(
        fd_void_fallback::hitsNow(&ctx),
        11.0,
        "`void (hits = hits + 10)` must still evaluate its operand when the fallback is taken"
    );
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let norm: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        norm.contains("letr:Option<f64>"),
        "`const r = k?.m() ?? void 0` is `number | undefined` and must declare an `Option<f64>`; \
         got:\n{rust}"
    );
    assert!(
        !norm.contains("=()"),
        "a `void` fallback must lower as `None` and must never be assigned as `()` into a value \
         slot; got:\n{rust}"
    );
    check_generated_crate("fd_void_fallback", &rust);
    let stdout = run_generated_crate("fd_void_fallback", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn an_annotated_binding_rejects_a_literal_void_fallback() {
    const SOURCE: &str = r"
class K { m(): number | undefined { return 3; } }
export function got(k: K | null): number { let r: number = k?.m() ?? void 0; return r ? 1 : 0; }
";
    let out = compile(SOURCE);
    assert!(
        out.diagnostics
            .errors()
            .any(|d| d.code.as_str() == "P0018" && d.message.contains("the annotation of `r`")),
        "`k?.m() ?? void 0` is `number | undefined` (`void 0` is `undefined`), so the `number` \
         annotation of `r` must be reported instead of silently widening it; got {:?}",
        codes(&out)
    );
    assert!(
        out.rust_source.is_none(),
        "a rejected annotated void fallback must not emit Rust; got {:?}",
        out.rust_source
    );
}

#[test]
fn an_annotated_binding_rejects_a_void_expression_fallback() {
    const SOURCE: &str = r"
let hits: number = 0;
function bump(): number { hits = hits + 1; return hits; }
class K { m(): number | undefined { return 3; } }
export function got(k: K | null): number { let r: number = k?.m() ?? void bump(); return r ? 1 : 0; }
";
    let out = compile(SOURCE);
    assert!(
        out.diagnostics
            .errors()
            .any(|d| d.code.as_str() == "P0018" && d.message.contains("the annotation of `r`")),
        "`void bump()` is `undefined`, so `k?.m() ?? void bump()` may be `undefined` and the \
         `number` annotation of `r` must be reported; got {:?}",
        codes(&out)
    );
    assert!(
        out.rust_source.is_none(),
        "a rejected annotated void expression fallback must not emit Rust; got {:?}",
        out.rust_source
    );
}

#[test]
fn a_nested_var_initializer_reads_the_parameter() {
    const SOURCE: &str = r"
export function viaFor(x: number): number { for (let i = 0; i < 2; i = i + 1) { var x = x + 1; } return x; }
export function viaWhile(x: number): number { let i = 0; while (i < 2) { i = i + 1; var x = x + 1; } return x; }
export function viaIf(x: number): number { if (true) { var x = x + 1; } return x; }
export function viaBlock(x: number): number { { var x = x + 1; } return x; }
export function viaEarlierRead(x: number): number { { const y = x; var x = x + 1; return y * 10 + x; } }
";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    // node: `var x = x + 1` assigns the parameter, so `viaFor(5)` is 7 (two
    // iterations), `viaWhile(5)` is 7, `viaIf(5)` and `viaBlock(5)` are 6, and a
    // read before the declaration sees the parameter (`viaEarlierRead(5)` is 56).
    assert_eq!(fd_var_nested::viaFor(&ctx, 5.0), 7.0);
    assert_eq!(fd_var_nested::viaWhile(&ctx, 5.0), 7.0);
    assert_eq!(fd_var_nested::viaIf(&ctx, 5.0), 6.0);
    assert_eq!(fd_var_nested::viaBlock(&ctx, 5.0), 6.0);
    assert_eq!(fd_var_nested::viaEarlierRead(&ctx, 5.0), 56.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let norm: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        !norm.contains("__local"),
        "the initializer of a hoisted `var` in a nested block must read the parameter's binding; \
         a `__local` reference means it read a hoisted local whose declaration was elided, which \
         makes the generated crate fail `cargo check` with E0425. got:\n{rust}"
    );
    check_generated_crate("fd_var_nested", &rust);
    let stdout = run_generated_crate("fd_var_nested", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_nullish_var_redeclaring_a_parameter_is_refused() {
    for source in [
        "export function probe(x: number): number { { var x = null; } return x; }",
        "export function probe(x: number): number { { var x = void 0; } return x; }",
        "export function probe(x: number): number { { const y = x; var x = undefined; return y; } }",
    ] {
        let out = compile(source);
        assert!(
            out.diagnostics
                .errors()
                .any(|d| { d.code.as_str() == "P0018" && d.message.contains("names a parameter") }),
            "`{source}` writes a nullish value into a non-optional parameter, which the target \
             cannot represent; silently dropping the write made an earlier read dangle, and \
             silently keeping it emitted `()` where a value is expected. got {:?}",
            codes(&out)
        );
        assert!(
            out.rust_source.is_none(),
            "`{source}` must be refused without emitting Rust; got {:?}",
            out.rust_source
        );
    }
}

#[test]
fn an_array_destructuring_var_assigns_the_parameter() {
    const SOURCE: &str = r"
export function fromFor(x: number): number { for (var [x] = [11]; false;) { } return x; }
export function fromBlock(x: number): number { { var [x] = [11]; } return x; }
export function fromI32(x: i32): i32 { for (var [x] = [11]; false;) { } return x; }
export function shadowed(x: number): number { { let [x] = [7]; } return x; }
export function earlierRead(x: number): number { { const y = x; var [x] = [11]; return y * 10 + x; } }
";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    // node: `var [x] = [11]` writes 11 into the parameter, while a block-scoped
    // `let [x] = [7]` leaves it alone; a read before the `var` sees the parameter
    // (`earlierRead(5)` is 61).
    assert_eq!(fd_var_array_destructure::fromFor(&ctx, 5.0), 11.0);
    assert_eq!(fd_var_array_destructure::fromBlock(&ctx, 5.0), 11.0);
    assert_eq!(fd_var_array_destructure::fromI32(&ctx, 5), 11);
    assert_eq!(fd_var_array_destructure::shadowed(&ctx, 5.0), 5.0);
    assert_eq!(fd_var_array_destructure::earlierRead(&ctx, 5.0), 61.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let norm: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        !norm.contains("__local"),
        "an array-destructuring `var` that names a parameter must write the parameter; got:\n\
         {rust}"
    );
    for name in ["fromFor", "fromBlock", "fromI32"] {
        let start = rust
            .find(&format!("pub fn {name} "))
            .unwrap_or_else(|| panic!("the generated crate must declare `{name}`; got:\n{rust}"));
        let tail = &rust[start..];
        let end = tail[1..].find("pub fn ").map_or(tail.len(), |i| i + 1);
        let body: String = tail[..end].chars().filter(|c| !c.is_whitespace()).collect();
        assert!(
            body.contains("x=__tmp"),
            "`{name}` must assign the destructured element to the parameter `x`; got:\n{body}"
        );
    }
    check_generated_crate("fd_var_array_destructure", &rust);
    let stdout = run_generated_crate("fd_var_array_destructure", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn an_unannotated_void_fallback_compiles_and_returns_the_node_value() {
    const SOURCE: &str = r"
export function pick(a: number | undefined): number | undefined {
  const r = a ?? void 0;
  return r;
}
export function got(): number { return pick(7) ?? 0; }
export function absent(): boolean { return pick(undefined) === undefined; }
";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    let some = ::core::option::Option::Some;
    let none = ::core::option::Option::None;
    // node: `7 ?? void 0` is 7, `undefined ?? void 0` is undefined, so
    // `pick(7) ?? 0` is 7 and `pick(undefined) === undefined` is true.
    assert_eq!(fd_void_zero_coalesce::pick(&ctx, some(7.0)), some(7.0));
    assert_eq!(fd_void_zero_coalesce::pick(&ctx, none), none);
    assert_eq!(fd_void_zero_coalesce::got(&ctx), 7.0);
    assert!(fd_void_zero_coalesce::absent(&ctx));
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let norm: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        norm.contains("letr:Option<f64>"),
        "`const r = a ?? void 0` is `number | undefined`, so `r` must declare an `Option<f64>`; \
         got:\n{rust}"
    );
    assert!(
        !norm.contains("=()"),
        "a `void` fallback must lower as `None` and must never be assigned as `()` into a value \
         slot; got:\n{rust}"
    );
    check_generated_crate("fd_void_zero_coalesce", &rust);
    let stdout = run_generated_crate("fd_void_zero_coalesce", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_void_call_fallback_evaluates_its_operand_once_and_only_in_the_fallback() {
    const SOURCE: &str = r"
let calls: number = 0;
function bump(): number { calls = calls + 1; return calls; }
export function pick(a: number | undefined): number | undefined {
  const r = a ?? void bump();
  return r;
}
export function callsNow(): number { return calls; }
";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    let some = ::core::option::Option::Some;
    let none = ::core::option::Option::None;
    // node: `void bump()` still evaluates `bump()`, but only when the left
    // operand is nullish; a present operand never reaches the fallback.
    assert_eq!(fd_void_call_fallback::pick(&ctx, some(4.0)), some(4.0));
    assert_eq!(fd_void_call_fallback::callsNow(&ctx), 0.0);
    assert_eq!(fd_void_call_fallback::pick(&ctx, none), none);
    assert_eq!(fd_void_call_fallback::callsNow(&ctx), 1.0);
    assert_eq!(fd_void_call_fallback::pick(&ctx, some(4.0)), some(4.0));
    assert_eq!(fd_void_call_fallback::callsNow(&ctx), 1.0);
    assert_eq!(fd_void_call_fallback::pick(&ctx, none), none);
    assert_eq!(fd_void_call_fallback::callsNow(&ctx), 2.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let norm: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        !norm.contains("=()"),
        "a `void` fallback must lower as `None` and must never be assigned as `()` into a value \
         slot; got:\n{rust}"
    );
    check_generated_crate("fd_void_call_fallback", &rust);
    let stdout = run_generated_crate("fd_void_call_fallback", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn an_annotated_number_binding_rejects_the_literal_void_zero() {
    const SOURCE: &str = r"
export function bad(): number { let x: number = void 0; return x; }
";
    let out = compile(SOURCE);
    assert!(
        out.diagnostics.errors().any(|d| d.code.as_str() == "P0018"),
        "`let x: number = void 0` writes `undefined` into a non-optional binding, so it must be \
         refused with P0018; got {:?}",
        codes(&out)
    );
    assert!(
        out.rust_source.is_none(),
        "a refused `void 0` binding must not emit Rust; got {:?}",
        out.rust_source
    );
}

#[test]
fn assigning_a_void_value_to_a_non_optional_target_is_refused() {
    for source in [
        "export function bad(x: number): number { x = void 0; return x; }",
        "export function bad(x: number): number { x = undefined; return x; }",
        "export function bad(): number { let x: number = 1; x = void 0; return x; }",
        "export function bad(x: number): number { x ??= void 0; return x; }",
    ] {
        let out = compile(source);
        assert!(
            out.diagnostics.errors().any(|d| d.code.as_str() == "E0409"),
            "`{source}` writes `undefined` into a non-optional target, which the generated crate \
             cannot represent; it must be refused with a hard E0409; got {:?}",
            codes(&out)
        );
        assert!(
            out.rust_source.is_none(),
            "`{source}` must be refused without emitting Rust; got {:?}",
            out.rust_source
        );
    }
}

#[test]
fn assigning_a_void_value_to_an_optional_target_behaves_like_undefined() {
    const SOURCE: &str = r"
export function clear(x: number | undefined): boolean {
  let y: number | undefined = x;
  y = void 0;
  return y === undefined;
}
export function freshly(): boolean { let y: number | undefined = void 0; return y === undefined; }
export function kept(): boolean {
  let y: number | undefined = void 0;
  y = 4;
  return y === 4;
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
    let some = ::core::option::Option::Some;
    // node: `y = void 0` is `y = undefined`, so every result is true.
    assert!(fd_void_optional_target::clear(&ctx, some(3.0)));
    assert!(fd_void_optional_target::clear(
        &ctx,
        ::core::option::Option::None
    ));
    assert!(fd_void_optional_target::freshly(&ctx));
    assert!(fd_void_optional_target::kept(&ctx));
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    check_generated_crate("fd_void_optional_target", &rust);
    let stdout = run_generated_crate("fd_void_optional_target", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_var_in_a_nested_block_is_the_single_function_scoped_binding() {
    const SOURCE: &str = r"
export function nestedBlock(): number { var y = 1; { var y = 5; } return y; }
export function firstInBlock(): number { { var z = 5; } return z; }
export function twoBlocks(): number { { var w = 1; } { var w = 5; } return w; }
export function assignedInBlock(): number { var v: number = 0; { v = 5; } return v; }
export function deepBlocks(): number { var d = 1; { { var d = 5; } } return d; }
export function ifMerge(c: boolean): number { var m = 1; if (c) { var m = 5; } return m; }
export function loopSum(): number { let t = 0; for (let i = 0; i < 3; i = i + 1) { var s = i; t = t + s; } return t; }
export function readBeforeTheDeclaration(): number { var a = 1; { var b = a; var a = 5; } return a * 10 + b; }
";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    // node: a `var` in a nested block is the one function-scoped binding, so
    // `nestedBlock()`, `firstInBlock()`, `twoBlocks()`, `assignedInBlock()`,
    // `deepBlocks()` and `ifMerge(true)` are 5 while `ifMerge(false)` is 1;
    // the loop sum is 0+1+2 = 3 and a read before the later `var` sees the
    // earlier value (`readBeforeTheDeclaration()` is 5*10+1 = 51).
    assert_eq!(fd_var_function_scoped::nestedBlock(&ctx), 5.0);
    assert_eq!(fd_var_function_scoped::firstInBlock(&ctx), 5.0);
    assert_eq!(fd_var_function_scoped::twoBlocks(&ctx), 5.0);
    assert_eq!(fd_var_function_scoped::assignedInBlock(&ctx), 5.0);
    assert_eq!(fd_var_function_scoped::deepBlocks(&ctx), 5.0);
    assert_eq!(fd_var_function_scoped::ifMerge(&ctx, true), 5.0);
    assert_eq!(fd_var_function_scoped::ifMerge(&ctx, false), 1.0);
    assert_eq!(fd_var_function_scoped::loopSum(&ctx), 3.0);
    assert_eq!(fd_var_function_scoped::readBeforeTheDeclaration(&ctx), 51.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let norm: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        !norm.contains("__local"),
        "a `var` declared in a nested block must be the function-scoped binding, so no local may \
         stay unnamed (a `__local` reference means a declaration was elided); got:\n{rust}"
    );
    check_generated_crate("fd_var_function_scoped", &rust);
    let stdout = run_generated_crate("fd_var_function_scoped", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_void_fallback_over_a_null_only_optional_drops_the_null_kind() {
    const SOURCE: &str = r"
export function pick(a: number | null): number | undefined { const r = a ?? void 0; return r; }
export function coNull(a: number | null): boolean { return (a ?? void 0) === null; }
export function coUndef(a: number | null): boolean { return (a ?? void 0) === undefined; }
";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    let some = ::core::option::Option::Some;
    let none = ::core::option::Option::None;
    // node: `a ?? void 0` is `number | undefined` (never `null`), so `=== null`
    // is false, `=== undefined` is presence, and the present value survives.
    assert_eq!(fd_void_null_lhs::pick(&ctx, some(5.0)), some(5.0));
    assert_eq!(fd_void_null_lhs::pick(&ctx, none), none);
    assert!(!fd_void_null_lhs::coNull(&ctx, none));
    assert!(!fd_void_null_lhs::coNull(&ctx, some(1.0)));
    assert!(fd_void_null_lhs::coUndef(&ctx, none));
    assert!(!fd_void_null_lhs::coUndef(&ctx, some(1.0)));
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let norm: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        !norm.contains("=()"),
        "a `void` fallback must lower as `None` and must never be assigned as `()` into a value \
         slot; got:\n{rust}"
    );
    check_generated_crate("fd_void_null_lhs", &rust);
    let stdout = run_generated_crate("fd_void_null_lhs", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

fn calls_in_function(rust: &str, name: &str, needle: &str) -> usize {
    let start = rust
        .find(&format!("pub fn {name} "))
        .unwrap_or_else(|| panic!("the generated crate must declare `{name}`; got:\n{rust}"));
    let tail = &rust[start..];
    let end = tail[1..].find("pub fn ").map_or(tail.len(), |i| i + 1);
    let body: String = tail[..end].chars().filter(|c| !c.is_whitespace()).collect();
    body.matches(needle).count()
}

#[test]
fn a_void_returning_call_into_an_optional_target_and_as_a_statement_runs_with_node_values() {
    const SOURCE: &str = r"
let calls: number = 0;
function nothing(): void { calls = calls + 1; }
export function keep(y: number | undefined): number | undefined {
  y = nothing();
  return y;
}
export function asStatement(): number {
  nothing();
  nothing();
  return calls;
}
export function inVoidContext(): void {
  nothing();
}
export function callsNow(): number { return calls; }
";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    let some = ::core::option::Option::Some;
    let none = ::core::option::Option::None;
    // node: `y = nothing()` evaluates `nothing()` once and writes `undefined`, so
    // `keep` always returns undefined while the counter moves by one; the two
    // statement calls and the `void`-context call move it by one each.
    assert_eq!(fd_void_call_targets::keep(&ctx, some(3.0)), none);
    assert_eq!(
        fd_void_call_targets::callsNow(&ctx),
        1.0,
        "`y = nothing()` must evaluate the callee exactly once"
    );
    assert_eq!(fd_void_call_targets::keep(&ctx, none), none);
    assert_eq!(fd_void_call_targets::callsNow(&ctx), 2.0);
    assert_eq!(fd_void_call_targets::asStatement(&ctx), 4.0);
    assert_eq!(fd_void_call_targets::callsNow(&ctx), 4.0);
    fd_void_call_targets::inVoidContext(&ctx);
    assert_eq!(fd_void_call_targets::callsNow(&ctx), 5.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let norm: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        norm.contains("Option<f64>={let_=nothing(__ctx);::core::option::Option::None}"),
        "`y = nothing()` on an optional target must drop the call's unit result and write `None` \
         once; got:\n{rust}"
    );
    assert!(
        !norm.contains("f64=()"),
        "a `void`-returning call must never be assigned as `()` into a value slot; got:\n{rust}"
    );
    assert_eq!(
        calls_in_function(&rust, "keep", "nothing(__ctx)"),
        1,
        "the accepted optional-target write must evaluate the callee exactly once; got:\n{rust}"
    );
    assert_eq!(
        calls_in_function(&rust, "asStatement", "nothing(__ctx)"),
        2,
        "a `void`-returning call used as a statement keeps its side effect; got:\n{rust}"
    );
    assert_eq!(
        calls_in_function(&rust, "inVoidContext", "nothing(__ctx)"),
        1,
        "a `void`-returning call in a `void` context keeps its side effect; got:\n{rust}"
    );
    check_generated_crate("fd_void_call_targets", &rust);
    let stdout = run_generated_crate("fd_void_call_targets", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn assigning_a_void_result_to_a_non_optional_target_is_a_hard_e0409() {
    for source in [
        "function nothing(): void {}\n\
         export function bad(x: number): number { x = nothing(); return x; }",
        "function nothing(): void {}\n\
         export function bad(): number { let x: number = 1; x = nothing(); return x; }",
        "function nothing(): void {}\n\
         class C { v: number = 0; }\n\
         export function bad(c: C): C { c = nothing(); return c; }",
        "function undef(): undefined { return undefined; }\n\
         export function bad(x: number): number { x = undef(); return x; }",
        "function nothing(): void {}\n\
         class C { m(): void { } }\n\
         export function bad(c: C, x: number): number { x = c.m(); return x; }",
        "function nothing(): void {}\n\
         class C { static s(): void { } }\n\
         export function bad(x: number): number { x = C.s(); return x; }",
        "function nothing(): void {}\n\
         function takes(n: number): number { return n; }\n\
         export function bad(): number { return takes(nothing()); }",
    ] {
        let out = compile(source);
        assert!(
            out.diagnostics
                .errors()
                .any(|d| d.code.as_str() == "E0409" || d.code.as_str() == "E0410"),
            "`{source}` writes a `void` result into a non-optional target, which the generated \
             crate cannot represent; it must be refused instead of silently emitting `()` where \
             a value is expected. got {:?}",
            codes(&out)
        );
        assert!(
            out.rust_source.is_none(),
            "`{source}` must be refused without emitting Rust, so the refused operand cannot run; \
             got {:?}",
            out.rust_source
        );
    }
}

#[test]
fn a_void_result_into_an_optional_target_or_a_void_parameter_still_compiles() {
    const SOURCE: &str = r"
function nothing(): void {}
function takes(n: number | undefined): number { return n === undefined ? 1 : 2; }
export function optionalTarget(): boolean {
  let y: number | undefined = 3;
  y = nothing();
  return y === undefined;
}
export function asArgument(): number { return takes(nothing()); }
export function declaredVoid(): void { let v: void = nothing(); v = nothing(); }
";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(
        fd_void_call_accepted::asArgument(&ctx),
        1.0,
        "`takes(nothing())` must pass the unit result as `None`, so `takes` answers 1"
    );
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let norm: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        !norm.contains("f64=()"),
        "an optional target or an `undefined`-accepting parameter must lower the call's unit \
         result as `None`, never as `()`; got:\n{rust}"
    );
    check_generated_crate("fd_void_call_accepted", &rust);
    let stdout = run_generated_crate("fd_void_call_accepted", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_void_result_in_a_return_position_reports_the_same_hard_p0018_as_void_zero() {
    let literal = compile("export function f(): number { return void 0; }");
    let expected = match literal
        .diagnostics
        .errors()
        .find(|d| d.code.as_str() == "P0018")
    {
        Some(d) => d.message.clone(),
        None => panic!(
            "`return void 0` in a `number`-returning function must be refused with P0018; got {:?}",
            codes(&literal)
        ),
    };
    assert!(
        literal.rust_source.is_none(),
        "`return void 0` must be refused without emitting Rust; got {:?}",
        literal.rust_source
    );
    for source in [
        "function nothing(): void {}\n\
         export function f(): number { return nothing(); }",
        "function undef(): undefined { return undefined; }\n\
         export function f(): number { return undef(); }",
        "function nothing(): void {}\n\
         class C { m(): void { } }\n\
         export function f(c: C): number { return c.m(); }",
        "function nothing(): void {}\n\
         class C { static s(): void { } }\n\
         export function f(): number { return C.s(); }",
    ] {
        let out = compile(source);
        let found = out
            .diagnostics
            .errors()
            .find(|d| d.code.as_str() == "P0018")
            .unwrap_or_else(|| {
                panic!(
                    "`{source}` returns a `void` result where a `number` is required, which the \
                     generated crate cannot represent; it must be refused with the same P0018 as \
                     `return void 0`. got {:?}",
                    codes(&out)
                )
            });
        assert_eq!(
            found.message, expected,
            "`{source}` must report the same P0018 message as `return void 0`"
        );
        assert!(
            out.rust_source.is_none(),
            "`{source}` must be refused without emitting Rust; got {:?}",
            out.rust_source
        );
    }
}

#[test]
fn a_void_result_in_a_declaration_initializer_reports_the_same_hard_p0018_as_void_zero() {
    let literal = compile("export function f(): number { let x: number = void 0; return x; }");
    let expected = match literal
        .diagnostics
        .errors()
        .find(|d| d.code.as_str() == "P0018")
    {
        Some(d) => d.message.clone(),
        None => panic!(
            "`let x: number = void 0` must be refused with P0018; got {:?}",
            codes(&literal)
        ),
    };
    assert!(
        literal.rust_source.is_none(),
        "`let x: number = void 0` must be refused without emitting Rust; got {:?}",
        literal.rust_source
    );
    for source in [
        "function nothing(): void {}\n\
         export function f(): number { let x: number = nothing(); return x; }",
        "function undef(): undefined { return undefined; }\n\
         export function f(): number { let x: number = undef(); return x; }",
        "function nothing(): void {}\n\
         class C { m(): void { } }\n\
         export function f(c: C): number { let x: number = c.m(); return x; }",
        "function nothing(): void {}\n\
         class C { static s(): void { } }\n\
         export function f(): number { let x: number = C.s(); return x; }",
    ] {
        let out = compile(source);
        let found = out
            .diagnostics
            .errors()
            .find(|d| d.code.as_str() == "P0018")
            .unwrap_or_else(|| {
                panic!(
                    "`{source}` initializes a non-optional binding from a `void` result, which the \
                     generated crate cannot represent; it must be refused with the same P0018 as \
                     `let x: number = void 0`. got {:?}",
                    codes(&out)
                )
            });
        assert_eq!(
            found.message, expected,
            "`{source}` must report the same P0018 message as `let x: number = void 0`"
        );
        assert!(
            out.rust_source.is_none(),
            "`{source}` must be refused without emitting Rust; got {:?}",
            out.rust_source
        );
    }
}

#[test]
fn a_void_call_in_a_return_or_declaration_position_keeps_its_side_effect_once() {
    const SOURCE: &str = r"
let calls: number = 0;
function nothing(): void { calls = calls + 1; }
export function asReturn(): void { return nothing(); }
export function declaredOptional(): number | undefined {
  let y: number | undefined = nothing();
  return y;
}
export function declaredVoid(): void { let v: void = nothing(); v = nothing(); }
export function asStatement(): number { nothing(); return calls; }
export function callsNow(): number { return calls; }
";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    let none = ::core::option::Option::None;
    fd_void_call_positions::asReturn(&ctx);
    assert_eq!(fd_void_call_positions::callsNow(&ctx), 1.0);
    assert_eq!(fd_void_call_positions::declaredOptional(&ctx), none);
    assert_eq!(fd_void_call_positions::callsNow(&ctx), 2.0);
    fd_void_call_positions::declaredVoid(&ctx);
    assert_eq!(fd_void_call_positions::callsNow(&ctx), 4.0);
    assert_eq!(fd_void_call_positions::asStatement(&ctx), 5.0);
    assert_eq!(fd_void_call_positions::callsNow(&ctx), 5.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let norm: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        norm.contains("Option<f64>={let_=nothing(__ctx);::core::option::Option::None}"),
        "a declaration initializer from a `void` call into an optional target must keep the call \
         and write `None`; got:\n{rust}"
    );
    assert!(
        norm.contains("returnnothing(__ctx)"),
        "`return nothing()` in a `void` function must keep returning the call; got:\n{rust}"
    );
    assert!(
        !norm.contains("f64=()"),
        "a `void`-returning call must never be assigned as `()` into a value slot; got:\n{rust}"
    );
    assert_eq!(
        calls_in_function(&rust, "asReturn", "nothing(__ctx)"),
        1,
        "`return nothing()` in a `void` function must evaluate the callee exactly once; got:\n{rust}"
    );
    assert_eq!(
        calls_in_function(&rust, "declaredOptional", "nothing(__ctx)"),
        1,
        "a `void` call initializing an optional binding must evaluate the callee exactly once; \
         got:\n{rust}"
    );
    assert_eq!(
        calls_in_function(&rust, "declaredVoid", "nothing(__ctx)"),
        2,
        "each `void`-context write must evaluate the callee exactly once; got:\n{rust}"
    );
    assert_eq!(
        calls_in_function(&rust, "asStatement", "nothing(__ctx)"),
        1,
        "a `void`-returning call used as a statement keeps its side effect; got:\n{rust}"
    );
    check_generated_crate("fd_void_call_positions", &rust);
    let stdout = run_generated_crate("fd_void_call_positions", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn an_unannotated_void_local_accepts_a_repeated_void_call_assignment() {
    const SOURCE: &str = r"
let calls: number = 0;
function nothing(): void { calls = calls + 1; }
export function f(): void { let x = nothing(); x = nothing(); }
export function callsNow(): number { return calls; }
";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    fd_unannotated_void_call::f(&ctx);
    assert_eq!(fd_unannotated_void_call::callsNow(&ctx), 2.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    check_generated_crate("fd_unannotated_void_call", &rust);
    let stdout = run_generated_crate("fd_unannotated_void_call", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn an_unannotated_void_local_accepts_an_undefined_assignment() {
    const SOURCE: &str = r"
let calls: number = 0;
function nothing(): void { calls = calls + 1; }
export function f(): void { let x = nothing(); x = undefined; }
export function callsNow(): number { return calls; }
";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    fd_unannotated_void_undefined::f(&ctx);
    assert_eq!(fd_unannotated_void_undefined::callsNow(&ctx), 1.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    check_generated_crate("fd_unannotated_void_undefined", &rust);
    let stdout = run_generated_crate("fd_unannotated_void_undefined", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn an_unannotated_void_local_accepts_a_void_zero_assignment() {
    const SOURCE: &str = r"
let calls: number = 0;
function nothing(): void { calls = calls + 1; }
export function f(): void { let x = nothing(); x = void 0; }
export function callsNow(): number { return calls; }
";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    fd_unannotated_void_zero::f(&ctx);
    assert_eq!(fd_unannotated_void_zero::callsNow(&ctx), 1.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    check_generated_crate("fd_unannotated_void_zero", &rust);
    let stdout = run_generated_crate("fd_unannotated_void_zero", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn an_unannotated_void_local_accepts_a_repeated_void_method_assignment() {
    const SOURCE: &str = r"
let calls: number = 0;
class C { m(): void { calls = calls + 1; } }
export function f(c: C): void { let x = c.m(); x = c.m(); }
export function callsNow(): number { return calls; }
";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    let c = fd_unannotated_void_method::C {};
    fd_unannotated_void_method::f(&ctx, c);
    assert_eq!(fd_unannotated_void_method::callsNow(&ctx), 2.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    check_generated_crate("fd_unannotated_void_method", &rust);
    let stdout = run_generated_crate("fd_unannotated_void_method", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn an_unannotated_void_global_accepts_a_repeated_void_call_assignment() {
    const SOURCE: &str = r"
let hits: number = 0;
function nothing(): void {}
let g = nothing();
export function f(): number { g = nothing(); hits = hits + 1; return hits; }
";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(fd_unannotated_void_global::f(&ctx), 1.0);
    assert_eq!(fd_unannotated_void_global::f(&ctx), 2.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    check_generated_crate("fd_unannotated_void_global", &rust);
    let stdout = run_generated_crate("fd_unannotated_void_global", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn known_non_optional_field_array_and_global_targets_still_refuse_a_void_result() {
    for (source, context) in [
        (
            "function nothing(): void {}\n\
             class C { n: number = 0; }\n\
             export function bad(c: C): number { c.n = nothing(); return c.n; }",
            "a known non-optional field",
        ),
        (
            "function nothing(): void {}\n\
             export function bad(a: number[]): number { a[0] = nothing(); return a[0]; }",
            "a known non-optional array element",
        ),
        (
            "function nothing(): void {}\n\
             let g: number = 0;\n\
             export function bad(): number { g = nothing(); return g; }",
            "a known non-optional global",
        ),
    ] {
        let out = compile(source);
        assert!(
            out.diagnostics.errors().any(|d| d.code.as_str() == "E0409"),
            "{context} must keep the hard E0409 for a `void` write; got {:?}",
            codes(&out)
        );
        assert!(
            out.rust_source.is_none(),
            "{context} must be refused without emitting Rust; got {:?}",
            out.rust_source
        );
    }
}

#[test]
fn a_fresh_class_value_field_selects_the_receiver_class_for_a_static_call() {
    const SOURCE: &str = r"
class Holder { C = class { static run(): number { return 1; }; }; }
export function got(): number {
  const h = new Holder();
  h.C = 0;
  h.C ||= class { static run(): number { return 2; } };
  return h.C.run();
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
    assert_eq!(
        fd_fresh_receiver_static_call::got(&ctx),
        2.0,
        "the field is `0`, so `||=` stores the right-hand class and `h.C.run()` is its `run`"
    );
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    check_generated_crate("fd_fresh_receiver_static_call", &rust);
    let stdout = run_generated_crate("fd_fresh_receiver_static_call", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn an_uninitialized_or_dead_var_keeps_the_parameter_inference() {
    const SOURCE: &str = r"
function takes(a: number): number { return a; }
export function uninitializedLoopHead(): number {
  function f(x) { for (var x; false;) {} return takes(x); }
  return f(7);
}
export function deadInitializer(): number {
  function f(x) { if (false) { var x = 1; } return takes(x); }
  return f(7);
}
export function deadForIn(): number {
  function f(x) { if (false) { for (var x in [1, 2]) {} } return takes(x); }
  return f(7);
}
export function deadForOf(): number {
  function f(x) { if (false) { for (var x of [1]) {} } return takes(x); }
  return f(7);
}
export function deadLoopHead(): number {
  function f(x) { if (false) { for (var x = 1; false;) {} } return takes(x); }
  return f(7);
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
    assert_eq!(
        fd_hoist_shadow_live::uninitializedLoopHead(&ctx),
        7.0,
        "`for (var x; false;)` never writes `x`, so the parameter survives"
    );
    assert_eq!(
        fd_hoist_shadow_live::deadInitializer(&ctx),
        7.0,
        "the initializer of a `var` behind `if (false)` never runs"
    );
    assert_eq!(
        fd_hoist_shadow_live::deadForIn(&ctx),
        7.0,
        "a `for-in` head behind `if (false)` never writes `x`"
    );
    assert_eq!(
        fd_hoist_shadow_live::deadForOf(&ctx),
        7.0,
        "a `for-of` head behind `if (false)` never writes `x`"
    );
    assert_eq!(
        fd_hoist_shadow_live::deadLoopHead(&ctx),
        7.0,
        "an initialized `for` head behind `if (false)` never writes `x`"
    );
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    check_generated_crate("fd_hoist_shadow_live", &rust);
    let stdout = run_generated_crate("fd_hoist_shadow_live", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_reachable_initialized_var_still_shadows_the_parameter() {
    for (context, source) in [
        (
            "an initialized `for` head",
            "function takes(a: number): number { return a; }\n\
             export function got(): number { \
                 function f(x) { for (var x = 1; false;) {} return takes(x); } \
                 return f(7); \
             }",
        ),
        (
            "a reachable `for-in` head",
            "function takes(a: number): number { return a; }\n\
             export function got(): number { \
                 function f(x) { for (var x in { a: 1 }) {} return takes(x); } \
                 return f(7); \
             }",
        ),
    ] {
        let out = compile(source);
        assert!(
            out.diagnostics.errors().any(|d| d.code.as_str() == "E0405"),
            "{context} writes the parameter, so the loop-head binding must keep shadowing the \
             parameter; got {:?}",
            codes(&out)
        );
        assert!(
            out.rust_source.is_none(),
            "{context} must be refused without emitting Rust; got {:?}",
            out.rust_source
        );
    }
}

#[test]
fn a_repair_of_field_initializer_captures_keeps_each_binding_type() {
    const SOURCE: &str = r#"
export function got(): number {
  class C { f: number = a; g: string = b; }
  const a = 1;
  const b = "xyz";
  const make = (): number => new C().g.length * 10 + new C().f;
  return make();
}
"#;
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(
        fd_capture_binding_types::got(&ctx),
        31.0,
        "the `string` capture is still a string (`\"xyz\".length` is 3) and the `number` capture is 1"
    );
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    check_generated_crate("fd_capture_binding_types", &rust);
    let stdout = run_generated_crate("fd_capture_binding_types", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_destructuring_assignment_used_as_an_expression_is_refused() {
    const SOURCE: &str = r"
export function got(): number {
  let x: number = 1;
  const arr: number[] = [2];
  const r = ([x] = arr);
  return x + r.length;
}
";
    let out = compile(SOURCE);
    assert!(
        out.diagnostics.errors().any(|d| {
            d.code.as_str() == "E0500"
                && d.message
                    .contains("destructuring assignment used as an expression")
        }),
        "an expression-position destructuring assignment would otherwise be skipped while the \
         target keeps its previous value; it must be a hard E0500 error, got {:?}",
        codes(&out)
    );
    assert!(
        out.rust_source.is_none(),
        "a refused expression-position destructuring assignment must not emit Rust; got {:?}",
        out.rust_source
    );
}
