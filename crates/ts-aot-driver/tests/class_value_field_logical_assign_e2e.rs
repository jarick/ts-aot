use ts_aot_driver::{CompileOptions, Driver, EmitStage};
use ts_aot_test_support::run_generated_crate;

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

fn codes(out: &ts_aot_driver::DriverOutput) -> Vec<&str> {
    out.diagnostics.iter().map(|d| d.code.as_str()).collect()
}

fn crate_rust_of(source: &str) -> String {
    let out = Driver::new().compile_source(
        "test.ts",
        source,
        &CompileOptions {
            emit: EmitStage::Crate,
            module: true,
        },
    );
    assert!(
        !out.has_errors(),
        "must compile through the full pipeline; got {:?}",
        out.diagnostics
    );
    out.rust_source
        .expect("emit-crate must populate rust_source")
}

const HOLDER: &str = "class Holder { C = class { static k: number = 1; }; }";

#[test]
fn an_or_assignment_after_a_falsy_poison_reaches_the_class_on_the_right() {
    const SOURCE: &str = "class Holder { C = class { static k: number = 1; }; } \
         export function got(): number { \
             const h = new Holder(); \
             h.C = 0; \
             h.C ||= class { static k: number = 2; }; \
             return h.C.k; \
         }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(logical_assign_falsy::got(&ctx), 2.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate("logical_assign_falsy", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_coalesce_assignment_after_a_non_nullish_falsy_poison_is_refused() {
    const SOURCE: &str = "class Holder { C = class { static k: number = 1; }; } \
         export function got(): number { \
             const h = new Holder(); \
             h.C = 0; \
             h.C ??= class { static k: number = 2; }; \
             return h.C.k; \
         }";
    let out = compile_module(SOURCE);
    assert!(
        codes(&out).contains(&"E0419"),
        "`??=` keeps a `0`, so the static read still has no class to select; got {:?}",
        out.diagnostics
    );
}

#[test]
fn a_coalesce_assignment_after_a_null_poison_is_accepted() {
    const SOURCE: &str = "class Holder { C = class { static k: number = 1; }; } \
         export function got(): number { \
             const h = new Holder(); \
             h.C = null; \
             h.C ??= class { static k: number = 2; }; \
             return h.C.k; \
         }";
    let out = compile_module(SOURCE);
    assert!(
        !codes(&out).contains(&"E0419"),
        "`??=` overwrites a `null`, so the field's class is known again; got {:?}",
        out.diagnostics
    );
    assert!(
        !out.has_errors(),
        "`??=` overwrites a `null`, so the field's class is known again; got {:?}",
        out.diagnostics
    );
    let rust = out
        .rust_source
        .expect("emit-rust must populate rust_source");
    let normalized: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
    let value_at = normalized
        .find(":f64=2.0;")
        .expect("the class on the right must own a static `k` global holding `2.0`");
    let prefix = &normalized[..value_at];
    let name_start = prefix
        .rfind("pubstatic")
        .expect("the `2.0` global must be declared with `pub static`")
        + "pubstatic".len();
    let name = &prefix[name_start..];
    assert!(
        name.starts_with("__k_"),
        "the `2.0` global must be a static `k` field, got `{name}`"
    );
    assert!(
        normalized.contains(&format!("return{name};")),
        "the static read must resolve to the class on the right (`{name}`), got:\n{rust}"
    );
}

#[test]
fn an_or_assignment_after_a_truthy_poison_is_refused() {
    const SOURCE: &str = "class Holder { C = class { static k: number = 1; }; } \
         export function got(): number { \
             const h = new Holder(); \
             h.C = 5; \
             h.C ||= class { static k: number = 2; }; \
             return h.C.k; \
         }";
    let out = compile_module(SOURCE);
    assert!(
        codes(&out).contains(&"E0419"),
        "`||=` keeps the truthy `5`, so the field is still not a class value; got {:?}",
        out.diagnostics
    );
}

#[test]
fn a_negated_zero_poison_reaches_the_class_on_the_right() {
    const SOURCE: &str = "class Zero { C = class { static k: number = 1; }; } \
         class Paren { C = class { static k: number = 1; }; } \
         class Product { C = class { static k: number = 1; }; } \
         export function from_neg_int_zero(): number { \
             const h = new Zero(); \
             h.C = -0; \
             h.C ||= class { static k: number = 2; }; \
             return h.C.k; \
         } \
         export function from_neg_parenthesized_zero(): number { \
             const h = new Paren(); \
             h.C = -(0); \
             h.C ||= class { static k: number = 3; }; \
             return h.C.k; \
         } \
         export function from_neg_float_product(): number { \
             const h = new Product(); \
             h.C = -(0.0 * 1); \
             h.C ||= class { static k: number = 4; }; \
             return h.C.k; \
         }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(negated_zero_poison::from_neg_int_zero(&ctx), 2.0);
    assert_eq!(negated_zero_poison::from_neg_parenthesized_zero(&ctx), 3.0);
    assert_eq!(negated_zero_poison::from_neg_float_product(&ctx), 4.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate("negated_zero_poison", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_negated_nonzero_poison_is_refused() {
    const SOURCE: &str = "class Holder { C = class { static k: number = 1; }; } \
         export function got(): number { \
             const h = new Holder(); \
             h.C = -5; \
             h.C ||= class { static k: number = 2; }; \
             return h.C.k; \
         }";
    let out = compile_module(SOURCE);
    assert!(
        codes(&out).contains(&"E0419"),
        "`||=` keeps the truthy `-5`, so the field is still not a class value; got {:?}",
        out.diagnostics
    );
}

#[test]
fn an_and_assignment_to_a_class_value_field_is_refused() {
    const SOURCE: &str = "class Holder { C = class { static k: number = 1; }; } \
         export function got(): number { \
             const h = new Holder(); \
             h.C = 0; \
             h.C &&= class { static k: number = 2; }; \
             return h.C.k; \
         }";
    let out = compile_module(SOURCE);
    assert!(
        out.diagnostics.iter().any(|d| {
            d.code.as_str() == "E0419"
                && d.message.contains("logical assignment `&&=`")
                && !d.message.contains("tag 0")
        }),
        "`&&=` must be refused because testing a class value for truthiness has no sound \
         lowering; got {:?}",
        out.diagnostics
    );
}

#[test]
fn an_and_assignment_to_a_fresh_class_value_field_takes_the_right_hand_side() {
    const SOURCE: &str = "class Holder { C = class { static k: number = 1; }; } \
         export function got(): number { \
             const h = new Holder(); \
             h.C &&= class { static k: number = 2; }; \
             return h.C.k; \
         }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(and_assign_fresh::got(&ctx), 2.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate("and_assign_fresh", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn an_and_assignment_after_a_class_write_takes_the_right_hand_side() {
    const SOURCE: &str = "class Holder { C = class { static k: number = 1; }; } \
         export function got(): number { \
             const h = new Holder(); \
             h.C = class { static k: number = 2; }; \
             h.C &&= class { static k: number = 3; }; \
             return h.C.k; \
         }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(and_assign_after_class::got(&ctx), 3.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate("and_assign_after_class", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn an_and_assignment_of_a_plain_value_after_a_class_write_poisons_the_field() {
    const SOURCE: &str = "class Holder { C = class { static k: number = 1; }; } \
         export function got(): number { \
             const h = new Holder(); \
             h.C = class { static k: number = 2; }; \
             h.C &&= 0; \
             return h.C.k; \
         }";
    let out = compile_module(SOURCE);
    assert!(
        codes(&out).contains(&"E0419"),
        "the class value is truthy, so the `&&=` stores the plain `0` and the field is no longer \
         a class value; got {:?}",
        out.diagnostics
    );
}

#[test]
fn a_coalesce_assignment_keeps_a_class_value_field_and_ignores_a_fieldless_class() {
    const SOURCE: &str = "class Fresh { C = class { static k: number = 1; }; } \
         class Written { C = class { static k: number = 1; }; } \
         class Other { static z: number = 9; } \
         export function fresh(): number { \
             const h = new Fresh(); \
             h.C ??= Other; \
             return h.C.k; \
         } \
         export function written(): number { \
             const h = new Written(); \
             h.C = class { static k: number = 3; }; \
             h.C ??= Other; \
             return h.C.k; \
         }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(coalesce_keeps_class::fresh(&ctx), 1.0);
    assert_eq!(coalesce_keeps_class::written(&ctx), 3.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate("coalesce_keeps_class", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn an_or_assignment_after_a_truthy_poison_then_a_falsy_write_is_refused() {
    const SOURCE: &str = "class Holder { C = class { static k: number = 1; }; } \
         export function got(): number { \
             const h = new Holder(); \
             h.C = 5; \
             h.C ||= 0; \
             h.C ||= class { static k: number = 2; }; \
             return h.C.k; \
         }";
    let out = compile_module(SOURCE);
    assert!(
        codes(&out).contains(&"E0419"),
        "the truthy `5` leaves the field's class unknown, and `||= 0` keeps that `5`, so the \
         static read cannot select the class on the right; got {:?}",
        out.diagnostics
    );
}

#[test]
fn an_or_assignment_after_a_truthy_poison_behind_a_branch_is_refused() {
    const SOURCE: &str = "class Holder { C = class { static k: number = 1; }; } \
         export function got(flag: boolean): number { \
             const h = new Holder(); \
             h.C = 5; \
             if (flag) { h.C = 0; } \
             h.C ||= class { static k: number = 2; }; \
             return h.C.k; \
         }";
    let out = compile_module(SOURCE);
    assert!(
        codes(&out).contains(&"E0419"),
        "the branch that does not run keeps the truthy `5`, so the field's class is still \
         unknown when the `||=` runs; got {:?}",
        out.diagnostics
    );
}

#[test]
fn a_plain_assignment_still_clears_a_poison() {
    let source = format!(
        "{HOLDER} export function got(): number {{ \
             const h = new Holder(); \
             h.C = 0; \
             h.C = class {{ static k: number = 3; }}; \
             return h.C.k; \
         }}"
    );
    let out = compile_module(&source);
    assert!(
        !out.has_errors(),
        "a plain `=` replaces the poisoned value, so the class is known again; got {:?}",
        out.diagnostics
    );
}

#[test]
fn an_unconditional_overwrite_after_a_branch_clears_the_poison() {
    const SOURCE: &str = "class Holder { C = class { static k: number = 1; }; } \
         export function got(flag: boolean): number { \
             const h = new Holder(); \
             if (flag) { h.C = 5; } \
             h.C = 0; \
             h.C ||= class { static k: number = 2; }; \
             return h.C.k; \
         }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(branch_then_unconditional_overwrite::got(&ctx, true), 2.0);
    assert_eq!(branch_then_unconditional_overwrite::got(&ctx, false), 2.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate("branch_then_unconditional_overwrite", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn an_identical_ternary_overwrite_clears_an_earlier_poison() {
    const SOURCE: &str = "class Holder { C = class { static k: number = 1; }; } \
         export function got(flag: boolean): number { \
             const h = new Holder(); \
             h.C = 5; \
             h.C = flag ? 0 : 0; \
             h.C ||= class { static k: number = 2; }; \
             return h.C.k; \
         }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(identical_ternary_overwrite::got(&ctx, true), 2.0);
    assert_eq!(identical_ternary_overwrite::got(&ctx, false), 2.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate("identical_ternary_overwrite", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_do_while_body_write_clears_an_earlier_poison() {
    const SOURCE: &str = "class Holder { C = class { static k: number = 1; }; } \
         export function got(): number { \
             const h = new Holder(); \
             h.C = 5; \
             do { h.C = 0; } while (false); \
             h.C ||= class { static k: number = 2; }; \
             return h.C.k; \
         }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(do_while_body_write::got(&ctx), 2.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate("do_while_body_write", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn other_unconditional_writes_clear_an_earlier_poison() {
    const SOURCE: &str = "class BothBranches { C = class { static k: number = 1; }; } \
         class ConstantTrue { C = class { static k: number = 1; }; } \
         class FinallyBlock { C = class { static k: number = 1; }; } \
         class DefaultCase { C = class { static k: number = 1; }; } \
         class ConstantLocal { C = class { static k: number = 1; }; } \
         export function both_branches(flag: boolean): number { \
             const h = new BothBranches(); \
             h.C = 5; \
             if (flag) { h.C = 0; } else { h.C = 0; } \
             h.C ||= class { static k: number = 2; }; \
             return h.C.k; \
         } \
         export function constant_true(): number { \
             const h = new ConstantTrue(); \
             h.C = 5; \
             if (true) { h.C = 0; } \
             h.C ||= class { static k: number = 2; }; \
             return h.C.k; \
         } \
         export function finally_block(): number { \
             const h = new FinallyBlock(); \
             h.C = 5; \
             try { } finally { h.C = 0; } \
             h.C ||= class { static k: number = 2; }; \
             return h.C.k; \
         } \
         export function default_case(n: number): number { \
             const h = new DefaultCase(); \
             h.C = 5; \
             switch (n) { default: h.C = 0; } \
             h.C ||= class { static k: number = 2; }; \
             return h.C.k; \
         } \
         export function constant_local(flag: boolean): number { \
             const h = new ConstantLocal(); \
             h.C = 5; \
             const z = flag ? 0 : 0; \
             h.C = z; \
             h.C ||= class { static k: number = 2; }; \
             return h.C.k; \
         }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(other_unconditional_writes::both_branches(&ctx, true), 2.0);
    assert_eq!(other_unconditional_writes::both_branches(&ctx, false), 2.0);
    assert_eq!(other_unconditional_writes::constant_true(&ctx), 2.0);
    assert_eq!(other_unconditional_writes::finally_block(&ctx), 2.0);
    assert_eq!(other_unconditional_writes::default_case(&ctx, 1.0), 2.0);
    assert_eq!(other_unconditional_writes::constant_local(&ctx, true), 2.0);
    assert_eq!(other_unconditional_writes::constant_local(&ctx, false), 2.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate("other_unconditional_writes", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_write_inside_a_ternary_branch_keeps_the_poison() {
    let cases = [
        (
            "a ternary branch passed as an argument",
            "h.C = 5; f(flag ? (h.C = 0) : 0);",
        ),
        (
            "a ternary branch bound to a local",
            "h.C = 5; const z = flag ? (h.C = 0) : 1;",
        ),
    ];
    for (what, body) in cases {
        let source = format!(
            "{HOLDER} function f(x: number): number {{ return x; }} \
             export function got(flag: boolean): number {{ \
                 const h = new Holder(); \
                 {body} \
                 h.C ||= class {{ static k: number = 2; }}; \
                 return h.C.k; \
             }}"
        );
        let out = compile_module(&source);
        assert!(
            codes(&out).contains(&"E0419"),
            "{what} runs only when the condition holds, so the truthy `5` may stay in the field, \
             the `||=` may keep it, and the static read must be refused; got {:?}",
            out.diagnostics
        );
    }
}

#[test]
fn writes_that_may_be_skipped_keep_the_poison() {
    let cases = [
        ("an `if (false)` body", "h.C = 5; if (false) { h.C = 0; }"),
        (
            "a `while (false)` body",
            "h.C = 5; while (false) { h.C = 0; }",
        ),
        (
            "a ternary whose branches disagree",
            "h.C = 5; h.C = flag ? 0 : 5;",
        ),
        (
            "an `else` branch that keeps the truthy value",
            "h.C = 5; if (flag) { h.C = 0; } else { h.C = 5; }",
        ),
        ("a `catch` body", "h.C = 5; try { } catch (e) { h.C = 0; }"),
    ];
    for (what, body) in cases {
        let source = format!(
            "{HOLDER} export function got(flag: boolean): number {{ \
                 const h = new Holder(); \
                 {body} \
                 h.C ||= class {{ static k: number = 2; }}; \
                 return h.C.k; \
             }}"
        );
        let out = compile_module(&source);
        assert!(
            codes(&out).contains(&"E0419"),
            "{what} may leave the truthy `5` in the field, so the `||=` does not restore a \
             known class and the static read must be refused; got {:?}",
            out.diagnostics
        );
    }
}

#[test]
fn a_method_body_write_clears_an_earlier_poison() {
    const SOURCE: &str = "class Holder { C = class { static k: number = 1; }; } \
         class Machine { \
             run(h: Holder): number { \
                 h.C = 5; \
                 h.C = 0; \
                 h.C ||= class { static k: number = 2; }; \
                 return h.C.k; \
             } \
         } \
         export function got(): number { \
             const h = new Holder(); \
             const m: Machine = new Machine(); \
             return m.run(h); \
         }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(method_body_write::got(&ctx), 2.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate("method_body_write", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_sequential_write_inside_a_branch_clears_an_earlier_poison() {
    const SOURCE: &str = "class Holder { C = class { static k: number = 1; }; } \
         export function got(flag: boolean): number { \
             const h = new Holder(); \
             if (flag) { \
                 h.C = 5; \
                 h.C = 0; \
                 h.C ||= class { static k: number = 2; }; \
                 return h.C.k; \
             } \
             return 0; \
         }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(branch_sequential_write::got(&ctx, true), 2.0);
    assert_eq!(branch_sequential_write::got(&ctx, false), 0.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate("branch_sequential_write", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_try_body_write_clears_an_earlier_poison() {
    const SOURCE: &str = "class Holder { C = class { static k: number = 1; }; } \
         export function got(): number { \
             const h = new Holder(); \
             h.C = 5; \
             try { \
                 h.C = 0; \
             } catch (e) { \
             } \
             h.C ||= class { static k: number = 2; }; \
             return h.C.k; \
         }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(try_body_write::got(&ctx), 2.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate("try_body_write", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_module_const_write_clears_an_earlier_poison() {
    const SOURCE: &str = "const zero = 0; \
         class Holder { C = class { static k: number = 1; }; } \
         export function got(): number { \
             const h = new Holder(); \
             h.C = 5; \
             h.C = zero; \
             h.C ||= class { static k: number = 2; }; \
             return h.C.k; \
         }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(module_const_write::got(&ctx), 2.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate("module_const_write", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn an_or_assignment_to_a_fresh_class_value_field_keeps_the_first_class() {
    const SOURCE: &str = "class Holder { C = class { static k: number = 1; }; } \
         export function got(): number { \
             const h = new Holder(); \
             h.C ||= class { static k: number = 2; }; \
             return h.C.k; \
         }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(or_keeps_the_first_class::got(&ctx), 1.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate("or_keeps_the_first_class", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn an_or_assignment_of_a_plain_value_to_a_fresh_class_value_field_is_skipped() {
    const SOURCE: &str = "class Holder { C = class { static k: number = 1; }; } \
         export function got(): number { \
             const h = new Holder(); \
             h.C ||= 0; \
             h.C ||= class { static k: number = 2; }; \
             return h.C.k; \
         }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(or_skips_the_plain_value::got(&ctx), 1.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate("or_skips_the_plain_value", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn an_or_assignment_after_a_conditional_falsy_write_keeps_the_first_class() {
    const SOURCE: &str = "class Holder { C = class { static k: number = 1; }; } \
         export function got(flag: boolean): number { \
             const h = new Holder(); \
             if (flag) { h.C = 0; } \
             h.C ||= class { static k: number = 2; }; \
             return h.C.k; \
         }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(conditional_falsy_or::got(&ctx, true), 2.0);
    assert_eq!(conditional_falsy_or::got(&ctx, false), 1.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate("conditional_falsy_or", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn an_or_assignment_after_a_conditional_write_behind_a_kept_value_keeps_the_class() {
    const SOURCE: &str = "class Holder { C = class { static k: number = 1; }; } \
         export function got(flag: boolean): number { \
             const h = new Holder(); \
             h.C ||= 5; \
             if (flag) { h.C = 0; } \
             h.C ||= class { static k: number = 2; }; \
             return h.C.k; \
         }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(behind_a_kept_value_or::got(&ctx, true), 2.0);
    assert_eq!(behind_a_kept_value_or::got(&ctx, false), 1.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate("behind_a_kept_value_or", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_class_write_inside_a_branch_keeps_the_earlier_poison() {
    const SOURCE: &str = "class Holder { C = class { static k: number = 1; }; } \
         export function got(flag: boolean): number { \
             const h = new Holder(); \
             h.C = 5; \
             if (flag) { h.C = class { static k: number = 3; }; } \
             h.C ||= class { static k: number = 2; }; \
             return h.C.k; \
         }";
    let out = compile_module(SOURCE);
    assert!(
        codes(&out).contains(&"E0419"),
        "the branch that does not run keeps the truthy `5`, so the `||=` keeps it and the static \
         read must be refused; got {:?}",
        out.diagnostics
    );
}

#[test]
fn a_coalesce_assignment_inside_a_branch_keeps_the_earlier_poison() {
    const SOURCE: &str = "class Holder { C = class { static k: number = 1; }; } \
         export function got(flag: boolean): number { \
             const h = new Holder(); \
             h.C = null; \
             if (flag) { h.C ??= class { static k: number = 2; }; } \
             return h.C.k; \
         }";
    let out = compile_module(SOURCE);
    assert!(
        codes(&out).contains(&"E0419"),
        "the branch that does not run keeps the null, so the `??=` cannot restore a known class; \
         got {:?}",
        out.diagnostics
    );
}

#[test]
fn a_conditional_falsy_write_after_a_coalesce_assignment_lets_the_or_assignment_restore() {
    const SOURCE: &str = "class Holder { C = class { static k: number = 1; }; } \
         export function got(flag: boolean): number { \
             const h = new Holder(); \
             h.C ??= 0; \
             if (flag) { h.C = 0; } \
             h.C ||= class { static k: number = 2; }; \
             return h.C.k; \
         }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(coalesce_then_conditional_falsy::got(&ctx, true), 2.0);
    assert_eq!(coalesce_then_conditional_falsy::got(&ctx, false), 1.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate("coalesce_then_conditional_falsy", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_conditional_truthy_write_after_a_coalesce_assignment_keeps_the_poison() {
    const SOURCE: &str = "class Holder { C = class { static k: number = 1; }; } \
         export function got(flag: boolean): number { \
             const h = new Holder(); \
             h.C ??= 0; \
             if (flag) { h.C = 5; } \
             h.C ||= class { static k: number = 2; }; \
             return h.C.k; \
         }";
    let out = compile_module(SOURCE);
    assert!(
        codes(&out).contains(&"E0419"),
        "the branch that runs keeps the truthy `5`, and the `||=` keeps it too, so the static read \
         has no class to select; got {:?}",
        out.diagnostics
    );
}

#[test]
fn unconditional_falsy_writes_still_let_an_or_assignment_restore_the_class() {
    const SOURCE: &str = "class Untouched { C = class { static k: number = 1; }; } \
         class Sequential { C = class { static k: number = 1; }; } \
         class BranchThenPlain { C = class { static k: number = 1; }; } \
         class TryBody { C = class { static k: number = 1; }; } \
         class DoBody { C = class { static k: number = 1; }; } \
         export function sequential(): number { \
             const h = new Sequential(); \
             h.C = 5; \
             h.C = 0; \
             h.C ||= class { static k: number = 2; }; \
             return h.C.k; \
         } \
         export function branch_then_plain(flag: boolean): number { \
             const h = new BranchThenPlain(); \
             if (flag) { h.C = 5; } \
             h.C = 0; \
             h.C ||= class { static k: number = 2; }; \
             return h.C.k; \
         } \
         export function try_body(): number { \
             const h = new TryBody(); \
             h.C = 5; \
             try { h.C = 0; } catch (e) { } \
             h.C ||= class { static k: number = 2; }; \
             return h.C.k; \
         } \
         export function do_body(): number { \
             const h = new DoBody(); \
             h.C = 5; \
             do { h.C = 0; } while (false); \
             h.C ||= class { static k: number = 2; }; \
             return h.C.k; \
         } \
         export function untouched(): number { \
             const h = new Untouched(); \
             h.C ||= class { static k: number = 2; }; \
             return h.C.k; \
         }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(unconditional_falsy::sequential(&ctx), 2.0);
    assert_eq!(unconditional_falsy::branch_then_plain(&ctx, true), 2.0);
    assert_eq!(unconditional_falsy::branch_then_plain(&ctx, false), 2.0);
    assert_eq!(unconditional_falsy::try_body(&ctx), 2.0);
    assert_eq!(unconditional_falsy::do_body(&ctx), 2.0);
    assert_eq!(unconditional_falsy::untouched(&ctx), 1.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate("unconditional_falsy", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn an_increment_of_a_class_value_field_keeps_the_poison() {
    const PLAIN: &str = "class Holder { C = class { static k: number = 1; }; } \
         export function got(): number { \
             const h = new Holder(); \
             const arr: number[] = [0]; \
             arr[0]++; \
             return arr[0] + 0; \
         }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(plain_increment::got(&ctx), 1.0);
    println!("ok");
}
"#;
    let source = format!(
        "{HOLDER} export function got(): number {{ \
             const h = new Holder(); \
             h.C = 0; \
             h.C++; \
             h.C ||= class {{ static k: number = 2; }}; \
             return h.C.k; \
         }}"
    );
    let out = compile_module(&source);
    assert!(
        codes(&out).contains(&"E0419"),
        "`h.C++` turns the falsy `0` into a truthy number, so the `||=` keeps it and the static \
         read has no class to select; got {:?}",
        out.diagnostics
    );

    let rust = rust_of(PLAIN);
    let stdout = run_generated_crate("plain_increment", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_throw_that_bypasses_a_restoring_write_keeps_the_poison() {
    const OWN_THROW: &str = "class Holder { C = class { static k: number = 1; }; } \
         export function got(flag: boolean): number { \
             const h = new Holder(); \
             h.C = 5; \
             try { if (flag) { throw 1; } h.C = 0; } catch (e) { } \
             h.C ||= class { static k: number = 2; }; \
             return h.C.k; \
         }";
    const CALLEE_THROW: &str = "class Holder { C = class { static k: number = 1; }; } \
         function maybe(flag: boolean): void { if (flag) { throw 1; } } \
         export function got(flag: boolean): number { \
             const h = new Holder(); \
             h.C = 5; \
             try { maybe(flag); h.C = 0; } catch (e) { } \
             h.C ||= class { static k: number = 2; }; \
             return h.C.k; \
         }";
    let cases = [
        ("a `throw` of the try body itself", OWN_THROW),
        ("a call that may throw", CALLEE_THROW),
    ];
    for (what, source) in cases {
        let out = compile_module(source);
        assert!(
            codes(&out).contains(&"E0419"),
            "{what} may skip the restoring write and the `catch` swallows the exception, so the \
             field may still hold the truthy `5` and the `||=` keeps it; the static read must be \
             refused; got {:?}",
            out.diagnostics
        );
    }
}

#[test]
fn an_or_assignment_after_a_sibling_branch_poison_keeps_its_right_hand_side() {
    const SOURCE: &str = "let hits: number = 0; \
         function rhs(): number { hits = hits + 1; return 0; } \
         class Holder { C = class { static k: number = 1; }; } \
         export function got(flag: boolean): number { \
             hits = 0; \
             const h = new Holder(); \
             if (flag) { h.C = 0; } \
             if (!flag) { h.C = class { static k: number = 3; }; } \
             h.C ||= rhs(); \
             return hits; \
         }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(sibling_branch_rhs::got(&ctx, true), 1.0);
    assert_eq!(sibling_branch_rhs::got(&ctx, false), 0.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate("sibling_branch_rhs", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn an_or_assignment_after_a_sibling_branch_poison_dispatches_the_reads() {
    const SOURCE: &str = "class Holder { C = class { static k: number = 1; }; } \
         export function got(flag: boolean): number { \
             const h = new Holder(); \
             if (flag) { h.C = 0; } \
             if (!flag) { h.C = class { static k: number = 3; }; } \
             if (!flag) { h.C = class { static k: number = 5; }; } \
             h.C ||= class { static k: number = 2; }; \
             return h.C.k; \
         }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(sibling_branch_reads::got(&ctx, true), 2.0);
    assert_eq!(sibling_branch_reads::got(&ctx, false), 5.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate("sibling_branch_reads", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn both_branches_writing_a_class_keep_the_conditional_poison_class() {
    const SOURCE: &str = "class Holder { C = class { static k: number = 1; }; } \
         export function got(flag: boolean, other: boolean): number { \
             const h = new Holder(); \
             if (flag) { h.C = class { static k: number = 2; }; } \
             else { h.C = class { static k: number = 3; }; } \
             if (other) { h.C = 0; } \
             h.C ||= class { static k: number = 4; }; \
             return h.C.k; \
         }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(conditional_poison_classes::got(&ctx, true, true), 4.0);
    assert_eq!(conditional_poison_classes::got(&ctx, true, false), 2.0);
    assert_eq!(conditional_poison_classes::got(&ctx, false, true), 4.0);
    assert_eq!(conditional_poison_classes::got(&ctx, false, false), 3.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate("conditional_poison_classes", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_while_body_falsy_write_still_lets_the_or_assignment_restore_the_class() {
    const SOURCE: &str = "class Holder { C = class { static k: number = 1; }; } \
         export function got(): number { \
             const h = new Holder(); \
             let i = 0; \
             while (i < 1) { i = i + 1; h.C = 0; } \
             h.C ||= class { static k: number = 2; }; \
             return h.C.k; \
         }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(while_body_falsy_or::got(&ctx), 2.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate("while_body_falsy_or", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_while_body_that_never_runs_keeps_the_first_class() {
    const SOURCE: &str = "class Holder { C = class { static k: number = 1; }; } \
         export function got(): number { \
             const h = new Holder(); \
             let i = 1; \
             while (i < 1) { i = i + 1; h.C = 0; } \
             h.C ||= class { static k: number = 2; }; \
             return h.C.k; \
         }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(while_body_skipped::got(&ctx), 1.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate("while_body_skipped", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_for_body_falsy_write_still_lets_the_or_assignment_restore_the_class() {
    const SOURCE: &str = "class Holder { C = class { static k: number = 1; }; } \
         export function got(): number { \
             const h = new Holder(); \
             for (let i = 0; i < 3; i = i + 1) { h.C = 0; break; } \
             h.C ||= class { static k: number = 2; }; \
             return h.C.k; \
         }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(for_body_falsy_or::got(&ctx), 2.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate("for_body_falsy_or", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_for_body_that_never_runs_keeps_the_first_class() {
    const SOURCE: &str = "class Holder { C = class { static k: number = 1; }; } \
         export function got(): number { \
             const h = new Holder(); \
             for (let i = 0; i < 0; i = i + 1) { h.C = 0; } \
             h.C ||= class { static k: number = 2; }; \
             return h.C.k; \
         }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(for_body_skipped::got(&ctx), 1.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate("for_body_skipped", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_fall_through_case_falsy_write_still_lets_the_or_assignment_restore_the_class() {
    const SOURCE: &str = "class Holder { C = class { static k: number = 1; }; } \
         export function got(): number { \
             const h = new Holder(); \
             switch (1) { \
                 case 1: h.C = 0; \
                 case 2: h.C ||= class { static k: number = 2; }; break; \
                 default: h.C ||= class { static k: number = 3; }; break; \
             } \
             return h.C.k; \
         }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(fall_through_falsy_or::got(&ctx), 2.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate("fall_through_falsy_or", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_switch_that_matches_no_case_keeps_the_first_class() {
    const SOURCE: &str = "class Holder { C = class { static k: number = 1; }; } \
         export function got(): number { \
             const h = new Holder(); \
             switch (2) { \
                 case 1: h.C = 0; \
                 case 2: h.C ||= class { static k: number = 2; }; break; \
                 default: h.C ||= class { static k: number = 3; }; break; \
             } \
             return h.C.k; \
         }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(switch_nomatch_falsy_or::got(&ctx), 1.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate("switch_nomatch_falsy_or", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn both_ternary_branches_writing_a_class_restore_the_field() {
    const SOURCE: &str = "class Holder { C = class { static k: number = 1; }; } \
         export function got(flag: boolean): number { \
             const h = new Holder(); \
             const r = flag \
                 ? (h.C = class { static k: number = 2; }) \
                 : (h.C ||= class { static k: number = 3; }); \
             return h.C.k; \
         }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(both_ternary_branches_write_a_class::got(&ctx, true), 2.0);
    assert_eq!(both_ternary_branches_write_a_class::got(&ctx, false), 1.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate("both_ternary_branches_write_a_class", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_branch_writing_a_plain_value_after_a_class_branch_keeps_the_poison() {
    let sources = [
        "class Holder { C = class { static k: number = 1; }; } \
         export function got(flag: boolean): number { \
             const h = new Holder(); \
             if (flag) { h.C = 0; } else { h.C = class { static k: number = 3; }; } \
             return h.C.k; \
         }",
        "class Holder { C = class { static k: number = 1; }; } \
         export function got(flag: boolean): number { \
             const h = new Holder(); \
             if (flag) { h.C = class { static k: number = 3; }; } else { h.C = 0; } \
             return h.C.k; \
         }",
        "class Holder { C = class { static k: number = 1; }; } \
         export function got(flag: boolean): number { \
             const h = new Holder(); \
             const r = flag ? (h.C = 0) : (h.C = class { static k: number = 3; }); \
             return h.C.k; \
         }",
    ];
    for source in sources {
        let out = compile_module(source);
        assert!(
            codes(&out).contains(&"E0419"),
            "one branch keeps a plain `0` while the other writes a class, so the static read has \
             no class to select on that path and must be refused; got {:?}",
            out.diagnostics
        );
    }
}

#[test]
fn a_class_write_in_the_same_branch_proves_the_or_assignment() {
    const SOURCE: &str = "class Holder { C = class { static k: number = 1; }; } \
         export function got(flag: boolean): number { \
             const h = new Holder(); \
             if (flag) { \
                 h.C = class { static k: number = 3; }; \
                 h.C ||= class { static k: number = 2; }; \
             } \
             else { \
                 h.C ||= class { static k: number = 4; }; \
             } \
             return h.C.k; \
         }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(same_branch_class_write::got(&ctx, true), 3.0);
    assert_eq!(same_branch_class_write::got(&ctx, false), 1.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate("same_branch_class_write", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_class_write_in_one_if_branch_proves_the_or_assignment() {
    const SOURCE: &str = "class Holder { C = class { static k: number = 1; }; } \
         export function got(flag: boolean): number { \
             const h = new Holder(); \
             if (flag) { h.C = class { static k: number = 3; }; h.C ||= class { static k: number = 2; }; } \
             else { h.C ||= class { static k: number = 4; }; } \
             return h.C.k; \
         }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(both_if_branches_class_write::got(&ctx, true), 3.0);
    assert_eq!(both_if_branches_class_write::got(&ctx, false), 1.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate("both_if_branches_class_write", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_class_write_in_both_ternary_branches_proves_the_or_assignment() {
    const SOURCE: &str = "class Holder { C = class { static k: number = 1; }; } \
         export function got(flag: boolean): number { \
             const h = new Holder(); \
             const r = flag ? (h.C = class { static k: number = 3; }) \
                 : (h.C ||= class { static k: number = 4; }); \
             return h.C.k; \
         }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(both_ternary_branches_class_write::got(&ctx, true), 3.0);
    assert_eq!(both_ternary_branches_class_write::got(&ctx, false), 1.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate("both_ternary_branches_class_write", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_class_write_in_both_nested_branches_proves_the_or_assignment() {
    const SOURCE: &str = "class Holder { C = class { static k: number = 1; }; } \
         export function got(flag: boolean, other: boolean): number { \
             const h = new Holder(); \
             if (flag) { \
                 if (other) { h.C = class { static k: number = 3; }; } \
                 h.C ||= class { static k: number = 2; }; \
             } \
             return h.C.k; \
         }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(nested_branches_class_write::got(&ctx, true, true), 3.0);
    assert_eq!(nested_branches_class_write::got(&ctx, true, false), 1.0);
    assert_eq!(nested_branches_class_write::got(&ctx, false, true), 1.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate("nested_branches_class_write", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_class_write_in_a_sibling_branch_keeps_the_or_assignment_right_hand_side() {
    const SOURCE: &str = "let hits: number = 0; \
         function rhs(): number { hits = hits + 1; return 0; } \
         class Holder { C = class { static k: number = 1; }; } \
         export function got(flag: boolean, other: boolean, third: boolean): number { \
             hits = 0; \
             const h = new Holder(); \
             if (flag) { h.C = 0; } \
             if (other) { h.C = class { static k: number = 3; }; } \
             if (third) { h.C ||= rhs(); } \
             return hits; \
         }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(sibling_branch_rhs_kept::got(&ctx, true, false, true), 1.0);
    assert_eq!(sibling_branch_rhs_kept::got(&ctx, true, true, true), 0.0);
    assert_eq!(sibling_branch_rhs_kept::got(&ctx, false, false, true), 0.0);
    assert_eq!(sibling_branch_rhs_kept::got(&ctx, true, false, false), 0.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate("sibling_branch_rhs_kept", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_class_write_in_a_sibling_conditional_expression_is_refused_by_the_and() {
    const SOURCE: &str = "let hits: number = 0; \
         function side(): number { hits = hits + 1; return 0; } \
         class Holder { C = class { static k: number = 1; }; } \
         export function got(flag: boolean, other: boolean, third: boolean): number { \
             const h = new Holder(); \
             flag && (h.C = 0); \
             other && (h.C = class { static k: number = 3; }); \
             third && (h.C ||= side()); \
             return hits; \
         }";
    let out = compile_module(SOURCE);
    assert!(
        out.has_errors(),
        "each `&&` right-hand side yields a number where `&&` needs a `boolean`, so the form is \
         still refused and the `||=` right-hand side cannot be dropped silently; got {:?}",
        out.diagnostics
    );
}

#[test]
fn an_or_assignment_that_restores_the_class_proves_the_next_one() {
    const SOURCE: &str = "class Fresh { C = class { static k: number = 1; }; } \
         class Holder { C = class { static k: number = 1; }; } \
         export function got(): number { \
             const h = new Holder(); \
             h.C = 0; \
             h.C ||= class { static k: number = 3; }; \
             h.C ||= class { static k: number = 2; }; \
             return h.C.k; \
         } \
         export function fresh(): number { \
             const h = new Fresh(); \
             h.C ||= class { static k: number = 3; }; \
             h.C ||= class { static k: number = 2; }; \
             return h.C.k; \
         }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(restoring_or_assignment::got(&ctx), 3.0);
    assert_eq!(restoring_or_assignment::fresh(&ctx), 1.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate("restoring_or_assignment", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_restoring_or_assignment_behind_a_branch_proves_the_class_outside_it() {
    const SOURCE: &str = "class Holder { C = class { static k: number = 1; }; } \
         export function got(flag: boolean): number { \
             const h = new Holder(); \
             if (flag) { h.C = 0; h.C ||= class { static k: number = 3; }; } \
             else { h.C = 0; h.C ||= class { static k: number = 4; }; } \
             h.C ||= class { static k: number = 2; }; \
             return h.C.k; \
         }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(restoring_or_assignment_in_branches::got(&ctx, true), 3.0);
    assert_eq!(restoring_or_assignment_in_branches::got(&ctx, false), 4.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate("restoring_or_assignment_in_branches", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_switch_writing_a_class_in_every_case_restores_the_field() {
    const SOURCE: &str = "class Holder { C = class { static k: number = 1; }; } \
         export function got(flag: boolean): number { \
             const h = new Holder(); \
             switch (flag ? 1 : 2) { \
                 case 1: h.C = class { static k: number = 2; }; break; \
                 default: h.C ||= class { static k: number = 3; }; \
             } \
             return h.C.k; \
         }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(switch_class_write::got(&ctx, true), 2.0);
    assert_eq!(switch_class_write::got(&ctx, false), 1.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate("switch_class_write", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_switch_that_cannot_write_a_class_in_every_case_keeps_the_poison() {
    let sources = [
        (
            "a `switch` without a `default`",
            "h.C = 0; \
             switch (flag ? 1 : 2) { \
                 case 1: h.C = class { static k: number = 2; }; break; \
                 case 2: h.C = class { static k: number = 3; }; \
             }",
        ),
        (
            "a `default` that writes no class",
            "h.C = 0; \
             switch (flag ? 1 : 2) { \
                 case 1: h.C = class { static k: number = 2; }; break; \
                 default: break; \
             }",
        ),
    ];
    for (what, body) in sources {
        let source = format!(
            "{HOLDER} export function got(flag: boolean): number {{ \
                 const h = new Holder(); \
                 {body} \
                 return h.C.k; \
             }}"
        );
        let out = compile_module(&source);
        assert!(
            codes(&out).contains(&"E0419"),
            "{what} leaves the falsy `0` in the field on some path, so the static read has no \
             class to select and must be refused; got {:?}",
            out.diagnostics
        );
    }
}

#[test]
fn a_join_of_a_plain_and_a_class_branch_keeps_the_poison_in_both_forms() {
    let sources = [
        "class Holder { C = class { static k: number = 1; }; } \
         export function got(flag: boolean): number { \
             const h = new Holder(); \
             const r = flag ? (h.C = 0) : (h.C = class { static k: number = 4; }); \
             h.C ||= class { static k: number = 2; }; \
             return h.C.k; \
         }",
        "class Holder { C = class { static k: number = 1; }; } \
         export function got(flag: boolean): number { \
             const h = new Holder(); \
             if (flag) { h.C = 0; } else { h.C = class { static k: number = 4; }; } \
             h.C ||= class { static k: number = 2; }; \
             return h.C.k; \
         }",
    ];
    for source in sources {
        let out = compile_module(source);
        assert!(
            codes(&out).contains(&"E0419"),
            "one branch keeps the falsy `0` and the other a class, so the field may still be a \
             plain value when the `||=` runs; the emitted `||=` compares the field against `0`, \
             which is also the tag of a class value, so the field cannot be treated as definitely \
             a class afterwards and the read must be refused in the ternary and in the `if`/`else` \
             form alike; got {:?}",
            out.diagnostics
        );
    }
}

#[test]
fn a_joined_plain_branch_keeps_the_field_poisoned() {
    const SOURCE: &str = "const First = class { static k: number = 9; }; \
         class Holder { C = class { static k: number = 1; }; } \
         export function got(flag: boolean): number { \
             const h = new Holder(); \
             if (flag) { h.C = 0; } else { h.C = First; } \
             h.C ||= class { static k: number = 2; }; \
             return h.C.k; \
         }";
    let out = compile_module(SOURCE);
    assert!(
        codes(&out).contains(&"E0419"),
        "one branch keeps the falsy `0` and the other a class, so the field may still be a \
         plain value when the `||=` runs; joining a plain branch with a class branch must keep \
         the poison so the read is refused; got {:?}",
        out.diagnostics
    );
}

#[test]
fn a_plain_write_in_one_branch_does_not_erase_the_other_branch_class() {
    const SOURCE: &str = "class Holder { C = class { static k: number = 1; }; } \
         export function got(flag: boolean): number { \
             const h = new Holder(); \
             h.C = 0; \
             if (flag) { h.C = 0; h.C = class { static k: number = 3; }; } \
             else { h.C = 0; h.C = class { static k: number = 4; }; } \
             return h.C.k; \
         }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(branch_plain_write_keeps_both_classes::got(&ctx, true), 3.0);
    assert_eq!(branch_plain_write_keeps_both_classes::got(&ctx, false), 4.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate("branch_plain_write_keeps_both_classes", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn an_early_break_in_a_switch_case_keeps_the_class_assignment_unproven() {
    const SOURCE: &str = "class Holder { C = class { static k: number = 1; }; } \
         export function got(flag: boolean, cond: boolean): number { \
             const h = new Holder(); \
             switch (flag ? 1 : 2) { \
                 case 1: if (cond) { break; } h.C = class { static k: number = 2; }; break; \
                 default: h.C = class { static k: number = 3; }; break; \
             } \
             h.C ||= class { static k: number = 4; }; \
             return h.C.k; \
         }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(early_break_switch::got(&ctx, true, true), 1.0);
    assert_eq!(early_break_switch::got(&ctx, true, false), 2.0);
    assert_eq!(early_break_switch::got(&ctx, false, true), 3.0);
    assert_eq!(early_break_switch::got(&ctx, false, false), 3.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate("early_break_switch", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_self_assignment_does_not_lift_the_poison() {
    const SOURCE: &str = "class Fresh { C = class { static k: number = 1; }; } \
         class Holder { C = class { static k: number = 1; }; } \
         export function got(flag: boolean): number { \
             const h = new Holder(); \
             h.C = 0; \
             if (flag) { h.C = class { static k: number = 3; }; } \
             h.C = h.C; \
             h.C ||= class { static k: number = 2; }; \
             return h.C.k; \
         } \
         export function clean(): number { \
             const h = new Fresh(); \
             h.C = h.C; \
             h.C ||= class { static k: number = 2; }; \
             return h.C.k; \
         }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(self_assign_poison::got(&ctx, true), 3.0);
    assert_eq!(self_assign_poison::got(&ctx, false), 2.0);
    assert_eq!(self_assign_poison::clean(&ctx), 1.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate("self_assign_poison", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn adjacent_conditional_writes_do_not_restore_the_field() {
    const SOURCE: &str = "class Holder { C = class { static k: number = 1; }; } \
         export function got(flag: boolean, other: boolean): number { \
             const h = new Holder(); \
             h.C = 0; \
             if (flag) { h.C = 0; } \
             if (other) { h.C = class { static k: number = 2; }; } \
             return h.C.k; \
         }";
    let out = compile_module(SOURCE);
    assert!(
        codes(&out).contains(&"E0419"),
        "the first branch keeps the plain `0` on the path where it runs, so the class write in \
         the second branch does not hold on every path and the static read must be refused; got \
         {:?}",
        out.diagnostics
    );
}

#[test]
fn a_sibling_branch_class_write_keeps_a_conditional_poison() {
    const SOURCE: &str = "class Holder { C = class { static k: number = 1; }; } \
         export function got(flag: boolean, other: boolean): number { \
             const h = new Holder(); \
             if (flag) { h.C = null; } \
             if (other) { h.C = class { static k: number = 3; }; } \
             return h.C.k; \
         }";
    let out = compile_module(SOURCE);
    assert!(
        codes(&out).contains(&"E0419"),
        "the class write sits in a sibling branch of the branch that writes the null, so it does \
         not dominate the read and the static read must be refused; got {:?}",
        out.diagnostics
    );
}

#[test]
fn a_try_body_that_may_throw_and_a_catch_write_do_not_restore_the_field() {
    const SOURCE: &str = "class Holder { C = class { static k: number = 1; }; } \
         export function got(flag: boolean): number { \
             const h = new Holder(); \
             h.C = 0; \
             try { if (flag) { throw 1; } h.C = 0; } catch (e) { \
                 h.C = class { static k: number = 2; }; \
             } \
             return h.C.k; \
         }";
    let out = compile_module(SOURCE);
    assert!(
        codes(&out).contains(&"E0419"),
        "the body may throw before it writes the `0`, and the catch writes a class only on that \
         path, so the two paths disagree and the static read must be refused; got {:?}",
        out.diagnostics
    );
}

#[test]
fn a_rebound_holder_starts_from_the_field_initializer() {
    const SOURCE: &str = "class Holder { C = class { static k: number = 1; }; } \
         export function got(): number { \
             let h = new Holder(); \
             h.C = 0; \
             h = new Holder(); \
             h.C ||= class { static k: number = 2; }; \
             return h.C.k; \
         }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(rebound_holder::got(&ctx), 1.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate("rebound_holder", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_conditional_holder_rebind_keeps_the_old_field_state_unproven() {
    const SOURCE: &str = "class Holder { C = class { static k: number = 1; }; } \
         export function got(flag: boolean): number { \
             let h = new Holder(); \
             h.C = 0; \
             if (flag) { h = new Holder(); } \
             h.C ||= class { static k: number = 2; }; \
             return h.C.k; \
         }";
    let out = compile_module(SOURCE);
    assert!(
        codes(&out).contains(&"E0419"),
        "the rebinding runs on one path only, so the field may still be the plain `0` when the \
         `||=` runs and the assignment cannot restore a class on every path; got {:?}",
        out.diagnostics
    );
}

#[test]
fn an_early_return_through_a_finally_keeps_the_or_assignment_side_effects() {
    const SOURCE: &str = "let hits: number = 0; \
         function side(): number { hits = hits + 1; return 0; } \
         class PlainHolder { C = class { static k: number = 1; }; } \
         class Holder { C = class { static k: number = 1; }; } \
         function got(flag: boolean, c: boolean): number { \
             const h = new Holder(); \
             h.C = 0; \
             try { \
                 if (flag) { if (c) { return 5; } h.C = class { static k: number = 2; }; } \
                 else { h.C = class { static k: number = 3; }; } \
             } catch (e) { } finally { h.C ||= side(); } \
             return 7; \
         } \
         function probe(flag: boolean, c: boolean): number { \
             hits = 0; \
             const r = got(flag, c); \
             return hits * 100 + r; \
         } \
         export function tt(): number { return probe(true, true); } \
         export function tf(): number { return probe(true, false); } \
         export function ft(): number { return probe(false, true); } \
         export function plain(): number { \
             hits = 0; \
             const h = new PlainHolder(); \
             try { if (true) { return 5; } } finally { h.C ||= side(); } \
             return 7; \
         } \
         export function read_hits(): number { return hits; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(finally_early_return::tt(&ctx), 105.0);
    assert_eq!(finally_early_return::tf(&ctx), 7.0);
    assert_eq!(finally_early_return::ft(&ctx), 7.0);
    assert_eq!(finally_early_return::plain(&ctx), 5.0);
    assert_eq!(finally_early_return::read_hits(&ctx), 0.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate("finally_early_return", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn an_early_break_through_a_finally_keeps_the_or_assignment_side_effects() {
    const SOURCE: &str = "let hits: number = 0; \
         function side(): number { hits = hits + 1; return 0; } \
         class PlainHolder { C = class { static k: number = 1; }; } \
         class Holder { C = class { static k: number = 1; }; } \
         function got(flag: boolean): number { \
             const h = new Holder(); \
             h.C = 0; \
             while (true) { \
                 try { if (flag) { break; } h.C = class { static k: number = 2; }; } \
                 finally { h.C ||= side(); } \
                 break; \
             } \
             return 7; \
         } \
         function probe(flag: boolean): number { \
             hits = 0; \
             const r = got(flag); \
             return hits * 100 + r; \
         } \
         export function t(): number { return probe(true); } \
         export function f(): number { return probe(false); } \
         export function plain(): number { \
             hits = 0; \
             const h = new PlainHolder(); \
             while (true) { try { break; } finally { h.C ||= side(); } } \
             return hits * 100 + h.C.k; \
         }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(finally_early_break::t(&ctx), 107.0);
    assert_eq!(finally_early_break::f(&ctx), 7.0);
    assert_eq!(finally_early_break::plain(&ctx), 1.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate("finally_early_break", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_conditional_holder_rebind_keeps_the_field_initializer_class() {
    const SOURCE: &str = "class ByIf { C = class { static k: number = 1; }; } \
         class ByWhile { C = class { static k: number = 1; }; } \
         class ByTry { C = class { static k: number = 1; }; } \
         function by_if(flag: boolean): number { \
             const a = new ByIf(); \
             const b = new ByIf(); \
             let h = a; \
             if (flag) { h = b; } \
             h.C ||= class { static k: number = 2; }; \
             return h.C.k; \
         } \
         function by_while(): number { \
             const a = new ByWhile(); \
             const b = new ByWhile(); \
             let h = a; \
             let i = 0; \
             while (i < 1) { i = i + 1; h = b; } \
             h.C ||= class { static k: number = 2; }; \
             return h.C.k; \
         } \
         function by_try(flag: boolean): number { \
             const a = new ByTry(); \
             const b = new ByTry(); \
             let h = a; \
             try { if (flag) { h = b; } } catch (e) { } \
             h.C ||= class { static k: number = 2; }; \
             return h.C.k; \
         } \
         export function it(): number { return by_if(true); } \
         export function iff(): number { return by_if(false); } \
         export function w(): number { return by_while(); } \
         export function tt(): number { return by_try(true); } \
         export function tf(): number { return by_try(false); }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(conditional_rebind::it(&ctx), 1.0);
    assert_eq!(conditional_rebind::iff(&ctx), 1.0);
    assert_eq!(conditional_rebind::w(&ctx), 1.0);
    assert_eq!(conditional_rebind::tt(&ctx), 1.0);
    assert_eq!(conditional_rebind::tf(&ctx), 1.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate("conditional_rebind", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_cross_holder_field_assignment_keeps_the_source_classes() {
    const SOURCE: &str = "class Holder { C = class { static k: number = 1; }; } \
         export function got(): number { \
             const a = new Holder(); \
             const b = new Holder(); \
             a.C = class { static k: number = 5; }; \
             a.C = b.C; \
             a.C ||= class { static k: number = 7; }; \
             return a.C.k; \
         }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(cross_holder_field::got(&ctx), 1.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate("cross_holder_field", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_grouped_case_label_covers_the_switch_end() {
    const SOURCE: &str = "class Holder { C = class { static k: number = 1; }; } \
         export function got(flag: boolean): number { \
             const h = new Holder(); \
             switch (flag ? 1 : 3) { \
                 case 1: case 2: h.C = class { static k: number = 2; }; break; \
                 default: h.C ||= class { static k: number = 3; }; break; \
             } \
             return h.C.k; \
         }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(grouped_case_label::got(&ctx, true), 2.0);
    assert_eq!(grouped_case_label::got(&ctx, false), 1.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate("grouped_case_label", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_fallthrough_case_covers_the_switch_end() {
    const SOURCE: &str = "class Holder { C = class { static k: number = 1; }; } \
         export function got(flag: boolean): number { \
             const h = new Holder(); \
             switch (flag ? 1 : 3) { \
                 case 1: h.C = class { static k: number = 2; }; \
                 case 2: h.C = class { static k: number = 8; }; break; \
                 default: h.C ||= class { static k: number = 3; }; break; \
             } \
             return h.C.k; \
         }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(fallthrough_case::got(&ctx, true), 8.0);
    assert_eq!(fallthrough_case::got(&ctx, false), 1.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate("fallthrough_case", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_fallthrough_case_that_reaches_a_class_write_covers_the_switch_end() {
    const SOURCE: &str = "class Holder { C = class { static k: number = 1; }; } \
         export function got(flag: boolean): number { \
             const h = new Holder(); \
             switch (flag ? 1 : 2) { \
                 case 1: h.C = class { static k: number = 2; }; \
                 default: h.C ||= class { static k: number = 3; }; \
             } \
             return h.C.k; \
         }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(fallthrough_into_default::got(&ctx, true), 2.0);
    assert_eq!(fallthrough_into_default::got(&ctx, false), 1.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate("fallthrough_into_default", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn an_early_break_after_a_class_write_still_covers_the_case() {
    const SOURCE: &str = "class Holder { C = class { static k: number = 1; }; } \
         export function got(flag: boolean, c: boolean): number { \
             const h = new Holder(); \
             switch (flag ? 1 : 2) { \
                 case 1: h.C = class { static k: number = 2; }; \
                     if (c) { break; } \
                     h.C = class { static k: number = 8; }; break; \
                 default: h.C ||= class { static k: number = 3; }; break; \
             } \
             return h.C.k; \
         }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(case_early_break::got(&ctx, true, true), 2.0);
    assert_eq!(case_early_break::got(&ctx, true, false), 8.0);
    assert_eq!(case_early_break::got(&ctx, false, true), 1.0);
    assert_eq!(case_early_break::got(&ctx, false, false), 1.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate("case_early_break", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_break_inside_a_nested_loop_does_not_uncover_the_case() {
    const SOURCE: &str = "class Holder { C = class { static k: number = 1; }; } \
         export function got(c: boolean): number { \
             const h = new Holder(); \
             let i = 0; \
             let total = 0; \
             while (i < 3) { \
                 switch (i) { \
                     case 1: while (c) { break; } h.C = class { static k: number = 13; }; break; \
                     default: h.C ||= class { static k: number = 4; }; total += h.C.k; break; \
                 } \
                 i++; \
             } \
             return total; \
         }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(case_nested_loop_break::got(&ctx, true), 14.0);
    assert_eq!(case_nested_loop_break::got(&ctx, false), 14.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate("case_nested_loop_break", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn an_or_assignment_inside_a_case_restores_a_write_in_the_same_case() {
    const SOURCE: &str = "class Holder { C = class { static k: number = 1; }; } \
         export function got(flag: boolean): number { \
             const h = new Holder(); \
             switch (flag ? 1 : 2) { \
                 case 1: h.C = 0; h.C ||= class { static k: number = 2; }; break; \
                 default: h.C ||= class { static k: number = 3; }; break; \
             } \
             return h.C.k; \
         }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(case_inner_or_assignment::got(&ctx, true), 2.0);
    assert_eq!(case_inner_or_assignment::got(&ctx, false), 1.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate("case_inner_or_assignment", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_branch_writing_another_holder_keeps_this_holders_class() {
    const SOURCE: &str = "class HolderA { C = class { static k: number = 1; }; } \
         class HolderB { C = class { static k: number = 1; }; } \
         function read_a(c: boolean): number { \
             const a = new HolderA(); \
             const b = new HolderA(); \
             if (c) { a.C = class { static k: number = 5; }; } \
             else { b.C = class { static k: number = 6; }; } \
             a.C ||= class { static k: number = 7; }; \
             return a.C.k; \
         } \
         function read_b(c: boolean): number { \
             const a = new HolderB(); \
             const b = new HolderB(); \
             if (c) { a.C = class { static k: number = 5; }; } \
             else { b.C = class { static k: number = 6; }; } \
             b.C ||= class { static k: number = 8; }; \
             return b.C.k; \
         } \
         export function at(): number { return read_a(true); } \
         export function af(): number { return read_a(false); } \
         export function bt(): number { return read_b(true); } \
         export function bf(): number { return read_b(false); }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(two_holders::at(&ctx), 5.0);
    assert_eq!(two_holders::af(&ctx), 1.0);
    assert_eq!(two_holders::bt(&ctx), 1.0);
    assert_eq!(two_holders::bf(&ctx), 6.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate("two_holders", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn an_or_assignment_inside_a_while_body_stays_refused() {
    const SOURCE: &str = "class Holder { C = class { static k: number = 1; }; } \
         export function got(flag: boolean): number { \
             const h = new Holder(); \
             h.C = 0; \
             while (flag) { h.C ||= class { static k: number = 2; }; } \
             return h.C.k; \
         }";
    let out = compile_module(SOURCE);
    assert!(
        codes(&out).contains(&"E0419"),
        "the loop body may never run, so the field may still hold the falsy `0` when the `||=` \
         runs and the assignment cannot decide statically whether its right-hand side is \
         evaluated; the refusal stays deliberate; got {:?}",
        out.diagnostics
    );
}

#[test]
fn an_or_assignment_after_a_loop_exit_that_skips_the_falsy_write_is_refused() {
    let sources = [
        "class Holder { C = class { static k: number = 5; }; } \
         export function got(): number { \
             const h = new Holder(); h.C = 5; let i = 0; \
             while (i < 3) { \
                 h.C = 5; \
                 if (i > 0) { break; } \
                 h.C = 0; \
                 i = i + 1; \
             } \
             h.C ||= class { static k: number = 6; }; \
             return h.C.k; \
         }",
        "class Holder { C = class { static k: number = 5; }; } \
         export function got(): number { \
             const h = new Holder(); h.C = 5; let i = 0; \
             while (i < 3) { \
                 h.C = 5; \
                 i = i + 1; \
                 if (i > 1) { continue; } \
                 h.C = 0; \
             } \
             h.C ||= class { static k: number = 6; }; \
             return h.C.k; \
         }",
        "class Holder { C = class { static k: number = 5; }; } \
         export function got(): number { \
             const h = new Holder(); h.C = 5; \
             for (let i = 0; i < 3; i = i + 1) { \
                 h.C = 5; \
                 if (i > 0) { break; } \
                 h.C = 0; \
             } \
             h.C ||= class { static k: number = 6; }; \
             return h.C.k; \
         }",
    ];
    for source in sources {
        let out = compile_module(source);
        assert!(
            codes(&out).contains(&"E0419"),
            "a `break`/`continue` before the later falsy write means the field can still hold the \
             earlier truthy `5` when the `||=` runs, so the `||=` must not be treated as \
             restoring a class; got {:?}",
            out.diagnostics
        );
    }
}

#[test]
fn an_or_assignment_after_a_loop_body_that_always_runs_restores_the_class() {
    const SOURCE: &str = "class Holder { C = class { static k: number = 1; }; } \
         export function got(): number { \
             const h = new Holder(); let i = 0; \
             while (i < 1) { i = i + 1; h.C = 0; } \
             h.C ||= class { static k: number = 2; }; \
             return h.C.k; \
         }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(loop_body_runs::got(&ctx), 2.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate("loop_body_runs", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn an_or_assignment_after_a_loop_that_never_runs_keeps_the_first_class() {
    const SOURCE: &str = "class Holder { C = class { static k: number = 1; }; } \
         export function got(): number { \
             const h = new Holder(); \
             while (false) { h.C = 0; } \
             h.C ||= class { static k: number = 2; }; \
             return h.C.k; \
         }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(loop_body_skipped::got(&ctx), 1.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate("loop_body_skipped", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_field_write_in_one_function_does_not_poison_another_function() {
    const SOURCE: &str = "class H { C = class { static k: number = 1; }; } \
         export function a(): number { \
             const h = new H(); \
             h.C = 0; \
             h.C ||= class { static k: number = 2; }; \
             return h.C.k; \
         } \
         export function b(): number { \
             const h = new H(); \
             return h.C.k; \
         }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(cross_function_fields::a(&ctx), 2.0);
    assert_eq!(cross_function_fields::b(&ctx), 1.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate("cross_function_fields", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_field_read_in_a_later_function_keeps_the_initializer_class() {
    const SOURCE: &str = "class H { C = class { static k: number = 1; }; } \
         export function a(): number { \
             const h = new H(); \
             h.C = 0; \
             h.C ||= class { static k: number = 2; }; \
             return h.C.k; \
         } \
         export function b(): number { \
             const h = new H(); \
             return h.C.k; \
         } \
         export function c(): number { \
             const h = new H(); \
             return h.C.k; \
         }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(reader_after_writer::a(&ctx), 2.0);
    assert_eq!(reader_after_writer::b(&ctx), 1.0);
    assert_eq!(reader_after_writer::c(&ctx), 1.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate("reader_after_writer", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_closure_sees_a_class_value_field_written_before_it() {
    const SOURCE: &str = "class H { C = class { static k: number = 1; }; } \
         export function outer(): number { \
             const h = new H(); \
             h.C = class { static k: number = 9; }; \
             const f = function (): number { return h.C.k; }; \
             const g = (): number => h.C.k; \
             return f() + g(); \
         }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(closure_field_write::outer(&ctx), 18.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate("closure_field_write", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_field_write_in_a_method_does_not_poison_a_function() {
    const SOURCE: &str = "class H { C = class { static k: number = 1; }; } \
         class Machine { \
             run(h: H): number { \
                 h.C = 0; \
                 h.C ||= class { static k: number = 2; }; \
                 return h.C.k; \
             } \
         } \
         export function a(): number { \
             const h = new H(); \
             const m: Machine = new Machine(); \
             return m.run(h); \
         } \
         export function b(): number { \
             const h = new H(); \
             return h.C.k; \
         }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(method_and_function::a(&ctx), 2.0);
    assert_eq!(method_and_function::b(&ctx), 1.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate("method_and_function", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn an_unconditional_plain_assignment_replaces_the_initial_field_class() {
    const SOURCE: &str = "class Holder { C = class { static run(): number { return 1; } }; } \
         export function got(): number { \
             const h = new Holder(); \
             h.C = class { static run(): number { return 2; } }; \
             return h.C.run(); \
         }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(plain_assign_replaces::got(&ctx), 2.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate("plain_assign_replaces", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_conditional_plain_assignment_keeps_both_field_classes_and_is_refused() {
    const SOURCE: &str = "class Holder { C = class { static run(): number { return 1; } }; } \
         export function got(flag: boolean): number { \
             const h = new Holder(); \
             if (flag) { h.C = class { static run(): number { return 2; } }; } \
             return h.C.run(); \
         }";
    let out = compile_module(SOURCE);
    assert!(
        out.diagnostics
            .errors()
            .any(|d| d.code.as_str() == "E0419" && d.message.contains("more than one class")),
        "only an unconditional plain assignment may replace the initial class, so an `if` that \
         may leave either class must stay refused; got {:?}",
        codes(&out)
    );
    assert!(
        out.rust_source.is_none(),
        "a refused source must not emit rust; got:\n{:?}",
        out.rust_source
    );
}

#[test]
fn two_conditionally_assigned_fresh_classes_stay_refused() {
    const SOURCE: &str = "class Holder { C = class { static run(): number { return 1; } }; } \
         export function got(flag: boolean): number { \
             const h = new Holder(); \
             if (flag) { h.C = class { static run(): number { return 2; } }; } \
             else { h.C = class { static run(): number { return 3; } }; } \
             return h.C.run(); \
         }";
    let out = compile_module(SOURCE);
    assert!(
        out.diagnostics
            .errors()
            .any(|d| d.code.as_str() == "E0419" && d.message.contains("more than one class")),
        "two different fresh classes assigned in the two branches are both possible, so the static \
         call must stay refused; got {:?}",
        codes(&out)
    );
    assert!(
        out.rust_source.is_none(),
        "a refused source must not emit rust; got:\n{:?}",
        out.rust_source
    );
}

#[test]
fn a_compound_assignment_over_a_maybe_falsy_field_keeps_the_union_and_is_refused() {
    const SOURCE: &str = "class Holder { C = class { static run(): number { return 1; } }; } \
         export function got(flag: boolean): number { \
             const h = new Holder(); \
             if (flag) { h.C = 0; } \
             h.C ||= class { static run(): number { return 2; } }; \
             return h.C.run(); \
         }";
    let out = compile_module(SOURCE);
    assert!(
        out.diagnostics
            .errors()
            .any(|d| d.code.as_str() == "E0419" && d.message.contains("more than one class")),
        "a compound write may keep the initial class or store the new one, so it must keep the \
         union; got {:?}",
        codes(&out)
    );
    assert!(
        out.rust_source.is_none(),
        "a refused source must not emit rust; got:\n{:?}",
        out.rust_source
    );
}

#[test]
fn a_module_class_value_assigned_to_a_fresh_field_keeps_its_class_value_tag() {
    const SOURCE: &str = "class K { static run(): number { return 4; } } \
         class Holder { C = class { static run(): number { return 1; } }; } \
         export function got(): number { \
             const h = new Holder(); \
             h.C = K; \
             return h.C.run(); \
         }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(flat_module_class_value::got(&ctx), 4.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate("flat_module_class_value", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_module_class_value_assigned_to_a_nested_field_keeps_its_class_value_tag() {
    const SOURCE: &str = "class K { static k: number = 4; } \
         class Inner { C = class { static k: number = 1; }; } \
         class Outer { I: Inner; } \
         export function got(): number { \
             const o = new Outer(); \
             o.I = new Inner(); \
             o.I.C = K; \
             return o.I.C.k; \
         }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    nested_module_class_value::__ts_aot_init();
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(nested_module_class_value::got(&ctx), 4.0);
    println!("ok");
}
"#;
    let rust = crate_rust_of(SOURCE);
    let stdout = run_generated_crate("nested_module_class_value", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_module_class_value_assigned_to_a_static_class_value_field_is_refused() {
    const SOURCE: &str = "class K { static run(): number { return 4; } } \
         class Holder { static C = class { static run(): number { return 1; } }; } \
         export function got(): number { \
             Holder.C = K; \
             return Holder.C.run(); \
         }";
    let out = compile_module(SOURCE);
    assert!(
        out.diagnostics
            .errors()
            .any(|d| d.code.as_str() == "E0419" && d.message.contains("static field")),
        "a static class-value field cannot store a module class value, so the assignment must be \
         refused; got {:?}",
        codes(&out)
    );
    assert!(
        out.rust_source.is_none(),
        "a refused source must not emit rust; got:\n{:?}",
        out.rust_source
    );
}

#[test]
fn a_class_assigned_from_a_parameter_must_stay_refused() {
    const SOURCE: &str = "class K { static run(): number { return 4; } } \
         class Holder { C = class { static run(): number { return 1; } }; } \
         export function got(k: K): number { \
             const h = new Holder(); \
             h.C = k; \
             return h.C.run(); \
         }";
    let out = compile_module(SOURCE);
    assert!(
        out.diagnostics
            .errors()
            .any(|d| d.code.as_str() == "E0419" && d.message.contains("not a class value")),
        "an instance passed in from a parameter is not a class value, so the assignment must be \
         refused; got {:?}",
        codes(&out)
    );
    assert!(
        out.rust_source.is_none(),
        "a refused source must not emit rust; got:\n{:?}",
        out.rust_source
    );
}

#[test]
fn a_second_unconditional_plain_assignment_replaces_the_first_class() {
    const SOURCE: &str = "class Holder { C = class { static run(): number { return 1; } }; } \
         export function got(): number { \
             const h = new Holder(); \
             h.C = class { static run(): number { return 2; } }; \
             h.C = class { static run(): number { return 3; } }; \
             return h.C.run(); \
         }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(second_plain_assign::got(&ctx), 3.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate("second_plain_assign", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}
