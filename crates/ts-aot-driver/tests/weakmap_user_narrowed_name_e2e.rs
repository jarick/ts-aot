use ts_aot_driver::{CompileOptions, Driver, EmitStage};
use ts_aot_test_support::{check_generated_crate, run_generated_crate};

const SOURCE: &str = "class K { n: number = 0; } \
     export function userNarrowed(wm: WeakMap<K, i64>): i64 { \
         let a: K = new K(); \
         { a = new K(); } \
         let _narrowed_0: K = a; \
         a = new K(); \
         wm.set(a, 7); \
         return wm.get(_narrowed_0); \
     } \
     export function selfAssignment(wm: WeakMap<K, i64>): i64 { \
         let k: K = new K(); \
         const g = (): number => k.n; \
         wm.set(k, 42); \
         k = k; \
         return wm.get(k); \
     } \
     export function copyBack(wm: WeakMap<K, i64>): i64 { \
         let k: K = new K(); \
         let a = k; \
         wm.set(k, 7); \
         k = a; \
         return wm.get(k); \
     } \
     export function remap(wm: WeakMap<K, i64>): i64 { \
         let k: K = new K(); \
         wm.set(k, 1); \
         k = new K(); \
         wm.set(k, 2); \
         k = k; \
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
        weakmap_user_narrowed_name::userNarrowed(&ctx, ts_aot_runtime::__ts_aot_weak_map_new()),
        0,
        "`_narrowed_0` here is a user binding, not the generated narrow-after-assignment \
         binding, so `wm.get(_narrowed_0)` must miss the entry stored for the new `a`"
    );
    assert_eq!(
        weakmap_user_narrowed_name::selfAssignment(&ctx, ts_aot_runtime::__ts_aot_weak_map_new()),
        42,
        "the generated narrowed binding of `k` must keep sharing `k`'s liveness token"
    );
    assert_eq!(
        weakmap_user_narrowed_name::copyBack(&ctx, ts_aot_runtime::__ts_aot_weak_map_new()),
        7,
        "`k = a` narrows `k` again; the read must still see the entry stored before the write"
    );
    assert_eq!(
        weakmap_user_narrowed_name::remap(&ctx, ts_aot_runtime::__ts_aot_weak_map_new()),
        2,
        "the latest `wm.set(k, 2)` must survive the trailing `k = k`"
    );
    println!("ok");
}
"#;

#[test]
fn a_user_local_named_narrowed_is_not_an_alias_of_its_initializer() {
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
        norm.contains("let_narrowed_0:K="),
        "the user's `_narrowed_0` must keep its raw name; got:\n{rust}"
    );
    let body = function_body(&rust, "userNarrowed");
    assert!(
        body.contains("letmut__wm_liveness_1:std::rc::Rc<()>"),
        "the user's `_narrowed_0` must own a liveness token distinct from `a`'s, so a second \
         binding must be declared in its own function; got:\n{body}"
    );
    check_generated_crate("weakmap_user_narrowed_name", &rust);
    let stdout = run_generated_crate("weakmap_user_narrowed_name", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

const REPRO_SOURCE: &str = "class E {} \
     export function repro(wm: WeakMap<E, i64>, flag: boolean): i64 { \
         let a: E = new E(); \
         if (flag) { a = new E(); } \
         let _narrowed_0: E = a; \
         a = new E(); \
         wm.set(a, 7); \
         return wm.get(_narrowed_0); \
     } \
     export function reproWhile(wm: WeakMap<E, i64>): i64 { \
         let a: E = new E(); \
         a = new E(); \
         let _narrowed_0: E = a; \
         let i: i64 = 0; \
         while (i < 1) { a = new E(); i = i + 1; } \
         wm.set(a, 7); \
         return wm.get(_narrowed_0); \
     } \
     export function reproTry(wm: WeakMap<E, i64>): i64 { \
         let a: E = new E(); \
         a = new E(); \
         let _narrowed_0: E = a; \
         try { a = new E(); } catch { } \
         wm.set(a, 7); \
         return wm.get(_narrowed_0); \
     }";

const REPRO_MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(
        weakmap_user_narrowed_name_repro::repro(
            &ctx,
            ts_aot_runtime::__ts_aot_weak_map_new(),
            true
        ),
        0,
        "the user binding `_narrowed_0` holds the object replaced by the later `a = new E()`, so \
         the entry stored for the new `a` must not be visible through it"
    );
    assert_eq!(
        weakmap_user_narrowed_name_repro::repro(
            &ctx,
            ts_aot_runtime::__ts_aot_weak_map_new(),
            false
        ),
        0,
        "the conditional write before the user binding must not make it an alias of `a`"
    );
    assert_eq!(
        weakmap_user_narrowed_name_repro::reproWhile(
            &ctx,
            ts_aot_runtime::__ts_aot_weak_map_new()
        ),
        0,
        "a write inside a `while` body must not let the user binding share `a`'s liveness token"
    );
    assert_eq!(
        weakmap_user_narrowed_name_repro::reproTry(
            &ctx,
            ts_aot_runtime::__ts_aot_weak_map_new()
        ),
        0,
        "a write inside a `try` body must not let the user binding share `a`'s liveness token"
    );
    println!("ok");
}
"#;

#[test]
fn a_user_local_named_narrowed_survives_a_later_write_to_its_initializer() {
    let out = Driver::new().compile_source(
        "test.ts",
        REPRO_SOURCE,
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
    check_generated_crate("weakmap_user_narrowed_name_repro", &rust);
    let stdout = run_generated_crate("weakmap_user_narrowed_name_repro", &rust, REPRO_MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
    let norm: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        norm.contains("let_narrowed_0:E="),
        "the user binding must keep its raw name in the generated Rust; got:\n{rust}"
    );
    assert!(
        !norm.contains("_u5f_"),
        "the user binding must not be escaped out of the reserved `_narrowed_` namespace; got:\n{rust}"
    );
}

const GENUINE_SOURCE: &str = "class E {} \
     export function userNarrowed(wm: WeakMap<E, i64>): i64 { \
         let a: E = new E(); \
         let _narrowed_0: E = a; \
         return wm.get(_narrowed_0); \
     } \
     export function genuine(wm: WeakMap<E, i64>): i64 { \
         let k: E = new E(); \
         wm.set(k, 5); \
         k = k; \
         return wm.get(k); \
     }";

fn function_body(rust: &str, name: &str) -> String {
    rust.split(&format!("fn {name}"))
        .nth(1)
        .and_then(|rest| rest.split("fn ").next())
        .unwrap_or("")
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect()
}

#[test]
fn a_generated_narrowing_binding_still_shares_its_origin_liveness_token() {
    let out = Driver::new().compile_source(
        "test.ts",
        GENUINE_SOURCE,
        &CompileOptions {
            emit: EmitStage::Rust,
            module: true,
        },
    );
    assert!(
        !out.has_errors(),
        "the genuine narrowing case must compile through the full pipeline; got {:?}",
        out.diagnostics
    );
    let rust = out
        .rust_source
        .expect("emit-rust must populate rust_source");
    let user = function_body(&rust, "userNarrowed");
    assert!(
        user.contains("let_narrowed_0:E="),
        "the user binding must keep its raw name next to a genuine narrowing; got:\n{user}"
    );
    assert!(
        !rust.contains("_u5f_"),
        "no name in the generated Rust may be escaped out of the `_narrowed_` namespace; got:\n{rust}"
    );
    let body = function_body(&rust, "genuine");
    assert_eq!(
        body.matches("letmut__wm_liveness_").count(),
        1,
        "`k = k` must keep the generated narrowing binding on `k`'s single liveness token; got:\n{body}"
    );
    let set = body
        .find("__ts_aot_weak_map_set")
        .unwrap_or_else(|| panic!("the write must be emitted; got:\n{body}"));
    let get = body
        .find("__ts_aot_weak_map_get")
        .unwrap_or_else(|| panic!("the read must be emitted; got:\n{body}"));
    assert!(
        body[set..].contains("__wm_liveness_0") && body[get..].contains("__wm_liveness_0"),
        "the write and the read after `k = k` must use the same liveness token; got:\n{body}"
    );
    check_generated_crate("weakmap_user_narrowed_name_genuine", &rust);
    let stdout = run_generated_crate(
        "weakmap_user_narrowed_name_genuine",
        &rust,
        r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(
        weakmap_user_narrowed_name_genuine::genuine(
            &ctx,
            ts_aot_runtime::__ts_aot_weak_map_new()
        ),
        5
    );
    println!("ok");
}
"#,
    );
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}
