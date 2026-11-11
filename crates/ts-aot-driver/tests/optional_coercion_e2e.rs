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

#[test]
fn optional_number_local_init_and_assignment_compile() {
    let out = compile_module(
        "export function f(): void { \
             let x: number | null = null; \
             x = 3; \
             const y: number | null = x; \
         }",
    );
    assert!(
        !out.has_errors(),
        "a `number | null` local must compile through the full pipeline; got {:?}",
        out.diagnostics
    );
    let rust = out
        .rust_source
        .expect("emit-rust must populate rust_source for the optional-number check");
    let norm: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        norm.contains("Option::None"),
        "a `null` init of a `number | null` local must emit `Option::None`; got:\n{rust}"
    );
    assert!(
        norm.contains("Option::Some("),
        "a `number` written into a `number | null` local must be wrapped in `Some(..)`; got:\n{rust}"
    );
    check_generated_crate("optional_number_local", &rust);
}

#[test]
fn optional_parameter_receives_a_coerced_argument() {
    let out = compile_module(
        "function isNull2(x: number | null): boolean { return x === null; } \
         export function withValue(n: number): boolean { return isNull2(n); } \
         export function withNull(): boolean { return isNull2(null); }",
    );
    assert!(
        !out.has_errors(),
        "calling a `number | null` parameter with a `number` or `null` must compile; got {:?}",
        out.diagnostics
    );
    let rust = out
        .rust_source
        .expect("emit-rust must populate rust_source for the optional-argument check");
    let norm: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        norm.contains("isNull2(__ctx,::core::option::Option::Some(n))"),
        "a `number` argument passed to a `number | null` parameter must be wrapped in `Some(..)`; \
         got:\n{rust}"
    );
    assert!(
        norm.contains("isNull2(__ctx,::core::option::Option::None)"),
        "a `null` argument passed to a `number | null` parameter must become `Option::None`; \
         got:\n{rust}"
    );
    check_generated_crate("optional_argument_coercion", &rust);
}

#[test]
fn optional_number_global_writes_and_reads_compile() {
    let out = compile_module(
        "let g: number | null; \
         export function setG(n: number | null): void { g = n; } \
         export function getG(): number | null { return g; }",
    );
    assert!(
        !out.has_errors(),
        "a `number | null` global must compile through the full pipeline; got {:?}",
        out.diagnostics
    );
    let rust = out
        .rust_source
        .expect("emit-rust must populate rust_source for the optional-number-global check");
    check_generated_crate("optional_number_global", &rust);
}

const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    optional_num_global::setG(&ctx, ::core::option::Option::Some(3.0));
    assert_eq!(optional_num_global::getG(&ctx), ::core::option::Option::Some(3.0));
    optional_num_global::setG(&ctx, ::core::option::Option::None);
    assert_eq!(optional_num_global::getG(&ctx), ::core::option::Option::None);
    println!("ok");
}
"#;

#[test]
fn optional_number_global_roundtrips_at_runtime() {
    let out = compile_module(
        "let g: number | null; \
         export function setG(n: number | null): void { g = n; } \
         export function getG(): number | null { return g; }",
    );
    assert!(!out.has_errors(), "got {:?}", out.diagnostics);
    let rust = out.rust_source.expect("rust_source");
    let stdout = run_generated_crate("optional_num_global", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

const OPTIONAL_RETURN_SOURCE: &str = "\
     export function fromValue(): number | null { return 1; } \
     export function fromNull(): number | null { return null; } \
     export function fromString(): string | null { return \"a\"; } \
     export function passThrough(x: number | null): number | null { return x; }";

#[test]
fn returning_a_plain_value_from_an_optional_function_wraps_it() {
    let out = compile_module(OPTIONAL_RETURN_SOURCE);
    assert!(
        !out.has_errors(),
        "returning `1` / `null` / a string from optional-returning functions must compile; got {:?}",
        out.diagnostics
    );
    let rust = out.rust_source.expect("rust_source");
    let norm: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        norm.contains("Option::Some(1.0)"),
        "`return 1` in a `number | null` function must emit `Some(1.0)`; got:\n{rust}"
    );
    assert!(
        norm.contains("Option::None"),
        "`return null` in a `number | null` function must emit `Option::None`; got:\n{rust}"
    );
    assert!(
        norm.contains("Option::Some(JsString")
            || norm.contains("Option::Some(ts_aot_runtime::JsString"),
        "`return \"a\"` in a `string | null` function must emit `Some(JsString)`; got:\n{rust}"
    );
    check_generated_crate("optional_return_coercion", &rust);
}

const OPTIONAL_RETURN_MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(
        optional_return_coercion::fromValue(&ctx),
        ::core::option::Option::Some(1.0)
    );
    assert_eq!(
        optional_return_coercion::fromNull(&ctx),
        ::core::option::Option::None
    );
    assert_eq!(
        optional_return_coercion::fromString(&ctx).map(|s| s.to_string_lossy()),
        ::core::option::Option::Some("a".to_string())
    );
    assert_eq!(
        optional_return_coercion::passThrough(&ctx, ::core::option::Option::None),
        ::core::option::Option::None
    );
    println!("ok");
}
"#;

#[test]
fn optional_returns_roundtrip_at_runtime() {
    let out = compile_module(OPTIONAL_RETURN_SOURCE);
    assert!(!out.has_errors(), "got {:?}", out.diagnostics);
    let rust = out.rust_source.expect("rust_source");
    let stdout = run_generated_crate("optional_return_coercion", &rust, OPTIONAL_RETURN_MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

const OPTIONAL_CONTAINER_SOURCE: &str = "\
     class Box { x: number | null = null; \
         set(v: number): void { this.x = v; } \
         setNull(): void { this.x = null; } \
         get(): number | null { return this.x; } } \
     export function fill(): number { \
         const b: Box = new Box(); \
         b.set(2); \
         return 1; \
     } \
     export function build(): (number | null)[] { \
         const a: (number | null)[] = [1, null]; \
         a.push(2); \
         a.push(null); \
         return a; \
     }";

#[test]
fn optional_field_and_array_element_writes_are_coerced() {
    let out = compile_module(OPTIONAL_CONTAINER_SOURCE);
    assert!(
        !out.has_errors(),
        "writes of plain / nullish values into optional fields and array elements must compile; \
         got {:?}",
        out.diagnostics
    );
    let rust = out.rust_source.expect("rust_source");
    let norm: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        norm.contains(
            "vec![in__ctx.arena;::core::option::Option::Some(1.0),::core::option::Option::None]"
        ),
        "an array literal of `(number | null)[]` must wrap its plain element in `Some(..)`; \
         got:\n{rust}"
    );
    assert!(
        norm.contains("__ts_aot_array_push(&muta,::core::option::Option::Some(2.0))"),
        "`push(2)` into a `(number | null)[]` must be wrapped in `Some(..)`; got:\n{rust}"
    );
    check_generated_crate("optional_container_coercion", &rust);
}

const OPTIONAL_CONTAINER_MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    optional_container_coercion::fill(&ctx);
    let built = optional_container_coercion::build(&ctx);
    let values: Vec<::core::option::Option<f64>> = built.iter().copied().collect();
    assert_eq!(
        values,
        vec![
            ::core::option::Option::Some(1.0),
            ::core::option::Option::None,
            ::core::option::Option::Some(2.0),
            ::core::option::Option::None,
        ]
    );
    println!("ok");
}
"#;

#[test]
fn optional_field_and_array_writes_roundtrip_at_runtime() {
    let out = compile_module(OPTIONAL_CONTAINER_SOURCE);
    assert!(!out.has_errors(), "got {:?}", out.diagnostics);
    let rust = out.rust_source.expect("rust_source");
    let stdout = run_generated_crate(
        "optional_container_coercion",
        &rust,
        OPTIONAL_CONTAINER_MAIN,
    );
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

const OPTIONAL_CALLEE_SOURCE: &str = "\
     class C { m(x?: number): boolean { return x === undefined; } \
         plain(): boolean { return true; } } \
     export function viaMethod(): boolean { const c: C = new C(); return c.m(); } \
     export function viaClosure(): boolean { \
         const cb = (x?: number): boolean => x === undefined; \
         return cb(); \
     }";

#[test]
fn omitted_optional_arguments_reach_methods_and_closures_as_none() {
    let out = compile_module(OPTIONAL_CALLEE_SOURCE);
    assert!(
        !out.has_errors(),
        "omitting an optional argument of a method / closure must compile; got {:?}",
        out.diagnostics
    );
    let rust = out.rust_source.expect("rust_source");
    let norm: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        norm.contains("__ctx,::core::option::Option::None)"),
        "the missing method argument must be passed as `Option::None`; got:\n{rust}"
    );
    check_generated_crate("optional_callee_padding", &rust);
}

const OPTIONAL_CALLEE_MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert!(optional_callee_padding::viaMethod(&ctx));
    assert!(optional_callee_padding::viaClosure(&ctx));
    println!("ok");
}
"#;

#[test]
fn omitted_optional_arguments_roundtrip_at_runtime() {
    let out = compile_module(OPTIONAL_CALLEE_SOURCE);
    assert!(!out.has_errors(), "got {:?}", out.diagnostics);
    let rust = out.rust_source.expect("rust_source");
    let stdout = run_generated_crate("optional_callee_padding", &rust, OPTIONAL_CALLEE_MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}
