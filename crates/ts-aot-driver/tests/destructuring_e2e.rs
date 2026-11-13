use ts_aot_driver::{CompileOptions, Driver};
use ts_aot_test_support::{check_generated_crate, run_generated_crate};

mod common;

#[test]
fn an_array_destructuring_of_a_temporary_reads_every_element() {
    const SOURCE: &str = r"
        export function fromLiteral(): number {
            const [a, b] = [10.5, 20.5];
            return a + b;
        }
        ";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(destructuring_temporary::fromLiteral(&ctx), 31.0);
    println!("ok");
}
"#;
    let opts = CompileOptions {
        module: true,
        ..CompileOptions::default()
    };
    let out = Driver::new().compile_source("test.ts", SOURCE, &opts);
    assert!(!out.has_errors(), "must compile; got {:?}", out.diagnostics);
    let rust = out.rust_source.expect("rust_source");
    check_generated_crate("destructuring_temporary", &rust);
    let stdout = run_generated_crate("destructuring_temporary", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn e2e_array_destructuring_emits_runtime_call_per_element() {
    let opts = CompileOptions {
        module: false,
        ..CompileOptions::default()
    };
    let out = Driver::new().compile_source(
        "test.ts",
        r"
        function f(arr: i64[]): i64 {
            let [a, b, c] = arr;
            return a + b + c;
        }
        ",
        &opts,
    );
    assert!(!out.has_errors(), "must compile; got {:?}", out.diagnostics);
    let rust = out
        .rust_source
        .expect("emit-rust must populate rust_source");
    let rust = common::normalize_rust(&rust);
    assert_eq!(
        rust.matches("__ts_aot_array_get_or_default::<").count(),
        3,
        "array destructuring must emit one typed runtime call per binding, got:\n{rust}"
    );
}

#[test]
fn e2e_array_destructuring_on_non_array_rhs_emits_diagnostic() {
    let opts = CompileOptions {
        module: false,
        ..CompileOptions::default()
    };
    let out = Driver::new().compile_source(
        "test.ts",
        r"
        function f(): i64 {
            let [a, b] = 42;
            return a;
        }
        ",
        &opts,
    );
    let has_diagnostic = out
        .diagnostics
        .iter()
        .any(|d| d.message.contains("array type") || d.message.contains("destructuring"));
    assert!(
        has_diagnostic,
        "destructuring non-array rhs must surface a diagnostic, got: {:?}",
        out.diagnostics
    );
}
