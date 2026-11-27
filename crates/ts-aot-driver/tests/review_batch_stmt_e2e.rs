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

fn compile_crate(source: &str) -> ts_aot_driver::DriverOutput {
    Driver::new().compile_source(
        "test.ts",
        source,
        &CompileOptions {
            emit: EmitStage::Crate,
            module: true,
        },
    )
}

fn error_message(out: &ts_aot_driver::DriverOutput, code: &str) -> String {
    out.diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code.as_str() == code)
        .unwrap_or_else(|| panic!("expected a {code} diagnostic; got {:?}", out.diagnostics))
        .message
        .clone()
}

#[test]
fn a_destructuring_temp_never_takes_a_source_local_name() {
    const SOURCE: &str = "export function collide(): number { \
         let _destructured_0: number = 42; let x: number = 0; const arr: number[] = [7]; \
         [x] = arr; return _destructured_0 * 100 + x; } \
         export function collideDecl(): number { \
         let _destructured_0: number = 42; const arr: number[] = [7]; const [y] = arr; \
         return _destructured_0 * 100 + y; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(
        review_batch_stmt_temp::collide(&ctx),
        4207.0,
        "node keeps the source local `_destructured_0` at 42 and writes the element 7 into `x`, so the result is 4207"
    );
    assert_eq!(
        review_batch_stmt_temp::collideDecl(&ctx),
        4207.0,
        "node keeps the source local `_destructured_0` at 42 and binds the element 7 to `y`, so the result is 4207"
    );
    println!("ok");
}
"#;
    let out = compile_module(SOURCE);
    assert!(
        !out.has_errors(),
        "a source local named like the compiler temp must compile; got {:?}",
        out.diagnostics
    );
    let rust = out.rust_source.expect("rust_source");
    check_generated_crate("review_batch_stmt_temp", &rust);
    let stdout = run_generated_crate("review_batch_stmt_temp", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn an_undefined_guard_does_not_survive_a_write_in_the_surviving_branch() {
    const SOURCE: &str = "export function narrowWrite(): number { \
         const xs: number[] = []; xs.push(3); let [n] = xs; \
         if (n === undefined) { return 1; } else { n = undefined; } \
         if (n === undefined) { return 2; } return n + 1; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(
        review_batch_stmt_guard::narrowWrite(&ctx),
        2.0,
        "node runs the surviving `else` branch, so `n` is `undefined` again and the second guard returns 2"
    );
    println!("ok");
}
"#;
    let out = compile_module(SOURCE);
    assert!(
        !out.has_errors(),
        "a guard whose surviving branch writes the binding must still compile; got {:?}",
        out.diagnostics
    );
    let rust = out.rust_source.expect("rust_source");
    check_generated_crate("review_batch_stmt_guard", &rust);
    let stdout = run_generated_crate("review_batch_stmt_guard", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_wider_optional_binding_accepts_an_absent_destructured_element() {
    const SOURCE: &str = "export function widenEmpty(x: number | null | undefined): number { \
         const arr: number[] = []; var [x] = arr; return x == null ? 1 : 2; } \
         export function widenPresent(x: number | null | undefined): number { \
         const arr: number[] = [7]; var [x] = arr; return x == null ? 1 : 2; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(
        review_batch_stmt_widen::widenEmpty(&ctx, ::core::option::Option::Some(5.0)),
        1.0,
        "node reads an absent element as `undefined`, which the wider binding can represent"
    );
    assert_eq!(
        review_batch_stmt_widen::widenPresent(&ctx, ::core::option::Option::Some(5.0)),
        2.0,
        "node reads the present element 7"
    );
    println!("ok");
}
"#;
    let out = compile_module(SOURCE);
    assert!(
        !out.has_errors(),
        "a binding that allows `null` and `undefined` can represent an absent element; got {:?}",
        out.diagnostics
    );
    let rust = out.rust_source.expect("rust_source");
    check_generated_crate("review_batch_stmt_widen", &rust);
    let stdout = run_generated_crate("review_batch_stmt_widen", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_plain_number_binding_still_refuses_an_absent_destructured_element() {
    const SOURCE: &str = "export function f(x: number): number { \
         const arr: number[] = []; var [x] = arr; return 1; }";
    let out = compile_module(SOURCE);
    assert!(
        out.has_errors(),
        "a plain `number` binding cannot represent an absent element; got {:?}",
        out.diagnostics
    );
    let message = error_message(&out, "P0018");
    assert!(
        message.contains("not optional"),
        "the P0018 must explain that the existing binding is not optional; got: {message}"
    );
    assert!(
        out.rust_source.is_none(),
        "a refused source must not emit Rust; got:\n{}",
        out.rust_source.unwrap_or_default()
    );
}

#[test]
fn a_write_in_an_if_condition_invalidates_the_element_proof() {
    const SOURCE: &str = "export function condReassign(): number { \
         let arr: number[] = [10]; const empty: number[] = []; let out: number = 0; \
         if ((arr = empty).length === 0) { let x: number | undefined = 0; [x] = arr; \
         out = x === undefined ? 7 : 1; } return out; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(
        review_batch_stmt_condition::condReassign(&ctx),
        7.0,
        "node empties `arr` while evaluating the condition, so the branch reads an absent element and returns 7; a stale presence proof would read the Rust default 0 and return 1"
    );
    println!("ok");
}
"#;
    let out = compile_module(SOURCE);
    assert!(
        !out.has_errors(),
        "the branch reads an optional target, so the source must compile; got {:?}",
        out.diagnostics
    );
    let rust = out.rust_source.expect("rust_source");
    check_generated_crate("review_batch_stmt_condition", &rust);
    let stdout = run_generated_crate("review_batch_stmt_condition", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_write_in_an_if_condition_refuses_a_non_optional_target() {
    const SOURCE: &str = "export function condReassign(): number { \
         let arr: number[] = [10]; const empty: number[] = []; let out: number = 0; \
         if ((arr = empty).length === 0) { let x: number = 0; [x] = arr; out = x + 1; } \
         return out; }";
    let out = compile_module(SOURCE);
    assert!(
        out.has_errors(),
        "the condition empties `arr`, so the read into the non-optional `x` must be refused; got {:?}",
        out.diagnostics
    );
    let message = error_message(&out, "P0018");
    assert!(
        message.contains("may be absent"),
        "the P0018 must explain that the element may be absent; got: {message}"
    );
    assert!(
        out.rust_source.is_none(),
        "a refused source must not emit Rust; got:\n{}",
        out.rust_source.unwrap_or_default()
    );
}

#[test]
fn an_aliased_caller_callback_keeps_its_receiver_origin() {
    const SOURCE: &str = "class C { x: number = 0; \
         m(cb: () => number): number { this.x = 1; return cb(); } \
         k(first: () => number, second: () => number, flag: boolean): number { \
             let alias: () => number = first; if (flag) alias = second; \
             return this.m(alias); } } \
         export function f(cs: C[]): number { \
             return cs[0].k((): number => 0, \
                 (): number => { cs[0] = new C(); return 0; }, true); }";
    let out = compile_module(SOURCE);
    assert!(
        out.has_errors(),
        "the callback assigned to the alias inside `k` still reassigns the receiver place \
         `cs[0]`, so the alias must keep the caller origin of `second`; got {:?}",
        out.diagnostics
    );
    let message = error_message(&out, "E0419");
    assert!(
        message.contains("may reassign"),
        "the E0419 must explain the receiver reassignment; got: {message}"
    );
    assert!(
        out.rust_source.is_none(),
        "a refused source must not emit Rust; got:\n{}",
        out.rust_source.unwrap_or_default()
    );
}

#[test]
fn a_top_level_destructuring_reads_the_lowered_prefix() {
    const SOURCE: &str = r"
let first: number = 0;
let second: number = 0;
const arr: number[] = [10, 20];
[first] = arr;
[second] = arr;
let fromGlobal: number = 0;
let garr: number[] = [30];
[fromGlobal] = garr;
let literal: number = 0;
[literal] = [40];
export function got(): number { return first * 1000 + second * 100 + fromGlobal * 10 + literal; }
";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    review_batch_stmt_tla::__ts_aot_init();
    assert_eq!(
        review_batch_stmt_tla::got(&ctx),
        11340.0,
        "node reads 10 twice through the local array, 30 through the global and 40 through the literal"
    );
    println!("ok");
}
"#;
    let out = compile_crate(SOURCE);
    assert!(
        !out.has_errors(),
        "every top-level element here is provably present; got {:?}",
        out.diagnostics
    );
    let rust = out.rust_source.expect("rust_source");
    check_generated_crate("review_batch_stmt_tla", &rust);
    let stdout = run_generated_crate("review_batch_stmt_tla", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_nested_array_literal_return_takes_the_declared_element_type() {
    const SOURCE: &str = "export function nested(): number[][] { return [[1, 2]]; } \
         export function emptyNested(): number[][] { return [[]]; } \
         export function twoRows(): number[][] { return [[1], [2, 3]]; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    let nested = review_batch_stmt_nested::nested(&ctx);
    assert_eq!(nested.len(), 1, "node returns one row");
    assert_eq!(&nested[0][..], &[1.0, 2.0][..], "node returns [[1, 2]]");
    let empty = review_batch_stmt_nested::emptyNested(&ctx);
    assert_eq!(empty.len(), 1, "node returns one empty row");
    assert!(empty[0].is_empty(), "node returns [[]]");
    let two = review_batch_stmt_nested::twoRows(&ctx);
    assert_eq!(two.len(), 2, "node returns two rows");
    assert_eq!(&two[0][..], &[1.0][..], "node returns the row [1]");
    assert_eq!(&two[1][..], &[2.0, 3.0][..], "node returns the row [2, 3]");
    println!("ok");
}
"#;
    let out = compile_module(SOURCE);
    assert!(
        !out.has_errors(),
        "nested array literals in a declared return must compile; got {:?}",
        out.diagnostics
    );
    let rust = out.rust_source.expect("rust_source");
    check_generated_crate("review_batch_stmt_nested", &rust);
    let stdout = run_generated_crate("review_batch_stmt_nested", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}
