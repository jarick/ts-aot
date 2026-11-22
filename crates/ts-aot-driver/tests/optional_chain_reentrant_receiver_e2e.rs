use ts_aot_driver::{CompileOptions, Driver, EmitStage};
use ts_aot_test_support::run_generated_crate;

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

fn normalized(rust: &str) -> String {
    rust.chars().filter(|c| !c.is_whitespace()).collect()
}

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

const REPRO_SOURCE: &str = "class E { v: number = 0; m(x: number): void { this.v = x; } } \
     class C { e: E | null = null; swap(): number { this.e = new E(); return 5; } } \
     export function got(): number { \
         const c = new C(); const e = new E(); c.e = e; \
         c.e?.m(c.swap()); \
         return e.v; \
     }";

#[test]
fn an_optional_call_on_a_function_value_dispatches_through_the_fn_call_helper() {
    const SOURCE: &str = "let hits: number = 0; \
         function arg(): number { hits = hits + 1; return 3; } \
         export function call(f: ((x: number) => number) | null): number { \
             const r: number | undefined = f?.(arg()); \
             return r ?? -1; \
         } \
         export function got(): number { \
             const g = (x: number): number => x + 4; \
             return call(g) * 10 + call(null) + hits; \
         }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(optional_fn_value_call::got(&ctx), 70.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let body = function_body(&rust, "call");
    assert!(
        body.contains("map(|__ts_recv|__ts_fn_call_0(__ctx,__ts_recv,arg(__ctx)))"),
        "`f?.(...)` on a function value must dispatch through the generated `__ts_fn_call_N` \
         helper inside the `Some` arm, with the argument evaluated inside that arm so `None` \
         skips it; got:\n{body}"
    );
    let stdout = run_generated_crate("optional_fn_value_call", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn an_optional_method_call_does_not_shadow_a_source_binding_named_like_the_receiver() {
    const SOURCE: &str = "class E { v: number = 0; m(x: number): void { this.v = x; } } \
         export function got(): number { \
             const orig = new E(); \
             let e: E | null = orig; \
             let __ts_recv: number = 7; \
             e?.m(__ts_recv); \
             return (e as E).v; \
         }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(optional_chain_shadow::got(&ctx), 7.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let body = function_body(&rust, "got");
    assert!(
        body.contains("map(|__ts_recv_|"),
        "the unwrapped optional receiver must be bound to a generated name that is distinct from \
         every source binding, so the argument `__ts_recv` keeps its own value; got:\n{body}"
    );
    let stdout = run_generated_crate("optional_chain_shadow", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn the_optional_chain_receiver_releases_the_cell_borrow_before_the_call() {
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(optional_chain_reentrant::got(&ctx), 5.0);
    println!("ok");
}
"#;
    let rust = rust_of(REPRO_SOURCE);
    let stdout = run_generated_crate("optional_chain_reentrant", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
    let body = function_body(&rust, "got");
    assert!(
        body.contains(
            "let__ts_recv_opt=(c.e.borrow().clone()).clone();__ts_recv_opt.as_ref().map("
        ),
        "`c.e?.m(c.swap())` must clone the optional receiver into a local before the closure runs, \
         so the `Ref` taken from `c.e` is dropped before `C__swap` borrows the same cell mutably; \
         got:\n{body}"
    );
    assert!(
        body.contains("let__ts_recv_opt=") && !body.contains(".borrow().clone().as_ref().map("),
        "the optional receiver must be cloned into a hoisted local and mapped over that local, not \
         mapped over a `.borrow().clone()` expression inline, because that keeps the `Ref` alive \
         across the callee and panics when the argument re-enters the field; got:\n{body}"
    );
}

#[test]
fn every_optional_chain_receiver_shape_releases_the_cell_borrow() {
    const SOURCE: &str = "class E { \
             v: number = 0; \
             m(x: number): void { this.v = x; } \
             add(x: number): number { return this.v + x; } \
         } \
         class D { e: E | null = null; } \
         class C { \
             e: E | null = null; \
             d: D | null = null; \
             arr: (E | null)[] = []; \
             swap(): number { this.e = new E(); return 5; } \
             swapD(): number { this.d = new D(); return 5; } \
             swapArr(): number { this.arr[0] = new E(); return 7; } \
         } \
         export function got_field(): number { \
             const c = new C(); const e = new E(); c.e = e; \
             c.e?.m(c.swap()); \
             return e.v; \
         } \
         export function got_readonly(): number { \
             const c = new C(); const e = new E(); c.e = e; \
             return c.e?.add(c.swap()) ?? -1; \
         } \
         export function got_index(): number { \
             const c = new C(); const e = new E(); \
             const arr: (E | null)[] = [e, null]; \
             c.arr = arr; \
             c.arr[0]?.m(c.swapArr()); \
             return e.v; \
         } \
         export function got_nested(): number { \
             const c = new C(); const d = new D(); const e = new E(); d.e = e; c.d = d; \
             c.d?.e?.m(c.swapD()); \
             return e.v; \
         } \
         export function got_local(): number { \
             const orig = new E(); \
             let e: E | null = orig; \
             const swap = (): number => { e = new E(); return 5; }; \
             e?.m(swap()); \
             return orig.v; \
         }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(optional_chain_shapes::got_field(&ctx), 5.0);
    assert_eq!(optional_chain_shapes::got_readonly(&ctx), 5.0);
    assert_eq!(optional_chain_shapes::got_index(&ctx), 7.0);
    assert_eq!(optional_chain_shapes::got_nested(&ctx), 5.0);
    assert_eq!(optional_chain_shapes::got_local(&ctx), 5.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate("optional_chain_shapes", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
    for name in [
        "got_field",
        "got_readonly",
        "got_index",
        "got_nested",
        "got_local",
    ] {
        let body = function_body(&rust, name);
        assert!(
            body.contains("let__ts_recv_opt=") && body.contains(";__ts_recv_opt.as_ref().map("),
            "`{name}` must drop the receiver path's cell borrow in the `let` binding and map over \
             that binding, so no `Ref` outlives the callee; got:\n{body}"
        );
    }
}
