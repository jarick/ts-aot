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
    const SOURCE: &str = r"class P { static m<T>(x: T): T { return x; } m<T>(x: T): T { return x; } }
export function got(): number { const p = new P(); const x = P.m(2); const y = p.m(4); return x * 10 + y; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(shared_method_name::got(&ctx), 24.0);
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
