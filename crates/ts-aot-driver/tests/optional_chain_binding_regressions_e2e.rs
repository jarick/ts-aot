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

fn compact(rust: &str) -> String {
    rust.chars().filter(|c| !c.is_whitespace()).collect()
}

#[test]
fn a_non_generic_optional_chain_call_binding_is_option() {
    const SOURCE: &str = "class K { plain(): number { return 7; } } \
         function f(k: K | undefined): number { const z = k?.plain(); return z === undefined ? 1 : 2; } \
         export function got(): number { \
             const p: K | undefined = new K(); const n: K | undefined = undefined; \
             return f(p) * 10 + f(n); \
         }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(non_generic_chain::got(&ctx), 21.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let norm = compact(&rust);
    assert!(
        norm.contains("letz:Option<f64>"),
        "a non-generic call through an optional chain yields `number | undefined`, so the local \
         must be `Option<f64>`; got:\n{rust}"
    );
    check_generated_crate("non_generic_chain", &rust);
    let stdout = run_generated_crate("non_generic_chain", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_non_generic_optional_chain_call_with_a_null_result_keeps_both_levels() {
    const SOURCE: &str = "class K { nul(): number | null { return null; } } \
         function f(k: K | undefined): number { const z = k?.nul(); return z === undefined ? 1 : 2; } \
         export function got(): number { \
             const p: K | undefined = new K(); const n: K | undefined = undefined; \
             return f(p) * 10 + f(n); \
         }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(non_generic_chain_null::got(&ctx), 21.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let norm = compact(&rust);
    assert!(
        norm.contains("letz:Option<Option<f64>>"),
        "`null` must keep its own level under the chain's `undefined`, because `null === \
         undefined` is false; got:\n{rust}"
    );
    assert!(
        !norm.contains("and_then"),
        "the nested result must not be flattened into one `Option`; got:\n{rust}"
    );
    check_generated_crate("non_generic_chain_null", &rust);
    let stdout = run_generated_crate("non_generic_chain_null", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_generic_optional_chain_call_binding_control_stays_option() {
    const SOURCE: &str = "class K { m<T>(x: T): T { return x; } } \
         function f(k: K | undefined): number { const z = k?.m(5); return z === undefined ? 1 : 2; } \
         export function got(): number { \
             const p: K | undefined = new K(); const n: K | undefined = undefined; \
             return f(p) * 10 + f(n); \
         }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(generic_chain_literal::got(&ctx), 21.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let norm = compact(&rust);
    assert!(
        norm.contains("letz:Option<i64>"),
        "the specialized `k?.m(5)` keeps the `undefined` alternative; got:\n{rust}"
    );
    check_generated_crate("generic_chain_literal", &rust);
    let stdout = run_generated_crate("generic_chain_literal", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_generic_optional_chain_call_with_an_undefined_argument_control_stays_single_option() {
    const SOURCE: &str = "class K { m<T>(x: T): T { return x; } } \
         function g(k: K | undefined, a: number | undefined): number { \
             const z = k?.m(a); return z === undefined ? 1 : 3; \
         } \
         export function got(): number { \
             const p: K | undefined = new K(); const n: K | undefined = undefined; \
             return g(p, undefined) * 100 + g(p, 4) * 10 + g(n, 4); \
         }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(generic_chain_undefined_arg::got(&ctx), 131.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let norm = compact(&rust);
    assert!(
        norm.contains("letz:Option<f64>") && !norm.contains("Option<Option<f64>>"),
        "an `undefined`-typed value type merges into the chain's `undefined`, so `z` stays a \
         single `Option<f64>`; got:\n{rust}"
    );
    check_generated_crate("generic_chain_undefined_arg", &rust);
    let stdout = run_generated_crate("generic_chain_undefined_arg", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_module_level_optional_chain_receiver_is_not_narrowed() {
    const SOURCE: &str = "class K { m<T>(x: T): T { return x; } } \
         let g: K | undefined = undefined; \
         export function absent(): number { return g?.m(g) === undefined ? 1 : 2; } \
         export function present(): number { g = new K(); return g?.m(g) === undefined ? 1 : 2; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(module_receiver_chain::absent(&ctx), 1.0);
    assert_eq!(module_receiver_chain::present(&ctx), 2.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let norm = compact(&rust);
    assert!(
        !norm.contains("(unsafe{g}).clone().unwrap()"),
        "a module-level receiver is a `static mut`, so the argument must not be narrowed with an \
         `unwrap()` that moves out of it; got:\n{rust}"
    );
    assert!(
        norm.contains("x:Option<K>"),
        "without narrowing, `T` is instantiated with the argument's own `K | undefined`; got:\n{rust}"
    );
    check_generated_crate("module_receiver_chain", &rust);
    let stdout = run_generated_crate("module_receiver_chain", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_local_optional_chain_receiver_is_still_narrowed_to_a_single_option() {
    const SOURCE: &str = "class K { m<T>(x: T): T { return x; } } \
         export function got(): number { \
             let l: K | undefined = new K(); \
             const r = l?.m(l); \
             return r === undefined ? 1 : 2; \
         }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(local_receiver_chain::got(&ctx), 2.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let norm = compact(&rust);
    assert!(
        !norm.contains("Option<Option<K>>"),
        "a local receiver is still narrowed to `K` inside the chain, so the result stays a single \
         `Option<K>`; got:\n{rust}"
    );
    check_generated_crate("local_receiver_chain", &rust);
    let stdout = run_generated_crate("local_receiver_chain", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_side_effecting_argument_of_an_optional_chain_call_follows_the_receiver() {
    const SOURCE: &str = "class K { m(x: number | undefined): number { bodies = bodies + 1; seen = calls + 100; return x ?? 7; } } \
         var calls: number = 0; \
         var bodies: number = 0; \
         var seen: number = 0; \
         function bump(): number { calls = calls + 1; return calls; } \
         export function run(k: K | undefined): number { return k?.m(void bump()) ?? -1; } \
         export function effects(): number { return calls; } \
         export function bodiesRun(): number { return bodies; } \
         export function seenAtCall(): number { return seen; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    use ::core::option::Option::{None, Some};
    assert_eq!(chain_arg_effect::run(&ctx, None), -1.0);
    assert_eq!(
        chain_arg_effect::effects(&ctx),
        0.0,
        "an absent receiver must skip the argument side effect"
    );
    assert_eq!(
        chain_arg_effect::bodiesRun(&ctx),
        0.0,
        "an absent receiver must skip the call itself"
    );
    assert_eq!(chain_arg_effect::seenAtCall(&ctx), 0.0);
    assert_eq!(
        chain_arg_effect::run(&ctx, Some(chain_arg_effect::K {})),
        7.0
    );
    assert_eq!(
        chain_arg_effect::effects(&ctx),
        1.0,
        "a present receiver must run the argument side effect exactly once"
    );
    assert_eq!(chain_arg_effect::bodiesRun(&ctx), 1.0);
    assert_eq!(
        chain_arg_effect::seenAtCall(&ctx),
        101.0,
        "the argument side effect must run exactly once before the method body"
    );
    assert_eq!(
        chain_arg_effect::run(&ctx, Some(chain_arg_effect::K {})),
        7.0
    );
    assert_eq!(chain_arg_effect::effects(&ctx), 2.0);
    assert_eq!(chain_arg_effect::bodiesRun(&ctx), 2.0);
    assert_eq!(
        chain_arg_effect::seenAtCall(&ctx),
        102.0,
        "every call must observe the counter incremented exactly once by its own argument"
    );
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    check_generated_crate("chain_arg_effect", &rust);
    let stdout = run_generated_crate("chain_arg_effect", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_super_method_call_reaches_the_parent_method() {
    const SOURCE: &str = "class A { m(): number { return 1; } } \
         class B extends A { m(): number { return super.m(); } } \
         export function got(): number { return new B().m(); }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(super_method_call::got(&ctx), 1.0);
    println!("ok");
}
"#;
    let out = compile_module(SOURCE);
    assert!(
        !out.has_errors(),
        "`super.m()` must compile; got {:?}",
        out.diagnostics
    );
    assert!(
        !out.diagnostics.iter().any(|d| d.code.as_str() == "P0010"),
        "`super.m()` must resolve to the parent method, not a placeholder field; got {:?}",
        out.diagnostics
    );
    let rust = out
        .rust_source
        .expect("emit-rust must populate rust_source");
    check_generated_crate("super_method_call", &rust);
    let stdout = run_generated_crate("super_method_call", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_guarded_optional_chain_call_snapshots_every_receiver_shape_by_clone() {
    const SOURCE: &str = "class K { m(a: number, b: number): number { return b; } } \
         class H { k: K | undefined = undefined; } \
         var g: K | undefined = undefined; \
         var seq: number = 0; \
         function bump(): number { seq = seq + 1; return seq; } \
         export function runLocal(): number { \
             seq = 0; const k: K | undefined = new K(); \
             const r = k?.m(bump(), k === undefined ? 1 : 2); \
             return (r === 2 ? 1 : 0) * 1000 + seq * 10; } \
         export function runParam(k: K | undefined): number { \
             seq = 0; const r = k?.m(bump(), k === undefined ? 1 : 2); \
             return (r === 2 ? 1 : 0) * 1000 + seq * 10; } \
         export function runArray(xs: (K | undefined)[]): number { \
             seq = 0; const r = xs[0]?.m(bump(), xs[0] === undefined ? 1 : 2); \
             return (r === 2 ? 1 : 0) * 1000 + seq * 10; } \
         export function runField(h: H): number { \
             seq = 0; h.k = new K(); \
             const r = h.k?.m(bump(), h.k === undefined ? 1 : 2); \
             return (r === 2 ? 1 : 0) * 1000 + seq * 10; } \
         export function runGlobal(): number { \
             seq = 0; g = new K(); \
             const r = g?.m(bump(), g === undefined ? 1 : 2); \
             return (r === 2 ? 1 : 0) * 1000 + seq * 10; } \
         export function runDouble(k: K | undefined): number { \
             seq = 0; const r = k?.m(bump(), bump()); \
             return (r === 2 ? 1 : 0) * 1000 + seq * 10; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    use ::core::option::Option::{None, Some};
    assert_eq!(chain_receiver_guard::runLocal(&ctx), 1010.0);
    assert_eq!(chain_receiver_guard::runParam(&ctx, None), 0.0);
    assert_eq!(chain_receiver_guard::runParam(&ctx, Some(chain_receiver_guard::K {})), 1010.0);
    let absent = ts_aot_runtime::bumpalo::vec![in ctx.arena; None];
    assert_eq!(chain_receiver_guard::runArray(&ctx, absent), 0.0);
    let empty = ts_aot_runtime::bumpalo::collections::Vec::new_in(ctx.arena);
    assert_eq!(
        chain_receiver_guard::runArray(&ctx, empty),
        0.0,
        "an empty array must yield `undefined` through the optional chain instead of panicking \
         with an out-of-bounds index"
    );
    let present = ts_aot_runtime::bumpalo::vec![in ctx.arena; Some(chain_receiver_guard::K {})];
    assert_eq!(chain_receiver_guard::runArray(&ctx, present), 1010.0);
    let h = chain_receiver_guard::H {
        k: ::std::rc::Rc::new(::core::cell::RefCell::new(None)),
    };
    assert_eq!(chain_receiver_guard::runField(&ctx, h), 1010.0);
    assert_eq!(chain_receiver_guard::runGlobal(&ctx), 1010.0);
    assert_eq!(chain_receiver_guard::runDouble(&ctx, None), 0.0);
    assert_eq!(chain_receiver_guard::runDouble(&ctx, Some(chain_receiver_guard::K {})), 1020.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let norm = compact(&rust);
    assert!(
        norm.contains(":Option<K>=k.clone();"),
        "a local receiver must be snapshotted by clone so the branch can still read it; got:\n{rust}"
    );
    assert!(
        norm.contains("__ts_aot_array_get_or_default::<Option<K>>("),
        "an array-element receiver must be read through the checked accessor, so an out-of-range \
         index yields `None`/`undefined` instead of panicking and the element is cloned out of \
         the array; got:\n{rust}"
    );
    assert!(
        !norm.contains(":Option<K>=k;") && !norm.contains(":Option<K>=xs[0asusize];"),
        "the receiver snapshot must never move a local or an array element; got:\n{rust}"
    );
    check_generated_crate("chain_receiver_guard", &rust);
    let stdout = run_generated_crate("chain_receiver_guard", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_guarded_optional_chain_call_transposes_a_throwing_callee_inside_the_guard() {
    const SOURCE: &str = "class K { m(a: number, b: number): number { throw 1; } } \
         var seq: number = 0; \
         function bump(): number { seq = seq + 1; return seq; } \
         export function run(k: K | undefined): number { \
             seq = 0; return k?.m(bump(), k === undefined ? 1 : 2) ?? -1; } \
         export function effects(): number { return seq; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    use ::core::option::Option::{None, Some};
    assert_eq!(chain_throw_guard::run(&ctx, None), Ok(-1.0));
    assert_eq!(
        chain_throw_guard::effects(&ctx),
        0.0,
        "an absent receiver must skip the argument side effects"
    );
    assert_eq!(
        chain_throw_guard::run(&ctx, Some(chain_throw_guard::K {})),
        Err(1.0)
    );
    assert_eq!(
        chain_throw_guard::effects(&ctx),
        1.0,
        "a present receiver must run the argument side effect exactly once before the throw"
    );
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let norm = compact(&rust);
    assert!(
        norm.contains(":Option<K>=k.clone();"),
        "the throwing-callee guard must also clone its receiver snapshot; got:\n{rust}"
    );
    assert!(
        norm.contains(").clone().unwrap()?)"),
        "the throwing call must still be transposed inside the receiver-present branch; got:\n{rust}"
    );
    check_generated_crate("chain_throw_guard", &rust);
    let stdout = run_generated_crate("chain_throw_guard", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_super_method_call_runs_the_parent_body_on_the_subclass_receiver() {
    const SOURCE: &str = "class A { v: number = 1; m(x: number): number { return x + this.v; } } \
         class B extends A { m(): number { this.v = 100; return super.m(4) + 10; } } \
         export function got(): number { return new B().m(); }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(
        super_method_args::got(&ctx),
        114.0,
        "`super.m(4)` must run the parent body on the subclass instance, so `this.v` reads the \
         value `B.m` stored just before the call (100) and the result is 4 + 100 + 10; a stale \
         receiver, or a super receiver that kept the parent's field, would yield 4 + 1 + 10 = 15"
    );
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let norm = compact(&rust);
    assert!(
        norm.contains("self.v.borrow().clone()"),
        "the parent body reached through `super.m` must read the inherited field off the receiver \
         it was given; got:\n{rust}"
    );
    check_generated_crate("super_method_args", &rust);
    let stdout = run_generated_crate("super_method_args", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}
