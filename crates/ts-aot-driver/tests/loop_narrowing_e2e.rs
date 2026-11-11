//! Regression tests for the third review round: loop widening, a narrowed
//! compound update in an expression position, and the `new` invalidation.
//!
//! * a narrowing established *before* a loop survives it when the loop body
//!   cannot write the variable (and is still withdrawn when it can);
//! * `x++`/`x += v` of a narrowed variable computes on the narrowed copy, so the
//!   new value has to be copied back into the storage - when the update is the
//!   whole expression of a statement slot it is, and a nested one is a blocking
//!   error instead of silently keeping the stale storage value;
//! * `new` withdraws the narrowing of a captured variable only when the
//!   constructor could actually call it.

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

/// B1: the narrowed copy is materialised once, outside the loop, and no
/// iteration can write the variable, so it is still the variable's value when
/// the loop ends. Before the fix the loop widening withdrew the narrowing of a
/// variable the body never writes, and every one of these was a false `P0018`.
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

/// B1: the `for..in` arm of the loop widening behaves like the others - a body
/// that cannot write the variable keeps the narrowing. This is a frontend-only
/// assertion: the backend does not lower `for..in` over an object literal yet, so
/// the generated crate would not compile for an unrelated reason.
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

/// B1 control: a loop that *can* write the variable still withdraws the
/// narrowing (the reads after the loop must see the storage), a conditional
/// write keeps the writes visible, and a narrowing established *inside* the body
/// still works.
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

/// B3: `x++` / `x += 2` as a whole expression statement computes on the narrowed
/// copy, and the copy has to reach the storage - a closure called afterwards must
/// observe the *updated* value. `x++` used to write only the copy (6 became 5 for
/// the closure), and `x += 2` already copied back, so both are pinned here.
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

/// B3: the same update in a `let`/`const` initializer has a statement slot too,
/// so `const y = x++` yields the old value (5) while the storage becomes the new
/// one (6): `h.v + y` is 11, as in JavaScript (it used to be 10). `x += 2` yields
/// the new value (7) and 7 + 7 = 14 (it used to be 12).
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

/// B3: a narrowed update nested in a larger expression has no slot to write the
/// storage from, so the storage would keep the old value while the narrowed copy
/// moves on - a program that still compiles and answers differently from
/// JavaScript. It is therefore a *blocking* `E0500` error, not a warning.
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

/// B3b: two updates of the *same* narrowed local in one expression. Only the
/// occurrence that is the whole statement slot can be copied back, so the second
/// one is still reported - the check used to skip every record with that local
/// id, which silently accepted the stale storage.
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

/// B4: a class whose constructor and field initializers contain no call cannot
/// reach the writing closure, so the narrowing of the captured variable survives
/// `new Holder()` and the function returns 5. It used to be a false `P0018`.
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

/// B4 conservatism: the relaxation only covers a class whose constructor and
/// field initializers are visible and contain no call. A constructor that *does*
/// contain a call, and a builtin constructor the frontend cannot inspect
/// (`new Date()`, `new Map()`), keep withdrawing the narrowing of a captured
/// variable - the constructor could run the closure, and a stale narrowed copy
/// would be unsound. `P0018` on `return x` is the conservative rejection.
#[test]
fn a_constructor_that_could_call_stays_conservative() {
    for source in [
        // The constructor calls a function, so the closure passed to (or stored
        // by) the class could be the one that writes `x`.
        "class Toucher { constructor() { poke(); } } function poke(): void {} \
         export function f(): number { let x: number | null = null; x = 5; \
             const g = (): void => { x = null; }; const t = new Toucher(); return x; }",
        // A builtin constructor is not a class of this program, so its body is
        // unknown and the withdrawal is kept.
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

/// B4 conservatism: a subclass runs its base constructor first, so the base could
/// call the closure; the frontend does not follow the base chain and keeps the
/// withdrawal.
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
