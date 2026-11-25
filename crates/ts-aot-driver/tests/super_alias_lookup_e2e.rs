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
fn a_super_alias_that_names_only_a_static_method_is_rejected_without_a_wrong_arity_call() {
    const SOURCE: &str = "class A { static m(x: number): number { return x; } } \
         class C {} \
         class B extends C { g(): number { return this.__super_1_A_m(5); } } \
         export function got(): number { return new B().g(); }";
    let out = compile_module(SOURCE);
    assert!(
        out.has_errors(),
        "a super alias that resolves only to a static method has no instance receiver and must \
         not be lowered; got {:?}",
        out.diagnostics
    );
    assert!(
        out.rust_source.is_none(),
        "must not emit Rust for a super alias that cannot be inherited; got:\n{:?}",
        out.rust_source
    );
    let rejected = out
        .diagnostics
        .iter()
        .find(|d| d.code.as_str() == "P0023")
        .unwrap_or_else(|| {
            panic!(
                "expected a P0023 diagnostic naming the static method; got {:?}",
                out.diagnostics
            )
        });
    assert!(
        rejected.message.contains("static method") && rejected.message.contains("instance"),
        "the diagnostic must explain that a static method has no instance receiver; got `{}`",
        rejected.message
    );
    assert!(
        !out.diagnostics.iter().any(|d| d.code.as_str() == "P0004"),
        "dropping the clone must not leave a wrong-arity call behind; got {:?}",
        out.diagnostics
    );
}

#[test]
fn a_super_call_uses_the_instance_method_over_a_static_same_name() {
    const SOURCE: &str = "class A { static tag(): number { return 1; } tag(): number { return 2; } } \
         class B extends A { tag(): number { return super.tag() + 10; } } \
         export function got(): number { return new B().tag(); }";
    let rust = rust_of(SOURCE);
    let norm = compact(&rust);
    assert!(
        norm.contains("fnB____super_1_A_tag<'a>(self,"),
        "`super.tag()` must clone the parent's instance method as a method on the subclass \
         receiver, not the static method; got:\n{rust}"
    );
    check_generated_crate("super_alias_static_instance", &rust);
    let stdout = run_generated_crate(
        "super_alias_static_instance",
        &rust,
        &main_that_asserts("super_alias_static_instance", "12.0"),
    );
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_super_call_skips_a_static_method_to_reach_the_grandparent_instance_method() {
    const SOURCE: &str = "class A { m(): number { return 111; } } \
         class B extends A { static m(): number { return 5; } } \
         class C extends B { m(): number { return super.m(); } } \
         export function got(): number { return new C().m(); }";
    let rust = rust_of(SOURCE);
    check_generated_crate("super_alias_skip_static", &rust);
    let stdout = run_generated_crate(
        "super_alias_skip_static",
        &rust,
        &main_that_asserts("super_alias_skip_static", "111.0"),
    );
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_super_method_clone_retypes_the_this_local_and_leaves_a_nested_closure_local_alone() {
    const SOURCE: &str = "class A { v: number; constructor() { this.v = 3; } \
         m(): number { const inc = (): number => { let d: number = 4; return d; }; \
         return this.v + inc(); } } \
         class B extends A { m(): number { return super.m() * 2; } } \
         export function got(): number { return new B().m(); }";
    let rust = rust_of(SOURCE);
    let norm = compact(&rust);
    assert!(
        norm.contains("fnB____super_1_A_m<'a>(self,"),
        "the inherited super clone must keep the instance receiver so that the subclass \
         receiver is retyped; got:\n{rust}"
    );
    assert_eq!(
        norm.matches("letd:f64=4.0;").count(),
        2,
        "a local of the nested closure must keep its own declared type in both the parent method \
         and its super clone; got:\n{rust}"
    );
    check_generated_crate("super_alias_nested_closure", &rust);
    let stdout = run_generated_crate(
        "super_alias_nested_closure",
        &rust,
        &main_that_asserts("super_alias_nested_closure", "14.0"),
    );
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_super_method_clone_retypes_a_local_bound_to_the_receiver_and_its_uses() {
    const SOURCE: &str = "class A { v: number; constructor() { this.v = 3; } \
         bump(): number { return this.v + 1; } \
         m(): number { let b = this; let other: A = new A(); \
         const inc = (): number => { let d: number = 4; return d; }; \
         return b.v + b.bump() + other.v + inc(); } } \
         class B extends A { m(): number { return super.m() * 2; } } \
         export function got(): number { return new B().m(); }";
    let rust = rust_of(SOURCE);
    let norm = compact(&rust);
    assert!(
        norm.contains("fnB____super_1_A_m<'a>(self,"),
        "the inherited super clone must keep the instance receiver so that the subclass \
         receiver is retyped; got:\n{rust}"
    );
    assert!(
        norm.contains("letb:B=self.clone();"),
        "a local bound to the super-clone receiver must be retyped to the subclass together with \
         its uses, otherwise the emitted crate fails to compile; got:\n{rust}"
    );
    assert!(
        norm.contains("letother:A=__tmp"),
        "an unrelated class-typed local must keep its declared type; got:\n{rust}"
    );
    assert_eq!(
        norm.matches("letd:f64=4.0;").count(),
        2,
        "a numeric local of the nested closure must keep its own declared type in both the parent \
         method and its super clone; got:\n{rust}"
    );
    check_generated_crate("super_alias_receiver_local", &rust);
    let stdout = run_generated_crate(
        "super_alias_receiver_local",
        &rust,
        &main_that_asserts("super_alias_receiver_local", "28.0"),
    );
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_super_method_clone_retypes_a_receiver_local_inside_a_capturing_closure() {
    const SOURCE: &str = "class A { v: number; constructor() { this.v = 3; } \
         m(): number { const f = (): number => { let b = this; return b.v; }; return f(); } } \
         class B extends A { m(): number { return super.m() * 2; } } \
         export function got(): number { return new B().m(); }";
    let rust = rust_of(SOURCE);
    let norm = compact(&rust);
    assert!(
        norm.contains("letb:B=__env"),
        "a local bound to a closure-captured receiver must be retyped to the subclass in the \
         super clone; got:\n{rust}"
    );
    assert!(
        norm.contains("letb:A=__env"),
        "the original parent method must keep its own receiver type; got:\n{rust}"
    );
    check_generated_crate("super_alias_closure_receiver_local", &rust);
    let stdout = run_generated_crate(
        "super_alias_closure_receiver_local",
        &rust,
        &main_that_asserts("super_alias_closure_receiver_local", "6.0"),
    );
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_super_method_clone_retypes_the_narrowing_local_derived_from_a_receiver_local() {
    const SOURCE: &str = "class A { v: number = 3; m(): number { let b = this; b = b; return b.v; } } \
         class B extends A { m(): number { return super.m() * 2; } } \
         export function got(): number { return new B().m(); }";
    let rust = rust_of(SOURCE);
    let norm = compact(&rust);
    assert!(
        norm.contains("let_narrowed_0:B=b.clone();"),
        "the narrowing local the frontend derives from a receiver local must be retyped to the \
         subclass together with the storage local, otherwise the clone emits `let _narrowed_0 : A \
         = b . clone ()` while `b : B`; got:\n{rust}"
    );
    check_generated_crate("super_alias_narrowed_receiver", &rust);
}

#[test]
fn a_super_method_clone_retypes_the_narrowing_local_and_keeps_the_receiver_value() {
    const SOURCE: &str = "class A { v: number; constructor() { this.v = 3; } \
         m(): number { let b = this; b = b; return b.v; } } \
         class B extends A { m(): number { return super.m() * 2; } } \
         export function got(): number { return new B().m(); }";
    let rust = rust_of(SOURCE);
    check_generated_crate("super_alias_narrowed_value", &rust);
    let stdout = run_generated_crate(
        "super_alias_narrowed_value",
        &rust,
        &main_that_asserts("super_alias_narrowed_value", "6.0"),
    );
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_super_method_clone_keeps_receiver_derived_aliases_consistent() {
    const SOURCE: &str = "class A { v: number; constructor() { this.v = 3; } \
         k(): number { return this.v + 1; } \
         m(): number { let b = this; b = b; let c = b; c = b; return b.k() + c.v; } } \
         class B extends A { m(): number { return super.m() * 2; } } \
         export function got(): number { return new B().m(); }";
    let rust = rust_of(SOURCE);
    check_generated_crate("super_alias_chain", &rust);
    let stdout = run_generated_crate(
        "super_alias_chain",
        &rust,
        &main_that_asserts("super_alias_chain", "14.0"),
    );
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_super_method_clone_rejects_a_receiver_local_reassigned_to_a_foreign_instance() {
    for (label, body) in [
        (
            "the storage local",
            "let b = this; b = new A(); return b.v;",
        ),
        (
            "a derived alias",
            "let b = this; let c = b; c = new A(); return c.v;",
        ),
    ] {
        let source = format!(
            "class A {{ v: number = 3; m(): number {{ {body} }} }} \
             class B extends A {{ m(): number {{ return super.m() * 2; }} }} \
             export function got(): number {{ return new B().m(); }}"
        );
        let out = compile_module(&source);
        assert!(
            out.diagnostics
                .errors()
                .any(|d| d.code.as_str() == "P0024" && d.message.contains("receiver")),
            "{label} bound to the receiver cannot be reassigned to a parent-class instance, \
             because the generated Rust represents the subclass and its parent as distinct structs; \
             got {:?}",
            out.diagnostics
        );
        assert!(
            out.rust_source.is_none(),
            "no Rust may be produced when {label} is reassigned to a foreign instance; got:\n{:?}",
            out.rust_source
        );
    }
}
