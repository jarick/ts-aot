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
fn assert_borrow_read_precedes(body: &str, read_marker: &str, call_marker: &str) {
    let read_at = body.find(read_marker).unwrap_or_else(|| {
        panic!("the borrow-bearing left read `{read_marker}` must be emitted; got:\n{body}")
    });
    let call_at = body.find(call_marker).unwrap_or_else(|| {
        panic!("the right operand call `{call_marker}` must be emitted; got:\n{body}")
    });
    assert!(
        read_at < call_at,
        "the borrow guard must be released before the right operand call runs; got:\n{body}"
    );
}

const HELPER_MAIN_HEADER: &str = r"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
";

const FINDING1_SOURCE: &str = "function pure(): i64 { return 10; } \
     export function vecPure(): i64 { const holder = { list: [1, 2] }; return holder.list[0] + pure(); }";

const FINDING2_SOURCE: &str = "const o: { a: i64 } = { a: 1 }; \
     function f(): i64 { o.a = 2; return 10; } \
     export function seedField(): void { o.a = 1; } \
     export function castFieldCall(): i64 { return (o.a as i64) + f(); } \
     export function nestedBinaryFieldCall(): i64 { return (o.a + 0) + f(); }";

const GLOBAL_SOURCE: &str = "const o: { a: i64 } = { a: 1 }; \
     function mutate(): i64 { o.a = 2; return 10; } \
     export function seedGlobal(): void { o.a = 1; } \
     export function globalFieldCall(): i64 { return o.a + mutate(); } \
     export function objectFieldCall(): i64 { \
         const p = { a: 1 }; \
         const f = (): i64 => { p.a = 2; return 10; }; \
         return p.a + f(); \
     }";

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

const STRING_SOURCE: &str = "export function stringCall(): string { \
         let s = \"a\"; \
         const f = (): string => { s = \"b\"; return \"b\"; }; \
         return s + f(); \
     }";

const INDEX_SOURCE: &str = "let seen: i64 = 0; \
     function index(): i64 { seen = seen + 1; return 0; } \
     function addend(): i64 { return 10; } \
     export function indexCallOnce(): i64 { \
         const a: i64[] = [1, 2]; \
         return a[index()] + addend(); \
     } \
     export function seenCount(): i64 { return seen; }";

const CONTROL_PURE_SOURCE: &str = "function pure(x: i64): i64 { return x + 1; } \
     export function callPlain(a: i64): i64 { return a + pure(a); }";

const CONTROL_PLAIN_SOURCE: &str = "export function control(a: i64, b: i64): i64 { return a + b; }";

const VALUE_SHAPES_SOURCE: &str = "export function localAdd(): i64 { let a: i64 = 1; return a + (a = 2); } \
     export function localCmp(): bool { let a: i64 = 1; return a < (a = 2); } \
     export function fieldAdd(): i64 { const o = { a: 1 }; return o.a + (o.a = 2); } \
     export function bigintAdd(): bigint { let a = 1n; return a + (a = 2n); }";

const EVAL_ORDER_SOURCE: &str = "class K { n: number = 0; } \
     export function evalOrder(wm: WeakMap<K, i64>): i64 { \
         let k = new K(); \
         wm.set(k, 1); \
         const outer = (): i64 => { wm.set(k, 2); return 10; }; \
         return outer() + wm.get(k); \
     } \
     export function keyOrder(): i64 { \
         const o = { a: 1, b: 2 }; \
         const outer = (): i64 => 10; \
         return outer() + Object.keys(o).length; \
     }";

#[test]
fn a_pure_call_on_an_untyped_local_object_element_compiles_and_runs() {
    let rust = emit("borrow_snapshot_finding1", FINDING1_SOURCE);
    let body = function_body(&rust, "vecPure");
    assert!(
        !body.contains("__ty"),
        "the snapshot must not name an unresolved type; got:\n{body}"
    );
    assert_borrow_read_precedes(&body, "holder.list.borrow()", "pure(__ctx)");
    let main = format!(
        "{HELPER_MAIN_HEADER}    assert_eq!(\n        \
         borrow_snapshot_finding1::vecPure(&ctx),\n        11,\n        \
         \"`holder.list[0]` reads 1 before `pure()` returns 10, so the sum is 11\"\n    );\n    \
         println!(\"ok\");\n}}\n"
    );
    let stdout = run_generated_crate("borrow_snapshot_finding1", &rust, &main);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_cast_and_a_nested_binary_around_a_field_read_snapshot_before_the_call() {
    let rust = emit("borrow_snapshot_finding2", FINDING2_SOURCE);
    let cast_body = function_body(&rust, "castFieldCall");
    assert_borrow_read_precedes(&cast_body, ".a.borrow()", "f(__ctx)");
    let nested_body = function_body(&rust, "nestedBinaryFieldCall");
    assert_borrow_read_precedes(&nested_body, ".a.borrow()", "f(__ctx)");
    let main = format!(
        "{HELPER_MAIN_HEADER}    \
         borrow_snapshot_finding2::seedField(&ctx);\n    assert_eq!(\n        \
         borrow_snapshot_finding2::castFieldCall(&ctx),\n        11,\n        \
         \"the cast must read `o.a` (1) before `f` writes 2, so the sum is 11\"\n    );\n    \
         borrow_snapshot_finding2::seedField(&ctx);\n    assert_eq!(\n        \
         borrow_snapshot_finding2::nestedBinaryFieldCall(&ctx),\n        11,\n        \
         \"the nested binary must read `o.a` (1) before `f` writes 2, so the sum is 11\"\n    );\n    \
         println!(\"ok\");\n}}\n"
    );
    let stdout = run_generated_crate("borrow_snapshot_finding2", &rust, &main);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn the_place_shapes_fixed_earlier_still_snapshot_and_run() {
    let rust = emit("borrow_snapshot_places", GLOBAL_SOURCE);
    assert_borrow_read_precedes(
        &function_body(&rust, "globalFieldCall"),
        ".a.borrow()",
        "mutate(__ctx)",
    );
    assert_borrow_read_precedes(
        &function_body(&rust, "objectFieldCall"),
        ".a.borrow()",
        "__ts_fn_call_0",
    );
    let main = format!(
        "{HELPER_MAIN_HEADER}    \
         borrow_snapshot_places::seedGlobal(&ctx);\n    assert_eq!(\n        \
         borrow_snapshot_places::globalFieldCall(&ctx),\n        11,\n        \
         \"`o.a` must read 1 before `mutate` writes 2, so the sum is 11\"\n    );\n    assert_eq!(\n        \
         borrow_snapshot_places::objectFieldCall(&ctx),\n        11,\n        \
         \"the captured `p.a` must read 1 before the closure writes 2, so the sum is 11\"\n    );\n    \
         println!(\"ok\");\n}}\n"
    );
    let stdout = run_generated_crate("borrow_snapshot_places", &rust, &main);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");

    let rust = emit("borrow_snapshot_class", CLASS_SOURCE);
    assert_borrow_read_precedes(
        &function_body(&rust, "classFieldCall"),
        ".x.borrow()",
        "__ts_fn_call_0",
    );
    assert_borrow_read_precedes(
        &function_body(&rust, "arrayElemCall"),
        "[0asusize]",
        "__ts_fn_call_0",
    );
    assert_borrow_read_precedes(
        &function_body(&rust, "classFieldThroughElemCall"),
        ".x.borrow()",
        "__ts_fn_call_0",
    );
    let main = format!(
        "{HELPER_MAIN_HEADER}    assert_eq!(\n        \
         borrow_snapshot_class::classFieldCall(&ctx),\n        11,\n        \
         \"the class field must read 1 before the closure writes 2, so the sum is 11\"\n    );\n    \
         assert_eq!(\n        borrow_snapshot_class::arrayElemCall(&ctx),\n        11,\n        \
         \"the array element must read 1 before the closure writes 2, so the sum is 11\"\n    );\n    \
         assert_eq!(\n        borrow_snapshot_class::classFieldThroughElemCall(&ctx),\n        11,\n        \
         \"the class field read through an array element must read 1 first, so the sum is 11\"\n    );\n    \
         println!(\"ok\");\n}}\n"
    );
    let stdout = run_generated_crate("borrow_snapshot_class", &rust, &main);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_string_place_with_a_mutating_call_concatenates_the_old_value() {
    let rust = emit("borrow_snapshot_string", STRING_SOURCE);
    let body = function_body(&rust, "stringCall");
    assert_eq!(
        body.matches("s_0.borrow()").count(),
        1,
        "the string arm must read the left place exactly once; got:\n{body}"
    );
    let main = format!(
        "{HELPER_MAIN_HEADER}    assert_eq!(\n        \
         borrow_snapshot_string::stringCall(&ctx),\n        ts_aot_runtime::JsString::from(\"ab\"),\n        \
         \"the string arm must read the old `s` (\\\"a\\\") once and concatenate what the call returns\"\n    );\n    \
         println!(\"ok\");\n}}\n"
    );
    let stdout = run_generated_crate("borrow_snapshot_string", &rust, &main);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn an_effectful_left_index_is_evaluated_exactly_once() {
    let rust = emit("borrow_snapshot_index", INDEX_SOURCE);
    let body = function_body(&rust, "indexCallOnce");
    assert_eq!(
        body.matches("index(__ctx)").count(),
        1,
        "the effectful index expression must be emitted exactly once; got:\n{body}"
    );
    assert_borrow_read_precedes(&body, "index(__ctx)", "addend(__ctx)");
    let main = format!(
        "{HELPER_MAIN_HEADER}    assert_eq!(\n        \
         borrow_snapshot_index::indexCallOnce(&ctx),\n        11,\n        \
         \"`a[index()]` reads 1 and `addend()` returns 10, so the sum is 11\"\n    );\n    assert_eq!(\n        \
         borrow_snapshot_index::seenCount(&ctx),\n        1,\n        \
         \"`index()` must run exactly once even though the read is snapshotted\"\n    );\n    \
         println!(\"ok\");\n}}\n"
    );
    let stdout = run_generated_crate("borrow_snapshot_index", &rust, &main);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn operands_that_never_interfere_gain_no_temporary() {
    let rust = emit("borrow_snapshot_control_pure", CONTROL_PURE_SOURCE);
    let body = function_body(&rust, "callPlain");
    assert!(
        body.contains("->i64{returna+pure(__ctx,a);}"),
        "a bare local cannot be invalidated by a call, so the sum must stay inline; got:\n{body}"
    );
    assert!(
        !body.contains("let") && !body.contains("__tmp"),
        "no temporary may be introduced for `a + pure(a)`; got:\n{body}"
    );
    let main = format!(
        "{HELPER_MAIN_HEADER}    assert_eq!(\n        \
         borrow_snapshot_control_pure::callPlain(&ctx, 5),\n        11,\n        \
         \"node evaluates `5 + pure(5)` as `5 + 6`\"\n    );\n    println!(\"ok\");\n}}\n"
    );
    let stdout = run_generated_crate("borrow_snapshot_control_pure", &rust, &main);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");

    let rust = emit("borrow_snapshot_control_plain", CONTROL_PLAIN_SOURCE);
    let body = function_body(&rust, "control");
    assert!(
        body.contains("->i64{returna+b;}"),
        "the sum must stay inline; got:\n{body}"
    );
    assert!(
        !body.contains("let") && !body.contains("__tmp"),
        "neither operand reads a place the other writes, so no temporary may be introduced; \
         got:\n{body}"
    );
    let main = format!(
        "{HELPER_MAIN_HEADER}    assert_eq!(borrow_snapshot_control_plain::control(&ctx, 3, 4), 7);\n    \
         println!(\"ok\");\n}}\n"
    );
    let stdout = run_generated_crate("borrow_snapshot_control_plain", &rust, &main);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn the_previously_fixed_value_shapes_keep_their_node_values() {
    let rust = emit("borrow_snapshot_values", VALUE_SHAPES_SOURCE);
    assert_borrow_read_precedes(&function_body(&rust, "localAdd"), "=a;", "a=__tmp");
    let main = format!(
        "{HELPER_MAIN_HEADER}    assert_eq!(\n        \
         borrow_snapshot_values::localAdd(&ctx),\n        3,\n        \
         \"`a + (a = 2)` reads the old 1, so the sum is 3\"\n    );\n    assert!(\n        \
         borrow_snapshot_values::localCmp(&ctx),\n        \
         \"`a < (a = 2)` compares the old 1 with the assigned 2\"\n    );\n    assert_eq!(\n        \
         borrow_snapshot_values::fieldAdd(&ctx),\n        3,\n        \
         \"`o.a + (o.a = 2)` reads the old field value 1, so the sum is 3\"\n    );\n    assert_eq!(\n        \
         ts_aot_runtime::__ts_aot_bigint_to_string(&borrow_snapshot_values::bigintAdd(&ctx), None),\n        \
         \"3\",\n        \"`a + (a = 2n)` reads the old 1n, so the sum is 3n\"\n    );\n    \
         println!(\"ok\");\n}}\n"
    );
    let stdout = run_generated_crate("borrow_snapshot_values", &rust, &main);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

const TYPED_OWNER_SOURCE: &str = "function pure(): i64 { return 10; } \
     function pureG(): i64 { return 10; } \
     function pureC(): i64 { return 10; } \
     function pureA(): i64 { return 10; } \
     class C { x: i64 = 1; } \
     export function annotatedOwner(): i64 { const holder: { list: i64[] } = { list: [1, 2] }; return holder.list[0] + pure(); } \
     export function ownedListOwner(): i64 { const holder = { list: [1, 2] as i64[] }; return holder.list[0] + pureG(); } \
     export function classOwner(): i64 { const c = new C(); return c.x + pureC(); } \
     export function arrayOwner(): i64 { const a: i64[] = [1, 2]; return a[0] + pureA(); } \
     export function capturedOwner(): i64 { const holder = { list: [1, 2] }; const h = (): i64 => holder.list[0]; return h() + pure(); } \
     export function stringOwner(): string { const holder = { v: \"a\" }; return holder.v + \"b\"; }";

#[test]
fn other_typed_owner_shapes_still_compile_and_run() {
    let rust = emit("borrow_snapshot_typed_owner", TYPED_OWNER_SOURCE);
    let main = format!(
        "{HELPER_MAIN_HEADER}    assert_eq!(borrow_snapshot_typed_owner::annotatedOwner(&ctx), 11);\n    \
         assert_eq!(borrow_snapshot_typed_owner::ownedListOwner(&ctx), 11);\n    \
         assert_eq!(borrow_snapshot_typed_owner::classOwner(&ctx), 11);\n    \
         assert_eq!(borrow_snapshot_typed_owner::arrayOwner(&ctx), 11);\n    \
         assert_eq!(borrow_snapshot_typed_owner::capturedOwner(&ctx), 11);\n    assert_eq!(\n        \
         borrow_snapshot_typed_owner::stringOwner(&ctx),\n        ts_aot_runtime::JsString::from(\"ab\")\n    );\n    \
         println!(\"ok\");\n}}\n"
    );
    let stdout = run_generated_crate("borrow_snapshot_typed_owner", &rust, &main);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_call_hoist_still_runs_after_the_inline_left_operand() {
    let rust = emit("borrow_snapshot_eval_order", EVAL_ORDER_SOURCE);
    let main = format!(
        "{HELPER_MAIN_HEADER}    assert_eq!(\n        \
         borrow_snapshot_eval_order::evalOrder(&ctx, ts_aot_runtime::__ts_aot_weak_map_new()),\n        12,\n        \
         \"`outer()` runs before `wm.get(k)`, so the sum is 10 + 2\"\n    );\n    assert_eq!(\n        \
         borrow_snapshot_eval_order::keyOrder(&ctx),\n        12,\n        \
         \"`outer()` returns 10 and the two-key object has length 2\"\n    );\n    \
         println!(\"ok\");\n}}\n"
    );
    let stdout = run_generated_crate("borrow_snapshot_eval_order", &rust, &main);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}
