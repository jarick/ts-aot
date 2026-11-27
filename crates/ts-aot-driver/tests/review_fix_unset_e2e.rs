use ts_aot_driver::{CompileOptions, Driver, DriverOutput, EmitStage};
use ts_aot_test_support::run_generated_crate_output;

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

fn accepted_rust(source: &str) -> String {
    let out = compile_module(source);
    assert!(
        !out.has_errors(),
        "the program must be accepted; got {:?}",
        out.diagnostics
    );
    out.rust_source
        .expect("emit-rust must populate rust_source")
}

fn main_calling_got(package: &str) -> String {
    format!(
        r#"
use ts_aot_runtime::{{Bump, Ctx, Host}};

struct NoHost;
impl Host for NoHost {{}}

fn main() {{
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx {{ arena: &bump, host: &host }};
    let _ = {package}::got(&ctx);
    println!("ok");
}}
"#
    )
}

fn assert_unset_call_throws_type_error(package: &str, source: &str) {
    let rust = accepted_rust(source);
    let stdout = run_generated_crate_output(package, &rust, &main_calling_got(package));
    let stderr = String::from_utf8_lossy(&stdout.stderr);
    assert!(
        !stdout.status.success(),
        "node raises an uncaught TypeError, so the AOT program must fail; stdout:\n{}stderr:\n{stderr}",
        String::from_utf8_lossy(&stdout.stdout)
    );
    assert!(
        !stderr.contains("entered unreachable code"),
        "an accepted program must not abort with an internal `unreachable!()` panic; got:\n{stderr}"
    );
    assert!(
        stderr.contains("TypeError"),
        "node raises an uncaught TypeError; got stderr:\n{stderr}"
    );
    assert!(
        !rust.contains("__Unset => unreachable"),
        "the unset function-value arm must raise a TypeError instead of `unreachable!()`; got:\n{rust}"
    );
}

fn assert_returns(package: &str, source: &str, expected: f64) {
    let rust = accepted_rust(source);
    let main = format!(
        r#"
use ts_aot_runtime::{{Bump, Ctx, Host}};

struct NoHost;
impl Host for NoHost {{}}

fn main() {{
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx {{ arena: &bump, host: &host }};
    assert_eq!({package}::got(&ctx), {expected:?});
    println!("ok");
}}
"#
    );
    let output = run_generated_crate_output(package, &rust, &main);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "the program must run; stderr:\n{stderr}"
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("ok"),
        "got stdout:\n{}",
        String::from_utf8_lossy(&output.stdout)
    );
}

fn assert_refused_without_rust(source: &str, code: &str) {
    let out = compile_module(source);
    let diagnostic = out
        .diagnostics
        .iter()
        .find(|d| d.code.as_str() == code)
        .unwrap_or_else(|| panic!("expected {code}; got {:?}", out.diagnostics));
    assert!(
        diagnostic.message.contains("unset function value"),
        "the diagnostic must explain the unset function value; got `{}`",
        diagnostic.message
    );
    assert!(
        out.rust_source.is_none(),
        "no Rust may be emitted for a refused call; got:\n{:?}",
        out.rust_source
    );
}

#[test]
fn an_uncalled_free_function_write_to_the_field_throws_a_type_error() {
    const SOURCE: &str = "class B { f: () => number; g(): number { return this.f(); } } \
         function init(b: B) { b.f = (): number => 9; } \
         export function got(): number { return new B().g(); }";
    assert_unset_call_throws_type_error("unset_uncalled_init", SOURCE);
}

#[test]
fn a_write_under_a_statically_false_branch_throws_a_type_error() {
    const SOURCE: &str = "class B { f: () => number; g(): number { return this.f(); } } \
         function init(b: B) { if (false) { b.f = (): number => 9; } } \
         export function got(): number { return new B().g(); }";
    assert_unset_call_throws_type_error("unset_false_branch", SOURCE);
}

#[test]
fn a_write_in_an_uncalled_method_throws_a_type_error() {
    const SOURCE: &str = "class B { f: () => number; \
         set(): void { this.f = (): number => 9; } \
         g(): number { return this.f(); } } \
         export function got(): number { return new B().g(); }";
    assert_unset_call_throws_type_error("unset_uncalled_method", SOURCE);
}

#[test]
fn a_same_named_write_to_another_class_throws_a_type_error() {
    const SOURCE: &str = "class A { f: () => number; } \
         class B { f: () => number; g(): number { return this.f(); } } \
         function init(a: A) { a.f = (): number => 9; } \
         export function got(): number { return new B().g(); }";
    assert_unset_call_throws_type_error("unset_other_class", SOURCE);
}

#[test]
fn a_same_named_write_to_another_instance_throws_a_type_error() {
    const SOURCE: &str = "class B { f: () => number; g(): number { return this.f(); } } \
         function init(other: B) { other.f = (): number => 9; } \
         export function got(): number { return new B().g(); }";
    assert_unset_call_throws_type_error("unset_other_instance", SOURCE);
}

#[test]
fn a_free_function_write_that_runs_before_the_call_returns_the_node_value() {
    const SOURCE: &str = "class B { f: () => number; g(): number { return this.f(); } } \
         function init(b: B) { b.f = (): number => 9; } \
         export function got(): number { const b = new B(); init(b); return b.g(); }";
    assert_returns("unset_called_init", SOURCE, 9.0);
}

#[test]
fn a_closure_field_initializer_still_refuses_without_rust() {
    const SOURCE: &str = "class C { f: () => number = (): number => 5; \
         g(): number { return this.f(); } } \
         export function got(): number { return new C().g(); }";
    assert_refused_without_rust(SOURCE, "E0421");
}
