//! `new C(args)` evaluation order.
//!
//! JavaScript evaluates the callee, then the arguments, then the field
//! initializers, and only then the constructor body. The AOT backend lowers the
//! call as `C::C__constructor(&mut alloc, __ctx, args...)`, so an argument that
//! is not pinned before the allocation would run *after* the field initializers
//! (they are emitted as the `StructLiteral` initializer of that allocation).
//! These tests compile a module through the whole pipeline, look at the emitted
//! Rust and run the generated crate against the JavaScript result.
//!
//! Note: a field initializer with a side effect (or one that reads a global) is
//! rejected by the frontend today (`E0406`, the field keeps its zero value), so
//! for such an initializer only the *statement order* can be observed; the
//! runtime value below it comes from a literal initializer.

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

/// The generated source without whitespace, which is what the quote-based
/// backend makes stable across formatting changes.
fn normalized(rust: &str) -> String {
    rust.chars().filter(|c| !c.is_whitespace()).collect()
}

/// The body of `pub fn <name><'a>(...)` from normalized output, cut at the next
/// top-level function (the dispatch wrapper that always follows).
fn function_body(rust: &str, name: &str) -> String {
    let norm = normalized(rust);
    let signature = format!("pubfn{name}<'a>(");
    let start = norm
        .find(&signature)
        .unwrap_or_else(|| panic!("no `{name}` in generated code:\n{norm}"));
    let tail = &norm[start..];
    let end = tail[1..].find("pubfn").map_or(tail.len(), |i| i + 1);
    tail[..end].to_owned()
}

/// An argument that calls a function is hoisted into a temporary *before* the
/// instance is allocated, so its side effects precede the field initializers.
#[test]
fn an_argument_is_materialized_before_the_instance_allocation() {
    const SOURCE: &str = "let order: number = 0; \
         function arg(): number { order = 1; return 5; } \
         class C { n: number = order === 1 ? 2 : 9; constructor(x: number) { } } \
         export function run(): number { order = 0; const c = new C(arg()); return c.n; }";
    let out = compile_module(SOURCE);
    assert!(!out.has_errors(), "got {:?}", out.diagnostics);
    assert!(
        out.diagnostics.iter().any(|d| d.code.as_str() == "E0406"),
        "a non-literal field initializer is still unsupported (`E0406`, the field keeps its zero \
         value), which is why this test asserts the statement order and not `c.n`; got {:?}",
        out.diagnostics
    );

    let rust = out
        .rust_source
        .expect("emit-rust must populate rust_source");
    let body = function_body(&rust, "run");
    let arg_at = body
        .find("arg(__ctx)")
        .unwrap_or_else(|| panic!("the argument call must survive; got:\n{body}"));
    let alloc_at = body
        .find("C{n:0.0}")
        .unwrap_or_else(|| panic!("the allocation must stay; got:\n{body}"));
    let ctor_at = body
        .find("C::C__constructor(")
        .unwrap_or_else(|| panic!("the constructor call must stay; got:\n{body}"));
    assert!(
        arg_at < alloc_at,
        "the argument must be evaluated before the instance (and so before the field \
         initializers) is emitted; got:\n{body}"
    );
    assert!(
        alloc_at < ctor_at,
        "the instance must be allocated before the constructor runs; got:\n{body}"
    );
}

/// The JavaScript result of the probe above is `2` (`arg()` sets `order` before
/// the initializer reads it); with the initializer unsupported the AOT target
/// zero-fills the field, so the argument's side effect is checked through a
/// literal initializer instead.
#[test]
fn an_argument_is_evaluated_before_the_field_initializer_and_the_constructor_body() {
    const SOURCE: &str = "let order: number = 0; \
         function arg(): number { order = 1; return 5; } \
         class C { n: number = 2; constructor(x: number) { } } \
         export function run(): number { order = 0; const c = new C(arg()); return c.n * 10 + order; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    // `arg()` ran (order == 1) and the literal initializer applied (n == 2).
    assert_eq!(some_init_order::run(&ctx), 21.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    check_generated_crate("some_init_order", &rust);
    let stdout = run_generated_crate("some_init_order", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

/// The argument is evaluated before the constructor body.
#[test]
fn an_argument_runs_before_the_constructor_body() {
    const SOURCE: &str = "let o2: number = 0; \
         function arg2(): number { o2 = 1; return 7; } \
         class D { constructor(x: number) { if (o2 === 1) { o2 = 2; } else { o2 = 3; } } } \
         export function run2(): number { o2 = 0; const d = new D(arg2()); return o2; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    // `arg2()` set o2 = 1, so the body takes the `2` branch (JavaScript: 2).
    assert_eq!(ctor_body_order::run2(&ctx), 2.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let body = function_body(&rust, "run2");
    let arg_at = body.find("arg2(__ctx)").expect("argument call");
    let ctor_at = body.find("D::D__constructor(").expect("constructor call");
    assert!(
        arg_at < ctor_at,
        "the argument must be evaluated before the constructor is called; got:\n{body}"
    );
    check_generated_crate("ctor_body_order", &rust);
    let stdout = run_generated_crate("ctor_body_order", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

/// Control: a constructor without arguments keeps allocating and calling in the
/// same order, and the field initializer still applies.
#[test]
fn a_constructor_without_arguments_is_unchanged() {
    const SOURCE: &str = "class E { v: number = 3; constructor() { } } \
         export function run3(): number { const e = new E(); return e.v; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(no_args_ctor::run3(&ctx), 3.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let body = function_body(&rust, "run3");
    let alloc_at = body.find("E{v:3.0}").expect("allocation with initializer");
    let ctor_at = body.find("E::E__constructor(").expect("constructor call");
    assert!(
        alloc_at < ctor_at,
        "without arguments the allocation still precedes the call; got:\n{body}"
    );
    check_generated_crate("no_args_ctor", &rust);
    let stdout = run_generated_crate("no_args_ctor", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

/// A defaulted parameter reaches the constructor as an `Option`: the argument is
/// pinned with the parameter's ABI type (`Some(<expr>)`), and it still runs
/// before the constructor body applies the default.
#[test]
fn an_argument_of_a_defaulted_parameter_is_materialized_with_the_parameter_type() {
    const SOURCE: &str = "let hits: number = 0; \
         function bump(): number { hits = hits + 1; return 9; } \
         class F { v: number; constructor(x: number = 5) { this.v = x; } } \
         export function run4(): number { const f = new F(bump()); return f.v * 10 + hits; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    // `bump()` ran once (hits == 1) and its value overrode the default (x == 9).
    assert_eq!(defaulted_ctor_arg::run4(&ctx), 91.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let body = function_body(&rust, "run4");
    let arg_at = body.find("bump(__ctx)").expect("argument call");
    let alloc_at = body.find("F{v:0.0}").expect("allocation");
    assert!(
        arg_at < alloc_at,
        "the argument must be evaluated before the instance; got:\n{body}"
    );
    check_generated_crate("defaulted_ctor_arg", &rust);
    let stdout = run_generated_crate("defaulted_ctor_arg", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

/// Several arguments keep their left-to-right order (they are pinned in
/// sequence), and an omitted defaulted parameter is still padded.
#[test]
fn several_arguments_keep_their_order_and_padding() {
    const SOURCE: &str = "let seenTwo: boolean = false; \
         let oneSawTwo: boolean = false; \
         function one(): number { if (seenTwo) { oneSawTwo = true; } return 1; } \
         function two(): number { seenTwo = true; return 2; } \
         class K { a: number; b: number; constructor(x: number, y: number = 30) { this.a = x; this.b = y; } } \
         export function run6(): number { const k = new K(one(), two()); \
         if (oneSawTwo) { return -1; } const j = new K(one()); return k.a * 10 + k.b + j.b; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    // `one()` ran before `two()` (no `-1`), `k.b == 2`, and the second call used
    // the default for the omitted parameter (`j.b == 30`).
    assert_eq!(two_args_order::run6(&ctx), 42.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let body = function_body(&rust, "run6");
    let one_at = body.find("one(__ctx)").expect("first argument");
    let two_at = body.find("two(__ctx)").expect("second argument");
    let alloc_at = body.find("K{a:0.0,b:0.0}").expect("allocation");
    assert!(
        one_at < two_at && two_at < alloc_at,
        "arguments must be evaluated left to right, before the instance; got:\n{body}"
    );
    check_generated_crate("two_args_order", &rust);
    let stdout = run_generated_crate("two_args_order", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

/// A `new` whose callee this target does not lower (`new Error(...)`) still has
/// to evaluate its arguments: they are ordinary JavaScript expressions whose
/// side effects are observable. The fallback used to drop them silently, so the
/// call was lost and the warning with it.
///
/// This is a pass-level assertion: the fallback also allocates a struct the
/// generated crate does not declare (`__struct1`), so the crate would not
/// compile for an unrelated reason.
#[test]
fn arguments_of_an_unlowered_new_are_still_evaluated() {
    const SOURCE: &str = "let hits: number = 0; \
         function bump(): number { hits = hits + 1; return 1; } \
         export function run(): number { new Error(bump()); return hits; }";
    let out = compile_module(SOURCE);
    assert!(
        !out.has_errors(),
        "an unlowered constructor is a warning, not an error; got {:?}",
        out.diagnostics
    );
    assert!(
        out.diagnostics.iter().any(|d| d.code.as_str() == "E0406"
            && d.message
                .contains("constructor arguments are not supported")),
        "the ignored constructor arguments must be reported; got {:?}",
        out.diagnostics
    );
    let rust = out
        .rust_source
        .expect("emit-rust must populate rust_source");
    assert!(
        normalized(&rust).contains("bump(__ctx);"),
        "the argument's side effect must reach the MIR; got:\n{rust}"
    );
}

fn codes(source: &str) -> Vec<String> {
    compile_module(source)
        .diagnostics
        .iter()
        .map(|d| d.code.as_str().to_owned())
        .collect()
}

/// A constructor that *reads another object's field* may run that class's
/// accessor, and the scan of the constructor body cannot see it: it only knows
/// the class being constructed (whose own accessors are rejected up front), so
/// `o.value` has to count as call-like. Such a `new` therefore keeps the
/// conservative withdrawal of the narrowings of captured variables - the closure
/// `set` may have been reached through the accessor and write `x`.
#[test]
fn reading_another_objects_field_makes_a_new_call_like() {
    const SOURCE: &str = "class Box { value: number; constructor() { this.value = 1; } } \
         class Holder { v: number; constructor(o: Box) { this.v = o.value; } } \
         export function f(o: Box): number { \
         let x: number | null = null; const set = (): void => { x = null; }; \
         x = 1; new Holder(o); return x; }";
    // The control reads the field *outside* the constructor and passes a plain
    // value, so nothing in the `new` can reach a callback and the narrowing of
    // the captured variable survives it.
    const CONTROL: &str = "class Box { value: number; constructor() { this.value = 1; } } \
         class Holder { v: number; constructor(v: number) { this.v = v; } } \
         export function f(b: Box): number { \
         let x: number | null = null; const set = (): void => { x = null; }; \
         x = 1; new Holder(b.value); return x; }";
    let diagnostics = codes(SOURCE);
    assert!(
        diagnostics.iter().any(|code| code == "P0018"),
        "`o.value` may run `Box`'s accessor, so the narrowing of the captured `x` has to be \
         withdrawn and `return x` reported; got {diagnostics:?}"
    );

    let diagnostics = codes(CONTROL);
    assert!(
        !diagnostics.iter().any(|code| code == "P0018"),
        "a constructor that reads no other object's field keeps the narrowing; \
         got {diagnostics:?}"
    );
}
