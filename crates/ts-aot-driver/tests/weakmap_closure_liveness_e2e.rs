use ts_aot_driver::{CompileOptions, Driver, EmitStage};
use ts_aot_test_support::run_generated_crate;

const CAPTURED_KEY: &str = "class K { n: number = 0; } \
     export function f(): i64 { \
         const wm: WeakMap<K, i64> = new WeakMap(); \
         let k = new K(); \
         wm.set(k, 5); \
         const g = (): number => wm.get(k); \
         return g(); \
     }";

const CAPTURED_KEY_MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(weakmap_closure_liveness::f(&ctx), 5);
    println!("ok");
}
"#;

const CLOSURE_LOCAL_KEY: &str = "class K { n: number = 0; } \
     export function run(): i64 { \
         const wm: WeakMap<K, i64> = new WeakMap(); \
         const g = (): number => { const k = new K(); wm.set(k, 9); return wm.get(k); }; \
         return g(); \
     }";

const CLOSURE_LOCAL_KEY_MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(wm_closure_scope::run(&ctx), 9);
    println!("ok");
}
"#;

const ASSIGN_ONLY: &str = "class K { n: number = 0; } \
     export function g(): i64 { \
         const wm: WeakMap<K, i64> = new WeakMap(); \
         let k = new K(); \
         wm.set(k, 7); \
         const h = (): number => { k = new K(); return 3; }; \
         return h(); \
     }";

const ASSIGN_ONLY_MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(weakmap_closure_assign::g(&ctx), 3);
    println!("ok");
}
"#;

fn emit(package: &str, source: &str) -> String {
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
        "{package} must compile through the full pipeline; got {:?}",
        out.diagnostics
    );
    out.rust_source
        .expect("emit-rust must populate rust_source")
}

#[test]
fn a_weakmap_read_in_a_lifted_closure_sees_the_enclosing_write() {
    let rust = emit("weakmap_closure_liveness", CAPTURED_KEY);
    assert!(
        rust.contains("__ts_closure_"),
        "the repro's arrow function must be lowered to a lifted closure helper; got:\n{rust}"
    );
    let stdout = run_generated_crate("weakmap_closure_liveness", &rust, CAPTURED_KEY_MAIN);
    assert!(
        stdout.contains("ok"),
        "`wm.get(k)` inside the lifted closure must observe the value stored by the enclosing \
         `wm.set(k, 5)`, so the program must return 5; got stdout:\n{stdout}"
    );
}

#[test]
fn a_weakmap_operation_on_a_closure_local_key_declares_its_liveness_token() {
    let rust = emit("wm_closure_scope", CLOSURE_LOCAL_KEY);
    assert!(
        rust.contains("__ts_closure_"),
        "the closure must be lowered to a lifted closure helper; got:\n{rust}"
    );
    let stdout = run_generated_crate("wm_closure_scope", &rust, CLOSURE_LOCAL_KEY_MAIN);
    assert!(
        stdout.contains("ok"),
        "a WeakMap set followed by a get on a key local to the lifted closure must return 9; got \
         stdout:\n{stdout}"
    );
}

#[test]
fn a_closure_that_only_assigns_a_captured_key_still_compiles() {
    let rust = emit("weakmap_closure_assign", ASSIGN_ONLY);
    let stdout = run_generated_crate("weakmap_closure_assign", &rust, ASSIGN_ONLY_MAIN);
    assert!(
        stdout.contains("ok"),
        "a closure that only reassigns a captured WeakMap key, without any WeakMap operation of \
         its own, must keep compiling and running; got stdout:\n{stdout}"
    );
}
