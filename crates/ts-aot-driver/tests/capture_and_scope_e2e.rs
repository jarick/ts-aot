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

#[test]
fn a_closure_local_class_shadows_an_inherited_class_of_the_same_name() {
    const SOURCE: &str = r"
export function got0(): number {
  class A { m(): number { return 1; } }
  const f = (): number => { class A { m(): number { return 2; } } class B extends A { } return new B().m(); };
  return f();
}
";
    check("local_class_shadows_inherited", SOURCE, &[2.0]);
}

#[test]
fn a_method_local_class_shadows_an_inherited_class_of_the_same_name() {
    const SOURCE: &str = r"
class Outer {
  m(): number {
    class A { m(): number { return 1; } }
    const f = (): number => { class A { m(): number { return 2; } } class B extends A { } return new B().m(); };
    return f();
  }
}
export function got0(): number { return new Outer().m(); }
";
    check("method_local_class_shadows_inherited", SOURCE, &[2.0]);
}

#[test]
fn a_closure_local_class_shadows_an_inherited_shadowed_marker() {
    const SOURCE: &str = r"
export function got0(): number {
  class A { m(): number { return 1; } }
  {
    const A = 5;
    const f = (): number => { class A { m(): number { return 2; } } class B extends A { } return new B().m(); };
    return f();
  }
}
";
    check("local_class_shadows_inherited_shadowed", SOURCE, &[2.0]);
}

#[test]
fn an_intervening_outer_value_binding_still_shadows_an_inherited_class() {
    const SOURCE: &str = r"
export function got0(): number {
  class A { m(): number { return 1; } }
  let result = 0;
  { const A = 5; const f = (): number => { class B extends A { } return 1; }; result = f() + A; }
  return result;
}
";
    let out = compile(SOURCE);
    assert!(
        codes(&out).iter().any(|code| code == "E0300"),
        "a value binding between a closure and an inherited class must still shadow it, got {:?}",
        codes(&out)
    );
    assert!(
        out.rust_source.is_none(),
        "no Rust may be produced for a shadowed `extends` parent; got:\n{:?}",
        out.rust_source
    );
}

#[test]
fn a_local_class_capture_of_a_later_annotated_local_gets_its_type() {
    const SOURCE: &str = r"
export function got0(): number {
  class C { m(): number { return x; } }
  let x: number = 4;
  const c = new C();
  return c.m() + x;
}
";
    let rust = rust_of(SOURCE);
    let norm: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        norm.contains("__capture_x") && norm.contains("RefCell<f64>"),
        "a capture of a later-declared local must take that binding's type, not the unit type of \
         the unresolved predeclaration; got:\n{rust}"
    );
    check("late_capture_annotated", SOURCE, &[8.0]);
}

#[test]
fn a_local_class_capture_of_a_later_unannotated_local_gets_its_type() {
    const SOURCE: &str = r"
export function got0(): number {
  class C { m(): number { return x; } }
  const x = 4;
  const c = new C();
  return c.m() + x;
}
";
    let rust = rust_of(SOURCE);
    let norm: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        norm.contains("RefCell<i64>"),
        "an unannotated later `const` must type the capture field from its inferred type; got:\n\
         {rust}"
    );
    check("late_capture_inferred", SOURCE, &[8.0]);
}

#[test]
fn a_local_class_field_initialized_from_a_binding_takes_its_type() {
    const SOURCE: &str = r"
const g: number = 3;
function fromParam(n: number): number { class C { v = n; } return new C().v; }
function fromLocal(): number { const m: number = 7; class C { v = m; } return new C().v; }
function fromGlobal(): number { class C { v = g; } return new C().v; }
function fromLiteral(): number { class C { v = 9; } return new C().v; }
export function got0(): number { return fromParam(41); }
export function got1(): number { return fromLocal(); }
export function got2(): number { return fromGlobal(); }
export function got3(): number { return fromLiteral(); }
";
    let rust = rust_of(SOURCE);
    let norm: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        !norm.contains("RefCell<()>"),
        "a field initialized from an enclosing binding must not stay the unit type; got:\n{rust}"
    );
    check("local_class_field_binding", SOURCE, &[41.0, 7.0, 3.0, 9.0]);
}

#[test]
fn a_write_to_a_local_class_static_field_from_its_own_static_method_is_refused() {
    const SOURCE: &str = r"
function f(): number {
  class C {
    static #n: number = 0;
    static read(): number { return this.#n; }
    static inc(): number { return ++this.#n; }
  }
  return C.read() + C.inc();
}
export function got0(): number { return f(); }
";
    let out = compile(SOURCE);
    assert!(
        codes(&out).iter().any(|code| code == "E0419"),
        "a static-field write inside the body of a class declared in a function mutates the one \
         module-level global shared by every evaluation, so it must be refused; got {:?}",
        codes(&out)
    );
    assert!(
        out.rust_source.is_none(),
        "no Rust may be produced for a shared local-class static write; got:\n{:?}",
        out.rust_source
    );
}

#[test]
fn a_write_to_a_local_class_static_field_from_its_own_instance_method_is_refused() {
    const SOURCE: &str = r"
function f(): number {
  class C {
    static n: number = 0;
    m(): number { return ++C.n; }
  }
  return new C().m();
}
export function got0(): number { return f(); }
";
    let out = compile(SOURCE);
    assert!(
        codes(&out).iter().any(|code| code == "E0419"),
        "an instance method of a local class writing the class's static field still hits the \
         shared module global, so it must be refused; got {:?}",
        codes(&out)
    );
    assert!(
        out.rust_source.is_none(),
        "no Rust may be produced for a shared local-class static write; got:\n{:?}",
        out.rust_source
    );
}

#[test]
fn assignments_to_a_local_class_static_field_inside_its_method_are_refused() {
    const SOURCE: &str = r"
function f(v: number): number {
  class C {
    static n: number = 0;
    static set(other: number): number { C.n = other; return C.n; }
  }
  return C.set(v);
}
export function got0(): number { return f(4); }
";
    let out = compile(SOURCE);
    assert!(
        codes(&out).iter().any(|code| code == "E0419"),
        "an assignment to a local-class static field inside the class body must be refused; got \
         {:?}",
        codes(&out)
    );
    assert!(
        out.rust_source.is_none(),
        "no Rust may be produced for a shared local-class static write; got:\n{:?}",
        out.rust_source
    );
}

#[test]
fn reads_of_a_local_class_static_field_inside_its_method_still_compile() {
    const SOURCE: &str = r"
function f(): number {
  class C {
    static #n: number = 3;
    static read(): number { return this.#n; }
  }
  return C.read();
}
export function got0(): number { return f(); }
";
    check("local_static_read_only", SOURCE, &[3.0]);
}

#[test]
fn writes_to_a_module_class_static_field_inside_its_methods_still_compile() {
    const SOURCE: &str = r"
class C {
  static #n: number = 0;
  static n: number = 0;
  static inc(): number { return ++this.#n; }
  static bump(): number { return ++C.#n; }
  static set(v: number): number { C.n = v; return C.n; }
}
export function got0(): number { return C.inc(); }
export function got1(): number { return C.bump(); }
export function got2(): number { return C.set(7); }
export function got3(): number { return C.n; }
";
    check("module_static_writes", SOURCE, &[1.0, 2.0, 7.0, 7.0]);
}

#[test]
fn an_outer_value_binding_shadows_a_module_class_for_a_closure_extends() {
    const SOURCE: &str = r"
class A { m(): number { return 1; } }
function f(): number {
  let A = 5;
  const g = (): number => { class D extends A { } return 1; };
  return g() + A;
}
export function got0(): number { return f(); }
";
    let out = compile(SOURCE);
    assert!(
        out.diagnostics
            .errors()
            .any(|d| d.code.as_str() == "E0300" && d.message.contains("extends")),
        "a closure extending a name whose nearest binding is an ordinary value must be reported, \
         not silently resolved to the module class; got {:?}",
        out.diagnostics
    );
    assert!(
        out.rust_source.is_none(),
        "no Rust may be produced for a shadowed `extends` parent; got:\n{:?}",
        out.rust_source
    );
}

#[test]
fn a_value_binding_initialized_from_a_parameter_shadows_a_module_class_for_a_closure_extends() {
    const SOURCE: &str = r"
class A { m(): number { return 1; } }
function f(n: number): number {
  let A = n;
  const g = (): number => { class D extends A { } return 1; };
  return g() + A;
}
export function got0(): number { return f(2); }
";
    let out = compile(SOURCE);
    assert!(
        out.diagnostics
            .errors()
            .any(|d| d.code.as_str() == "E0300" && d.message.contains("extends")),
        "a value binding that shadows a module class must stay shadowed inside a nested closure; \
         got {:?}",
        out.diagnostics
    );
    assert!(
        out.rust_source.is_none(),
        "no Rust may be produced for a shadowed `extends` parent; got:\n{:?}",
        out.rust_source
    );
}

#[test]
fn a_binding_declared_after_a_closure_still_shadows_a_module_class_for_its_extends() {
    const WITH_VAR: &str = r"
class A { m(): number { return 1; } }
function f(): number {
  const g = (): number => { class D extends A { } return 1; };
  var A = 5;
  return g() + A;
}
export function got0(): number { return f(); }
";
    const WITH_LET: &str = r"
class A { m(): number { return 1; } }
function f(): number {
  const g = (): number => { class D extends A { } return 1; };
  let A = 5;
  return g() + A;
}
export function got0(): number { return f(); }
";
    for (kind, source) in [("var", WITH_VAR), ("let", WITH_LET)] {
        let out = compile(source);
        assert_eq!(
            codes(&out),
            vec!["E0300".to_owned()],
            "a `{kind}` binding written after the closure is predeclared for the whole body, so \
             it already shadows the module class `A` when the closure is walked; got {:?}",
            out.diagnostics
        );
        assert!(
            out.diagnostics.errors().any(|d| d.message.contains(
                "the class extends `A`, but the nearest `A` in scope is not a class value"
            )),
            "the `extends A` inside the closure must resolve to the shadowing `{kind}` binding, \
             not to the module class; got {:?}",
            out.diagnostics
        );
        assert!(
            out.rust_source.is_none(),
            "no Rust may be produced while the shadowing `{kind}` binding comes later; got:\n{:?}",
            out.rust_source
        );
    }
}

#[test]
fn a_closure_extends_a_module_class_when_nothing_shadows_it() {
    const SOURCE: &str = r"
class A { m(): number { return 7; } }
export function got0(): number {
  const g = (): number => { class D extends A { } return new D().m(); };
  return g();
}
";
    check("closure_extends_module_class", SOURCE, &[7.0]);
}

#[test]
fn a_closure_extends_a_module_class_past_an_unrelated_outer_binding() {
    const SOURCE: &str = r"
class A { m(): number { return 1; } }
function f(): number {
  let x = 5;
  const g = (): number => { class D extends A { } return new D().m() + x; };
  return g();
}
export function got0(): number { return f(); }
";
    check("closure_extends_past_unrelated", SOURCE, &[6.0]);
}

#[test]
fn a_closure_class_declaration_replaces_an_inherited_shadow_marker() {
    const SOURCE: &str = r"
class A { m(): number { return 1; } }
function f(): number {
  let A = 5;
  const g = (): number => { class A { m(): number { return 2; } } class B extends A { } return new B().m(); };
  return g();
}
export function got0(): number { return f(); }
";
    check("closure_class_replaces_marker", SOURCE, &[2.0]);
}

#[test]
fn a_value_binding_in_a_closed_block_does_not_shadow_a_module_class() {
    const SOURCE: &str = r"
class A { m(): number { return 1; } }
function f(): number {
  { const A = 5; const unused: number = A; }
  const g = (): number => { class D extends A { } return new D().m(); };
  return g();
}
export function got0(): number { return f(); }
";
    check("closed_block_no_shadow", SOURCE, &[1.0]);
}

#[test]
fn a_callback_that_reassigns_the_receiver_through_a_helper_is_refused() {
    const SOURCE: &str = r"
class Holder { n: number = 0; run(cb: () => void): number { this.n = this.n + 1; callIt(cb); return this.n; } }
function callIt(cb: () => void): void { cb(); }
export function got0(): number {
  let h = new Holder();
  h.run((): void => { h = new Holder(); });
  return h.n;
}
";
    let out = compile(SOURCE);
    assert!(
        out.diagnostics
            .errors()
            .any(|d| d.code.as_str() == "E0419" && d.message.contains("may reassign")),
        "a callback forwarded through a helper still reassigns the receiver before the call, so it \
         must be reported; got {:?}",
        out.diagnostics
    );
    assert!(
        out.rust_source.is_none(),
        "no Rust may be produced for a receiver reassigned through a forwarded callback; got:\n{:?}",
        out.rust_source
    );
}

#[test]
fn a_callback_that_reassigns_the_receiver_through_two_helpers_is_refused() {
    const SOURCE: &str = r"
class Holder { n: number = 0; run(cb: () => void): number { this.n = this.n + 1; callIt(cb); return this.n; } }
function callIt(cb: () => void): void { callIt2(cb); }
function callIt2(cb: () => void): void { cb(); }
export function got0(): number {
  let h = new Holder();
  h.run((): void => { h = new Holder(); });
  return h.n;
}
";
    let out = compile(SOURCE);
    assert!(
        out.diagnostics
            .errors()
            .any(|d| d.code.as_str() == "E0419" && d.message.contains("may reassign")),
        "the callback's provenance must survive two forwarding helpers, so the reassignment is \
         still reported; got {:?}",
        out.diagnostics
    );
    assert!(
        out.rust_source.is_none(),
        "no Rust may be produced for a receiver reassigned through forwarded callbacks; got:\n{:?}",
        out.rust_source
    );
}

#[test]
fn a_forwarded_callback_keeps_its_provenance_across_reordered_arguments() {
    const SOURCE: &str = r"
class Holder { n: number = 0; run(cb: () => void): number { this.n = this.n + 1; callIt(cb, 1); return this.n; } }
function callIt(cb: () => void, extra: number): void { callIt2(extra, cb); }
function callIt2(extra: number, cb: () => void): void { cb(); }
export function got0(): number {
  let h = new Holder();
  h.run((): void => { h = new Holder(); });
  return h.n;
}
";
    let out = compile(SOURCE);
    assert!(
        out.diagnostics
            .errors()
            .any(|d| d.code.as_str() == "E0419" && d.message.contains("may reassign")),
        "provenance belongs to the callback argument, not to the call position, so a reordered \
         forwarding chain must still report the reassignment; got {:?}",
        out.diagnostics
    );
    assert!(
        out.rust_source.is_none(),
        "no Rust may be produced for a receiver reassigned through forwarded callbacks; got:\n{:?}",
        out.rust_source
    );
}

#[test]
fn a_forwarded_callback_that_mutates_the_receiver_still_compiles() {
    const SOURCE: &str = r"
class Holder {
  n: number = 0;
  run(cb: () => void): number { this.n = this.n + 1; callIt(cb); return this.n; }
  addOne(): void { this.n = this.n + 1; }
}
function callIt(cb: () => void): void { cb(); }
export function got0(): number {
  let h = new Holder();
  h.run((): void => { h.addOne(); });
  return h.n;
}
";
    check("forwarded_callback_mutates_receiver", SOURCE, &[2.0]);
}

#[test]
fn a_forwarded_callback_that_only_reads_the_receiver_still_compiles() {
    const SOURCE: &str = r"
let observed: number = -1;
class Holder { n: number = 0; run(cb: () => void): number { this.n = this.n + 1; callIt(cb); return this.n; } }
function callIt(cb: () => void): void { callIt2(cb); }
function callIt2(cb: () => void): void { cb(); }
export function got0(): number {
  let h = new Holder();
  h.run((): void => { observed = h.n; });
  return h.n;
}
export function got1(): number { return observed; }
";
    check("forwarded_callback_reads_receiver", SOURCE, &[1.0, 1.0]);
}

#[test]
fn a_function_scoped_var_in_a_nested_body_shadows_a_module_class_for_a_closure_extends() {
    for (label, body) in [
        (
            "an if body",
            "if (true) { var A = class { m(): number { return 2; } }; }",
        ),
        (
            "a loop body",
            "while (false) { var A = class { m(): number { return 2; } }; }",
        ),
        (
            "an unreachable if body",
            "if (false) { var A = class { m(): number { return 2; } }; }",
        ),
    ] {
        let source = format!(
            "class A {{ m(): number {{ return 1; }} }}\n\
             function f(): number {{\n  {body}\n  \
             const g = (): number => {{ class D extends A {{ }} return new D().m(); }};\n  \
             return g();\n}}\n\
             export function got0(): number {{ return f(); }}\n"
        );
        let out = compile(&source);
        assert!(
            out.diagnostics
                .errors()
                .any(|d| d.code.as_str() == "E0300" && d.message.contains("extends")),
            "a `var` first declared in {label} is function-scoped, so the closure's `extends A` \
             must resolve to that hoisted binding rather than the module class; got {:?}",
            out.diagnostics
        );
        assert!(
            out.rust_source.is_none(),
            "no Rust may be produced while the hoisted `var A` shadows the module class; got:\n{:?}",
            out.rust_source
        );
    }
}

#[test]
fn a_block_scoped_binding_in_a_nested_body_does_not_shadow_a_module_class() {
    for (label, package, statement) in [
        (
            "a `let` in an if body",
            "nested_let_no_shadow",
            "if (true) { let A = 5; }",
        ),
        (
            "a `const` in an if body",
            "nested_const_no_shadow",
            "if (true) { const A = 5; }",
        ),
        (
            "a `for` head `let`",
            "nested_for_head_no_shadow",
            "for (let A = 0; A < 1; A++) { }",
        ),
    ] {
        let source = format!(
            "class A {{ m(): number {{ return 1; }} }}\n\
             function f(): number {{\n  {statement}\n  \
             const g = (): number => {{ class D extends A {{ }} return new D().m(); }};\n  \
             return g();\n}}\n\
             export function got0(): number {{ return f(); }}\n"
        );
        let out = compile(&source);
        assert!(
            !out.has_errors(),
            "{label} is block-scoped, so it must not shadow the module class outside its block; \
             got {:?}",
            out.diagnostics
        );
        check(package, &source, &[1.0]);
    }
}

#[test]
fn a_parameter_and_a_catch_parameter_shadow_a_module_class_for_a_closure_extends() {
    const PARAMETER: &str = r"
class A { m(): number { return 1; } }
function f(A: number): number {
  const g = (): number => { class D extends A { } return new D().m(); };
  return g();
}
export function got0(): number { return f(0); }
";
    const CATCH_PARAMETER: &str = r"
class A { m(): number { return 1; } }
function f(): number {
  let r = 0;
  try { throw 1; } catch (A) {
    const g = (): number => { class D extends A { } return new D().m(); };
    r = g();
  }
  return r;
}
export function got0(): number { return f(); }
";
    for (label, source) in [
        ("a parameter", PARAMETER),
        ("a catch parameter", CATCH_PARAMETER),
    ] {
        let out = compile(source);
        assert!(
            out.diagnostics
                .errors()
                .any(|d| d.code.as_str() == "E0300" && d.message.contains("extends")),
            "{label} named `A` must shadow the module class for a closure's `extends A`; got {:?}",
            out.diagnostics
        );
        assert!(
            out.rust_source.is_none(),
            "no Rust may be produced for a shadowed `extends` parent; got:\n{:?}",
            out.rust_source
        );
    }
}
