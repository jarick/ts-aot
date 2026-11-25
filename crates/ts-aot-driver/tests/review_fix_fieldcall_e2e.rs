use ts_aot_driver::{CompileOptions, Driver, DriverOutput, EmitStage};
use ts_aot_test_support::{check_generated_crate, run_generated_crate};

fn compile_module(source: &str) -> DriverOutput {
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

fn assert_refused(source: &str, code: &str, message_part: &str) {
    let out = compile_module(source);
    let rejected = out
        .diagnostics
        .iter()
        .find(|d| d.code.as_str() == code)
        .unwrap_or_else(|| panic!("expected {code}; got {:?}", out.diagnostics));
    assert!(
        rejected.message.contains(message_part),
        "the diagnostic must contain `{message_part}`; got `{}`",
        rejected.message
    );
    assert!(
        out.rust_source.is_none(),
        "no Rust may be emitted for a refused call; got:\n{:?}",
        out.rust_source
    );
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

fn runs_to(package: &str, source: &str, expected: &str) {
    let rust = rust_of(source);
    check_generated_crate(package, &rust);
    let stdout = run_generated_crate(package, &rust, &main_that_asserts(package, expected));
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_call_of_an_unresolved_field_value_is_refused_without_rust() {
    const SOURCE: &str = "class C { f: Missing = 1; g(): number { return this.f(); } } \
         export function got(): number { return new C().g(); }";
    assert_refused(SOURCE, "E0418", "instance field");
}

#[test]
fn a_call_of_an_untyped_field_value_is_refused_without_rust() {
    const SOURCE: &str = "class C { f; constructor() { this.f = (): number => 5; } \
         g(): number { return this.f(); } } \
         export function got(): number { return new C().g(); }";
    assert_refused(SOURCE, "E0418", "instance field");
}

#[test]
fn a_call_of_a_number_valued_object_literal_field_is_refused_without_rust() {
    const SOURCE: &str = "export function got(): number { return ({ f: 1 }).f(); }";
    assert_refused(SOURCE, "E0418", "instance field");
}

#[test]
fn a_call_of_an_object_literal_closure_field_returns_the_node_value() {
    const SOURCE: &str = "export function got(): number { return ({ f: (): number => 5 }).f(); }";
    runs_to("fieldcall_object_literal_closure", SOURCE, "5.0");
}

#[test]
fn a_call_of_a_locally_bound_object_literal_closure_field_returns_the_node_value() {
    const SOURCE: &str = "export function got(): number { \
         const o = { f: (): number => 7 }; return o.f(); }";
    runs_to("fieldcall_local_object_literal_closure", SOURCE, "7.0");
}

#[test]
fn a_call_of_a_closure_field_initialized_in_the_field_is_refused_without_rust() {
    const SOURCE: &str = "class C { f: () => number = (): number => 5; \
         g(): number { return this.f(); } } \
         export function got(): number { return new C().g(); }";
    assert_refused(SOURCE, "E0421", "unset function value");
}

#[test]
fn a_call_of_an_alias_shaped_closure_field_initialized_in_the_field_is_refused_without_rust() {
    const SOURCE: &str = "class A { m(): number { return 1; } } \
         class B extends A { __super_1_A_m: () => number = (): number => 3; \
         g(): number { return this.__super_1_A_m(); } } \
         export function got(): number { return new B().g(); }";
    assert_refused(SOURCE, "E0421", "unset function value");
}

#[test]
fn a_call_of_a_closure_field_assigned_in_the_constructor_returns_the_node_value() {
    const SOURCE: &str = "class C { f: () => number; \
         constructor() { this.f = (): number => 5; } \
         g(): number { return this.f(); } } \
         export function got(): number { return new C().g(); }";
    runs_to("fieldcall_constructor_closure", SOURCE, "5.0");
}

#[test]
fn a_call_of_a_closure_field_assigned_in_a_parent_constructor_returns_the_node_value() {
    const SOURCE: &str = "class A { f: () => number; \
         constructor() { this.f = (): number => 6; } } \
         class B extends A { g(): number { return this.f(); } } \
         export function got(): number { return new B().g(); }";
    runs_to("fieldcall_parent_constructor_closure", SOURCE, "6.0");
}

#[test]
fn a_duplicate_field_and_method_named_like_a_super_alias_is_refused_without_rust() {
    const SOURCE: &str = "class A { m(): number { return 1; } } \
         class B extends A { __super_1_A_m: number = 2; \
         __super_1_A_m(): number { return 4; } \
         g(): number { return this.__super_1_A_m(); } } \
         export function got(): number { return new B().g(); }";
    assert_refused(SOURCE, "E0418", "instance field");
}
