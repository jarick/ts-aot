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

fn main_that_asserts(package: &str, expected: &str) -> String {
    format!(
        r#"
use ts_aot_runtime::{{Bump, Ctx, Host}};

struct NoHost;
impl Host for NoHost {{}}

fn main() {{
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx {{ arena: &bump, host: &host }};
    assert_eq!({package}::got(&ctx), {expected});
    println!("ok");
}}
"#
    )
}

#[test]
fn an_inherited_constructor_in_a_three_level_hierarchy_retypes_the_subclass_receiver() {
    const SOURCE: &str = "class A { v: number; constructor(){ this.v = 1 } m(): number { return this.v } } \
         class B extends A { constructor(){ super() } } \
         class C extends B {} \
         export function got(): number { return new C().m(); }";
    let rust = rust_of(SOURCE);
    check_generated_crate("super_ctor_three_level", &rust);
    let stdout = run_generated_crate(
        "super_ctor_three_level",
        &rust,
        &main_that_asserts("super_ctor_three_level", "1.0"),
    );
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn an_inherited_constructor_in_a_four_level_hierarchy_initializes_the_base_field() {
    const SOURCE: &str = "class A { v: number; constructor(){ this.v = 1 } m(): number { return this.v } } \
         class B extends A { constructor(){ super() } } \
         class C extends B {} \
         class D extends C {} \
         export function got(): number { return new D().m(); }";
    let rust = rust_of(SOURCE);
    check_generated_crate("super_ctor_four_level", &rust);
    let stdout = run_generated_crate(
        "super_ctor_four_level",
        &rust,
        &main_that_asserts("super_ctor_four_level", "1.0"),
    );
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_three_level_super_method_call_reaches_the_grandparent_body() {
    const SOURCE: &str = "class A { m(): number { return 111; } } \
         class B extends A { m(): number { return super.m(); } } \
         class C extends B { m(): number { return super.m(); } } \
         export function got(): number { return new C().m(); }";
    let rust = rust_of(SOURCE);
    check_generated_crate("super_method_three_level", &rust);
    let stdout = run_generated_crate(
        "super_method_three_level",
        &rust,
        &main_that_asserts("super_method_three_level", "111.0"),
    );
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_super_call_to_a_generic_parent_method_returns_the_argument() {
    const SOURCE: &str = "class A { m<T>(x: T): T { return x; } } \
         class B extends A { m(): number { return super.m<number>(5); } } \
         export function got(): number { return new B().m(); }";
    let rust = rust_of(SOURCE);
    check_generated_crate("super_method_generic", &rust);
    let stdout = run_generated_crate(
        "super_method_generic",
        &rust,
        &main_that_asserts("super_method_generic", "5.0"),
    );
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}
