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
fn a_super_call_beside_a_user_field_with_the_alias_name_returns_the_parent_value() {
    const SOURCE: &str = "class A { m(): number { return 1; } } \
         class B extends A { __super_1_A_m: number = 2; \
         g(): number { return super.m(); } } \
         export function got(): number { return new B().g(); }";
    let rust = rust_of(SOURCE);
    let norm = compact(&rust);
    assert!(
        norm.contains("fnB____super_1_A_m_2"),
        "a `super.m()` next to a user field with the alias-shaped name must still create its \
         synthetic clone, de-duplicated away from the field; got:\n{rust}"
    );
    assert!(
        norm.contains("pub__super_1_A_m:"),
        "the user's field must keep its name; got:\n{rust}"
    );
    assert!(
        norm.contains("returnB::B____super_1_A_m_2("),
        "`super.m()` must bind to the synthetic clone rather than to the field; got:\n{rust}"
    );
    check_generated_crate("super_field_alias_call", &rust);
    let stdout = run_generated_crate(
        "super_field_alias_call",
        &rust,
        &main_that_asserts("super_field_alias_call", "1.0"),
    );
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_super_call_and_a_user_field_read_with_the_alias_name_stay_distinct() {
    const SOURCE: &str = "class A { m(): number { return 1; } } \
         class B extends A { __super_1_A_m: number = 2; \
         g(): number { return super.m() + this.__super_1_A_m; } } \
         export function got(): number { return new B().g(); }";
    let rust = rust_of(SOURCE);
    let norm = compact(&rust);
    assert!(
        norm.contains("B::B____super_1_A_m_2("),
        "the real `super.m()` must bind to the de-duplicated clone; got:\n{rust}"
    );
    assert!(
        norm.contains("self.__super_1_A_m.borrow().clone()"),
        "the user's `this.__super_1_A_m` read must keep reading the field; got:\n{rust}"
    );
    check_generated_crate("super_field_alias_read", &rust);
    let stdout = run_generated_crate(
        "super_field_alias_read",
        &rust,
        &main_that_asserts("super_field_alias_read", "3.0"),
    );
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_user_call_of_the_alias_shaped_field_is_refused_without_rust() {
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
                "calling the value of a field must be refused with E0418; got {:?}",
                out.diagnostics
            )
        });
    assert!(
        rejected.message.contains("instance field")
            && rejected.message.contains("cannot be called"),
        "the diagnostic must explain that the name is an instance field, not a method; got `{}`",
        rejected.message
    );
    assert!(
        out.rust_source.is_none(),
        "no Rust may be emitted when a field's value is called; got:\n{:?}",
        out.rust_source
    );
}

#[test]
fn a_tuple_element_read_is_refused_without_rust() {
    const SOURCE: &str = "export function got(): number { \
         const rows: [number, number][] = [[1, 2]]; const first = rows[0]; return first[0]; }";
    let out = compile_module(SOURCE);
    let rejected = out
        .diagnostics
        .errors()
        .find(|d| d.code.as_str() == "E0420")
        .unwrap_or_else(|| {
            panic!(
                "reading a tuple element by index must be refused with E0420 instead of emitting \
                 Rust that indexes `()`; got {:?}",
                out.diagnostics
            )
        });
    assert!(
        rejected.message.contains("tuple"),
        "the refusal must name the tuple; got `{}`",
        rejected.message
    );
    assert!(
        out.rust_source.is_none(),
        "no Rust may be emitted when a tuple element is read by index; got:\n{:?}",
        out.rust_source
    );
}

#[test]
fn a_direct_tuple_element_read_is_refused_without_rust() {
    const SOURCE: &str = "export function got(): number { \
         const pair: [number, number] = [1, 2]; return pair[0]; }";
    let out = compile_module(SOURCE);
    assert!(
        out.diagnostics.errors().any(|d| d.code.as_str() == "E0420"),
        "indexing a tuple-typed local must be refused with E0420; got {:?}",
        out.diagnostics
    );
    assert!(
        out.rust_source.is_none(),
        "no Rust may be emitted for a tuple element read; got:\n{:?}",
        out.rust_source
    );
}

#[test]
fn array_literals_of_erased_tuples_still_compile_and_run() {
    const SOURCE: &str = "class A { x: number = 1; } \
         export function got(): number { \
             const plain: [number, number][] = [[1, 2]]; \
             const mixed: [string, A][] = [[\"x\", new A()]]; \
             const empty: [][] = [[]]; \
             const nested: [[number, number], A][] = [[[1, 2], new A()]]; \
             const [p] = plain; const [m] = mixed; const [e] = empty; const [n] = nested; \
             return 17; }";
    let rust = rust_of(SOURCE);
    check_generated_crate("tuple_array_literals_erased", &rust);
    let stdout = run_generated_crate(
        "tuple_array_literals_erased",
        &rust,
        &main_that_asserts("tuple_array_literals_erased", "17.0"),
    );
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}
