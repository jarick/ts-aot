use std::fmt::Write as _;

use ts_aot_driver::{CompileOptions, Driver, DriverOutput, EmitStage};
use ts_aot_test_support::run_generated_crate;

fn compile(source: &str) -> DriverOutput {
    Driver::new().compile_source(
        "test.ts",
        source,
        &CompileOptions {
            emit: EmitStage::Rust,
            module: true,
        },
    )
}

fn f64_literal(value: f64) -> String {
    if value.is_nan() {
        "f64::NAN".to_owned()
    } else if value.is_infinite() {
        if value.is_sign_negative() {
            "f64::NEG_INFINITY".to_owned()
        } else {
            "f64::INFINITY".to_owned()
        }
    } else {
        format!("{value}_f64")
    }
}

fn numbers_main(package: &str, calls: &[(&str, f64)]) -> String {
    let mut checks = String::new();
    for (name, value) in calls {
        let _ = writeln!(
            checks,
            "    assert_eq!({package}::{name}(&ctx), {});",
            f64_literal(*value)
        );
    }
    format!(
        "use ts_aot_runtime::{{Bump, Ctx, Host}};\n\nstruct NoHost;\nimpl Host for NoHost {{}}\n\n\
         fn main() {{\n    let host = NoHost;\n    let bump = Bump::new();\n    \
         let ctx = Ctx {{ arena: &bump, host: &host }};\n{checks}    println!(\"ok\");\n}}\n"
    )
}

fn check(package: &str, source: &str, calls: &[(&str, f64)]) {
    let out = compile(source);
    assert!(
        !out.has_errors(),
        "must compile through the full pipeline; got {:?}",
        out.diagnostics
    );
    let rust = out
        .rust_source
        .expect("emit-rust must populate rust_source");
    let main = numbers_main(package, calls);
    let stdout = run_generated_crate(package, &rust, &main);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

fn check_with_module_init(package: &str, source: &str, calls: &[(&str, f64)]) {
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
    let rust = out
        .rust_source
        .expect("emit-crate must populate rust_source");
    let mut main = String::from(
        "use ts_aot_runtime::{Bump, Ctx, Host};\n\nstruct NoHost;\nimpl Host for NoHost {}\n\n\
         fn main() {\n",
    );
    let _ = writeln!(main, "    {package}::__ts_aot_init();");
    main.push_str(
        "    let host = NoHost;\n    let bump = Bump::new();\n    \
         let ctx = Ctx { arena: &bump, host: &host };\n",
    );
    for (name, value) in calls {
        let _ = writeln!(
            main,
            "    assert_eq!({package}::{name}(&ctx), {});",
            f64_literal(*value)
        );
    }
    main.push_str("    println!(\"ok\");\n}\n");
    let stdout = run_generated_crate(package, &rust, &main);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

fn refuses(source: &str) -> String {
    let out = compile(source);
    let diagnostic = out
        .diagnostics
        .errors()
        .find(|d| d.code.as_str() == "E0419")
        .unwrap_or_else(|| panic!("expected an E0419 refusal; got {:?}", out.diagnostics));
    let message = diagnostic.message.clone();
    assert!(
        out.rust_source.is_none(),
        "a refused source must not emit rust; got:\n{}",
        out.rust_source.unwrap_or_default()
    );
    message
}

fn capture_fixture(body: &str) -> String {
    format!(
        r"
function f(): number {{
  const items: number[] = [1, 2];
  const other: number[] = [9, 9];
  let alias: number[] = other;
  class C {{ read(): number {{ return items[0]; }} }}
  const c = new C();
  try {{
{body}
  }} catch {{
    alias[0] = 9;
  }}
  return c.read();
}}
export function got(): number {{ return f(); }}
"
    )
}

const N2_STATIC_IF: &str = r"    if (true) { throw 1; }
    alias = items;
    throw 2;";

const N3_INFINITE_WHILE: &str = r"    while (true) { throw 1; }
    alias = items;
    throw 2;";

const N1_NESTED_CATCH: &str = r"    try { alias = items; throw 1; } catch { alias = other; }";

const N4_NESTED_FINALLY: &str = r"    try { alias = items; throw 1; } finally { alias = other; }";

#[test]
fn a_statically_taken_throwing_branch_does_not_poison_the_catch_entry() {
    check(
        "abrupt_static_if",
        &capture_fixture(N2_STATIC_IF),
        &[("got", 1.0)],
    );
}

#[test]
fn a_provably_infinite_loop_does_not_poison_the_catch_entry() {
    check(
        "abrupt_infinite_while",
        &capture_fixture(N3_INFINITE_WHILE),
        &[("got", 1.0)],
    );
}

#[test]
fn a_nested_catch_consumes_its_body_throw_before_the_outer_catch() {
    check(
        "abrupt_nested_catch",
        &capture_fixture(N1_NESTED_CATCH),
        &[("got", 1.0)],
    );
}

#[test]
fn a_nested_finally_that_completes_normally_rebinds_before_the_outer_catch() {
    check(
        "abrupt_nested_finally",
        &capture_fixture(N4_NESTED_FINALLY),
        &[("got", 1.0)],
    );
}

const G2_PARENTHESIZED_UNION_NULL: &str = r"
export function got(): number {
  const C = class { static k: (number | null) = null; };
  return C.k === null ? 1 : 0;
}
";

const G2_PARENTHESIZED_INITIALIZER: &str = r"
export function got(): number {
  const C = class { static k: number | undefined = (undefined); };
  return C.k === undefined ? 1 : 0;
}
";

const G2_PLAIN_UNION_NULL: &str = r"
export function got(): number {
  const C = class { static k: number | null = null; };
  return C.k === null ? 1 : 0;
}
";

const G2_PLAIN_UNION_UNDEFINED: &str = r"
export function got(): number {
  const C = class { static k: number | undefined = undefined; };
  return C.k === undefined ? 1 : 0;
}
";

const G2_VOID_ZERO: &str = r"
export function got(): number {
  const C = class { static k: number | undefined = void 0; };
  return C.k === undefined ? 1 : 0;
}
";

const G2_STRING_UNION_NULL: &str = r"
export function got(): number {
  const C = class { static k: string | null = null; };
  return C.k === null ? 1 : 0;
}
";

const G2_NULL_ONLY: &str = r"
export function got(): number {
  const C = class { static k: null = null; };
  return C.k === null ? 1 : 0;
}
";

const G2_VOID_UNDEFINED: &str = r"
export function got(): number {
  const C = class { static k: void = undefined; };
  return C.k === undefined ? 1 : 0;
}
";

const G2_UNANNOTATED_NEGATIVE: &str = r"
export function got(): number {
  const C = class { static k = -3; };
  return C.k;
}
";

const G2_UNANNOTATED_NEGATIVE_FLOAT: &str = r"
export function got(): number {
  const C = class { static k = -3.5; };
  return C.k;
}
";

const G2_MODULE_LEVEL_STRING_STATIC: &str = r#"
class Top { static k: string = "ab"; }
export function got(): number { return Top.k.length; }
"#;

#[test]
fn parenthesized_nullish_static_field_initializers_keep_their_value() {
    check(
        "paren_union_null",
        G2_PARENTHESIZED_UNION_NULL,
        &[("got", 1.0)],
    );
    check(
        "paren_initializer_undefined",
        G2_PARENTHESIZED_INITIALIZER,
        &[("got", 1.0)],
    );
}

#[test]
fn plain_nullish_static_field_initializers_keep_their_value() {
    check("plain_union_null", G2_PLAIN_UNION_NULL, &[("got", 1.0)]);
    check(
        "plain_union_undefined",
        G2_PLAIN_UNION_UNDEFINED,
        &[("got", 1.0)],
    );
    check("void_zero", G2_VOID_ZERO, &[("got", 1.0)]);
    check("string_union_null", G2_STRING_UNION_NULL, &[("got", 1.0)]);
    check("null_only", G2_NULL_ONLY, &[("got", 1.0)]);
    check("void_undefined", G2_VOID_UNDEFINED, &[("got", 1.0)]);
}

#[test]
fn an_unannotated_negative_static_field_carries_its_value() {
    check(
        "unannotated_negative",
        G2_UNANNOTATED_NEGATIVE,
        &[("got", -3.0)],
    );
    check(
        "unannotated_negative_float",
        G2_UNANNOTATED_NEGATIVE_FLOAT,
        &[("got", -3.5)],
    );
}

#[test]
fn a_module_level_string_static_still_works() {
    check_with_module_init(
        "module_level_string_static",
        G2_MODULE_LEVEL_STRING_STATIC,
        &[("got", 2.0)],
    );
}

const G2_STRING_STATIC_EXPRESSION: &str = r#"
export function got(): number {
  const C = class { static k: string = "ab"; };
  return C.k.length;
}
"#;

const G2_STRING_STATIC_DECLARATION: &str = r#"
export function got(): number {
  class C { static k: string = "ab"; }
  return C.k.length;
}
"#;

const G2_NUMBER_NULL: &str = r"
export function got(): number {
  const C = class { static k: number = null; };
  return C.k === null ? 1 : 0;
}
";

const G2_UNTYPED_NULL: &str = r"
export function got(): number {
  const C = class { static k = null; };
  return C.k === null ? 1 : 0;
}
";

const G2_MISSING_NULL: &str = r"
export function got(): number {
  const C = class { static k: Missing | null = null; };
  return C.k === null ? 1 : 0;
}
";

const G2_VOID_CALL: &str = r"
function f(): void {}
export function got(): number {
  const C = class { static k: number | undefined = void f(); };
  return C.k === undefined ? 1 : 0;
}
";

#[test]
fn the_string_static_field_refusals_stay() {
    assert!(
        refuses(G2_STRING_STATIC_EXPRESSION).contains("cannot be initialized here"),
        "the local class expression must be refused for its static field initializer"
    );
    assert!(
        refuses(G2_STRING_STATIC_DECLARATION).contains("cannot be initialized here"),
        "the local class declaration must be refused for its static field initializer"
    );
}

#[test]
fn the_nullish_static_field_refusals_stay() {
    for source in [
        G2_NUMBER_NULL,
        G2_UNTYPED_NULL,
        G2_MISSING_NULL,
        G2_VOID_CALL,
    ] {
        assert!(
            refuses(source).contains("cannot be initialized here"),
            "a nullish initializer the annotation does not admit must stay refused"
        );
    }
}

const N5_NESTED_TRY_THROW: &str = r"    try { alias = items; throw 1; } catch { alias = other; }
    throw 2;";

#[test]
fn a_nested_try_whose_body_always_throws_does_not_poison_the_outer_catch_entry() {
    check(
        "abrupt_nested_try_throw",
        &capture_fixture(N5_NESTED_TRY_THROW),
        &[("got", 1.0)],
    );
}

const N9_LOOP_VARIANT: &str = r"    for (let i = 0; i < 1; i++) { try { alias = items; throw 1; } catch { alias = other; } }
    throw 2;";

#[test]
fn a_try_inside_a_loop_whose_body_always_throws_does_not_poison_the_outer_catch_entry() {
    check(
        "abrupt_loop_try_throw",
        &capture_fixture(N9_LOOP_VARIANT),
        &[("got", 1.0)],
    );
}

const N6_FOR_EVER: &str = r"    for (;;) { throw 1; }
    alias = items;
    throw 2;";

#[test]
fn a_nested_for_ever_does_not_poison_the_catch_entry() {
    check(
        "abrupt_for_ever",
        &capture_fixture(N6_FOR_EVER),
        &[("got", 1.0)],
    );
}

const N7_DO_WHILE_THROW: &str = r"    do { throw 1; } while (true);
    alias = items;
    throw 2;";

#[test]
fn a_nested_do_while_that_always_throws_does_not_poison_the_catch_entry() {
    check(
        "abrupt_do_while_throw",
        &capture_fixture(N7_DO_WHILE_THROW),
        &[("got", 1.0)],
    );
}

const N8_DO_WHILE_FOREVER: &str = r"    do { alias = items; } while (true);
    throw 1;";

#[test]
fn a_nested_do_while_forever_is_provably_abrupt() {
    let out = compile(&capture_fixture(N8_DO_WHILE_FOREVER));
    assert!(
        !out.has_errors(),
        "a do-while whose condition is a literal true must not poison the catch entry; got {:?}",
        out.diagnostics
    );
    assert!(
        out.rust_source.is_some(),
        "an accepted source must emit rust"
    );
}

const G3_NEGATIVE_ZERO_LOCAL: &str = r"
export function got(): number {
  class C { static k = -0; }
  return 1 / C.k;
}
";

#[test]
fn a_negative_zero_static_field_stays_negative() {
    check(
        "negative_zero_static",
        G3_NEGATIVE_ZERO_LOCAL,
        &[("got", f64::NEG_INFINITY)],
    );
}

const G3_NEGATIVE_ZERO_MODULE: &str = r"
const C = class { static k = -0; };
export function got(): number { return 1 / C.k; }
";

#[test]
fn a_module_level_negative_zero_static_field_stays_negative() {
    check_with_module_init(
        "negative_zero_module_static",
        G3_NEGATIVE_ZERO_MODULE,
        &[("got", f64::NEG_INFINITY)],
    );
}

const G4_PARENTHESIZED_NEGATIVE_ZERO: &str = r"
export function got(): number {
  const C = class { static k: number = (-0); };
  return 1 / C.k;
}
";

const G4_PARENTHESIZED_NEGATIVE: &str = r"
export function got(): number {
  const C = class { static k: number = (-3); };
  return C.k;
}
";

const G4_PARENTHESIZED_POSITIVE: &str = r"
export function got(): number {
  const C = class { static k: number = (3); };
  return C.k;
}
";

const G4_PARENTHESIZED_BOOLEAN: &str = r"
export function got(): number {
  const C = class { static k: boolean = (true); };
  return C.k ? 1 : 0;
}
";

const G4_UNARY_PLUS: &str = r"
export function got(): number {
  const C = class { static k: number = +3; };
  return C.k;
}
";

const G4_UNARY_NOT: &str = r"
export function got(): number {
  const C = class { static k: number = ~3; };
  return C.k;
}
";

#[test]
fn parenthesized_static_field_initializers_keep_their_value() {
    check(
        "paren_negative_zero",
        G4_PARENTHESIZED_NEGATIVE_ZERO,
        &[("got", f64::NEG_INFINITY)],
    );
    check(
        "paren_negative",
        G4_PARENTHESIZED_NEGATIVE,
        &[("got", -3.0)],
    );
    check("paren_positive", G4_PARENTHESIZED_POSITIVE, &[("got", 3.0)]);
    check("paren_boolean", G4_PARENTHESIZED_BOOLEAN, &[("got", 1.0)]);
}

#[test]
fn unary_static_field_initializers_keep_their_value() {
    check("unary_plus", G4_UNARY_PLUS, &[("got", 3.0)]);
    check("unary_not", G4_UNARY_NOT, &[("got", -4.0)]);
}

const G4_NON_LITERAL_NEGATION: &str = r"
export function got(): number {
  const x = 3;
  const C = class { static k: number = -x; };
  return C.k;
}
";

const G4_NON_LITERAL_UNARY_PLUS: &str = r"
export function got(): number {
  const x = 3;
  const C = class { static k: number = +x; };
  return C.k;
}
";

const G4_NON_LITERAL_BITWISE_NOT: &str = r"
export function got(): number {
  const x = 3;
  const C = class { static k: number = ~x; };
  return C.k;
}
";

#[test]
fn a_non_literal_unary_static_field_initializer_stays_refused() {
    for source in [
        G4_NON_LITERAL_NEGATION,
        G4_NON_LITERAL_UNARY_PLUS,
        G4_NON_LITERAL_BITWISE_NOT,
    ] {
        assert!(
            refuses(source).contains("cannot be initialized here"),
            "a unary initializer whose operand is not a numeric literal must stay refused"
        );
    }
}

const D1_BREAK_IN_WHILE_TRUE: &str = r"    while (true) { try { alias = items; break; } catch { alias = other; } }
    throw 2;";

const D1_BREAK_IN_FOR: &str = r"    for (let i = 0; i < 1; i++) { try { alias = items; break; } catch { alias = other; } }
    throw 2;";

const D1_CONTINUE_IN_FOR: &str = r"    for (let i = 0; i < 1; i++) { try { alias = items; continue; } catch { alias = other; } }
    throw 2;";

const MUTATED_MESSAGE: &str = "the local `items` is read by a method of a local class but the object it holds is modified in the enclosing function after the class is constructed; the class keeps a copy taken at construction, so the method could read stale contents; modify the object before constructing the class or pass it as a parameter";

#[test]
fn a_break_out_of_a_literal_true_loop_keeps_the_state_that_reaches_the_loop_exit() {
    assert_eq!(
        refuses(&capture_fixture(D1_BREAK_IN_WHILE_TRUE)),
        MUTATED_MESSAGE,
        "the break transfers alias===items to the loop exit, so the outer catch mutates items \
         and the snapshot must stay refused"
    );
}

#[test]
fn a_break_out_of_a_for_loop_keeps_the_state_that_reaches_the_loop_exit() {
    assert_eq!(
        refuses(&capture_fixture(D1_BREAK_IN_FOR)),
        MUTATED_MESSAGE,
        "the break transfers alias===items to the loop exit, so the outer catch mutates items \
         and the snapshot must stay refused"
    );
}

#[test]
fn a_continue_in_a_for_loop_keeps_the_state_that_reaches_the_loop_exit() {
    assert_eq!(
        refuses(&capture_fixture(D1_CONTINUE_IN_FOR)),
        MUTATED_MESSAGE,
        "the continue leaves alias===items live on the path that exits the loop, so the outer \
         catch mutates items and the snapshot must stay refused"
    );
}

const D2_FINALLY_ALWAYS_THROWS: &str = r"    try { alias = items; throw 1; } catch { alias = other; } finally { throw 9; }
    throw 2;";

#[test]
fn an_abrupt_finally_is_not_polluted_by_the_intercepted_body_end_state() {
    check(
        "abrupt_finally_intercepted",
        &capture_fixture(D2_FINALLY_ALWAYS_THROWS),
        &[("got", 1.0)],
    );
}

const D3_NESTED_BREAK_IN_LITERAL_TRUE_LOOP: &str = r"    while (true) { while (true) { break; } throw 1; }
    alias = items;
    throw 2;";

#[test]
fn a_break_of_an_inner_loop_does_not_make_the_outer_loop_breakable() {
    check(
        "abrupt_nested_loop_break",
        &capture_fixture(D3_NESTED_BREAK_IN_LITERAL_TRUE_LOOP),
        &[("got", 1.0)],
    );
}

const D4_NEGATIVE_ZERO_CALL_SITE: &str = r"
export function got(): number {
  class C { static k = -0; }
  return Object.is(C.k, -0) ? 1 : 0;
}
";

const D4_NEGATIVE_ZERO_CALL_SITE_MODULE: &str = r"
class Top { static k = -0; }
export function got(): number { return Object.is(Top.k, -0) ? 1 : 0; }
";

#[test]
fn a_negative_zero_literal_at_a_call_site_stays_negative() {
    check(
        "negative_zero_call_site",
        D4_NEGATIVE_ZERO_CALL_SITE,
        &[("got", 1.0)],
    );
    check_with_module_init(
        "negative_zero_call_site_module",
        D4_NEGATIVE_ZERO_CALL_SITE_MODULE,
        &[("got", 1.0)],
    );
}

const D4_NEGATIVE_ZERO_LOCAL: &str = r"
export function got(): number {
  const z = -0;
  return 1 / z;
}
";

const D4_NEGATIVE_ZERO_LOCAL_IDENTITY: &str = r"
export function got(): number {
  const z = -0;
  return Object.is(z, -0) ? 1 : 0;
}
";

const D4_NEGATIVE_ZERO_INDEX: &str = r"
export function got(): number {
  const a: number[] = [7, 8];
  return a[-0];
}
";

const D4_NEGATIVE_ZERO_ARITHMETIC: &str = r"
export function got(): number {
  const z = -0;
  return z + 3;
}
";

#[test]
fn a_negative_zero_local_carries_its_sign() {
    check(
        "negative_zero_local",
        D4_NEGATIVE_ZERO_LOCAL,
        &[("got", f64::NEG_INFINITY)],
    );
    check(
        "negative_zero_local_identity",
        D4_NEGATIVE_ZERO_LOCAL_IDENTITY,
        &[("got", 1.0)],
    );
    check(
        "negative_zero_arithmetic",
        D4_NEGATIVE_ZERO_ARITHMETIC,
        &[("got", 3.0)],
    );
}

#[test]
fn a_negative_zero_index_is_the_zero_index() {
    check(
        "negative_zero_index",
        D4_NEGATIVE_ZERO_INDEX,
        &[("got", 7.0)],
    );
}
