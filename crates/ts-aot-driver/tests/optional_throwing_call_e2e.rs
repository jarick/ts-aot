//! A *throwing* method called through an optional chain (`a?.m()` where `m`
//! throws).
//!
//! `a?.m()` is emitted as a `map` over the receiver, so the call yields an
//! `Option<Result<T, E>>` - not the `Option<T>` its MIR type describes. Applying
//! `?` (what `MirExpr::Propagate` emits) to that value does not compile: the
//! enclosing function returns `Result<T, E>`, not `Option<..>`. The lowering
//! therefore transposes `Option<Result<T, E>>` into `Result<Option<T>, E>`
//! before propagating:
//!
//! ```text
//! let __mapped: Option<Result<T, E>> = a.as_ref().map(|r| A::m(r));
//! let mut __value: Option<T>;
//! if __mapped.is_some() { __value = Some(__mapped.unwrap()?); }
//! else { __value = None; }
//! ```
//!
//! When the enclosing context cannot propagate at all (a closure body, a `try`
//! block, a generator or top-level code) the call is rejected with `E0416`
//! instead of emitting Rust that cannot compile.
//!
//! The generated crates are compiled and their runtime results compared with
//! JavaScript: `a = null` yields the fallback without calling the method, while
//! a present receiver runs the method and propagates its `throw`.

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

/// Whitespace-free Rust, so a shape can be asserted without depending on the
/// emitter's spacing.
fn normalized(rust: &str) -> String {
    rust.chars().filter(|c| !c.is_whitespace()).collect()
}

const THROWING_METHOD: &str = "class A { m(): number { throw 1; } } ";

#[test]
fn throwing_method_through_an_optional_chain_is_transposed_before_propagating() {
    const SOURCE: &str = "class A { m(): number { throw 1; } } \
         export function opt(a: A | null): number { return a?.m() ?? 0; } \
         export function opt2(a: A | null): number | undefined { return a?.m(); } \
         export function plain(a: A): number { return a.m(); }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    // a = null: `a?.m()` is absent, so `?? 0` yields 0 and `m` never runs.
    assert_eq!(opt_throw::opt(&ctx, ::core::option::Option::None), Ok(0.0));
    // a is present: `m` runs and throws 1, which the caller propagates.
    assert_eq!(
        opt_throw::opt(&ctx, ::core::option::Option::Some(opt_throw::A {})),
        Err(1.0)
    );
    assert_eq!(
        opt_throw::opt2(&ctx, ::core::option::Option::None),
        Ok(::core::option::Option::None)
    );
    assert_eq!(
        opt_throw::opt2(&ctx, ::core::option::Option::Some(opt_throw::A {})),
        Err(1.0)
    );
    // The non-optional call is unchanged: it always runs and always throws.
    assert_eq!(opt_throw::plain(&ctx, opt_throw::A {}), Err(1.0));
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let norm = normalized(&rust);
    // The mapped-over-the-receiver call is bound as `Option<Result<T, E>>`, and
    // the `Result` is propagated *inside* the `Some` arm - `?` is never applied
    // to the `Option<Result<..>>` itself (which would not compile).
    assert!(
        norm.contains("Option<Result<f64,f64>>"),
        "`a?.m()` where `m` throws must bind the mapped call as \
         `Option<Result<f64, f64>>`; got:\n{rust}"
    );
    assert!(
        norm.contains(".unwrap()?"),
        "the `Result` inside the mapped `Option` must be propagated in the `Some` \
         arm (`Some(v?)`), not by applying `?` to the `Option<Result<..>>`; got:\n{rust}"
    );
    assert!(
        !norm.contains(".map(|__ts_recv|A::A__m(__ts_recv.clone(),__ctx,))?"),
        "`?` must never be applied directly to the optional-chain call mapping; got:\n{rust}"
    );
    assert!(
        norm.contains("A::A__m(a.clone(),__ctx)?"),
        "the non-optional `a.m()` path must keep its plain `expr?`; got:\n{rust}"
    );

    check_generated_crate("opt_throw", &rust);
    let stdout = run_generated_crate("opt_throw", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

/// `?` needs a `Result` to propagate into, and a string-returning throwing
/// method exercises the non-`Copy` value the transposed `Option` holds.
#[test]
fn transposing_keeps_working_for_a_non_copy_optional_result() {
    const SOURCE: &str = "class B { s(): string { throw 1; } } \
         export function optStr(b: B | null): string { return b?.s() ?? \"fallback\"; } \
         export function optStrOpt(b: B | null): string | undefined { return b?.s(); }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(
        opt_str::optStr(&ctx, ::core::option::Option::None),
        Ok("fallback".into())
    );
    assert_eq!(
        opt_str::optStrOpt(&ctx, ::core::option::Option::None),
        Ok(::core::option::Option::None)
    );
    assert!(opt_str::optStr(&ctx, ::core::option::Option::Some(opt_str::B {})).is_err());
    assert!(opt_str::optStrOpt(&ctx, ::core::option::Option::Some(opt_str::B {})).is_err());
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let norm = normalized(&rust);
    assert!(
        norm.contains("Option<Result<ts_aot_runtime::JsString<'a>,f64>>"),
        "a non-`Copy` result is transposed the same way; got:\n{rust}"
    );
    check_generated_crate("opt_str", &rust);
    let stdout = run_generated_crate("opt_str", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

/// A context that returns no `Result` cannot propagate the callee's error, so
/// the call is reported (`E0416`) instead of emitting a transposition that has
/// nowhere to send the error - and no Rust is produced at all.
#[test]
fn a_throwing_optional_call_outside_a_result_context_is_reported() {
    let closure = format!(
        "{THROWING_METHOD} export function f(a: A | null): number {{ \
         const g = (): number => a?.m() ?? 0; return g(); }}"
    );
    let try_block = format!(
        "{THROWING_METHOD} export function f(a: A | null): number {{ \
         try {{ return a?.m() ?? 0; }} catch (e) {{ return -1; }} }}"
    );
    let generator = format!(
        "{THROWING_METHOD} export function* f(a: A | null): number {{ yield a?.m() ?? 0; }}"
    );
    let cases: [(&str, &str, &str); 3] = [
        ("closure body", closure.as_str(), "cannot propagate it"),
        ("try block", try_block.as_str(), "cannot catch an error"),
        ("generator body", generator.as_str(), "cannot propagate it"),
    ];
    for (context, source, expected) in cases {
        let out = compile_module(source);
        assert!(
            out.has_errors(),
            "a throwing optional call in a {context} must be rejected; got {:?}",
            out.diagnostics
        );
        let diagnostic = out
            .diagnostics
            .iter()
            .find(|d| d.code.as_str() == "E0416")
            .unwrap_or_else(|| panic!("expected E0416 for a {context}; got {:?}", out.diagnostics));
        assert!(
            diagnostic.message.contains(expected),
            "the {context} E0416 must explain why the error cannot be propagated \
             (expected {expected:?}); got: {}",
            diagnostic.message
        );
        assert!(
            diagnostic.message.contains("A::m"),
            "the {context} E0416 must name the throwing method; got: {}",
            diagnostic.message
        );
        assert!(
            out.rust_source.is_none(),
            "no Rust may be produced for a rejected {context}; got:\n{:?}",
            out.rust_source
        );
    }
}
