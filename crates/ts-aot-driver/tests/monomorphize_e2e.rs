mod common;

use common::normalize_rust;
use ts_aot_driver::{CompileOptions, Driver, EmitStage};
use ts_aot_test_support::{check_generated_crate, run_generated_crate};

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

#[test]
fn generic_identity_infers_the_return_type_of_a_call_argument() {
    const SOURCE: &str = "function id<T>(x: T): T { return x; } \
         function n(): number { return 5; } \
         export function got(): number { return id(n()); }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(mono_call_argument::got(&ctx), 5.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let norm = normalize_rust(&rust);
    assert!(
        norm.contains("fn id_mono_") && norm.contains("x : f64"),
        "the call argument's declared return type must specialize `T` to number; got:\n{rust}"
    );
    check_generated_crate("mono_call_argument", &rust);
    let stdout = run_generated_crate("mono_call_argument", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_method_call_on_a_call_receiver_resolves_by_the_declared_return_type() {
    const SOURCE: &str = "class C { m(x: number): number { return x + 1; } } \
         function make(): C { return new C(); } \
         export function got(): number { return make().m(4); }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(mono_call_receiver::got(&ctx), 5.0);
    println!("ok");
}
"#;
    let out = compile_module(SOURCE);
    assert!(!out.has_errors(), "got {:?}", out.diagnostics);
    assert!(
        !out.diagnostics.iter().any(|d| d.code.as_str() == "P0013"),
        "the receiver type must resolve instead of being skipped; got {:?}",
        out.diagnostics
    );
    let rust = out
        .rust_source
        .expect("emit-rust must populate rust_source");
    let norm = normalize_rust(&rust);
    assert!(
        norm.contains("C::C__m (make (__ctx), __ctx, 4.0)"),
        "the method must be dispatched through the receiver's declared class; got:\n{rust}"
    );
    assert!(
        !norm.contains("__field0"),
        "no placeholder field access may be emitted; got:\n{rust}"
    );
    check_generated_crate("mono_call_receiver", &rust);
    let stdout = run_generated_crate("mono_call_receiver", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_specialized_call_pads_its_omitted_optional_parameter() {
    const SOURCE: &str = "function f<T>(x: T, y?: number): number { \
             if (y === undefined) { return 1; } return 2; \
         } \
         export function got(): number { return f(7); }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(mono_optional_padding::got(&ctx), 1.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let norm = normalize_rust(&rust);
    assert!(
        norm.contains("f_mono_") && norm.contains("Option::None"),
        "a specialized call must pad the omitted optional argument; got:\n{rust}"
    );
    check_generated_crate("mono_optional_padding", &rust);
    let stdout = run_generated_crate("mono_optional_padding", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

fn binding_style_rust(source: &str, package: &str, main: &str, binding: &str) {
    let rust = rust_of(source);
    assert!(
        !rust.contains("__generic"),
        "{package}: no stale generic type may reach the backend; got:\n{rust}"
    );
    let norm = normalize_rust(&rust);
    assert!(
        norm.contains(binding),
        "{package}: expected `{binding}` in the emitted Rust; got:\n{rust}"
    );
    check_generated_crate(package, &rust);
    let stdout = run_generated_crate(package, &rust, main);
    assert!(stdout.contains("ok"), "{package}: got stdout:\n{stdout}");
}

#[test]
fn a_nullish_of_two_generic_method_calls_keeps_the_first_instantiation() {
    const SOURCE: &str = r"class C { m<T>(x: T): T { return x; } }
export function got(): number { const c = new C(); const y = c.m(5) ?? c.m(null); return y * 2; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(nullish_two_calls::got(&ctx), 10.0);
    println!("ok");
}
"#;
    binding_style_rust(SOURCE, "nullish_two_calls", MAIN, "let y : i64");
}

#[test]
fn a_nullish_with_a_literal_fallback_repairs_the_binding() {
    const SOURCE: &str = r"class C { m<T>(x: T): T { return x; } }
export function got(): number { const c = new C(); const y = c.m(5) ?? 0; return y * 2; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(nullish_literal_fallback::got(&ctx), 10.0);
    println!("ok");
}
"#;
    binding_style_rust(SOURCE, "nullish_literal_fallback", MAIN, "let y : i64");
}

#[test]
fn a_generic_method_call_assigned_to_a_const_is_repaired() {
    const SOURCE: &str = r"class C { m<T>(x: T): T { return x; } }
export function got(): number { const c = new C(); const y = c.m(5); return y * 2; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(const_binding::got(&ctx), 10.0);
    println!("ok");
}
"#;
    binding_style_rust(SOURCE, "const_binding", MAIN, "let y : i64");
}

#[test]
fn a_generic_method_call_inside_a_closure_is_repaired() {
    const SOURCE: &str = r"class C { m<T>(x: T): T { return x; } }
export function got(): number {
    const c = new C();
    const f = (): number => { const y = c.m(5); return y * 2; };
    return f();
}";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(closure_binding::got(&ctx), 10.0);
    println!("ok");
}
"#;
    binding_style_rust(SOURCE, "closure_binding", MAIN, "let y : i64");
}

#[test]
fn a_generic_method_with_an_optional_parameter_keeps_both_instantiations() {
    const SOURCE: &str = r"class C { m<T>(x: T, y?: number): number { if (y === undefined) { return 10; } return 15; } }
export function a(): number { const c = new C(); const y = c.m(1); return y; }
export function b(): number { const c = new C(); const y = c.m(1, 2); return y; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(optional_param_method::a(&ctx), 10.0);
    assert_eq!(optional_param_method::b(&ctx), 15.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    assert!(!rust.contains("__generic"), "got:\n{rust}");
    let norm = normalize_rust(&rust);
    assert!(
        norm.contains("Option::None") && norm.contains("Option::Some"),
        "the omitted and the supplied optional argument must both be padded; got:\n{rust}"
    );
    check_generated_crate("optional_param_method", &rust);
    let stdout = run_generated_crate("optional_param_method", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_static_and_an_instance_generic_method_with_the_same_name_are_distinct() {
    const SOURCE: &str = r"class P { static m<T>(x: T): T { return x; } base: i64 = 7; m<T>(x: T): i64 { return this.base; } }
export function got(): number { const p = new P(); const x = P.m(2); const y = p.m(4); return x * 10 + y; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(
        shared_method_name::got(&ctx),
        27.0,
        "the static call returns its argument, while the instance call reads the receiver field, \
         so a call routed to the wrong specialization cannot produce 27"
    );
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    assert!(!rust.contains("__generic"), "got:\n{rust}");
    let norm = normalize_rust(&rust);
    assert!(
        norm.contains("let x : i64") && norm.contains("let y : i64"),
        "both the static and the instance call must be repaired; got:\n{rust}"
    );
    check_generated_crate("shared_method_name", &rust);
    let stdout = run_generated_crate("shared_method_name", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn two_classes_with_a_same_named_generic_method_are_distinct() {
    const SOURCE: &str = r"class A { m<T>(x: T): T { return x; } }
class B { m<T>(x: T): T { return x; } }
export function got(): number { const a = new A(); const b = new B(); const x = a.m(5); const y = b.m(6); return x + y; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(two_class_methods::got(&ctx), 11.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    assert!(!rust.contains("__generic"), "got:\n{rust}");
    let norm = normalize_rust(&rust);
    assert!(
        norm.contains("A::A__m_mono") && norm.contains("B::B__m_mono"),
        "each class must resolve its own specialization; got:\n{rust}"
    );
    check_generated_crate("two_class_methods", &rust);
    let stdout = run_generated_crate("two_class_methods", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn an_inherited_generic_method_is_specialized_for_the_subclass() {
    const SOURCE: &str = r"class A { m<T>(x: T): T { return x; } }
class B extends A { }
export function got(): number { const b = new B(); const y = b.m(5); return y * 2; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(inherited_method::got(&ctx), 10.0);
    println!("ok");
}
"#;
    binding_style_rust(SOURCE, "inherited_method", MAIN, "let y : i64");
}

#[test]
fn three_generic_instantiations_in_one_nullish_expression_keep_the_first() {
    const SOURCE: &str = r#"class C { m<T>(x: T): T { return x; } }
export function got(): number { const c = new C(); const y = c.m(5) ?? c.m(true) ?? c.m("s"); return y * 2; }"#;
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(three_instantiations::got(&ctx), 10.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    assert!(!rust.contains("__generic"), "got:\n{rust}");
    assert!(
        rust.contains("bool") && rust.contains("JsString"),
        "all three instantiations must be specialized; got:\n{rust}"
    );
    let norm = normalize_rust(&rust);
    assert!(norm.contains("let y : i64"), "got:\n{rust}");
    check_generated_crate("three_instantiations", &rust);
    let stdout = run_generated_crate("three_instantiations", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_nullish_of_an_optional_chain_generic_call_repairs_the_binding() {
    const SOURCE: &str = r"class K { m<T>(x: T): T { return x; } }
export function got(): number { const k = new K(); const y = k?.m(5) ?? 0; return y * 2; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(chain_binding::got(&ctx), 10.0);
    println!("ok");
}
"#;
    binding_style_rust(SOURCE, "chain_binding", MAIN, "let y : i64");
}

#[test]
fn a_logical_operator_over_equal_generic_instantiations_repairs_the_binding() {
    const SOURCE: &str = r"class C { m<T>(x: T): T { return x; } }
export function both(): number { const c = new C(); const a = c.m(5) && c.m(6); const b = c.m(0) || c.m(7); return a + b; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(logical_binding::both(&ctx), 13.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    assert!(!rust.contains("__generic"), "got:\n{rust}");
    let norm = normalize_rust(&rust);
    assert!(
        norm.contains("let a : i64") && norm.contains("let b : i64"),
        "both logical bindings must be repaired; got:\n{rust}"
    );
    check_generated_crate("logical_binding", &rust);
    let stdout = run_generated_crate("logical_binding", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_conflicting_nullish_fallback_is_still_rejected() {
    const SOURCE: &str = r#"class K { m<T>(x: T): T { return x; } }
export function got(k: K | null): number { const kk = new K(); const y = k?.m(5) ?? kk.m("s"); return y; }"#;
    let out = compile_module(SOURCE);
    assert!(
        out.diagnostics.iter().any(|d| d.code.as_str() == "P0021"),
        "a fallback whose type cannot be lowered against the left-hand side must still be \
         rejected; got {:?}",
        out.diagnostics
    );
    assert!(
        out.rust_source.is_none(),
        "a rejected program must not emit Rust; got {:?}",
        out.rust_source
    );
}

#[test]
fn a_nullish_of_a_null_instantiation_takes_the_literal_fallback() {
    const SOURCE: &str = r"class C { m<T>(x: T): T { return x; } }
export function got(): number { const c = new C(); const y = c.m(null) ?? 0; return y * 2; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(coalesce_null_literal::got(&ctx), 0.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    assert!(!rust.contains("__generic"), "got:\n{rust}");
    let norm = normalize_rust(&rust);
    assert!(
        norm.contains("let y : i64"),
        "the binding must take the fallback's type instead of the null instantiation; got:\n{rust}"
    );
    check_generated_crate("coalesce_null_literal", &rust);
    let stdout = run_generated_crate("coalesce_null_literal", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_nullish_of_a_null_instantiation_takes_the_second_call() {
    const SOURCE: &str = r"class C { m<T>(x: T): T { return x; } }
export function got(): number { const c = new C(); const y = c.m(null) ?? c.m(5); return y * 2; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(coalesce_null_call::got(&ctx), 10.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    assert!(!rust.contains("__generic"), "got:\n{rust}");
    let norm = normalize_rust(&rust);
    assert!(
        norm.contains("let y : i64"),
        "the binding must take the fallback call's type; got:\n{rust}"
    );
    check_generated_crate("coalesce_null_call", &rust);
    let stdout = run_generated_crate("coalesce_null_call", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn an_annotated_nullish_of_a_null_instantiation_takes_the_second_call() {
    const SOURCE: &str = r"class C { m<T>(x: T): T { return x; } }
export function got(): number { const c = new C(); const y: number = c.m(null) ?? c.m(5); return y * 2; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(coalesce_null_call_annotated::got(&ctx), 10.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    assert!(!rust.contains("__generic"), "got:\n{rust}");
    let norm = normalize_rust(&rust);
    assert!(
        norm.contains("let y : f64"),
        "the annotation must be preserved and the null instantiation must not narrow it; got:\n{rust}"
    );
    check_generated_crate("coalesce_null_call_annotated", &rust);
    let stdout = run_generated_crate("coalesce_null_call_annotated", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_nullish_of_an_optional_instantiation_inside_an_expression_keeps_the_inner_type() {
    const SOURCE: &str = r"class C { m<T>(x: T): T { return x; } }
export function got(k: number | null): number { const c = new C(); return (c.m(k) ?? 0) * 2; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(coalesce_optional_inline::got(&ctx, Option::None), 0.0);
    assert_eq!(coalesce_optional_inline::got(&ctx, Option::Some(4.0)), 8.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    assert!(!rust.contains("__generic"), "got:\n{rust}");
    let norm = normalize_rust(&rust);
    assert!(
        norm.contains("* 2.0"),
        "the coalesce must yield the unwrapped inner type so the multiplication stays numeric; \
         got:\n{rust}"
    );
    check_generated_crate("coalesce_optional_inline", &rust);
    let stdout = run_generated_crate("coalesce_optional_inline", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_nullish_of_an_optional_instantiation_repairs_the_binding() {
    const SOURCE: &str = r"class C { m<T>(x: T): T { return x; } }
export function got(k: number | null): number { const c = new C(); const y = c.m(k) ?? 0; return y * 2; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(coalesce_optional_binding::got(&ctx, Option::None), 0.0);
    assert_eq!(coalesce_optional_binding::got(&ctx, Option::Some(4.0)), 8.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    assert!(!rust.contains("__generic"), "got:\n{rust}");
    let norm = normalize_rust(&rust);
    assert!(
        norm.contains("let y : f64"),
        "the binding must take the unwrapped inner type; got:\n{rust}"
    );
    check_generated_crate("coalesce_optional_binding", &rust);
    let stdout = run_generated_crate("coalesce_optional_binding", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn an_array_element_of_a_nullish_call_is_repaired() {
    const SOURCE: &str = r"class C { m<T>(x: T): T { return x; } }
export function got(): number { const c = new C(); const a = [c.m(5) ?? c.m(null)]; return a[0] * 2; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(coalesce_array_element::got(&ctx), 10.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    assert!(!rust.contains("__generic"), "got:\n{rust}");
    let norm = normalize_rust(&rust);
    assert!(
        norm.contains("Vec<'a, i64>"),
        "the array element must specialize from its elements; got:\n{rust}"
    );
    check_generated_crate("coalesce_array_element", &rust);
    let stdout = run_generated_crate("coalesce_array_element", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn an_annotated_array_element_of_a_nullish_call_is_repaired() {
    const SOURCE: &str = r"class C { m<T>(x: T): T { return x; } }
export function got(): number {
    const c = new C();
    const a: number[] = [c.m(5) ?? c.m(null)];
    return a[0] * 2;
}";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(coalesce_array_element_annotated::got(&ctx), 10.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    assert!(!rust.contains("__generic"), "got:\n{rust}");
    let norm = normalize_rust(&rust);
    assert!(
        norm.contains("Vec<'a, f64>"),
        "the annotation must keep the element type concrete; got:\n{rust}"
    );
    check_generated_crate("coalesce_array_element_annotated", &rust);
    let stdout = run_generated_crate("coalesce_array_element_annotated", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn an_object_field_of_a_nullish_call_is_repaired() {
    const SOURCE: &str = r"class C { m<T>(x: T): T { return x; } }
export function got(): number { const c = new C(); const o = { a: c.m(5) ?? c.m(null) }; return o.a * 2; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(coalesce_object_field::got(&ctx), 10.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    assert!(!rust.contains("__generic"), "got:\n{rust}");
    let norm = normalize_rust(&rust);
    assert!(
        norm.contains("struct ObjectLit1") && norm.contains("RefCell<i64>"),
        "the synthesized struct must carry the field's specialized type; got:\n{rust}"
    );
    check_generated_crate("coalesce_object_field", &rust);
    let stdout = run_generated_crate("coalesce_object_field", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn an_annotated_object_field_of_a_nullish_call_is_repaired() {
    const SOURCE: &str = r"class C { m<T>(x: T): T { return x; } }
export function got(): number {
    const c = new C();
    const o: { a: number } = { a: c.m(5) ?? c.m(null) };
    return o.a * 2;
}";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(coalesce_object_field_annotated::got(&ctx), 10.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    assert!(!rust.contains("__generic"), "got:\n{rust}");
    let norm = normalize_rust(&rust);
    assert!(
        norm.contains("struct ObjectLit1"),
        "the synthesized struct must be emitted; got:\n{rust}"
    );
    check_generated_crate("coalesce_object_field_annotated", &rust);
    let stdout = run_generated_crate("coalesce_object_field_annotated", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_conflicting_nullish_fallback_inside_an_array_is_still_rejected() {
    const SOURCE: &str = r#"class K { m<T>(x: T): T { return x; } }
export function got(k: K | null): number {
    const kk = new K();
    const a = [k?.m(5) ?? kk.m("s")];
    return a[0] * 2;
}"#;
    let out = compile_module(SOURCE);
    assert!(
        out.diagnostics.iter().any(|d| {
            d.code.as_str() == "P0021"
                && d.message
                    .contains("the left-hand side is `number`, the fallback is `string`")
        }),
        "a fallback that cannot be lowered against the optional instantiation must still be \
         rejected inside a container, and the conflict must name the concrete `number` \
         instantiation rather than an unresolved type parameter; got {:?}",
        out.diagnostics
    );
    assert!(
        out.rust_source.is_none(),
        "a rejected program must not emit Rust; got {:?}",
        out.rust_source
    );
}

#[test]
fn a_conflicting_nullish_fallback_inside_an_object_is_still_rejected() {
    const SOURCE: &str = r#"class K { m<T>(x: T): T { return x; } }
export function got(k: K | null): number {
    const kk = new K();
    const o = { a: k?.m(5) ?? kk.m("s") };
    return o.a * 2;
}"#;
    let out = compile_module(SOURCE);
    assert!(
        out.diagnostics.iter().any(|d| {
            d.code.as_str() == "P0021"
                && d.message
                    .contains("the left-hand side is `number`, the fallback is `string`")
        }),
        "a fallback that cannot be lowered against the optional instantiation must still be \
         rejected inside a container, and the conflict must name the concrete `number` \
         instantiation rather than an unresolved type parameter; got {:?}",
        out.diagnostics
    );
    assert!(
        out.rust_source.is_none(),
        "a rejected program must not emit Rust; got {:?}",
        out.rust_source
    );
}

#[test]
fn a_bare_optional_chain_generic_call_binding_is_typed_option() {
    const SOURCE: &str = r"class K { m<T>(x: T): T { return x; } }
export function got(k: K | null): number { const z = k?.m(5); return 0; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(optchain_bare::got(&ctx, ::core::option::Option::None), 0.0);
    assert_eq!(optchain_bare::got(&ctx, ::core::option::Option::Some(optchain_bare::K {})), 0.0);
    println!("ok");
}
"#;
    binding_style_rust(SOURCE, "optchain_bare", MAIN, "let z : Option<i64>");
}

#[test]
fn an_optional_chain_generic_call_binding_keeps_undefined() {
    const SOURCE: &str = r"class K { m<T>(x: T): T { return x; } }
export function got(k: K | null): number { const z = k?.m(5); return z === undefined ? 1 : 2; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(optchain_binding::got(&ctx, ::core::option::Option::None), 1.0);
    assert_eq!(optchain_binding::got(&ctx, ::core::option::Option::Some(optchain_binding::K {})), 2.0);
    println!("ok");
}
"#;
    binding_style_rust(SOURCE, "optchain_binding", MAIN, "let z : Option<i64>");
}

#[test]
fn an_optional_chain_generic_call_binding_compared_to_null_is_not_folded() {
    const SOURCE: &str = r"class K { m<T>(x: T): T { return x; } }
export function got(k: K | null): number { const z = k?.m(5); return z === null ? 1 : 2; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(optchain_null::got(&ctx, ::core::option::Option::None), 2.0);
    assert_eq!(optchain_null::got(&ctx, ::core::option::Option::Some(optchain_null::K {})), 2.0);
    println!("ok");
}
"#;
    binding_style_rust(SOURCE, "optchain_null", MAIN, "let z : Option<i64>");
}

#[test]
fn an_optional_chain_generic_call_binding_under_a_non_null_assertion_is_option() {
    const SOURCE: &str = r"class K { m<T>(x: T): T { return x; } }
export function got(k: K | null): number { const z = k?.m(5); return z! * 2; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(optchain_bang::got(&ctx, ::core::option::Option::Some(optchain_bang::K {})), 10.0);
    let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        optchain_bang::got(&ctx, ::core::option::Option::None)
    }))
    .is_err();
    assert!(panicked, "a violated non-null assertion must panic");
    println!("ok");
}
"#;
    binding_style_rust(SOURCE, "optchain_bang", MAIN, "let z : Option<i64>");
}

#[test]
fn an_optional_chain_generic_call_with_a_generic_argument_is_option() {
    const SOURCE: &str = r"class K { m<T>(x: T): T { return x; } }
export function got(k: K | null): number { const z = k?.m(k); return z === undefined ? 1 : 2; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(optchain_generic_arg::got(&ctx, ::core::option::Option::None), 1.0);
    assert_eq!(optchain_generic_arg::got(&ctx, ::core::option::Option::Some(optchain_generic_arg::K {})), 2.0);
    println!("ok");
}
"#;
    binding_style_rust(SOURCE, "optchain_generic_arg", MAIN, "let z : Option<K>");
    let rust = rust_of(SOURCE);
    let norm = normalize_rust(&rust);
    assert!(
        !norm.contains("Option<Option<K>>"),
        "an optional chain over its own receiver narrows the receiver to `K`, so the call result \
         must be a single `Option`; got:\n{rust}"
    );
}

#[test]
fn an_assignment_of_an_optional_chain_generic_call_keeps_undefined() {
    const SOURCE: &str = r"class K { m<T>(x: T): T { return x; } }
export function got(k: K | null): number { let z: number | undefined = 0; z = k?.m(5); return z === undefined ? 1 : 2; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(optchain_assignment::got(&ctx, ::core::option::Option::None), 1.0);
    assert_eq!(optchain_assignment::got(&ctx, ::core::option::Option::Some(optchain_assignment::K {})), 2.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let norm = normalize_rust(&rust);
    assert!(
        norm.contains("Option<i64>") && norm.contains("is_none"),
        "the assigned value must keep the `undefined` alternative; got:\n{rust}"
    );
    check_generated_crate("optchain_assignment", &rust);
    let stdout = run_generated_crate("optchain_assignment", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_single_optional_target_accepts_an_optional_chain_generic_call() {
    const SOURCE: &str = r"class K { m<T>(x: T): T { return x; } }
export function f(k: K | null): number { let z: K | undefined; z = k?.m(k); return z === undefined ? 1 : 2; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(optchain_single_target::f(&ctx, ::core::option::Option::None), 1.0);
    assert_eq!(optchain_single_target::f(&ctx, ::core::option::Option::Some(optchain_single_target::K {})), 2.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let norm = normalize_rust(&rust);
    assert!(
        norm.contains("let mut z : Option<K>") && !norm.contains("Option<Option<K>>"),
        "`k?.m(k)` narrows the receiver to `K` inside the chain, so the result is a single \
         `K | undefined`; got:\n{rust}"
    );
    check_generated_crate("optchain_single_target", &rust);
    let stdout = run_generated_crate("optchain_single_target", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_single_optional_argument_accepts_an_optional_chain_generic_call() {
    const SOURCE: &str = r"class K { m<T>(x: T): T { return x; } }
function want(v: K | undefined): number { return v === undefined ? 1 : 2; }
export function f(k: K | undefined): number { return want(k?.m(k)); }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(optchain_single_argument::f(&ctx, ::core::option::Option::None), 1.0);
    assert_eq!(optchain_single_argument::f(&ctx, ::core::option::Option::Some(optchain_single_argument::K {})), 2.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let norm = normalize_rust(&rust);
    assert!(
        norm.contains("v : Option<K>") && !norm.contains("Option<Option<K>>"),
        "`want(k?.m(k))` must pass a single `Option<K>` argument; got:\n{rust}"
    );
    check_generated_crate("optchain_single_argument", &rust);
    let stdout = run_generated_crate("optchain_single_argument", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_single_optional_return_accepts_an_optional_chain_generic_call() {
    const SOURCE: &str = r"class K { m<T>(x: T): T { return x; } }
export function f(k: K | undefined): K | undefined { return k?.m(k); }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    let absent = optchain_return::f(&ctx, ::core::option::Option::None);
    assert!(absent.is_none(), "an absent receiver must return undefined");
    let present = optchain_return::f(&ctx, ::core::option::Option::Some(optchain_return::K {}));
    assert!(present.is_some(), "a present receiver must return the argument");
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let norm = normalize_rust(&rust);
    assert!(
        norm.contains("->Option<K>") && !norm.contains("Option<Option<K>>"),
        "`return k?.m(k)` from a `K | undefined` function must stay a single `Option<K>`; got:\n{rust}"
    );
    check_generated_crate("optchain_return", &rust);
    let stdout = run_generated_crate("optchain_return", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_nested_optional_chain_call_is_not_written_into_a_single_optional() {
    const SOURCE: &str = r"class K { m<T>(x: T): T { return x; } }
export function f(k: K | undefined, a: number | null): number { let z: K | undefined; z = k?.m(a); return z === undefined ? 1 : 2; }";
    let out = compile_module(SOURCE);
    assert!(
        out.diagnostics
            .iter()
            .any(|d| { d.code.as_str() == "P0018" && d.message.contains("two nested `Option`s") }),
        "a `null`-typed argument makes the optional chain's result a nested `Option` that a single \
         optional target cannot hold, because the target would conflate `null` with `undefined`; \
         the write must be diagnosed rather than emitted; got {:?}",
        out.diagnostics
    );
    assert!(
        out.rust_source.is_none(),
        "a rejected program must not emit Rust; got {:?}",
        out.rust_source
    );
}

#[test]
fn a_non_null_receiver_of_an_optional_chain_generic_call_stays_plain() {
    const SOURCE: &str = r"class K { m<T>(x: T): T { return x; } }
export function got(k: K): number { const z = k?.m(5); return z * 2; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(optchain_non_null_receiver::got(&ctx, optchain_non_null_receiver::K {}), 10.0);
    println!("ok");
}
"#;
    binding_style_rust(SOURCE, "optchain_non_null_receiver", MAIN, "let z : i64");
}

#[test]
fn an_annotated_optional_chain_generic_call_binding_stays_optional() {
    const SOURCE: &str = r"class K { m<T>(x: T): T { return x; } }
export function got(k: K | null): number { const z: number | undefined = k?.m(5); return z === undefined ? 1 : 2; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(optchain_annotated::got(&ctx, ::core::option::Option::None), 1.0);
    assert_eq!(optchain_annotated::got(&ctx, ::core::option::Option::Some(optchain_annotated::K {})), 2.0);
    println!("ok");
}
"#;
    binding_style_rust(SOURCE, "optchain_annotated", MAIN, "let z : Option<f64>");
}

#[test]
fn a_nullish_of_an_optional_chain_generic_call_keeps_the_inner_type() {
    const SOURCE: &str = r"class K { m<T>(x: T): T { return x; } }
export function got(k: K | null): number { const y = k?.m(5) ?? 0; return y * 2; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(optchain_coalesce::got(&ctx, ::core::option::Option::None), 0.0);
    assert_eq!(optchain_coalesce::got(&ctx, ::core::option::Option::Some(optchain_coalesce::K {})), 10.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let norm = normalize_rust(&rust);
    assert!(
        norm.contains("Option<i64>") && norm.contains("is_some"),
        "`?? 0` must keep the `Option<i64>` test and the `i64` inner type; got:\n{rust}"
    );
    check_generated_crate("optchain_coalesce", &rust);
    let stdout = run_generated_crate("optchain_coalesce", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn an_optional_chain_generic_call_with_an_undefined_typed_argument_keeps_undefined() {
    const SOURCE: &str = r"class K { m<T>(x: T): T { return x; } }
export function f(k: K | undefined, a: number | undefined): number { const z = k?.m(a); return z === undefined ? 1 : 2; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    let absent_k: ::core::option::Option<optchain_undefined_arg::K> = ::core::option::Option::None;
    let absent_a: ::core::option::Option<f64> = ::core::option::Option::None;
    let present = || ::core::option::Option::Some(optchain_undefined_arg::K {});
    assert_eq!(optchain_undefined_arg::f(&ctx, present(), absent_a), 1.0);
    assert_eq!(optchain_undefined_arg::f(&ctx, present(), ::core::option::Option::Some(4.0)), 2.0);
    assert_eq!(optchain_undefined_arg::f(&ctx, absent_k.clone(), absent_a), 1.0);
    assert_eq!(optchain_undefined_arg::f(&ctx, absent_k, ::core::option::Option::Some(4.0)), 1.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let norm = normalize_rust(&rust);
    assert!(
        norm.contains("let z : Option<f64>") && !norm.contains("Option<Option<f64>>"),
        "the `undefined`-typed value type merges into the outer `undefined`, so `z` must be a \
         single `Option<f64>`; got:\n{rust}"
    );
    check_generated_crate("optchain_undefined_arg", &rust);
    let stdout = run_generated_crate("optchain_undefined_arg", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn an_optional_chain_generic_call_with_a_null_typed_argument_compares_to_null() {
    const SOURCE: &str = r"class K { m<T>(x: T): T { return x; } }
export function f(k: K | undefined, a: number | null): number { const z = k?.m(a); return z === null ? 1 : 2; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    let absent_k: ::core::option::Option<optchain_null_arg::K> = ::core::option::Option::None;
    let absent_a: ::core::option::Option<f64> = ::core::option::Option::None;
    let present = || ::core::option::Option::Some(optchain_null_arg::K {});
    assert_eq!(optchain_null_arg::f(&ctx, present(), absent_a), 1.0);
    assert_eq!(optchain_null_arg::f(&ctx, present(), ::core::option::Option::Some(4.0)), 2.0);
    assert_eq!(optchain_null_arg::f(&ctx, absent_k.clone(), absent_a), 2.0);
    assert_eq!(optchain_null_arg::f(&ctx, absent_k, ::core::option::Option::Some(4.0)), 2.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let norm = normalize_rust(&rust);
    assert!(
        norm.contains("let z : Option<Option<f64>>") && norm.contains("Some (None)"),
        "a `null`-typed value type must keep its own `Option` level and compare the inner \
         `None`, because `null === undefined` is false; got:\n{rust}"
    );
    check_generated_crate("optchain_null_arg", &rust);
    let stdout = run_generated_crate("optchain_null_arg", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn an_optional_chain_generic_call_with_a_null_typed_argument_loosely_compares() {
    const SOURCE: &str = r"class K { m<T>(x: T): T { return x; } }
export function f(k: K | undefined, a: number | null): number { const z = k?.m(a); return z == undefined ? 1 : 2; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    let absent_k: ::core::option::Option<optchain_null_loose::K> = ::core::option::Option::None;
    let absent_a: ::core::option::Option<f64> = ::core::option::Option::None;
    let present = || ::core::option::Option::Some(optchain_null_loose::K {});
    assert_eq!(optchain_null_loose::f(&ctx, present(), absent_a), 1.0);
    assert_eq!(optchain_null_loose::f(&ctx, present(), ::core::option::Option::Some(4.0)), 2.0);
    assert_eq!(optchain_null_loose::f(&ctx, absent_k.clone(), absent_a), 1.0);
    assert_eq!(optchain_null_loose::f(&ctx, absent_k, ::core::option::Option::Some(4.0)), 1.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let norm = normalize_rust(&rust);
    assert!(
        norm.contains("let z : Option<Option<f64>>")
            && norm.contains("Some (None)")
            && norm.contains("matches !"),
        "a loose `== undefined` must accept either `Option` level of the nested value; got:\n{rust}"
    );
    check_generated_crate("optchain_null_loose", &rust);
    let stdout = run_generated_crate("optchain_null_loose", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn an_optional_chain_generic_call_with_a_null_typed_argument_is_asserted() {
    const SOURCE: &str = r"class K { m<T>(x: T): T { return x; } }
export function f(k: K | undefined, a: number | null): number { const z = k?.m(a); return z! * 2; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(
        optchain_null_bang::f(&ctx, ::core::option::Option::Some(optchain_null_bang::K {}), ::core::option::Option::Some(4.0)),
        8.0
    );
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let norm = normalize_rust(&rust);
    assert!(
        norm.contains("unwrap () . unwrap ()"),
        "a non-null assertion on a nested optional must strip every nullish level; got:\n{rust}"
    );
    check_generated_crate("optchain_null_bang", &rust);
    let stdout = run_generated_crate("optchain_null_bang", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn an_annotated_array_element_accepts_an_optional_chain_generic_call() {
    const SOURCE: &str = r"class K { m<T>(x: T): T { return x; } }
export function f(k: K | undefined): number { const zs: (K | undefined)[] = [k?.m(k)]; return zs[0] === undefined ? 1 : 2; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(optchain_array_element::f(&ctx, ::core::option::Option::None), 1.0);
    assert_eq!(optchain_array_element::f(&ctx, ::core::option::Option::Some(optchain_array_element::K {})), 2.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let norm = normalize_rust(&rust);
    assert!(
        norm.contains("Option<K>>") && !norm.contains("Option<Option<K>>"),
        "the array element must be a single `Option<K>` so the annotated element type holds; got:\n{rust}"
    );
    check_generated_crate("optchain_array_element", &rust);
    let stdout = run_generated_crate("optchain_array_element", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn an_assertion_wrapped_optional_chain_generic_call_writes_a_single_optional() {
    const SOURCE: &str = r"class K { m<T>(x: T): T { return x; } }
export function f(k: K | undefined): number {
  let z: K | undefined;
  z = k?.m(k) as K | undefined;
  return z === undefined ? 1 : 2;
}";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(optchain_asserted_write::f(&ctx, ::core::option::Option::None), 1.0);
    assert_eq!(optchain_asserted_write::f(&ctx, ::core::option::Option::Some(optchain_asserted_write::K {})), 2.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let norm = normalize_rust(&rust);
    assert!(
        norm.contains("let mut z : Option<K>") && !norm.contains("Option<Option<K>>"),
        "the assertion target must be the single `K | undefined` the write declares; got:\n{rust}"
    );
    check_generated_crate("optchain_asserted_write", &rust);
    let stdout = run_generated_crate("optchain_asserted_write", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_nested_nullish_chain_uses_the_fallback_when_the_inner_value_is_null() {
    const SOURCE: &str = r"class K { pick(x: number): number | null { if (x > 0) { return x; } return null; } }
export function got(k: K | null, x: number): number { return k?.pick(x) ?? 7; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(
        nested_nullish_chain::got(&ctx, ::core::option::Option::None, 5.0),
        7.0
    );
    assert_eq!(
        nested_nullish_chain::got(
            &ctx,
            ::core::option::Option::Some(nested_nullish_chain::K {}),
            -1.0
        ),
        7.0
    );
    assert_eq!(
        nested_nullish_chain::got(
            &ctx,
            ::core::option::Option::Some(nested_nullish_chain::K {}),
            5.0
        ),
        5.0
    );
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    assert!(!rust.contains("__generic"), "got:\n{rust}");
    let norm = normalize_rust(&rust);
    assert!(
        norm.contains("unwrap () . unwrap ()"),
        "the fallback must be selected unless both optional layers are present, so a present \
         value must unwrap both; got:\n{rust}"
    );
    check_generated_crate("nested_nullish_chain", &rust);
    let stdout = run_generated_crate("nested_nullish_chain", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_null_typed_fallback_keeps_the_null_kind_of_the_result() {
    const SOURCE: &str = r"export function got(x: number | undefined, n: null): number | null {
  const y = x ?? n;
  return y;
}
export function isNull(x: number | undefined, n: null): boolean {
  const y = x ?? n;
  return y === null;
}";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(
        null_typed_fallback::got(&ctx, ::core::option::Option::None, ()),
        ::core::option::Option::None
    );
    assert_eq!(
        null_typed_fallback::got(&ctx, ::core::option::Option::Some(4.0), ()),
        ::core::option::Option::Some(4.0)
    );
    assert!(
        null_typed_fallback::isNull(&ctx, ::core::option::Option::None, ()),
        "`y === null` must be true when `x` is absent"
    );
    assert!(
        !null_typed_fallback::isNull(&ctx, ::core::option::Option::Some(4.0), ()),
        "`y === null` must be false when `x` is present"
    );
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    assert!(!rust.contains("__generic"), "got:\n{rust}");
    let norm = normalize_rust(&rust);
    assert!(
        norm.contains("let y : Option<f64>"),
        "`x ?? null` must be `number | null`, not `number | undefined`; got:\n{rust}"
    );
    check_generated_crate("null_typed_fallback", &rust);
    let stdout = run_generated_crate("null_typed_fallback", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_non_null_assertion_of_a_generic_call_types_the_binding_as_its_target() {
    const SOURCE: &str = r"class C { m<T>(x: T): T { return x; } }
export function got(k: number | undefined): number {
  const c = new C();
  const y = c.m(k)!;
  return y;
}";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(
        assertion_target_binding::got(&ctx, ::core::option::Option::Some(4.0)),
        4.0
    );
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    assert!(!rust.contains("__generic"), "got:\n{rust}");
    let norm = normalize_rust(&rust);
    assert!(
        norm.contains("let y : f64") && !norm.contains("let y : Option<f64>"),
        "the assertion target must type the binding, not the optional operand; got:\n{rust}"
    );
    check_generated_crate("assertion_target_binding", &rust);
    let stdout = run_generated_crate("assertion_target_binding", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn an_object_literal_binds_generic_fields_in_declaration_order() {
    const SOURCE: &str = r#"class Pair<T, U> { a: T; b: U; }
export function first(): number { const p: Pair<number, string> = { b: "hello", a: 5 }; return p.a; }
export function second(): string { const p: Pair<number, string> = { b: "hello", a: 5 }; return p.b; }"#;
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(ordered_literal_fields::first(&ctx), 5.0);
    assert_eq!(
        ordered_literal_fields::second(&ctx),
        ts_aot_runtime::JsString::from("hello")
    );
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    assert!(!rust.contains("__generic"), "got:\n{rust}");
    let norm = normalize_rust(&rust);
    assert!(
        norm.contains("RefCell<i64>") && norm.contains("RefCell<ts_aot_runtime::JsString"),
        "the declared field order must drive the generic substitution, so `a` is the number and \
         `b` the string; got:\n{rust}"
    );
    check_generated_crate("ordered_literal_fields", &rust);
    let stdout = run_generated_crate("ordered_literal_fields", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn an_inferred_nullish_of_a_generic_call_keeps_undefined_in_the_type() {
    const SOURCE: &str = r"class C { m<T>(x: T): T { return x; } }
export function got(x: number | undefined, y: i64 | undefined): number {
  const c = new C();
  const z = x ?? c.m(y);
  return z === undefined ? 1 : 2;
}";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(
        inferred_nullish_generic_call::got(
            &ctx,
            ::core::option::Option::None,
            ::core::option::Option::None
        ),
        1.0
    );
    assert_eq!(
        inferred_nullish_generic_call::got(
            &ctx,
            ::core::option::Option::Some(5.0),
            ::core::option::Option::None
        ),
        2.0
    );
    assert_eq!(
        inferred_nullish_generic_call::got(
            &ctx,
            ::core::option::Option::None,
            ::core::option::Option::Some(7_i64)
        ),
        2.0
    );
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let norm = normalize_rust(&rust);
    assert!(
        norm.contains("let z : Option<f64>"),
        "`x ?? c.m(y)` can yield the fallback's `undefined`, so the inferred binding must be \
         `Option<f64>` instead of the frontend's non-optional `number`; got:\n{rust}"
    );
    assert!(!rust.contains("__generic"), "got:\n{rust}");
    check_generated_crate("inferred_nullish_generic_call", &rust);
    let stdout = run_generated_crate("inferred_nullish_generic_call", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn an_optional_chain_argument_that_reads_the_receiver_is_narrowed() {
    const SOURCE: &str = r"class K { pick<T>(n: number, b: T): T { return b; } }
export function got(k: K | null): K | undefined { const z = k?.pick(0, k); return z; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    let absent = optchain_receiver_arg::got(&ctx, ::core::option::Option::None);
    assert!(absent.is_none(), "an absent receiver must yield undefined");
    let present =
        optchain_receiver_arg::got(&ctx, ::core::option::Option::Some(optchain_receiver_arg::K {}));
    assert!(
        present.is_some(),
        "the receiver argument must be narrowed to the guarded value"
    );
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    assert!(!rust.contains("__generic"), "got:\n{rust}");
    let norm = normalize_rust(&rust);
    assert!(
        norm.contains("K__pick_mono") && norm.contains("unwrap ()"),
        "a call without a receiver write must keep the receiver-argument narrowing; got:\n{rust}"
    );
    check_generated_crate("optchain_receiver_arg", &rust);
    let stdout = run_generated_crate("optchain_receiver_arg", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn an_optional_chain_argument_that_reads_a_reassigned_receiver_is_rejected() {
    const SOURCE: &str = r"class K { pick<T>(n: number, b: T): T { return b; } }
export function got(k: K | null): K | undefined { const z = k?.pick((k = null, 0), k); return z; }";
    let out = compile_module(SOURCE);
    assert!(
        out.diagnostics.iter().any(|d| {
            d.code.as_str() == "E0300" && d.message.contains("reassigns the receiver")
        }),
        "an argument that reads a receiver an earlier argument reassigns cannot be narrowed; got \
         {:?}",
        out.diagnostics
    );
    assert!(
        out.rust_source.is_none(),
        "a rejected program must not emit Rust; got {:?}",
        out.rust_source
    );
}

#[test]
fn an_optional_chain_argument_that_reads_a_later_reassigned_receiver_is_rejected() {
    const SOURCE: &str = r"class K { pick<T>(a: T, b: number, c: number): T { return a; } }
function take(v: K | null): number { return 0; }
export function got(k: K | null, other: K | null): K | undefined { const z = k?.pick(k, 0, take(k = other)); return z; }";
    let out = compile_module(SOURCE);
    assert!(
        out.diagnostics.iter().any(|d| {
            d.code.as_str() == "E0300" && d.message.contains("reassigns the receiver")
        }),
        "the receiver argument is read before a later argument reassigns the receiver, so narrowing \
         it to the guarded non-nullish type would pass the post-write value; got {:?}",
        out.diagnostics
    );
    assert!(
        out.rust_source.is_none(),
        "a rejected program must not emit Rust; got {:?}",
        out.rust_source
    );
}

#[test]
fn a_parameterless_static_generic_method_is_specialized() {
    const SOURCE: &str = r"class C {
  static wrap<T>(): T | null { return null; }
  static five<T>(): number { return 5; }
  pick<T>(): T | null { return null; }
}
export function num(): boolean { return C.wrap<number>() === null; }
export function str(): boolean { return C.wrap<string>() === null; }
export function plain(): number { return C.five<number>(); }
export function instNum(): boolean { return new C().pick<number>() === null; }
export function instStr(): boolean { return new C().pick<string>() === null; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(parameterless_static_generic::num(&ctx), true);
    assert_eq!(parameterless_static_generic::str(&ctx), true);
    assert_eq!(parameterless_static_generic::plain(&ctx), 5.0);
    assert_eq!(parameterless_static_generic::instNum(&ctx), true);
    assert_eq!(parameterless_static_generic::instStr(&ctx), true);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    assert!(!rust.contains("__generic"), "got:\n{rust}");
    let norm = normalize_rust(&rust);
    assert!(
        norm.contains("C__wrap_mono"),
        "a parameterless static generic method whose signature uses the type parameter must get \
         its own specialization for the explicit type arguments; got:\n{rust}"
    );
    assert!(
        norm.contains("C__five_mono"),
        "a parameterless static generic method that does not use its type parameter must still be \
         indexed, so an explicit-type-argument call reaches its specialization; got:\n{rust}"
    );
    assert!(
        norm.contains("C__pick_mono"),
        "a parameterless instance generic method must be specialized too; got:\n{rust}"
    );
    check_generated_crate("parameterless_static_generic", &rust);
    let stdout = run_generated_crate("parameterless_static_generic", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn an_optional_chain_nullish_condition_keeps_the_optional_type() {
    const SOURCE: &str = r"class K { m(): number | undefined { return 3; } }
export function got(k: K | null): number { if (k?.m() ?? null) { return 1; } return 0; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(
        optchain_nullish_condition::got(&ctx, ::core::option::Option::None),
        0.0
    );
    assert_eq!(
        optchain_nullish_condition::got(
            &ctx,
            ::core::option::Option::Some(optchain_nullish_condition::K {})
        ),
        1.0
    );
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let norm = normalize_rust(&rust);
    assert!(
        !norm.contains(": f64 = __tmp"),
        "`(k?.m() ?? null)` may be null, so the condition must read an `Option`, never a plain \
         `f64`; got:\n{rust}"
    );
    assert!(
        norm.contains("Option<f64>") && norm.contains("is_some"),
        "the nullish condition must stay optional; got:\n{rust}"
    );
    check_generated_crate("optchain_nullish_condition", &rust);
    let stdout = run_generated_crate("optchain_nullish_condition", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn an_optional_chain_nullish_ternary_condition_keeps_the_optional_type() {
    const SOURCE: &str = r"class K { m(): number | undefined { return 3; } }
export function got(k: K | null): number { return (k?.m() ?? null) ? 1 : 0; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(
        optchain_nullish_ternary::got(&ctx, ::core::option::Option::None),
        0.0
    );
    assert_eq!(
        optchain_nullish_ternary::got(
            &ctx,
            ::core::option::Option::Some(optchain_nullish_ternary::K {})
        ),
        1.0
    );
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let norm = normalize_rust(&rust);
    assert!(
        !norm.contains(": f64 = __tmp"),
        "`(k?.m() ?? null)` may be null, so the ternary condition must read an `Option`; got:\n\
         {rust}"
    );
    check_generated_crate("optchain_nullish_ternary", &rust);
    let stdout = run_generated_crate("optchain_nullish_ternary", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn an_optional_chain_undefined_condition_keeps_the_optional_type() {
    const SOURCE: &str = r"class K { m(): number | undefined { return 3; } }
export function got(k: K | null): number { if (k?.m() ?? undefined) { return 1; } return 0; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(
        optchain_undefined_condition::got(&ctx, ::core::option::Option::None),
        0.0
    );
    assert_eq!(
        optchain_undefined_condition::got(
            &ctx,
            ::core::option::Option::Some(optchain_undefined_condition::K {})
        ),
        1.0
    );
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let norm = normalize_rust(&rust);
    assert!(
        !norm.contains(": f64 = __tmp"),
        "`(k?.m() ?? undefined)` may be undefined, so the condition must read an `Option`; got:\n\
         {rust}"
    );
    check_generated_crate("optchain_undefined_condition", &rust);
    let stdout = run_generated_crate("optchain_undefined_condition", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn an_optional_chain_nullish_const_binding_keeps_the_optional_type() {
    const SOURCE: &str = r"class K { m(): number | undefined { return 3; } }
export function got(k: K | null): number { const r = k?.m() ?? null; return r ? 1 : 0; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(
        optchain_nullish_const::got(&ctx, ::core::option::Option::None),
        0.0
    );
    assert_eq!(
        optchain_nullish_const::got(
            &ctx,
            ::core::option::Option::Some(optchain_nullish_const::K {})
        ),
        1.0
    );
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let norm = normalize_rust(&rust);
    assert!(
        norm.contains("let r : Option<f64>"),
        "the inferred binding of `k?.m() ?? null` is `number | null`, so `r` must be \
         `Option<f64>`; got:\n{rust}"
    );
    check_generated_crate("optchain_nullish_const", &rust);
    let stdout = run_generated_crate("optchain_nullish_const", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn an_optional_chain_nullish_annotated_binding_stays_optional() {
    const SOURCE: &str = r"class K { m(): number | undefined { return 3; } }
function use(x: number | null): number { return x ?? 0; }
export function got(k: K | null): number { let r: number | null = k?.m() ?? null; return use(r); }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(
        optchain_nullish_annotated::got(&ctx, ::core::option::Option::None),
        0.0
    );
    assert_eq!(
        optchain_nullish_annotated::got(
            &ctx,
            ::core::option::Option::Some(optchain_nullish_annotated::K {})
        ),
        3.0
    );
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let norm = normalize_rust(&rust);
    assert!(
        norm.contains("let r : Option<f64>"),
        "an annotated `number | null` binding must stay `Option<f64>`; got:\n{rust}"
    );
    check_generated_crate("optchain_nullish_annotated", &rust);
    let stdout = run_generated_crate("optchain_nullish_annotated", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn an_optional_chain_nullish_control_shapes_stay_working() {
    const SOURCE: &str = r"class K { m(): number | undefined { return 3; } }
export function seven(k: K | null): number { return k?.m() ?? 7; }
export function direct(k: K | null): number | null { return k?.m() ?? null; }
export function eqNull(k: K | null): boolean { return (k?.m() ?? null) === null; }
class L { m(): number | null { return 3; } }
export function fromNull(k: L | null): number { if (k?.m() ?? undefined) { return 1; } return 0; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(optchain_nullish_controls::seven(&ctx, ::core::option::Option::None), 7.0);
    assert_eq!(
        optchain_nullish_controls::seven(
            &ctx,
            ::core::option::Option::Some(optchain_nullish_controls::K {})
        ),
        3.0
    );
    assert_eq!(
        optchain_nullish_controls::direct(&ctx, ::core::option::Option::None),
        ::core::option::Option::None
    );
    assert_eq!(
        optchain_nullish_controls::direct(
            &ctx,
            ::core::option::Option::Some(optchain_nullish_controls::K {})
        ),
        ::core::option::Option::Some(3.0)
    );
    assert_eq!(optchain_nullish_controls::eqNull(&ctx, ::core::option::Option::None), true);
    assert_eq!(
        optchain_nullish_controls::eqNull(
            &ctx,
            ::core::option::Option::Some(optchain_nullish_controls::K {})
        ),
        false
    );
    assert_eq!(optchain_nullish_controls::fromNull(&ctx, ::core::option::Option::None), 0.0);
    assert_eq!(
        optchain_nullish_controls::fromNull(
            &ctx,
            ::core::option::Option::Some(optchain_nullish_controls::L {})
        ),
        1.0
    );
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    check_generated_crate("optchain_nullish_controls", &rust);
    let stdout = run_generated_crate("optchain_nullish_controls", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn an_optional_chain_receiver_argument_in_the_generic_position_is_narrowed() {
    const SOURCE: &str = r"class K { pick<T>(n: number, b: T): number { return n; } }
export function got(k: K | null): number { const z = k?.pick(0, k) ?? 0; return z; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(
        optchain_generic_position_arg::got(&ctx, ::core::option::Option::None),
        0.0
    );
    assert_eq!(
        optchain_generic_position_arg::got(
            &ctx,
            ::core::option::Option::Some(optchain_generic_position_arg::K {})
        ),
        0.0
    );
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let norm = normalize_rust(&rust);
    assert!(
        norm.contains("K__pick_mono") && norm.contains("unwrap ()"),
        "the receiver argument in the `T`-typed position must still be narrowed to `K`; got:\n\
         {rust}"
    );
    check_generated_crate("optchain_generic_position_arg", &rust);
    let stdout = run_generated_crate("optchain_generic_position_arg", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn an_optional_chain_receiver_argument_in_a_non_generic_position_is_rejected() {
    const SOURCE: &str = r"class K { pick<T>(n: number, b: T): T { return b; } }
export function got(k: K | null): K | undefined { const z = k?.pick(k, 0); return z; }";
    let out = compile_module(SOURCE);
    assert!(
        out.diagnostics
            .iter()
            .any(|d| d.code.as_str() == "P0018"
                && d.message.contains("non-optional value is required")),
        "the receiver local sits in the non-generic `n: number` position, which cannot be typed \
         soundly, so the call must be rejected instead of emitting broken Rust; got {:?}",
        out.diagnostics
    );
    assert!(
        out.rust_source.is_none(),
        "a rejected program must not emit Rust; got {:?}",
        out.rust_source
    );
}

#[test]
fn an_optional_chain_receiver_argument_in_a_non_generic_position_with_a_fallback_is_rejected() {
    const SOURCE: &str = r"class K { pick<T>(n: number, b: T): T { return b; } }
export function got(k: K | null): K | undefined { const z = k?.pick(k, 0) ?? 0; return z; }";
    let out = compile_module(SOURCE);
    assert!(
        out.diagnostics
            .iter()
            .any(|d| d.code.as_str() == "P0018"
                && d.message.contains("non-optional value is required")),
        "the `?? 0` fallback does not make `k?.pick(k, 0)` typeable; the receiver local still sits \
         in the non-generic `n: number` position and must be rejected; got {:?}",
        out.diagnostics
    );
    assert!(
        out.rust_source.is_none(),
        "a rejected program must not emit Rust; got {:?}",
        out.rust_source
    );
}

#[test]
fn a_nullish_array_element_takes_the_coalesce_result_type() {
    const SOURCE: &str = r"class K { m(): number | undefined { return 3; } }
export function got(k: K | undefined): number { const a = [k?.m() ?? null]; return a.length; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(nullish_array_element::got(&ctx, ::core::option::Option::None), 1.0);
    assert_eq!(
        nullish_array_element::got(
            &ctx,
            ::core::option::Option::Some(nullish_array_element::K {})
        ),
        1.0
    );
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let norm = normalize_rust(&rust);
    assert!(
        norm.contains("Vec<'a, Option<f64>>"),
        "the array element type must be the `??` result `Option<f64>`, not the frontend's stale \
         `f64`; got:\n{rust}"
    );
    check_generated_crate("nullish_array_element", &rust);
    let stdout = run_generated_crate("nullish_array_element", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_nullish_object_field_takes_the_coalesce_result_type() {
    const SOURCE: &str = r"class K { m(): number | undefined { return 3; } }
export function got(k: K | undefined): number { const o = { x: k?.m() ?? null }; return o.x === null ? 0 : 1; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(nullish_object_field::got(&ctx, ::core::option::Option::None), 0.0);
    assert_eq!(
        nullish_object_field::got(
            &ctx,
            ::core::option::Option::Some(nullish_object_field::K {})
        ),
        1.0
    );
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let norm = normalize_rust(&rust);
    assert!(
        norm.contains("RefCell<Option<f64>>"),
        "the synthesized struct field must be the `??` result `Option<f64>`, not the frontend's \
         stale `f64`; got:\n{rust}"
    );
    check_generated_crate("nullish_object_field", &rust);
    let stdout = run_generated_crate("nullish_object_field", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_nullish_array_element_in_a_call_argument_takes_the_coalesce_result_type() {
    const SOURCE: &str = r"class K { m(): number | undefined { return 3; } }
function len(a: (number | null)[]): number { return a.length; }
export function got(k: K | undefined): number { return len([k?.m() ?? null]); }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(
        nullish_argument_array_element::got(&ctx, ::core::option::Option::None),
        1.0
    );
    assert_eq!(
        nullish_argument_array_element::got(
            &ctx,
            ::core::option::Option::Some(nullish_argument_array_element::K {})
        ),
        1.0
    );
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let norm = normalize_rust(&rust);
    assert!(
        norm.contains("Vec<'a, Option<f64>>"),
        "a container nested in a call argument must still carry the `??` result `Option<f64>`; \
         got:\n{rust}"
    );
    check_generated_crate("nullish_argument_array_element", &rust);
    let stdout = run_generated_crate("nullish_argument_array_element", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn an_inferred_binding_of_a_retyped_nullish_local_keeps_the_optional_type() {
    const SOURCE: &str = r"class K { m(): number | undefined { return 3; } }
export function got(k: K | undefined): number { const r = k?.m() ?? null; const s = r; return s ? 1 : 0; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(retyped_local_binding::got(&ctx, ::core::option::Option::None), 0.0);
    assert_eq!(
        retyped_local_binding::got(
            &ctx,
            ::core::option::Option::Some(retyped_local_binding::K {})
        ),
        1.0
    );
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let norm = normalize_rust(&rust);
    assert!(
        norm.contains("let s : Option<f64>"),
        "`const s = r` must follow the repaired `Option<f64>` type of `r`; got:\n{rust}"
    );
    check_generated_crate("retyped_local_binding", &rust);
    let stdout = run_generated_crate("retyped_local_binding", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_plain_nullish_local_binding_keeps_the_optional_type() {
    const SOURCE: &str = "export function got(x: number | undefined): number { \
         const r = x ?? null; return r === null ? 0 : 1; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(plain_nullish_binding::got(&ctx, ::core::option::Option::None), 0.0);
    assert_eq!(
        plain_nullish_binding::got(&ctx, ::core::option::Option::Some(5.0)),
        1.0
    );
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let norm = normalize_rust(&rust);
    assert!(
        norm.contains("let r : Option<f64>"),
        "`x ?? null` may be null, so the binding must be `Option<f64>`; got:\n{rust}"
    );
    check_generated_crate("plain_nullish_binding", &rust);
    let stdout = run_generated_crate("plain_nullish_binding", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn an_optional_chain_argument_with_a_nullable_generic_parameter_is_accepted() {
    const SOURCE: &str = r"class K { pick<T>(n: number, b: T | null): T { return b!; } }
export function got(k: K | null): K | undefined { const z = k?.pick(0, k); return z; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert!(
        nullable_generic_argument::got(&ctx, ::core::option::Option::None).is_none(),
        "an absent receiver must yield undefined"
    );
    assert!(
        nullable_generic_argument::got(
            &ctx,
            ::core::option::Option::Some(nullable_generic_argument::K {})
        )
        .is_some(),
        "the receiver argument must survive the `T | null` parameter"
    );
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    assert!(!rust.contains("__generic"), "got:\n{rust}");
    check_generated_crate("nullable_generic_argument", &rust);
    let stdout = run_generated_crate("nullable_generic_argument", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_field_receiver_optional_chain_argument_with_a_nullable_generic_parameter_is_accepted() {
    const SOURCE: &str = r"class K { pick<T>(n: number, b: T | null): T { return b!; } }
class H { k: K | null; f(next: K | null): K | undefined { this.k = next; const z = this.k?.pick(0, this.k); return z; } }
export function got(next: K | null): K | undefined { const h = new H(); return h.f(next); }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert!(
        field_receiver_nullable_generic::got(&ctx, ::core::option::Option::None).is_none(),
        "an absent field receiver must yield undefined"
    );
    assert!(
        field_receiver_nullable_generic::got(
            &ctx,
            ::core::option::Option::Some(field_receiver_nullable_generic::K {})
        )
        .is_some(),
        "the field receiver argument must survive the `T | null` parameter"
    );
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    assert!(!rust.contains("__generic"), "got:\n{rust}");
    check_generated_crate("field_receiver_nullable_generic", &rust);
    let stdout = run_generated_crate("field_receiver_nullable_generic", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_two_type_parameter_generic_call_specializes_both() {
    const SOURCE: &str = r#"function first<T, U>(a: T, b: U): T { return a; }
export function got(): number { return first(5, "s"); }"#;
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(two_type_parameter_call::got(&ctx), 5.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    assert!(!rust.contains("__generic"), "got:\n{rust}");
    let norm = normalize_rust(&rust);
    assert!(
        norm.contains("fn first_mono_") && norm.contains("a : i64") && norm.contains("JsString"),
        "both type parameters must be specialized from the call arguments; got:\n{rust}"
    );
    check_generated_crate("two_type_parameter_call", &rust);
    let stdout = run_generated_crate("two_type_parameter_call", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_four_level_inherited_generic_method_is_specialized_for_the_deepest_subclass() {
    const SOURCE: &str = r"class A { m<T>(x: T): T { return x; } }
class B extends A { }
class C extends B { }
class D extends C { }
export function got(): number { const d = new D(); const y = d.m(5); return y * 2; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(four_level_inherited_method::got(&ctx), 10.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let norm = normalize_rust(&rust);
    assert!(
        norm.contains("D__m_mono") && norm.contains("let y : i64"),
        "a method inherited four levels down must specialize on the deepest subclass; got:\n{rust}"
    );
    check_generated_crate("four_level_inherited_method", &rust);
    let stdout = run_generated_crate("four_level_inherited_method", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_top_level_generic_generator_specializes_from_its_argument() {
    const SOURCE: &str = r"export function* gen<T>(x: T): Generator<T> { yield x; }
export function got(): number { let s = 0; for (const v of gen(5)) { s = s + v; } return s; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(top_level_generic_generator::got(&ctx), 5.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let norm = normalize_rust(&rust);
    assert!(
        norm.contains("gen_mono") && norm.contains("x : i64"),
        "a top-level generic generator must specialize `T` from its argument; got:\n{rust}"
    );
    check_generated_crate("top_level_generic_generator", &rust);
    let stdout = run_generated_crate("top_level_generic_generator", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}
