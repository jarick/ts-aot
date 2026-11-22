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

const LOCAL_SOURCE: &str = "export function localAdd(): i64 { let a: i64 = 1; return a + (a = 2); } \
     export function localCmp(): bool { let a: i64 = 1; return a < (a = 2); }";

const LOCAL_MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(
        binary_place_order_local::localAdd(&ctx),
        3,
        "JavaScript reads the left `a` (1) before `(a = 2)` assigns, so the sum is 3"
    );
    assert!(
        binary_place_order_local::localCmp(&ctx),
        "JavaScript compares the old left `a` (1) with the value the right operand assigned (2)"
    );
    println!("ok");
}
"#;

const FIELD_SOURCE: &str = "export function fieldAdd(): i64 { const o = { a: 1 }; return o.a + (o.a = 2); } \
     export function bigintAdd(): bigint { let a = 1n; return a + (a = 2n); }";

const FIELD_MAIN: &str = r#"
use ts_aot_runtime::{BigIntHandle, Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn text(handle: BigIntHandle) -> String {
    ts_aot_runtime::__ts_aot_bigint_to_string(&handle, None)
}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(
        binary_place_order_field::fieldAdd(&ctx),
        3,
        "the field read must observe the old `o.a` (1), not the write performed by the right operand"
    );
    assert_eq!(
        text(binary_place_order_field::bigintAdd(&ctx)),
        "3",
        "the BigInt read must observe the old `a` (1n), not the write performed by the right operand"
    );
    println!("ok");
}
"#;

const COMPOUND_SOURCE: &str =
    "export function localCompound(): i64 { let a: i64 = 1; return a + (a += 1); }";

const COMPOUND_MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(
        binary_place_order_compound::localCompound(&ctx),
        3,
        "`a + (a += 1)` must read the old `a` (1) and add the updated value (2)"
    );
    println!("ok");
}
"#;

const STRING_SOURCE: &str =
    "export function stringPlace(): string { let s = \"a\"; return s + (s = \"b\"); }";

const STRING_MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(
        binary_place_order_string::stringPlace(&ctx),
        ts_aot_runtime::JsString::from("ab"),
        "the string arm must read the old `s` (\"a\") once and concatenate the assigned value"
    );
    println!("ok");
}
"#;

const CONTROL_SOURCE: &str = "export function control(a: i64, b: i64): i64 { return a + b; }";

const CONTROL_MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(binary_place_order_control::control(&ctx, 3, 4), 7);
    println!("ok");
}
"#;

#[test]
fn a_local_operand_and_a_comparison_read_the_old_left_value() {
    let rust = emit("binary_place_order_local", LOCAL_SOURCE);
    let add_body = function_body(&rust, "localAdd");
    let snapshot_at = add_body
        .find("=a;")
        .unwrap_or_else(|| panic!("the left place read must be snapshotted; got:\n{add_body}"));
    let write_at = add_body
        .find("a=__tmp")
        .unwrap_or_else(|| panic!("the right operand must still assign `a`; got:\n{add_body}"));
    assert!(
        snapshot_at < write_at,
        "the snapshot must precede the right operand's write; got:\n{add_body}"
    );
    let cmp_body = function_body(&rust, "localCmp");
    let cmp_snapshot_at = cmp_body
        .find("=a;")
        .unwrap_or_else(|| panic!("the compared place read must be snapshotted; got:\n{cmp_body}"));
    let cmp_write_at = cmp_body
        .find("a=__tmp")
        .unwrap_or_else(|| panic!("the right operand must still assign `a`; got:\n{cmp_body}"));
    assert!(
        cmp_snapshot_at < cmp_write_at,
        "the comparison must read the old place; got:\n{cmp_body}"
    );
    let stdout = run_generated_crate("binary_place_order_local", &rust, LOCAL_MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn an_object_field_and_a_bigint_operand_read_the_old_left_value() {
    let rust = emit("binary_place_order_field", FIELD_SOURCE);
    let field_body = function_body(&rust, "fieldAdd");
    let field_snapshot_at = field_body
        .find("=o.a.borrow().clone()")
        .unwrap_or_else(|| panic!("the field read must be snapshotted; got:\n{field_body}"));
    let field_write_at = field_body.find("borrow_mut()").unwrap_or_else(|| {
        panic!("the right operand must still write the field; got:\n{field_body}")
    });
    assert!(
        field_snapshot_at < field_write_at,
        "the field snapshot must precede the right operand's write; got:\n{field_body}"
    );
    let bigint_body = function_body(&rust, "bigintAdd");
    let bigint_snapshot_at = bigint_body
        .find("=a.clone();")
        .unwrap_or_else(|| panic!("the BigInt read must be snapshotted; got:\n{bigint_body}"));
    let bigint_write_at = bigint_body
        .find("a=__tmp")
        .unwrap_or_else(|| panic!("the right operand must still assign `a`; got:\n{bigint_body}"));
    assert!(
        bigint_snapshot_at < bigint_write_at,
        "the BigInt snapshot must precede the right operand's write; got:\n{bigint_body}"
    );
    let stdout = run_generated_crate("binary_place_order_field", &rust, FIELD_MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_compound_update_reads_the_old_left_value() {
    let rust = emit("binary_place_order_compound", COMPOUND_SOURCE);
    let body = function_body(&rust, "localCompound");
    let snapshot_at = body
        .find("=a;")
        .unwrap_or_else(|| panic!("the left place read must be snapshotted; got:\n{body}"));
    let write_at = body
        .find("a=__tmp")
        .unwrap_or_else(|| panic!("the compound update must still assign `a`; got:\n{body}"));
    assert!(
        snapshot_at < write_at,
        "the snapshot must precede the compound update; got:\n{body}"
    );
    let stdout = run_generated_crate("binary_place_order_compound", &rust, COMPOUND_MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_string_place_is_snapshotted_once() {
    let rust = emit("binary_place_order_string", STRING_SOURCE);
    let body = function_body(&rust, "stringPlace");
    assert_eq!(
        body.matches("s.clone()").count(),
        1,
        "the string arm must read the left place exactly once; got:\n{body}"
    );
    let stdout = run_generated_crate("binary_place_order_string", &rust, STRING_MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn operands_that_never_interfere_gain_no_temporary() {
    let rust = emit("binary_place_order_control", CONTROL_SOURCE);
    let body = function_body(&rust, "control");
    assert!(
        body.contains("->i64{returna+b;}"),
        "the sum must stay inline; got:\n{body}"
    );
    assert!(
        !body.contains("let") && !body.contains("__tmp"),
        "neither operand writes anything the other reads, so no temporary may be introduced; \
         got:\n{body}"
    );
    let stdout = run_generated_crate("binary_place_order_control", &rust, CONTROL_MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}
