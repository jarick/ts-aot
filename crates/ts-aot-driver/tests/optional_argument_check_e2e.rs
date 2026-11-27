use ts_aot_driver::{CompileOptions, Driver, DriverOutput, EmitStage};
use ts_aot_test_support::{check_generated_crate, run_generated_crate};

fn compile(source: &str) -> DriverOutput {
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
    let out = compile(source);
    assert!(
        !out.has_errors(),
        "must compile through the full pipeline; got {:?}",
        out.diagnostics
    );
    out.rust_source
        .expect("emit-rust must populate rust_source")
}

fn normalized(rust: &str) -> String {
    rust.chars().filter(|c| !c.is_whitespace()).collect()
}

fn error_message(out: &DriverOutput, code: &str) -> String {
    out.diagnostics
        .errors()
        .find(|d| d.code.as_str() == code)
        .map_or_else(
            || panic!("expected {code}; got {:?}", out.diagnostics),
            |d| d.message.clone(),
        )
}

fn codes(out: &DriverOutput) -> Vec<&str> {
    out.diagnostics
        .iter()
        .map(|diagnostic| diagnostic.code.as_str())
        .collect()
}

#[test]
fn a_nested_optional_argument_forwarded_to_a_nested_optional_parameter_runs() {
    const SOURCE: &str = "\
        class K { \
            m(x: number | null = 7): number | null { return x; } \
            take(x: number | null = 7): number | null { return x; } \
        } \
        function fwd(x: number | null = 7): number | null { return x; } \
        export function viaMethod(k: K | undefined): number | null { \
            const z = k?.m(5); const b = new K(); return b.take(z); \
        } \
        export function viaFunction(k: K | undefined): number | null { \
            const z = k?.m(5); return fwd(z); \
        } \
        export function viaMethodNull(k: K | undefined): number | null { \
            const z = k?.m(null); const b = new K(); return b.take(z); \
        } \
        export function viaFunctionNull(k: K | undefined): number | null { \
            const z = k?.m(null); return fwd(z); \
        }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    use ::core::option::Option::{None, Some};
    let k = nested_forward::K {};
    assert_eq!(nested_forward::viaMethod(&ctx, Some(k)), Some(5.0));
    assert_eq!(nested_forward::viaMethod(&ctx, None), Some(7.0));
    let k = nested_forward::K {};
    assert_eq!(nested_forward::viaFunction(&ctx, Some(k)), Some(5.0));
    assert_eq!(nested_forward::viaFunction(&ctx, None), Some(7.0));
    let k = nested_forward::K {};
    assert_eq!(
        nested_forward::viaMethodNull(&ctx, Some(k)),
        None,
        "`k?.m(null)` returns `null`, and a default parameter does not replace an explicit `null`"
    );
    let k = nested_forward::K {};
    assert_eq!(
        nested_forward::viaFunctionNull(&ctx, Some(k)),
        None,
        "`k?.m(null)` returns `null`, and a default parameter does not replace an explicit `null`"
    );
    assert_eq!(
        nested_forward::viaMethodNull(&ctx, None),
        Some(7.0),
        "an absent receiver is `undefined`, so the default parameter still applies"
    );
    assert_eq!(
        nested_forward::viaFunctionNull(&ctx, None),
        Some(7.0),
        "an absent receiver is `undefined`, so the default parameter still applies"
    );
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let norm = normalized(&rust);
    assert!(
        norm.contains("Option<Option<f64>>"),
        "the optional chain result and the defaulted nullable parameters keep both `Option` \
         layers; got:\n{rust}"
    );
    check_generated_crate("nested_forward", &rust);
    let stdout = run_generated_crate("nested_forward", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_nested_optional_argument_written_into_a_single_optional_parameter_is_rejected() {
    const SOURCE: &str = "\
        class K { \
            m(x: number | null = 7): number | null { return x; } \
            plain(x: number | undefined): number { return x ?? 0; } \
        } \
        export function f(k: K | undefined): number { \
            const z = k?.m(5); const b = new K(); b.plain(z); return 1; \
        }";
    let out = compile(SOURCE);
    let message = error_message(&out, "P0018");
    assert!(
        message.contains("two nested `Option`s"),
        "a nested optional value has no representation in a single-optional parameter; got \
         {message:?}"
    );
    assert!(
        out.rust_source.is_none(),
        "no Rust may be emitted for a nested value written into a single-optional parameter"
    );
}

#[test]
fn a_void_expression_is_accepted_as_undefined_for_an_optional_parameter() {
    const SOURCE: &str = "\
        var calls: number = 0; \
        function bump(): number { calls = calls + 1; return calls; } \
        function take(x: number | undefined): number { return x ?? 0; } \
        export function withSideEffect(): number { return take(void bump()); } \
        export function callsNow(): number { return calls; } \
        export function reset(): number { calls = 0; return calls; } \
        export function withLiteral(): number { return take(void 0); }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    void_argument::reset(&ctx);
    assert_eq!(
        void_argument::callsNow(&ctx),
        0.0,
        "the side-effect counter must be seeded explicitly"
    );
    assert_eq!(void_argument::withLiteral(&ctx), 0.0);
    assert_eq!(
        void_argument::withSideEffect(&ctx),
        0.0,
        "`take(void bump())` must receive the fallback `undefined`, so `x ?? 0` yields 0"
    );
    assert_eq!(
        void_argument::callsNow(&ctx),
        1.0,
        "`bump()` must run exactly once"
    );
    println!("ok");
}
"#;
    let out = compile(SOURCE);
    assert!(
        codes(&out).is_empty(),
        "`void` used where the target accepts `undefined` must compile through the full pipeline \
         without reporting E0410 or any other diagnostic; got {:?}",
        out.diagnostics
    );
    let rust = out
        .rust_source
        .expect("emit-rust must populate rust_source");
    check_generated_crate("void_argument", &rust);
    let stdout = run_generated_crate("void_argument", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_void_expression_is_still_rejected_for_a_non_optional_parameter() {
    const SOURCE: &str = "\
        function takeNumber(x: number): number { return x; } \
        export function f(): number { return takeNumber(void 0); }";
    let out = compile(SOURCE);
    let message = error_message(&out, "E0410");
    assert!(
        message.contains("`void` value"),
        "E0410 must name the `void` value; got {message:?}"
    );
    assert!(
        out.rust_source.is_none(),
        "no Rust may be emitted for a `void` value passed to a non-optional parameter"
    );
}

#[test]
fn an_optional_string_argument_for_an_optional_numeric_parameter_is_rejected() {
    const SOURCE: &str = "\
        function take(x: number | undefined): number { return x ?? 0; } \
        export function f(s: string | undefined): number { return take(s); }";
    let out = compile(SOURCE);
    let message = error_message(&out, "E0408");
    assert!(
        message.contains("string and numeric"),
        "E0408 must name the string/numeric mismatch of an optional argument; got {message:?}"
    );
    assert!(
        out.rust_source.is_none(),
        "no Rust may be emitted for a string-typed optional argument passed to a numeric \
         optional parameter"
    );
}

#[test]
fn an_optional_string_argument_for_an_optional_numeric_method_parameter_is_rejected() {
    const SOURCE: &str = "\
        class Box { m(x: number | undefined): number { return x ?? 0; } } \
        export function f(s: string | undefined): number { \
            const b = new Box(); return b.m(s); \
        }";
    let out = compile(SOURCE);
    let message = error_message(&out, "E0408");
    assert!(
        message.contains("string and numeric"),
        "E0408 must name the string/numeric mismatch of a method argument; got {message:?}"
    );
    assert!(
        out.rust_source.is_none(),
        "no Rust may be emitted for a string-typed optional method argument passed to a numeric \
         optional parameter"
    );
}

#[test]
fn an_optional_string_argument_for_an_optional_numeric_constructor_parameter_is_rejected() {
    const SOURCE: &str = "\
        class C { constructor(v: number | undefined) { } } \
        export function f(s: string | undefined): number { \
            const c = new C(s); return 1; \
        }";
    let out = compile(SOURCE);
    let message = error_message(&out, "E0408");
    assert!(
        message.contains("string and numeric"),
        "E0408 must name the string/numeric mismatch of a constructor argument; got {message:?}"
    );
    assert!(
        out.rust_source.is_none(),
        "no Rust may be emitted for a string-typed optional constructor argument passed to a \
         numeric optional parameter"
    );
}

#[test]
fn a_nested_optional_argument_written_into_a_nested_optional_constructor_parameter_runs() {
    const SOURCE: &str = "\
        type MaybeNum = number | null; \
        class Inner { m(x: number | null = 7): number | null { return x; } } \
        class B { \
            v: MaybeNum | undefined; \
            constructor(p: MaybeNum | undefined) { this.v = p } \
        } \
        export function fromNull(k: Inner | undefined): B { \
            const z = k?.m(null); return new B(z); \
        } \
        export function fromValue(k: Inner | undefined): B { \
            const z = k?.m(5); return new B(z); \
        }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    use ::core::option::Option::{None, Some};
    let k = ctor_nested_optional::Inner {};
    assert_eq!(
        *ctor_nested_optional::fromNull(&ctx, Some(k)).v.borrow(),
        Some(None)
    );
    let k = ctor_nested_optional::Inner {};
    assert_eq!(
        *ctor_nested_optional::fromValue(&ctx, Some(k)).v.borrow(),
        Some(Some(5.0))
    );
    assert_eq!(
        *ctor_nested_optional::fromValue(&ctx, None).v.borrow(),
        None
    );
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let norm = normalized(&rust);
    assert!(
        norm.contains("Option<Option<f64>>"),
        "the nested-optional constructor parameter and field keep both `Option` layers; got:\n{rust}"
    );
    check_generated_crate("ctor_nested_optional", &rust);
    let stdout = run_generated_crate("ctor_nested_optional", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_nested_optional_argument_written_into_a_single_optional_constructor_parameter_is_rejected() {
    const SOURCE: &str = "\
        class Inner { m(x: number | null = 7): number | null { return x; } } \
        class C { constructor(p: number | undefined) { } } \
        export function f(k: Inner | undefined): number { \
            const z = k?.m(5); const c = new C(z); return 1; \
        }";
    let out = compile(SOURCE);
    let message = error_message(&out, "P0018");
    assert!(
        message.contains("two nested `Option`s"),
        "a nested optional value has no representation in a single-optional constructor parameter; \
         got {message:?}"
    );
    assert!(
        out.rust_source.is_none(),
        "no Rust may be emitted for a nested value written into a single-optional constructor \
         parameter"
    );
}

#[test]
fn single_optional_and_plain_constructor_arguments_keep_compiling_and_running() {
    const SOURCE: &str = "\
        class C { \
            v: number | undefined; \
            w: number; \
            constructor(p: number | undefined, q: number) { this.v = p; this.w = q; } \
        } \
        export function f(s: number | undefined): C { return new C(s, 5); }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    use ::core::option::Option::{None, Some};
    let c = ctor_plain_argument::f(&ctx, Some(3.0));
    assert_eq!(*c.v.borrow(), Some(3.0));
    assert_eq!(*c.w.borrow(), 5.0);
    let c = ctor_plain_argument::f(&ctx, None);
    assert_eq!(*c.v.borrow(), None);
    assert_eq!(*c.w.borrow(), 5.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let norm = normalized(&rust);
    assert!(
        !norm.contains("Option<Option<"),
        "a single-optional constructor parameter and a plain value stay flat; got:\n{rust}"
    );
    check_generated_crate("ctor_plain_argument", &rust);
    let stdout = run_generated_crate("ctor_plain_argument", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_nested_optional_string_argument_is_rejected_for_a_nested_optional_numeric_parameter() {
    const SOURCE: &str = "\
        type MaybeNum = number | null; \
        type MaybeStr = string | null; \
        function take(x: MaybeNum | undefined): number { return x === undefined ? 0 : 1; } \
        export function f(s: MaybeStr | undefined): number { return take(s); }";
    let out = compile(SOURCE);
    let message = error_message(&out, "E0408");
    assert!(
        message.contains("string and numeric"),
        "the innermost payload of a nested optional argument must be compared with the innermost \
         payload of a nested optional parameter; got {message:?}"
    );
    assert!(
        out.rust_source.is_none(),
        "no Rust may be emitted for a nested `string | null` argument passed to a nested numeric \
         optional parameter"
    );
}

#[test]
fn a_nested_optional_string_method_argument_is_rejected_for_a_nested_optional_numeric_parameter() {
    const SOURCE: &str = "\
        type MaybeNum = number | null; \
        type MaybeStr = string | null; \
        class K { m(x: MaybeNum | undefined): number { return x === undefined ? 0 : 1; } } \
        export function f(k: K, s: MaybeStr | undefined): number { return k.m(s); }";
    let out = compile(SOURCE);
    let message = error_message(&out, "E0408");
    assert!(
        message.contains("string and numeric"),
        "a nested optional method argument must be compared the same way; got {message:?}"
    );
    assert!(
        out.rust_source.is_none(),
        "no Rust may be emitted for a nested `string | null` method argument passed to a nested \
         numeric optional parameter"
    );
}

#[test]
fn a_single_optional_string_argument_is_rejected_for_a_nested_optional_numeric_parameter() {
    const SOURCE: &str = "\
        type MaybeNum = number | null; \
        function take(x: MaybeNum | undefined): number { return x === undefined ? 0 : 1; } \
        export function f(s: string | undefined): number { return take(s); }";
    let out = compile(SOURCE);
    let message = error_message(&out, "E0408");
    assert!(
        message.contains("string and numeric"),
        "a string payload cannot be written into a nested numeric optional parameter; got \
         {message:?}"
    );
    assert!(
        out.rust_source.is_none(),
        "no Rust may be emitted for a `string | undefined` argument passed to a nested numeric \
         optional parameter"
    );
}

#[test]
fn a_matching_nested_optional_argument_for_a_nested_optional_parameter_compiles_and_runs() {
    const SOURCE: &str = "\
        type MaybeNum = number | null; \
        function take(x: MaybeNum | undefined): number { return x === undefined ? 0 : (x ?? -1); } \
        export function f(s: MaybeNum | undefined): number { return take(s); } \
        export function g(n: number): number { return take(n); } \
        export function h(): number { return take(null); }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    use ::core::option::Option::{None, Some};
    assert_eq!(nested_payload_ok::f(&ctx, None), 0.0);
    assert_eq!(nested_payload_ok::f(&ctx, Some(None)), -1.0);
    assert_eq!(nested_payload_ok::f(&ctx, Some(Some(5.0))), 5.0);
    assert_eq!(nested_payload_ok::g(&ctx, 4.0), 4.0);
    assert_eq!(nested_payload_ok::h(&ctx), -1.0);
    println!("ok");
}
"#;
    let out = compile(SOURCE);
    assert!(
        codes(&out).is_empty(),
        "matching nested numeric payloads, plain numeric values and `null` must stay accepted; got \
         {:?}",
        out.diagnostics
    );
    let rust = out
        .rust_source
        .expect("emit-rust must populate rust_source");
    check_generated_crate("nested_payload_ok", &rust);
    let stdout = run_generated_crate("nested_payload_ok", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_matching_optional_argument_for_an_optional_parameter_compiles_and_runs() {
    const SOURCE: &str = "\
        function take(x: number | undefined): number { return x ?? 0; } \
        export function f(s: number | undefined): number { return take(s); }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    use ::core::option::Option::{None, Some};
    assert_eq!(matching_optional_argument::f(&ctx, Some(5.0)), 5.0);
    assert_eq!(matching_optional_argument::f(&ctx, None), 0.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    check_generated_crate("matching_optional_argument", &rust);
    let stdout = run_generated_crate("matching_optional_argument", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}
