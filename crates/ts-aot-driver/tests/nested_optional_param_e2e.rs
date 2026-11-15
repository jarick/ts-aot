use ts_aot_driver::{CompileOptions, Driver, EmitStage};
use ts_aot_test_support::{check_generated_crate, run_generated_crate};

fn rust_of(source: &str) -> String {
    let out = Driver::new().compile_source(
        "test.ts",
        source,
        &CompileOptions {
            emit: EmitStage::Rust,
            module: true,
        },
    );
    assert!(
        !out.has_errors(),
        "must compile through the full pipeline; got {:?}",
        out.diagnostics
    );
    out.rust_source
        .expect("emit-rust must populate rust_source")
}

#[test]
fn nested_optional_default_parameter_keeps_null_and_wraps_values() {
    const SOURCE: &str = "\
        export function withDefault(x: number | null = 7): number | null { return x; } \
        export function run(): number | null { return withDefault(null); } \
        export function run2(): number | null { return withDefault(5); } \
        export function run3(): number | null { return withDefault(); } \
        export function withDefaultStr(x: string | null = \"d\"): string | null { return x; } \
        export function run4(): string | null { return withDefaultStr(\"x\"); } \
        export function run5(): string | null { return withDefaultStr(null); } \
        export function withPlain(x: number = 7): number { return x; } \
        export function run6(): number { return withPlain(5); } \
        export function run7(): number { return withPlain(); }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    // withDefault(null) -> JS null, *not* the default 7
    assert_eq!(nested_optional_param::run(&ctx), None);
    // withDefault(5) -> JS 5
    assert_eq!(nested_optional_param::run2(&ctx), Some(5.0));
    // withDefault() -> JS 7 (default)
    assert_eq!(nested_optional_param::run3(&ctx), Some(7.0));
    // withDefaultStr("x") -> JS "x"
    assert_eq!(
        nested_optional_param::run4(&ctx).map(|s| s.to_string_lossy()),
        Some("x".to_string())
    );
    // withDefaultStr(null) -> JS null, *not* the default "d"
    assert_eq!(
        nested_optional_param::run5(&ctx).map(|s| s.to_string_lossy()),
        None
    );
    // a non-nullable defaulted parameter is unchanged
    assert_eq!(nested_optional_param::run6(&ctx), 5.0);
    assert_eq!(nested_optional_param::run7(&ctx), 7.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let norm: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        norm.contains("Option<Option<f64>>"),
        "a defaulted nullable parameter is nested in the ABI; got:\n{rust}"
    );
    check_generated_crate("nested_optional_param", &rust);
    let stdout = run_generated_crate("nested_optional_param", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn fn_value_bucket_serves_optional_and_undefined_only_closures() {
    const SOURCE: &str = "\
        export function mkOpt(): (x?: number) => number { \
            const f = (x?: number): number => x ?? 0; return f; \
        } \
        export function mkUndef(): (x: number | undefined) => number { \
            const g = (x: number | undefined): number => x ?? 0; return g; \
        } \
        export function run(): number { \
            const a = mkOpt(); const b = mkUndef(); \
            return a(2) + b(3) + a() + b(undefined); \
        }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    // a(2) + b(3) + a() + b(undefined) == 2 + 3 + 0 + 0
    assert_eq!(fn_value_optional_closure::run(&ctx), 5.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let norm: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        !norm.contains("__Uninhabited"),
        "no function-value bucket may be left empty; got:\n{rust}"
    );
    check_generated_crate("fn_value_optional_closure", &rust);
    let stdout = run_generated_crate("fn_value_optional_closure", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn fn_value_bucket_serves_optional_and_undefined_only_declarations() {
    const SOURCE: &str = "\
        export function maybe(x?: number): number { return x ?? 0; } \
        export function strict(x: number | undefined): number { return x ?? 0; } \
        export function run(): number { \
            const a: (x?: number) => number = maybe; \
            const b: (x: number | undefined) => number = strict; \
            return a(4) + b(5) + a(); \
        }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    // maybe(4) + strict(5) + maybe() == 4 + 5 + 0
    assert_eq!(fn_value_optional_decl::run(&ctx), 9.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let norm: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        !norm.contains("__Uninhabited"),
        "no function-value bucket may be left empty; got:\n{rust}"
    );
    check_generated_crate("fn_value_optional_decl", &rust);
    let stdout = run_generated_crate("fn_value_optional_decl", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn optional_chain_field_copies_non_copy_field() {
    const SOURCE: &str = "\
        class Q { s: string; constructor() { this.s = \"hello\"; } } \
        class R { q: Q; constructor() { const q = new Q(); this.q = q; } } \
        export function f(r: R | null): Q | undefined { return r?.q; } \
        export function mkR(): R { const r = new R(); return r; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    let r = optional_chain_field::mkR(&ctx);
    // the cloned field still holds its value
    let q = optional_chain_field::f(&ctx, Some(r)).expect("r?.q on a present receiver");
    assert_eq!(q.s.to_string_lossy(), "hello");
    // and `null` still short-circuits to `undefined`
    assert!(optional_chain_field::f(&ctx, None).is_none());
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let norm: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        norm.contains(".q.clone()"),
        "`?.field` must clone a non-Copy field; got:\n{rust}"
    );
    check_generated_crate("optional_chain_field", &rust);
    let stdout = run_generated_crate("optional_chain_field", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn optional_chain_index_copies_non_copy_element() {
    const SOURCE: &str = "\
        export function g(a: number[][] | null, i: number): number[] | undefined { return a?.[i]; } \
        export function mkA(): number[][] { const a: number[][] = [[1.5, 2.5, 3.5]]; return a; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    let a = optional_chain_index::mkA(&ctx);
    let e = optional_chain_index::g(&ctx, Some(a), 0.0).expect("a?.[i] on a present receiver");
    assert_eq!(e.len(), 3);
    assert!((e[2] - 3.5).abs() < f64::EPSILON);
    assert!(optional_chain_index::g(&ctx, None, 0.0).is_none());
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let norm: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        norm.contains("[iasusize].clone()"),
        "`?.[i]` must clone a non-Copy element; got:\n{rust}"
    );
    check_generated_crate("optional_chain_index", &rust);
    let stdout = run_generated_crate("optional_chain_index", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}
