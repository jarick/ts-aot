use ts_aot_driver::{CompileOptions, Driver, EmitStage};
use ts_aot_test_support::run_generated_crate;

const SOURCE: &str = "class K { n: number = 0; } \
     export function f(n: i64): i64 { \
         const wm: WeakMap<K, i64> = new WeakMap(); \
         let k: K = new K(); \
         const g = (): number => k.n; \
         wm.set(k, 42); \
         let r: i64 = 0; \
         for (let i: i64 = 0; i < n; i = i + 1) { r = wm.get(k); k = k; } \
         return r; \
     }";

const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(weakmap_shared_key_identity::f(&ctx, 2), 42);
    println!("ok");
}
"#;

const AFTER_REASSIGNMENT_SOURCE: &str = "class K { n: number = 0; } \
     export function f(): i64 { \
         const wm: WeakMap<K, i64> = new WeakMap(); \
         let k: K = new K(); \
         const g = (): number => k.n; \
         wm.set(k, 42); \
         k = k; \
         return wm.get(k); \
     }";

const AFTER_REASSIGNMENT_MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(weakmap_key_read_after_reassignment::f(&ctx), 42);
    println!("ok");
}
"#;

#[test]
fn self_assignment_of_a_captured_weakmap_key_keeps_its_entries() {
    let out = Driver::new().compile_source(
        "test.ts",
        SOURCE,
        &CompileOptions {
            emit: EmitStage::Rust,
            module: true,
        },
    );
    assert!(
        !out.has_errors(),
        "the repro must compile through the full pipeline; got {:?}",
        out.diagnostics
    );
    let rust = out
        .rust_source
        .expect("emit-rust must populate rust_source");
    let norm: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        norm.contains("let__ts_aot_weakmap_key_") && norm.contains("if__ts_aot_weakmap_key_"),
        "reassigning a captured WeakMap key must refresh its liveness token only when the object \
         identity changed, so the emitted guard must record the previous field cell and compare \
         it with the new one; got:\n{rust}"
    );
    let stdout = run_generated_crate("weakmap_shared_key_identity", &rust, MAIN);
    assert!(
        stdout.contains("ok"),
        "`k = k` must not purge the entry stored for the still-identical object, so \
         `wm.get(k)` must return 42; got stdout:\n{stdout}"
    );
}

#[test]
fn weakmap_read_after_a_captured_key_reassignment_keeps_the_entry() {
    let out = Driver::new().compile_source(
        "test.ts",
        AFTER_REASSIGNMENT_SOURCE,
        &CompileOptions {
            emit: EmitStage::Rust,
            module: true,
        },
    );
    assert!(
        !out.has_errors(),
        "the repro must compile through the full pipeline; got {:?}",
        out.diagnostics
    );
    let rust = out
        .rust_source
        .expect("emit-rust must populate rust_source");
    let stdout = run_generated_crate(
        "weakmap_key_read_after_reassignment",
        &rust,
        AFTER_REASSIGNMENT_MAIN,
    );
    assert!(
        stdout.contains("ok"),
        "a WeakMap read that happens after `k = k` must share the write's liveness token, so \
         `wm.get(k)` must return the 42 stored before the reassignment; got stdout:\n{stdout}"
    );
}

const COPY_BACK_SOURCE: &str = "class K { n: number = 0; } \
     export function f(): i64 { \
         const wm: WeakMap<K, i64> = new WeakMap(); \
         let k: K = new K(); \
         let a = k; \
         wm.set(k, 7); \
         k = a; \
         return wm.get(k); \
     }";

const COPY_BACK_MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(weakmap_key_copy_back::f(&ctx), 7);
    println!("ok");
}
"#;

const REMAP_SOURCE: &str = "class K { n: number = 0; } \
     export function f(): i64 { \
         const wm: WeakMap<K, i64> = new WeakMap(); \
         let k: K = new K(); \
         wm.set(k, 1); \
         k = new K(); \
         wm.set(k, 2); \
         k = k; \
         return wm.get(k); \
     }";

const REMAP_MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(weakmap_key_remap::f(&ctx), 2);
    println!("ok");
}
"#;

fn emit_source(source: &str) -> String {
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
        "the repro must compile through the full pipeline; got {:?}",
        out.diagnostics
    );
    out.rust_source
        .expect("emit-rust must populate rust_source")
}

#[test]
fn a_weakmap_read_after_a_key_copy_back_keeps_the_entry() {
    let rust = emit_source(COPY_BACK_SOURCE);
    let stdout = run_generated_crate("weakmap_key_copy_back", &rust, COPY_BACK_MAIN);
    assert!(
        stdout.contains("ok"),
        "`k = a` must keep reading through the same key token as the earlier `wm.set(k, 7)`, so \
         `wm.get(k)` must return 7; got stdout:\n{stdout}"
    );
}

#[test]
fn a_weakmap_read_after_a_key_remap_and_self_assignment_returns_the_latest_entry() {
    let rust = emit_source(REMAP_SOURCE);
    let stdout = run_generated_crate("weakmap_key_remap", &rust, REMAP_MAIN);
    assert!(
        stdout.contains("ok"),
        "the latest `wm.set(k, 2)` entry must survive the trailing `k = k`, so `wm.get(k)` must \
         return 2; got stdout:\n{stdout}"
    );
}

const CLOSURE_RESET_SOURCE: &str = "class K { n: number = 0; } \
     export function f(): i64 { \
         const wm: WeakMap<K, i64> = new WeakMap(); \
         let k: K = new K(); \
         wm.set(k, 5); \
         const g = (): void => { k = new K(); }; \
         g(); \
         return wm.has(k) ? 1 : 0; \
     }";

const CLOSURE_RESET_MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(weakmap_closure_reset::f(&ctx), 0);
    println!("ok");
}
"#;

#[test]
fn a_closure_that_only_assigns_a_captured_key_invalidates_the_entry() {
    let rust = emit_source(CLOSURE_RESET_SOURCE);
    let stdout = run_generated_crate("weakmap_closure_reset", &rust, CLOSURE_RESET_MAIN);
    assert!(
        stdout.contains("ok"),
        "a closure that only reassigns a captured WeakMap key, without any WeakMap operation of \
         its own, must refresh the shared liveness token so the enclosing `wm.has(k)` observes the \
         entry as gone; got stdout:\n{stdout}"
    );
}
