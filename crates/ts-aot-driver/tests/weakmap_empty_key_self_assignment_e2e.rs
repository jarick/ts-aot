use ts_aot_driver::{CompileOptions, Driver, EmitStage};
use ts_aot_test_support::{check_generated_crate, run_generated_crate};

const SOURCE: &str = "class E {} \
     export function selfAssignment(wm: WeakMap<E, i64>): i64 { \
         let k: E = new E(); \
         wm.set(k, 5); \
         k = k; \
         return wm.get(k); \
     } \
     export function capturedSelfAssignment(wm: WeakMap<E, i64>): i64 { \
         let k: E = new E(); \
         wm.set(k, 5); \
         const g = (): boolean => wm.has(k); \
         k = k; \
         return g() ? 5 : 0; \
     } \
     export function freshObject(wm: WeakMap<E, i64>): i64 { \
         let k: E = new E(); \
         wm.set(k, 5); \
         k = new E(); \
         return wm.get(k); \
     } \
     export function otherObject(wm: WeakMap<E, i64>, o: E): i64 { \
         let k: E = new E(); \
         wm.set(k, 5); \
         k = o; \
         return wm.get(k); \
     } \
     export function copyBack(wm: WeakMap<E, i64>): i64 { \
         let k: E = new E(); \
         let a = k; \
         wm.set(k, 7); \
         k = a; \
         return wm.get(k); \
     } \
     export function remap(wm: WeakMap<E, i64>): i64 { \
         let k: E = new E(); \
         wm.set(k, 1); \
         k = new E(); \
         wm.set(k, 2); \
         k = k; \
         return wm.get(k); \
     } \
     export function linkOverwritten(wm: WeakMap<E, i64>): i64 { \
         let k: E = new E(); \
         let a = k; \
         wm.set(k, 7); \
         a = new E(); \
         k = a; \
         return wm.get(k); \
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
        weakmap_empty_key_self_assignment::selfAssignment(
            &ctx,
            ts_aot_runtime::__ts_aot_weak_map_new()
        ),
        5,
        "`k = k` cannot change which object `k` holds, so the stored entry must survive"
    );
    assert_eq!(
        weakmap_empty_key_self_assignment::capturedSelfAssignment(
            &ctx,
            ts_aot_runtime::__ts_aot_weak_map_new()
        ),
        5,
        "a closure must still observe the entry when the key local was only self-assigned"
    );
    assert_eq!(
        weakmap_empty_key_self_assignment::freshObject(
            &ctx,
            ts_aot_runtime::__ts_aot_weak_map_new()
        ),
        0,
        "a field-less key struct has no identity probe, so binding it to a fresh object must \
         refresh the liveness token"
    );
    assert_eq!(
        weakmap_empty_key_self_assignment::otherObject(
            &ctx,
            ts_aot_runtime::__ts_aot_weak_map_new(),
            weakmap_empty_key_self_assignment::E {}
        ),
        0,
        "binding the key to a different object must not expose the old object's entry"
    );
    assert_eq!(
        weakmap_empty_key_self_assignment::copyBack(
            &ctx,
            ts_aot_runtime::__ts_aot_weak_map_new()
        ),
        7,
        "`let a = k; ...; k = a` returns the same object to the key, so the entry must survive even \
         though a field-less key struct has no identity probe"
    );
    assert_eq!(
        weakmap_empty_key_self_assignment::remap(&ctx, ts_aot_runtime::__ts_aot_weak_map_new()),
        2,
        "the trailing `k = k` of a remap must keep the latest entry alive"
    );
    assert_eq!(
        weakmap_empty_key_self_assignment::linkOverwritten(
            &ctx,
            ts_aot_runtime::__ts_aot_weak_map_new()
        ),
        0,
        "`a = new E()` between `let a = k` and `k = a` breaks the copy chain, so the key really \
         changed and the old object's entry must not be visible"
    );
    println!("ok");
}
"#;

fn function_body(rust: &str, name: &str) -> String {
    let fragment = rust
        .split(&format!("fn {name}"))
        .nth(1)
        .and_then(|rest| rest.split("fn ").next())
        .unwrap_or("")
        .to_owned();
    fragment.chars().filter(|c| !c.is_whitespace()).collect()
}

#[test]
fn a_field_less_key_self_assignment_keeps_its_entry() {
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
        "the field-less key repro must compile through the full pipeline; got {:?}",
        out.diagnostics
    );
    let rust = out
        .rust_source
        .expect("emit-rust must populate rust_source");
    let reset = "__wm_liveness_0=std::rc::Rc::new(())";
    let self_body = function_body(&rust, "selfAssignment");
    assert!(
        !self_body.contains(reset),
        "a field-less key struct has no identity probe, but `k = k` cannot change the object, so \
         the liveness token must not be refreshed; got:\n{self_body}"
    );
    let captured_body = function_body(&rust, "capturedSelfAssignment");
    assert!(
        !captured_body.contains("borrow_mut()=std::rc::Rc::new(())"),
        "a captured field-less key struct shares the same limit, so `k = k` must not refresh the \
         shared liveness token; got:\n{captured_body}"
    );
    let hoist = captured_body
        .find("let__ts_aot_weakmap_liveness:std::rc::Rc<()>=")
        .unwrap_or_else(|| {
            panic!("the shared liveness token must be hoisted into a local; got:\n{captured_body}")
        });
    let call = captured_body
        .find("__ts_aot_weak_map_set")
        .unwrap_or_else(|| panic!("the shared WeakMap.set must be emitted; got:\n{captured_body}"));
    assert!(
        hoist < call,
        "the token must be copied out of its RefCell before the call, so no `Ref` is alive while \
         the value expression runs; got:\n{captured_body}"
    );
    assert!(
        captured_body[call..].starts_with(
            "__ts_aot_weak_map_set::<i64>(&__ts_aot_weakmap_receiver,\
             &__ts_aot_weakmap_liveness,__ts_aot_weakmap_ptr,"
        ),
        "the call must take the hoisted token and pointer instead of two live `borrow()` \
         temporaries; got:\n{captured_body}"
    );
    let fresh_body = function_body(&rust, "freshObject");
    assert!(
        fresh_body.contains(reset),
        "a genuinely different object must still refresh the liveness token, because a field-less \
         struct carries no identity to compare; got:\n{fresh_body}"
    );
    let copy_back_body = function_body(&rust, "copyBack");
    assert!(
        !copy_back_body.contains(reset),
        "`k = a` provably returns the object `k` already holds, so the liveness token must not be \
         refreshed; got:\n{copy_back_body}"
    );
    let remap_body = function_body(&rust, "remap");
    assert_eq!(
        remap_body.matches(reset).count(),
        1,
        "the genuine `k = new E()` must refresh the token exactly once, while the trailing \
         `k = k` must not; got:\n{remap_body}"
    );
    let link_overwritten_body = function_body(&rust, "linkOverwritten");
    assert!(
        link_overwritten_body.contains(reset),
        "an intervening write to the copied binding invalidates the chain, so `k = a` must refresh \
         the token; got:\n{link_overwritten_body}"
    );
    check_generated_crate("weakmap_empty_key_self_assignment", &rust);
    let stdout = run_generated_crate("weakmap_empty_key_self_assignment", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}
