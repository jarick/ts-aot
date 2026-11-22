use ts_aot_driver::{CompileOptions, Driver, EmitStage};
use ts_aot_test_support::run_generated_crate;

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

fn function_body(rust: &str, name: &str) -> String {
    rust.split(&format!("fn {name}"))
        .nth(1)
        .and_then(|rest| rest.split("fn ").next())
        .unwrap_or("")
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect()
}

const WEAKMAP_SOURCE: &str = "class K { n: number = 0; } \
     export function evalOrder(wm: WeakMap<K, i64>): i64 { \
         let k = new K(); \
         wm.set(k, 1); \
         const outer = (): i64 => { wm.set(k, 2); return 10; }; \
         return outer() + wm.get(k); \
     }";

const WEAKMAP_MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(
        runtime_eval_order_wm::evalOrder(&ctx, ts_aot_runtime::__ts_aot_weak_map_new()),
        12,
        "JavaScript evaluates `outer()` before `wm.get(k)`, so the read observes the `wm.set(k, 2)` \
         performed by `outer()` and the sum is 12"
    );
    println!("ok");
}
"#;

const KEYS_SOURCE: &str = "export function keyOrder(): i64 { \
         const o = { a: 1, b: 2 }; \
         const outer = (): i64 => 10; \
         return outer() + Object.keys(o).length; \
     }";

const KEYS_MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(
        runtime_eval_order_keys::keyOrder(&ctx),
        12,
        "`outer()` returns 10 and the statically known two-key object has length 2"
    );
    println!("ok");
}
"#;

const BOTH_SOURCE: &str = "class K { n: number = 0; } \
     let order: i64 = 0; \
     function f(m: WeakMap<K, i64>, k2: K): i64 { order = order * 10 + 1; return m.has(k2) ? 100 : 10; } \
     function g(m: WeakMap<K, i64>): i64 { order = order * 10 + 2; return 1; } \
     export function bothHoist(wm: WeakMap<K, i64>): i64 { \
         let k1 = new K(); \
         let k2 = new K(); \
         return f(wm.set(k1, 1), k2) + g(wm.set(k2, 2)); \
     } \
     export function bothOrder(): i64 { return order; }";

const BOTH_MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(
        runtime_eval_order_both::bothHoist(&ctx, ts_aot_runtime::__ts_aot_weak_map_new()),
        11,
        "`f` must observe no entry for `k2` yet, because JavaScript evaluates `f(wm.set(k1, 1), k2)` \
         before the second argument `wm.set(k2, 2)` and its call to `g`"
    );
    assert_eq!(
        runtime_eval_order_both::bothOrder(&ctx),
        12,
        "the recorded side effects must still be `f` then `g`"
    );
    println!("ok");
}
"#;

const PLAIN_SOURCE: &str = "export function plainOrder(a: i64, b: i64): i64 { return a + b; }";

const PLAIN_MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(runtime_eval_order_plain::plainOrder(&ctx, 3, 4), 7);
    println!("ok");
}
"#;

#[test]
fn a_weakmap_read_follows_an_inline_left_operand() {
    let rust = emit("runtime_eval_order_wm", WEAKMAP_SOURCE);
    let body = function_body(&rust, "evalOrder");
    let call_at = body
        .find("__ts_fn_call_0")
        .unwrap_or_else(|| panic!("`outer()` must be lowered to an indirect call; got:\n{body}"));
    let get_at = body
        .find("__ts_aot_weak_map_get")
        .unwrap_or_else(|| panic!("`wm.get(k)` must be lowered to a runtime read; got:\n{body}"));
    assert!(
        call_at < get_at,
        "the read must not run before the inline left operand; got:\n{body}"
    );
    let stdout = run_generated_crate("runtime_eval_order_wm", &rust, WEAKMAP_MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn an_object_keys_length_follows_an_inline_left_operand() {
    let rust = emit("runtime_eval_order_keys", KEYS_SOURCE);
    let body = function_body(&rust, "keyOrder");
    let call_at = body
        .find("__ts_fn_call_0")
        .unwrap_or_else(|| panic!("`outer()` must be lowered to an indirect call; got:\n{body}"));
    let create_at = body
        .find("__ts_aot_array_create")
        .unwrap_or_else(|| panic!("`Object.keys(o)` must build its key array; got:\n{body}"));
    assert!(
        call_at < create_at,
        "the hoisted `Object.keys` work must not run before the inline left operand; got:\n{body}"
    );
    let stdout = run_generated_crate("runtime_eval_order_keys", &rust, KEYS_MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn both_hoisting_sides_keep_left_to_right_order() {
    let rust = emit("runtime_eval_order_both", BOTH_SOURCE);
    let body = function_body(&rust, "bothHoist");
    let f_at = body
        .find("f(__ctx")
        .unwrap_or_else(|| panic!("the call to `f` must survive; got:\n{body}"));
    let second_set_at = body
        .match_indices("__ts_aot_weak_map_set")
        .nth(1)
        .map_or_else(
            || panic!("both WeakMap writes must be emitted; got:\n{body}"),
            |(index, _)| index,
        );
    assert!(
        f_at < second_set_at,
        "the left operand's inline call must run before the right operand's hoisted statements; \
         got:\n{body}"
    );
    let stdout = run_generated_crate("runtime_eval_order_both", &rust, BOTH_MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn operands_that_never_hoist_gain_no_temporary() {
    let rust = emit("runtime_eval_order_plain", PLAIN_SOURCE);
    let body = function_body(&rust, "plainOrder");
    assert!(
        !body.contains("let") && !body.contains("__tmp"),
        "neither operand hoists, so the sum must stay inline; got:\n{body}"
    );
    let stdout = run_generated_crate("runtime_eval_order_plain", &rust, PLAIN_MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}
