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
    main_that_calls(package, expected, "")
}

fn main_that_passes_unit(package: &str, expected: &str) -> String {
    main_that_calls(package, expected, ", ()")
}

fn main_that_calls(package: &str, expected: &str, extra_arg: &str) -> String {
    format!(
        r#"
use ts_aot_runtime::{{Bump, Ctx, Host}};

struct NoHost;
impl Host for NoHost {{}}

fn main() {{
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx {{ arena: &bump, host: &host }};
    assert_eq!({package}::got(&ctx{extra_arg}), {expected});
    println!("ok");
}}
"#
    )
}

#[test]
fn a_tuple_of_default_capable_elements_keeps_the_or_default_array_read() {
    const SOURCE: &str = "type U = number | string; \
         export function got(u: U): number { \
         const rows: [U, U][] = [u, u]; \
         const [a] = rows; \
         return 11; }";
    let rust = rust_of(SOURCE);
    let norm = compact(&rust);
    assert!(
        norm.contains("__ts_aot_array_get_or_default::<()>("),
        "a tuple whose every element implements `Default` must keep the infallible `_or_default` \
         read; got:\n{rust}"
    );
    assert!(
        !norm.contains("unreachable!("),
        "no fallible read may be emitted for a tuple whose every element implements `Default`; \
         got:\n{rust}"
    );
    check_generated_crate("tuple_default_read", &rust);
    let stdout = run_generated_crate(
        "tuple_default_read",
        &rust,
        &main_that_passes_unit("tuple_default_read", "11.0"),
    );
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_tuple_with_a_class_element_takes_the_fallible_array_read() {
    const SOURCE: &str = "class A { v: number = 1; } \
         type W = A | number; \
         export function got(w: W): number { \
         const rows: [A, number][] = [w, w]; \
         const [a] = rows; \
         return 12; }";
    let rust = rust_of(SOURCE);
    let norm = compact(&rust);
    assert!(
        norm.contains("match__ts_aot_array_get::<()>("),
        "a tuple with a class element has no Rust `Default`, so its element read must go through \
         the fallible accessor instead of a `Default` bound that cannot be satisfied; got:\n{rust}"
    );
    assert!(
        norm.contains("unreachable!("),
        "the fallible read must handle the absent element explicitly; got:\n{rust}"
    );
    assert!(
        !norm.contains("__ts_aot_array_get_or_default"),
        "a tuple with a class element must not be read through a `Default` bound; got:\n{rust}"
    );
    check_generated_crate("tuple_class_read", &rust);
    let stdout = run_generated_crate(
        "tuple_class_read",
        &rust,
        &main_that_passes_unit("tuple_class_read", "12.0"),
    );
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_class_array_element_takes_the_fallible_array_read() {
    const SOURCE: &str = "class A { v: number = 1; } \
         function f(): number { \
         const rows: A[] = [new A()]; \
         const [a] = rows; \
         return a.v; } \
         export function got(): number { return f(); }";
    let rust = rust_of(SOURCE);
    let norm = compact(&rust);
    assert!(
        norm.contains("match__ts_aot_array_get::<A>("),
        "a class element has no Rust `Default`, so its element read must go through the fallible \
         accessor; got:\n{rust}"
    );
    assert!(
        !norm.contains("__ts_aot_array_get_or_default"),
        "a class element must not be read through a `Default` bound; got:\n{rust}"
    );
    check_generated_crate("class_element_read", &rust);
    let stdout = run_generated_crate(
        "class_element_read",
        &rust,
        &main_that_asserts("class_element_read", "1.0"),
    );
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_user_method_named_like_a_super_alias_is_not_replaced_by_a_parent_clone() {
    const SOURCE: &str = "class A { m(): number { return 5; } } \
         class B extends A { \
         __super_1_A_m(): number { return 7; } \
         g(): number { return this.__super_1_A_m(); } } \
         export function got(): number { return new B().g(); }";
    let rust = rust_of(SOURCE);
    let norm = compact(&rust);
    assert_eq!(
        norm.matches("fnB____super_1_A_m").count(),
        1,
        "a declared user method whose name only has the `__super_` shape must not be joined by a \
         synthetic clone of the same name that shadows it; got:\n{rust}"
    );
    check_generated_crate("alias_shaped_user_method", &rust);
    let stdout = run_generated_crate(
        "alias_shaped_user_method",
        &rust,
        &main_that_asserts("alias_shaped_user_method", "7.0"),
    );
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn an_inherited_method_that_returns_the_receiver_returns_the_subclass() {
    const SOURCE: &str = "class A { v(): number { return 3; } self(): A { return this; } } \
         class B extends A {} \
         export function got(): number { return 3; }";
    let rust = rust_of(SOURCE);
    let norm = compact(&rust);
    assert!(
        norm.contains("fnB__self<'a>(self,__ctx:&ts_aot_runtime::Ctx<'a>,)->B{returnself;}"),
        "the inherited copy of a method that returns the receiver must declare the subclass as its \
         return type, so its declaration and its returned value agree; got:\n{rust}"
    );
    check_generated_crate("inherited_self_return", &rust);
    let stdout = run_generated_crate(
        "inherited_self_return",
        &rust,
        &main_that_asserts("inherited_self_return", "3.0"),
    );
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn an_inherited_method_that_returns_a_new_parent_instance_keeps_the_parent_return_type() {
    const SOURCE: &str = "class A { v: number = 1; make(): A { return new A(); } } \
         class B extends A {} \
         export function got(): number { return new B().make().v; }";
    let rust = rust_of(SOURCE);
    let norm = compact(&rust);
    assert!(
        norm.contains("fnB__make<'a>(self,__ctx:&ts_aot_runtime::Ctx<'a>,)->A{"),
        "the inherited copy of a method that returns a fresh parent instance must keep the parent \
         as its return type, because the returned value is not the receiver; got:\n{rust}"
    );
    check_generated_crate("inherited_fresh_parent_return", &rust);
    let stdout = run_generated_crate(
        "inherited_fresh_parent_return",
        &rust,
        &main_that_asserts("inherited_fresh_parent_return", "1.0"),
    );
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_conditional_of_two_receivers_may_write_a_receiver_bound_local() {
    const SOURCE: &str = "class A { v: number; constructor() { this.v = 3; } \
         m(c: boolean): number { \
         let b = this; \
         b = c ? this : this; \
         let d = c ? this : this; \
         return b.v + d.v; } } \
         class B extends A { constructor() { super(); } \
         m(c: boolean): number { return super.m(c) * 2; } } \
         export function got(): number { return new B().m(true); }";
    let rust = rust_of(SOURCE);
    check_generated_crate("conditional_receiver", &rust);
    let stdout = run_generated_crate(
        "conditional_receiver",
        &rust,
        &main_that_asserts("conditional_receiver", "12.0"),
    );
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_conditional_with_a_non_receiver_branch_cannot_write_a_receiver_bound_local() {
    const SOURCE: &str = "class A { v: number; constructor() { this.v = 3; } \
         m(c: boolean): number { \
         let b = this; \
         b = c ? this : new A(); \
         return b.v; } } \
         class B extends A { constructor() { super(); } \
         m(c: boolean): number { return super.m(c) * 2; } } \
         export function got(): number { return new B().m(true); }";
    let out = compile_module(SOURCE);
    let rejected = out
        .diagnostics
        .errors()
        .find(|d| d.code.as_str() == "P0024")
        .unwrap_or_else(|| {
            panic!(
                "a conditional whose other branch produces a foreign instance must stay rejected; \
                 got {:?}",
                out.diagnostics
            )
        });
    assert!(
        rejected.message.contains("receiver"),
        "the diagnostic must explain that the binding cannot hold a non-receiver value; got `{}`",
        rejected.message
    );
    assert!(
        out.rust_source.is_none(),
        "no Rust may be produced when a receiver-bound local is assigned a foreign instance; got:\n{:?}",
        out.rust_source
    );
}
