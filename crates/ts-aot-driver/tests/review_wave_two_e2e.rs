use ts_aot_driver::{CompileOptions, Driver, EmitStage};
use ts_aot_test_support::{check_generated_crate, run_generated_crate};

fn compile(source: &str) -> ts_aot_driver::DriverOutput {
    Driver::new().compile_source(
        "test.ts",
        source,
        &CompileOptions {
            emit: EmitStage::Rust,
            module: true,
        },
    )
}

fn codes(out: &ts_aot_driver::DriverOutput) -> Vec<&str> {
    out.diagnostics
        .iter()
        .map(|diagnostic| diagnostic.code.as_str())
        .collect()
}

fn compact(rust: &str) -> String {
    rust.chars().filter(|c| !c.is_whitespace()).collect()
}

#[test]
fn a_throwing_optional_chain_method_keeps_its_own_optional_result() {
    const SOURCE: &str = "class A { \
         m(flag: boolean): number | undefined { if (flag) { throw 1; } return undefined; } \
         v(flag: boolean): number | undefined { if (flag) { throw 1; } return 7; } } \
         export function opt(a: A | null, flag: boolean): number | undefined { return a?.m(flag); } \
         export function optValue(a: A | null, flag: boolean): number | undefined { \
             return a?.v(flag); }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(review_wave_two_optional::opt(&ctx, ::core::option::Option::None, false), Ok(::core::option::Option::None));
    assert_eq!(review_wave_two_optional::opt(&ctx, ::core::option::Option::Some(review_wave_two_optional::A {}), false), Ok(::core::option::Option::None));
    assert_eq!(review_wave_two_optional::opt(&ctx, ::core::option::Option::Some(review_wave_two_optional::A {}), true), Err(1.0));
    assert_eq!(review_wave_two_optional::optValue(&ctx, ::core::option::Option::None, false), Ok(::core::option::Option::None));
    assert_eq!(review_wave_two_optional::optValue(&ctx, ::core::option::Option::Some(review_wave_two_optional::A {}), false), Ok(::core::option::Option::Some(7.0)));
    assert_eq!(review_wave_two_optional::optValue(&ctx, ::core::option::Option::Some(review_wave_two_optional::A {}), true), Err(1.0));
    println!("ok");
}
"#;
    let out = compile(SOURCE);
    assert!(
        !out.has_errors(),
        "a `number | undefined` throwing method through an optional chain must compile; got {:?}",
        out.diagnostics
    );
    let rust = out
        .rust_source
        .expect("emit-rust must populate rust_source");
    let norm = compact(&rust);
    assert!(
        norm.contains("Option<Result<Option<f64>,f64>>"),
        "the transposed binding must keep the method's own optional result as the success \
         payload; got:\n{rust}"
    );
    assert!(
        norm.contains(".unwrap()?"),
        "the inner `Result` must be propagated inside the `Some` arm; got:\n{rust}"
    );
    check_generated_crate("review_wave_two_optional", &rust);
    let stdout = run_generated_crate("review_wave_two_optional", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_direct_callback_that_reassigns_the_receiver_is_refused() {
    const SOURCE: &str = "class C { x: number = 0; \
         m(cb: () => number): number { this.x = 1; return cb(); } } \
         export function direct(cs: C[]): number { \
             return cs[0].m(() => { cs[0] = new C(); return 0; }); }";
    let out = compile(SOURCE);
    assert!(
        out.has_errors(),
        "a callback argument that reassigns the receiver place must be refused; got {:?}",
        out.diagnostics
    );
    let diagnostic = out
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code.as_str() == "E0419")
        .unwrap_or_else(|| panic!("expected E0419; got {:?}", out.diagnostics));
    assert!(
        diagnostic.message.contains("may reassign"),
        "the E0419 must explain the receiver reassignment; got: {}",
        diagnostic.message
    );
    assert!(
        out.rust_source.is_none(),
        "no Rust may be produced for a refused callback receiver write; got:\n{:?}",
        out.rust_source
    );
}

#[test]
fn a_callback_expanded_inside_a_callee_that_reassigns_the_receiver_is_refused() {
    const SOURCE: &str = "class C { x: number = 0; \
         m(cb: () => number): number { this.x = 1; return cb(); } } \
         function invoke(cb: () => number): number { return cb(); } \
         export function nested(cs: C[]): number { \
             return cs[0].m(invoke(() => { cs[0] = new C(); return 0; })); }";
    let out = compile(SOURCE);
    assert!(
        out.has_errors(),
        "a callback expanded inside a callee that reassigns the receiver place must be \
         refused; got {:?}",
        out.diagnostics
    );
    let diagnostic = out
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code.as_str() == "E0419")
        .unwrap_or_else(|| panic!("expected E0419; got {:?}", out.diagnostics));
    assert!(
        diagnostic.message.contains("may reassign"),
        "the E0419 must explain the receiver reassignment; got: {}",
        diagnostic.message
    );
    assert!(
        out.rust_source.is_none(),
        "no Rust may be produced for a refused callback receiver write; got:\n{:?}",
        out.rust_source
    );
}

#[test]
fn the_recursive_receiver_write_scan_terminates_and_keeps_the_receiver() {
    const SOURCE: &str = "class Node { children: Node[]; x: number = 0; \
         walk(n: number): number { if (n > 0) { this.children[0].walk(n - 1); } \
             this.x = 1; return this.x; } } \
         export function f(): number { const box: Node[] = []; box.push(new Node()); \
             return box[0].walk(0); }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(review_wave_two_recursive::f(&ctx), 1.0);
    println!("ok");
}
"#;
    let out = compile(SOURCE);
    assert!(
        !out.has_errors(),
        "a recursive `this.field[].method()` path must terminate the receiver analysis; got {:?}",
        out.diagnostics
    );
    let rust = out
        .rust_source
        .expect("emit-rust must populate rust_source");
    check_generated_crate("review_wave_two_recursive", &rust);
    let stdout = run_generated_crate("review_wave_two_recursive", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_generator_reference_before_its_declaration_is_reported_from_the_lifted_body() {
    const SOURCE: &str = "export function outer(): i64 { \
         function* first(): i64 { second(); yield 1; return 0; } \
         function* second(): i64 { yield 1; return 0; } \
         const f = first(); return 0; }";
    let out = compile(SOURCE);
    assert!(
        out.has_errors(),
        "a local generator referenced before its declaration must be refused; got {:?}",
        out.diagnostics
    );
    let diagnostic = out
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code.as_str() == "E0500")
        .unwrap_or_else(|| panic!("expected E0500; got {:?}", out.diagnostics));
    assert!(
        diagnostic
            .message
            .contains("referenced before its declaration"),
        "the E0500 must explain the forward reference; got: {}",
        diagnostic.message
    );
    assert!(
        out.rust_source.is_none(),
        "no Rust may be produced for an unresolved local generator reference; got:\n{:?}",
        out.rust_source
    );
}

#[test]
fn a_parameter_shadows_an_inherited_class_in_extends() {
    const SOURCE: &str = "export function outer(): number { \
         class A { m(): number { return 1; } } \
         function inner(A: number): number { class B extends A { } return A; } \
         return inner(2); }";
    let out = compile(SOURCE);
    assert!(
        out.has_errors(),
        "a parameter named like an inherited class must shadow it in `extends`; got {:?}",
        out.diagnostics
    );
    let diagnostic = out
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code.as_str() == "E0300")
        .unwrap_or_else(|| panic!("expected E0300; got {:?}", out.diagnostics));
    assert!(
        diagnostic.message.contains("extends"),
        "the E0300 must name the unresolved `extends`; got: {}",
        diagnostic.message
    );
    assert!(
        out.rust_source.is_none(),
        "no Rust may be produced for a shadowed `extends` parent; got:\n{:?}",
        out.rust_source
    );
}

#[test]
fn an_intervening_value_binding_shadows_an_inherited_class_in_a_closure() {
    const SOURCE: &str = "export function outer(): number { \
         class A { m(): number { return 1; } } let result = 0; \
         { const A = 5; const f = (): number => { class B extends A { } return 1; }; \
           result = f() + A; } \
         return result; }";
    let out = compile(SOURCE);
    assert!(
        out.has_errors(),
        "a value binding between a closure and a class must shadow the inherited class; got {:?}",
        out.diagnostics
    );
    let diagnostic = out
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code.as_str() == "E0300")
        .unwrap_or_else(|| panic!("expected E0300; got {:?}", out.diagnostics));
    assert!(
        diagnostic.message.contains("extends"),
        "the E0300 must name the unresolved `extends`; got: {}",
        diagnostic.message
    );
    assert!(
        out.rust_source.is_none(),
        "no Rust may be produced for a shadowed `extends` parent; got:\n{:?}",
        out.rust_source
    );
}

#[test]
fn an_unshadowed_inherited_class_still_resolves_inside_a_closure() {
    const SOURCE: &str = "export function okInherited(): number { \
         class A { m(): number { return 1; } } \
         const f = (): number => { class B extends A { } const b = new B(); return b.m(); }; \
         return f(); } \
         export function okLocal(): number { \
         class A { m(): number { return 1; } } \
         class B extends A { n(): number { return this.m() + 2; } } \
         const b = new B(); return b.n(); }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(review_wave_two_control::okInherited(&ctx), 1.0);
    assert_eq!(review_wave_two_control::okLocal(&ctx), 3.0);
    println!("ok");
}
"#;
    let out = compile(SOURCE);
    assert_eq!(
        codes(&out),
        ["P0005"],
        "an unshadowed local class parent must still resolve without an E0300 shadowing \
         diagnostic and without any other unexpected code; got {:?}",
        out.diagnostics
    );
    let rust = out
        .rust_source
        .expect("emit-rust must populate rust_source");
    check_generated_crate("review_wave_two_control", &rust);
    let stdout = run_generated_crate("review_wave_two_control", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}
