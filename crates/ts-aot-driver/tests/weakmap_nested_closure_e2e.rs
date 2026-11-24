use ts_aot_driver::{CompileOptions, Driver, EmitStage};
use ts_aot_test_support::{check_generated_crate, run_generated_crate};

const SOURCE: &str = "class L { n: number = 0; } \
     export function nestedGet(wm: WeakMap<L, i64>): i64 { \
         let k: L = new L(); \
         wm.set(k, 9); \
         const outer = (): i64 => { const inner = (): i64 => wm.get(k); return inner(); }; \
         return outer(); \
     } \
     export function nestedSet(wm: WeakMap<L, i64>): i64 { \
         let k: L = new L(); \
         const outer = (): i64 => { \
             const inner = (): i64 => { wm.set(k, 11); return 1; }; \
             return inner(); \
         }; \
         const r: i64 = outer(); \
         return r + wm.get(k); \
     } \
     export function nestedSelfAssign(wm: WeakMap<L, i64>): i64 { \
         let k: L = new L(); \
         wm.set(k, 9); \
         const outer = (): i64 => { \
             const inner = (): i64 => { k = k; return wm.get(k); }; \
             return inner(); \
         }; \
         return outer(); \
     } \
     export function depthOne(wm: WeakMap<L, i64>): i64 { \
         let k: L = new L(); \
         wm.set(k, 9); \
         const g = (): i64 => wm.get(k); \
         return g(); \
     } \
     export function nestedNonWeakMap(v: i64): i64 { \
         let x: i64 = 3; \
         const outer = (): i64 => { const inner = (): i64 => x; return inner(); }; \
         return outer() + v; \
     }";

const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(
        weakmap_nested_closure::nestedGet(&ctx, ts_aot_runtime::__ts_aot_weak_map_new()),
        9,
        "a WeakMap read of an outer local from a depth-2 closure must see the entry stored by the \
         enclosing `wm.set(k, 9)`"
    );
    assert_eq!(
        weakmap_nested_closure::nestedSet(&ctx, ts_aot_runtime::__ts_aot_weak_map_new()),
        12,
        "a WeakMap write from a depth-2 closure must store through the same key token the enclosing \
         read uses, so `outer() + wm.get(k)` is 1 + 11"
    );
    assert_eq!(
        weakmap_nested_closure::nestedSelfAssign(&ctx, ts_aot_runtime::__ts_aot_weak_map_new()),
        9,
        "`k = k` inside a depth-2 closure cannot change the object, so the stored entry must survive"
    );
    assert_eq!(
        weakmap_nested_closure::depthOne(&ctx, ts_aot_runtime::__ts_aot_weak_map_new()),
        9,
        "a depth-1 closure read of a captured WeakMap key must keep working"
    );
    assert_eq!(
        weakmap_nested_closure::nestedNonWeakMap(&ctx, 4),
        7,
        "a depth-2 closure that captures a plain local must keep working"
    );
    println!("ok");
}
"#;

#[test]
fn a_weakmap_operation_in_a_depth_two_closure_uses_the_captured_key() {
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
        "the nested-closure repro must compile through the full pipeline; got {:?}",
        out.diagnostics
    );
    let rust = out
        .rust_source
        .expect("emit-rust must populate rust_source");
    let norm: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        !norm.contains("{return();}"),
        "no lifted closure may have its body replaced by a bare `return ()`; got:\n{rust}"
    );
    let inner = rust
        .split("fn __ts_closure_1")
        .nth(1)
        .and_then(|rest| rest.split("fn ").next())
        .unwrap_or("")
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>();
    assert!(
        inner.contains("__ts_aot_weak_map_get"),
        "the inner closure's body must contain the WeakMap read instead of being dropped; got:\n{inner}"
    );
    check_generated_crate("weakmap_nested_closure", &rust);
    let stdout = run_generated_crate("weakmap_nested_closure", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}
