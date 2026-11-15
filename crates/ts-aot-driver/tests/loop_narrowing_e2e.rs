use ts_aot_driver::{CompileOptions, Driver, EmitStage};
use ts_aot_test_support::{check_generated_crate, run_generated_crate};

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

#[test]
fn narrowing_before_a_loop_survives_a_body_without_writes() {
    const SOURCE: &str = "export function whileNoWrite(c: boolean): number { \
             let x: number | null = null; x = 5; while (c) { c = false; } return x; } \
         export function forNoWrite(): number { \
             let x: number | null = null; x = 5; for (let i = 0; i < 1; i = i + 1) { } return x; } \
         export function doWhileNoWrite(): number { \
             let x: number | null = null; x = 5; do { } while (false); return x; } \
         export function forOfNoWrite(): number { \
             let x: number | null = null; x = 5; for (const n of [1, 2, 3]) { } return x; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(loop_no_write::whileNoWrite(&ctx, true), 5.0);
    assert_eq!(loop_no_write::whileNoWrite(&ctx, false), 5.0);
    assert_eq!(loop_no_write::forNoWrite(&ctx), 5.0);
    assert_eq!(loop_no_write::doWhileNoWrite(&ctx), 5.0);
    assert_eq!(loop_no_write::forOfNoWrite(&ctx), 5.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    check_generated_crate("loop_no_write", &rust);
    let stdout = run_generated_crate("loop_no_write", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn for_in_without_writes_keeps_the_narrowing() {
    const SOURCE: &str = "export function forInNoWrite(): number { \
         let x: number | null = null; x = 5; for (const k in { a: 1 }) { } return x; }";
    let out = compile_module(SOURCE);
    assert!(
        !out.has_errors(),
        "`{SOURCE}` must not be rejected; got {:?}",
        out.diagnostics
    );
}

#[test]
fn a_loop_that_writes_still_withdraws_the_narrowing() {
    const SOURCE: &str = "export function whileWrites(): number | null { \
             let x: number | null = null; x = 5; let i = 0; \
             while (i < 2) { x = null; i = i + 1; } return x; } \
         export function forIfWrite(): number { \
             let x: number | null = null; x = 5; \
             for (let i = 0; i < 2; i = i + 1) { if (i === 1) { x = 5; } } return x ?? 0; } \
         export function narrowInsideBody(): number { \
             let x: number | null = null; let i = 0; \
             while (i < 2) { x = 7; i = i + 1; } return x ?? 0; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(loop_writes::whileWrites(&ctx), ::core::option::Option::None);
    assert_eq!(loop_writes::forIfWrite(&ctx), 5.0);
    assert_eq!(loop_writes::narrowInsideBody(&ctx), 7.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    check_generated_crate("loop_writes", &rust);
    let stdout = run_generated_crate("loop_writes", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_narrowed_update_statement_copies_the_new_value_back() {
    const SOURCE: &str = "class Holder { v: number; constructor(v: number) { this.v = v; } } \
         export function postIncrementStmt(): number { \
             let x: number | null = null; x = 5; const g = (): number => x ?? 0; \
             x++; const h = new Holder(g()); return h.v; } \
         export function compoundAssignStmt(): number { \
             let x: number | null = null; x = 5; const g = (): number => x ?? 0; \
             x += 2; const h = new Holder(g()); return h.v; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(update_stmt::postIncrementStmt(&ctx), 6.0);
    assert_eq!(update_stmt::compoundAssignStmt(&ctx), 7.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    check_generated_crate("update_stmt", &rust);
    let stdout = run_generated_crate("update_stmt", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_narrowed_update_initializer_copies_the_new_value_back() {
    const SOURCE: &str = "class Holder { v: number; constructor(v: number) { this.v = v; } } \
         export function postIncrementExpr(): number { \
             let x: number | null = null; x = 5; const g = (): number => x ?? 0; \
             const y = x++; const h = new Holder(g()); return h.v + y; } \
         export function compoundAssignExpr(): number { \
             let x: number | null = null; x = 5; const g = (): number => x ?? 0; \
             const y = (x += 2); const h = new Holder(g()); return h.v + y; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(update_init::postIncrementExpr(&ctx), 11.0);
    assert_eq!(update_init::compoundAssignExpr(&ctx), 14.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    check_generated_crate("update_init", &rust);
    let stdout = run_generated_crate("update_init", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_nested_narrowed_update_is_rejected() {
    const SOURCE: &str = "class Holder { v: number; constructor(v: number) { this.v = v; } } \
         export function nestedUpdate(): number { \
             let x: number | null = null; x = 5; const g = (): number => x ?? 0; \
             const y: number = x++ + 1; const h = new Holder(g()); return h.v + y; }";
    let out = compile_module(SOURCE);
    assert!(
        out.has_errors(),
        "a nested narrowed update must block the compilation; got {:?}",
        out.diagnostics
    );
    assert!(
        out.diagnostics.iter().any(|d| d.code.as_str() == "E0500"
            && d.severity == ts_aot_driver::Severity::Error
            && d.message.contains("narrowed optional variable")),
        "a nested narrowed update must be an error; got {:?}",
        out.diagnostics
    );
}

#[test]
fn a_second_update_of_the_same_local_is_reported() {
    const SOURCE: &str = "export function doubleUpdate(): number { \
         let x: number | null = null; x = 1; x += (x += 1); return x; }";
    let out = compile_module(SOURCE);
    assert!(
        out.has_errors(),
        "the nested second update of the same local must be reported; got {:?}",
        out.diagnostics
    );
    assert_eq!(
        out.diagnostics
            .iter()
            .filter(|d| d.message.contains("narrowed optional variable"))
            .count(),
        1,
        "exactly the nested occurrence is reported (the root is copied back); got {:?}",
        out.diagnostics
    );
}

#[test]
fn a_constructor_that_cannot_call_keeps_the_narrowing() {
    const SOURCE: &str = "class Holder { n: number; constructor() { this.n = 1; } } \
         export function afterNewClosure(): number { \
             let x: number | null = null; x = 5; \
             const g = (): void => { x = null; }; \
             const created = new Holder(); return x; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(new_closure::afterNewClosure(&ctx), 5.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    check_generated_crate("new_closure", &rust);
    let stdout = run_generated_crate("new_closure", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_constructor_that_could_call_stays_conservative() {
    for source in [
        "class Toucher { constructor() { poke(); } } function poke(): void {} \
         export function f(): number { let x: number | null = null; x = 5; \
             const g = (): void => { x = null; }; const t = new Toucher(); return x; }",
        "export function f(): number { let x: number | null = null; x = 5; \
             const g = (): void => { x = null; }; const d = new Date(0); return x; }",
    ] {
        let out = compile_module(source);
        assert!(
            out.has_errors(),
            "`{source}` must stay conservatively rejected; got {:?}",
            out.diagnostics
        );
        assert!(
            out.diagnostics.iter().any(|d| d.code.as_str() == "P0018"),
            "`{source}` must be rejected with P0018; got {:?}",
            out.diagnostics
        );
    }
}

#[test]
fn a_subclass_constructor_stays_conservative() {
    const SOURCE: &str = "class Base { constructor() {} } \
         class Sub extends Base { constructor() { super(); } } \
         export function f(): number { let x: number | null = null; x = 5; \
             const g = (): void => { x = null; }; const s = new Sub(); return x; }";
    let out = compile_module(SOURCE);
    assert!(
        out.has_errors(),
        "a subclass must stay conservatively rejected; got {:?}",
        out.diagnostics
    );
    assert!(
        out.diagnostics.iter().any(|d| d.code.as_str() == "P0018"),
        "a subclass must be rejected with P0018; got {:?}",
        out.diagnostics
    );
}
