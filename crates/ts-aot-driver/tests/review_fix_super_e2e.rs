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

fn compact(rust: &str) -> String {
    rust.chars().filter(|c| !c.is_whitespace()).collect()
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
fn a_user_method_named_like_a_super_alias_does_not_shadow_the_real_super_clone() {
    const SOURCE: &str = "class A { m(): number { return 1; } } \
         class B extends A { __super_1_A_m(): number { return 2; } \
         g(): number { return super.m(); } } \
         export function got(): number { return new B().g(); }";
    let rust = rust_of(SOURCE);
    let norm = compact(&rust);
    assert!(
        norm.contains("fnB____super_1_A_m_2"),
        "a real `super.m()` must still create its synthetic clone, de-duplicated away from the \
         user method that happens to have the alias-shaped name; got:\n{rust}"
    );
    assert!(
        norm.contains("returnB::B____super_1_A_m_2("),
        "`super.m()` must bind to the synthetic clone rather than to the user's method of the \
         same alias-shaped name; got:\n{rust}"
    );
    check_generated_crate("review_fix_super_alias_shadow", &rust);
    let stdout = run_generated_crate(
        "review_fix_super_alias_shadow",
        &rust,
        &main_that_asserts("review_fix_super_alias_shadow", "1.0"),
    );
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_user_method_with_a_different_return_type_does_not_shadow_the_real_super_clone() {
    const SOURCE: &str = "class A { m(): number { return 1; } } \
         class B extends A { __super_1_A_m(): string { return \"user\"; } \
         g(): number { return super.m(); } } \
         export function got(): number { return new B().g(); }";
    let rust = rust_of(SOURCE);
    let norm = compact(&rust);
    assert!(
        norm.contains("fnB____super_1_A_m_2"),
        "the clone must be created even when the user method with the alias-shaped name returns a \
         different type, because the frontend types the super call site with the parent return \
         type; got:\n{rust}"
    );
    assert!(
        norm.contains("returnB::B____super_1_A_m_2("),
        "`super.m()` must still bind to the clone when the user method's return type differs; \
         got:\n{rust}"
    );
    check_generated_crate("review_fix_super_alias_diff_ret", &rust);
    let stdout = run_generated_crate(
        "review_fix_super_alias_diff_ret",
        &rust,
        &main_that_asserts("review_fix_super_alias_diff_ret", "1.0"),
    );
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_user_method_named_like_a_super_alias_does_not_shadow_a_three_level_clone() {
    const SOURCE: &str = "class A { m(): number { return 1; } } \
         class B extends A { __super_1_A_m(): number { return 2; } } \
         class C extends B { g(): number { return super.m(); } } \
         export function got(): number { return new C().g(); }";
    let rust = rust_of(SOURCE);
    let norm = compact(&rust);
    assert!(
        norm.contains("fnC____super_1_A_m_2"),
        "a `super.m()` in a grandchild class must still create its synthetic clone when an \
         intermediate ancestor declares a member with the alias-shaped name; got:\n{rust}"
    );
    assert!(
        norm.contains("returnC::C____super_1_A_m_2("),
        "the grandchild's `super.m()` must bind to the synthetic clone; got:\n{rust}"
    );
    check_generated_crate("review_fix_super_alias_shadow_three_level", &rust);
    let stdout = run_generated_crate(
        "review_fix_super_alias_shadow_three_level",
        &rust,
        &main_that_asserts("review_fix_super_alias_shadow_three_level", "1.0"),
    );
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_real_super_call_and_a_user_call_of_the_alias_shaped_name_stay_distinct() {
    const SOURCE: &str = "class A { m(): number { return 1; } } \
         class B extends A { __super_1_A_m(): number { return 2; } \
         g(): number { return super.m() + this.__super_1_A_m(); } } \
         export function got(): number { return new B().g(); }";
    let rust = rust_of(SOURCE);
    let norm = compact(&rust);
    assert!(
        norm.contains("B::B____super_1_A_m_2("),
        "the real `super.m()` must bind to the synthetic clone; got:\n{rust}"
    );
    assert!(
        norm.contains("B::B____super_1_A_m("),
        "the genuine `this.__super_1_A_m()` call must keep binding to the user's method; \
         got:\n{rust}"
    );
    check_generated_crate("review_fix_super_alias_both_calls", &rust);
    let stdout = run_generated_crate(
        "review_fix_super_alias_both_calls",
        &rust,
        &main_that_asserts("review_fix_super_alias_both_calls", "3.0"),
    );
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_user_call_of_a_field_named_like_a_super_alias_is_rejected_without_rust() {
    const SOURCE: &str = "class A { m(): number { return 1; } } \
         class B extends A { __super_1_A_m: number = 2; \
         g(): number { return this.__super_1_A_m(); } } \
         export function got(): number { return new B().g(); }";
    let out = compile_module(SOURCE);
    let rejected = out
        .diagnostics
        .iter()
        .find(|d| d.code.as_str() == "E0418")
        .unwrap_or_else(|| {
            panic!(
                "calling the value of a field named like a super alias must be refused with \
                 E0418; got {:?}",
                out.diagnostics
            )
        });
    assert!(
        rejected.message.contains("instance field")
            && rejected.message.contains("cannot be called"),
        "the diagnostic must explain that the name is an instance field, not a method; got \
         `{}`",
        rejected.message
    );
    assert!(
        out.rust_source.is_none(),
        "no Rust may be emitted when a field's value is called; got:\n{:?}",
        out.rust_source
    );
}

#[test]
fn a_shadowed_super_clone_for_a_receiver_returning_method_keeps_the_subclass_result() {
    const SOURCE: &str = "class A { v: number = 0; self(): A { this.v = 5; return this; } } \
         class B extends A { __super_1_A_self(): number { return 9; } \
         g(): number { const x = super.self(); return x.v; } } \
         export function got(): number { return new B().g(); }";
    let rust = rust_of(SOURCE);
    let norm = compact(&rust);
    assert!(
        norm.contains("fnB____super_1_A_self_2"),
        "the synthetic clone of the receiver-returning parent method must be de-duplicated from \
         the user method with the alias-shaped name; got:\n{rust}"
    );
    assert!(
        norm.contains("letx:B="),
        "the local bound to `super.self()` must take the shadowed clone's subclass ret; \
         got:\n{rust}"
    );
    assert!(
        norm.contains("B::B____super_1_A_self_2("),
        "`super.self()` must call the de-duplicated clone, not the user method with the \
         alias-shaped name; got:\n{rust}"
    );
    check_generated_crate("review_fix_super_alias_receiver_ret", &rust);
    let stdout = run_generated_crate(
        "review_fix_super_alias_receiver_ret",
        &rust,
        &main_that_asserts("review_fix_super_alias_receiver_ret", "5.0"),
    );
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_super_call_to_a_receiver_returning_method_binds_the_subclass_result() {
    const SOURCE: &str = "class A { v: number = 0; self(): A { this.v = 7; return this; } } \
         class B extends A { g(): number { const x = super.self(); return x.v; } } \
         export function got(): number { return new B().g(); }";
    let rust = rust_of(SOURCE);
    let norm = compact(&rust);
    assert!(
        norm.contains("letx:B="),
        "the local bound to `super.self()` must take the retyped clone's subclass type, because \
         the clone returns the retyped receiver; got:\n{rust}"
    );
    assert!(
        !norm.contains("letx:A="),
        "the local bound to `super.self()` must not keep the parent type the frontend recorded \
         for the parent method; got:\n{rust}"
    );
    check_generated_crate("review_fix_super_receiver_ret", &rust);
    let stdout = run_generated_crate(
        "review_fix_super_receiver_ret",
        &rust,
        &main_that_asserts("review_fix_super_receiver_ret", "7.0"),
    );
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}
