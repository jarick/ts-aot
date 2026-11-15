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

fn norm(rust: &str) -> String {
    rust.chars().filter(|c| !c.is_whitespace()).collect()
}

fn messages(source: &str) -> String {
    let out = compile_module(source);
    assert!(
        out.has_errors(),
        "the comparison must be rejected, not silently lowered; got {:?}",
        out.diagnostics
    );
    out.diagnostics
        .iter()
        .map(|d| format!("{}: {}", d.code.as_str(), d.message))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn strict_equality_of_two_optionals_with_the_same_inner_type_keeps_the_option_comparison() {
    const SOURCE: &str = "export function cmp(a: number | null, b: number | null): boolean { return a === b; } \
         export function cmpStr(a: string | null, b: string | null): boolean { return a === b; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    // JavaScript: 1 === 1 is true, 1 === 2 is false, null === null is true,
    // 1 === null is false.
    assert!(backend_compare_neg_string::cmp(&ctx, Some(1.0), Some(1.0)));
    assert!(!backend_compare_neg_string::cmp(&ctx, Some(1.0), Some(2.0)));
    assert!(backend_compare_neg_string::cmp(&ctx, None, None));
    assert!(!backend_compare_neg_string::cmp(&ctx, Some(1.0), None));
    assert!(!backend_compare_neg_string::cmp(&ctx, None, Some(1.0)));
    // The same for a string inner type.
    let s = Some(ts_aot_runtime::JsString::from("x"));
    assert!(backend_compare_neg_string::cmpStr(&ctx, s, s));
    assert!(!backend_compare_neg_string::cmpStr(&ctx, s, None));
    assert!(backend_compare_neg_string::cmpStr(&ctx, None, None));
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let flat = norm(&rust);
    assert!(
        flat.contains("a==b"),
        "two optionals with the same inner type keep the plain `Option` comparison; got:\n{rust}"
    );
    assert!(
        flat.contains("Option<f64>") && flat.contains("Option<ts_aot_runtime::JsString"),
        "the same-inner optionals stay `Option<T>`; got:\n{rust}"
    );
    check_generated_crate("backend_compare_neg_string", &rust);

    let stdout = run_generated_crate("backend_compare_neg_string", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn strict_equality_of_two_optionals_with_different_inner_types_is_reported() {
    let report =
        messages("export function f(a: number | null, b: i64 | null): boolean { return a === b; }");
    assert!(
        report.contains("P0022"),
        "two optionals with different inner types must be reported as P0022; got:\n{report}"
    );
    assert!(
        report.contains("==="),
        "the message must name the operator; got:\n{report}"
    );

    let report =
        messages("export function f(a: i32 | null, b: f64 | null): boolean { return a !== b; }");
    assert!(
        report.contains("P0022") && report.contains("!=="),
        "`!==` over different inner types must be reported too; got:\n{report}"
    );

    let report =
        messages("export function f(a: i64 | null, b: i32 | null): boolean { return a === b; }");
    assert!(
        report.contains("P0022"),
        "two integer inner types of different width must be reported too; got:\n{report}"
    );
}

#[test]
fn strict_equality_of_two_optional_structs_is_reported() {
    let report = messages(
        "class C { n: number; constructor() { this.n = 1; } } \
         export function f(a: C | null, b: C | null): boolean { return a === b; }",
    );
    assert!(
        report.contains("P0022"),
        "two optional structures must be reported as P0022; got:\n{report}"
    );
    assert!(
        report.contains("PartialEq"),
        "the message must explain the missing `PartialEq`; got:\n{report}"
    );
}

#[test]
fn composite_negation_widens_integer_operands_to_f64() {
    const SOURCE: &str = "export function neg(a: i64, b: i64): number { return -(a + b); } \
         export function negMul(a: i64, b: i64): number { return -(a * b); } \
         export function negSumHalf(a: i64, b: i64): number { return -(a + b) + 0.5; } \
         export function neg2(a: i64): number { return 1.5 + -a + -a; } \
         export function add(a: i64, b: i64): number { return a + b; } \
         export function negLocal(y: i64): number { return -y; } \
         export function negField(o: Box): number { return -o.v; } \
         export function negCall(a: i64): number { return -id(a); } \
         export function bitnot(a: i64, b: i64): number { return ~(a + b); } \
         class Box { v: i64; constructor(v: i64) { this.v = v; } } \
         function id(x: i64): i64 { return x; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    // JavaScript: -(2 + 3) = -5, -(2 * 3) = -6, -(2 + 3) + 0.5 = -4.5,
    // 1.5 + (-2) + (-2) = -2.5, 2 + 3 = 5, -(5) = -5, -(7) = -7, -(4) = -4.
    assert_eq!(backend_compare_neg_string::neg(&ctx, 2, 3), -5.0);
    assert_eq!(backend_compare_neg_string::negMul(&ctx, 2, 3), -6.0);
    assert_eq!(backend_compare_neg_string::negSumHalf(&ctx, 2, 3), -4.5);
    assert_eq!(backend_compare_neg_string::neg2(&ctx, 2), -2.5);
    assert_eq!(backend_compare_neg_string::add(&ctx, 2, 3), 5.0);
    assert_eq!(backend_compare_neg_string::negLocal(&ctx, 5), -5.0);
    assert_eq!(
        backend_compare_neg_string::negField(&ctx, backend_compare_neg_string::Box { v: 7 }),
        -7.0
    );
    assert_eq!(backend_compare_neg_string::negCall(&ctx, 4), -4.0);
    // JavaScript: ~(1 + 2) = ~3 = -4.
    assert_eq!(backend_compare_neg_string::bitnot(&ctx, 1, 2), -4.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let flat = norm(&rust);
    assert!(
        flat.contains("((-((a+b)))asf64)"),
        "a negation of a composite expression must be cast to `f64`; got:\n{rust}"
    );
    assert!(
        flat.contains("((a+b)asf64)"),
        "an `i64` binary expression returned as `number` must be cast to `f64`; got:\n{rust}"
    );
    assert!(
        flat.contains("((-(y))asf64)"),
        "`-y` on an `i64` local must still be cast; got:\n{rust}"
    );
    assert!(
        flat.contains("((-(o.v))asf64)"),
        "`-o.v` on an `i64` field must still be cast; got:\n{rust}"
    );
    assert!(
        flat.contains("((-(id(__ctx,a)))asf64)"),
        "`-id(a)` on an `i64` call must still be cast; got:\n{rust}"
    );
    check_generated_crate("backend_compare_neg_string", &rust);

    let stdout = run_generated_crate("backend_compare_neg_string", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn optional_string_index_uses_the_string_index_helper() {
    const SOURCE: &str = "export function idx(s: string | null, i: number): string | null | undefined { \
         return s?.[i]; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    // JavaScript: "hello"[1] is "e", and a nullish receiver short-circuits to
    // undefined (one `Option::None` here).
    let hello = Some(ts_aot_runtime::JsString::from("hello"));
    assert_eq!(
        backend_compare_neg_string::idx(&ctx, hello, 1.0).map(|c| c.to_string_lossy()),
        Some("e".to_owned())
    );
    assert_eq!(backend_compare_neg_string::idx(&ctx, None, 1.0), None);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let flat = norm(&rust);
    assert!(
        flat.contains("__ts_aot_string_char_at"),
        "`s?.[i]` on a string must go through the string-index helper; got:\n{rust}"
    );
    assert!(
        !flat.contains("o[iasusize]"),
        "the `JsString` must not be indexed directly (E0608); got:\n{rust}"
    );
    check_generated_crate("backend_compare_neg_string", &rust);

    let stdout = run_generated_crate("backend_compare_neg_string", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}
