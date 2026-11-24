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

#[track_caller]
fn assert_snapshot_before_call(body: &str, call_marker: &str) {
    let boundary = body.find(";return").unwrap_or_else(|| {
        panic!(
            "the left place read must be evaluated into a statement of its own before the right \
             operand runs, otherwise the `RefCell` borrow guard is still alive when the call \
             mutably borrows the same cell; got:\n{body}"
        )
    });
    assert!(
        body[..boundary].contains(".borrow()"),
        "the snapshot must read the place through its borrow; got:\n{body}"
    );
    let call_at = body.find(call_marker).unwrap_or_else(|| {
        panic!("the right operand call `{call_marker}` must be emitted; got:\n{body}")
    });
    assert!(
        call_at > boundary,
        "the borrow guard must be released before the right operand call runs; got:\n{body}"
    );
}

const GLOBAL_SOURCE: &str = "const o: { a: i64 } = { a: 1 }; \
     function mutate(): i64 { o.a = 2; return 10; } \
     export function seedGlobal(): void { o.a = 1; } \
     export function globalFieldCall(): i64 { return o.a + mutate(); } \
     export function objectFieldCall(): i64 { \
         const p = { a: 1 }; \
         const f = (): i64 => { p.a = 2; return 10; }; \
         return p.a + f(); \
     }";

const GLOBAL_MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    field_borrow_global::seedGlobal(&ctx);
    assert_eq!(
        field_borrow_global::globalFieldCall(&ctx),
        11,
        "`o.a` must read 1 before `mutate` writes 2, so the sum is 11"
    );
    assert_eq!(
        field_borrow_global::objectFieldCall(&ctx),
        11,
        "the captured `p.a` must read 1 before the closure writes 2, so the sum is 11"
    );
    println!("ok");
}
"#;

const CLASS_SOURCE: &str = "class C { x: i64 = 1; } \
     export function classFieldCall(): i64 { \
         const c = new C(); \
         const f = (): i64 => { c.x = 2; return 10; }; \
         return c.x + f(); \
     } \
     export function arrayElemCall(): i64 { \
         const a: i64[] = [1]; \
         const f = (): i64 => { a[0] = 2; return 10; }; \
         return a[0] + f(); \
     } \
     export function classFieldThroughElemCall(): i64 { \
         const arr: C[] = [new C()]; \
         const f = (): i64 => { arr[0].x = 2; return 10; }; \
         return arr[0].x + f(); \
     }";

const CLASS_MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(
        field_borrow_class::classFieldCall(&ctx),
        11,
        "the class field must read 1 before the closure writes 2, so the sum is 11"
    );
    assert_eq!(
        field_borrow_class::arrayElemCall(&ctx),
        11,
        "the array element must read 1 before the closure writes 2, so the sum is 11"
    );
    assert_eq!(
        field_borrow_class::classFieldThroughElemCall(&ctx),
        11,
        "the class field read through an array element must read 1 before the closure writes 2, \
         so the sum is 11"
    );
    println!("ok");
}
"#;

const STRING_SOURCE: &str = "export function stringCall(): string { \
         let s = \"a\"; \
         const f = (): string => { s = \"b\"; return \"b\"; }; \
         return s + f(); \
     }";

const STRING_MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(
        field_borrow_string::stringCall(&ctx),
        ts_aot_runtime::JsString::from("ab"),
        "the string arm must read the old `s` (\"a\") once and concatenate what the call returns"
    );
    println!("ok");
}
"#;

const PURE_SOURCE: &str = "function pure(x: i64): i64 { return x + 1; } \
     export function callPlain(a: i64): i64 { return a + pure(a); }";

const PURE_MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(
        field_borrow_pure::callPlain(&ctx, 5),
        11,
        "node evaluates `5 + pure(5)` as `5 + 6`"
    );
    println!("ok");
}
"#;

#[test]
fn a_global_field_and_a_captured_field_snapshot_before_the_mutating_call() {
    let rust = emit("field_borrow_global", GLOBAL_SOURCE);
    assert_snapshot_before_call(&function_body(&rust, "globalFieldCall"), "mutate(__ctx)");
    assert_snapshot_before_call(&function_body(&rust, "objectFieldCall"), "__ts_fn_call_0");
    let stdout = run_generated_crate("field_borrow_global", &rust, GLOBAL_MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_class_field_and_an_array_element_snapshot_before_the_mutating_call() {
    let rust = emit("field_borrow_class", CLASS_SOURCE);
    assert_snapshot_before_call(&function_body(&rust, "classFieldCall"), "__ts_fn_call_0");
    assert_snapshot_before_call(&function_body(&rust, "arrayElemCall"), "__ts_fn_call_0");
    assert_snapshot_before_call(
        &function_body(&rust, "classFieldThroughElemCall"),
        "__ts_fn_call_0",
    );
    let stdout = run_generated_crate("field_borrow_class", &rust, CLASS_MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_string_place_is_still_snapshotted_exactly_once() {
    let rust = emit("field_borrow_string", STRING_SOURCE);
    let body = function_body(&rust, "stringCall");
    assert_eq!(
        body.matches("s_0.borrow()").count(),
        1,
        "the string arm must read the left place exactly once; got:\n{body}"
    );
    let read_at = body
        .find("s_0.borrow()")
        .unwrap_or_else(|| panic!("the string place read must be emitted; got:\n{body}"));
    let concat_at = body
        .find("string_concat(__ctx,")
        .unwrap_or_else(|| panic!("the concatenation must be emitted; got:\n{body}"));
    assert!(
        read_at < concat_at,
        "the snapshot must precede the concatenation; got:\n{body}"
    );
    let stdout = run_generated_crate("field_borrow_string", &rust, STRING_MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_place_the_right_operand_cannot_invalidate_gains_no_temporary() {
    let rust = emit("field_borrow_pure", PURE_SOURCE);
    let body = function_body(&rust, "callPlain");
    assert!(
        body.contains("->i64{returna+pure(__ctx,a);}"),
        "a bare local cannot be invalidated by a call, so the sum must stay inline; got:\n{body}"
    );
    assert!(
        !body.contains("let") && !body.contains("__tmp"),
        "no temporary may be introduced for `a + pure(a)`; got:\n{body}"
    );
    let stdout = run_generated_crate("field_borrow_pure", &rust, PURE_MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}
