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

fn numbers_main(package: &str, calls: &[(&str, f64)]) -> String {
    let mut checks = String::new();
    for (name, value) in calls {
        let _ = writeln!(
            checks,
            "    assert_eq!({package}::{name}(&ctx), {value}_f64);"
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

const F1_REBIND_INSIDE_THE_THROWN_EXPRESSION: &str = r"
function f(): number {
  const items: number[] = [1, 2];
  const other: number[] = [9, 9];
  let alias: number[] = items;
  class C { read(): number { return items[0]; } }
  const c = new C();
  try {
    alias = other;
    throw (alias = items)[0];
  } catch {
    alias[0] = 9;
  }
  return c.read();
}
export function got(): number { return f(); }
";

#[test]
fn a_rebind_inside_the_thrown_expression_reaches_the_catch() {
    let message = refuses(F1_REBIND_INSIDE_THE_THROWN_EXPRESSION);
    assert!(
        message.contains("the object it holds is modified"),
        "the catch mutation must be linked to `items` through the rebind performed by the thrown \
         expression; got {message:?}"
    );
}

const F1_THROW_WITHOUT_A_REBIND_STILL_SEES_THE_OLD_ALIAS: &str = r"
function f(): number {
  const items: number[] = [1, 2];
  const other: number[] = [9, 9];
  let alias: number[] = items;
  class C { read(): number { return items[0]; } }
  const c = new C();
  try {
    alias = other;
    throw 1;
  } catch {
    alias[0] = 9;
  }
  return c.read();
}
export function got(): number { return f(); }
";

#[test]
fn a_throw_without_a_rebind_still_enters_the_catch_from_the_pre_throw_state() {
    check(
        "catch_throw_without_rebind",
        F1_THROW_WITHOUT_A_REBIND_STILL_SEES_THE_OLD_ALIAS,
        &[("got", 1.0)],
    );
}

const F2_LOOP_THAT_NEVER_RUNS: &str = r"
function f(): number {
  let i: number = 0;
  const items: number[] = [1, 2];
  const other: number[] = [9, 9];
  let alias: number[] = other;
  class C { read(): number { return items[0]; } }
  const c = new C();
  try {
    while (i < 0) {
      alias = items;
      throw 1;
    }
    throw 2;
  } catch {
    alias[0] = 7;
  }
  return c.read();
}
export function got(): number { return f(); }
";

#[test]
fn a_loop_that_never_runs_does_not_poison_the_catch_entry() {
    check(
        "catch_unreachable_loop_body",
        F2_LOOP_THAT_NEVER_RUNS,
        &[("got", 1.0)],
    );
}

const F2_LOOP_THAT_RUNS: &str = r"
function f(): number {
  let i: number = 0;
  const items: number[] = [1, 2];
  const other: number[] = [9, 9];
  let alias: number[] = other;
  class C { read(): number { return items[0]; } }
  const c = new C();
  try {
    while (i < 1) {
      alias = items;
      throw 1;
    }
  } catch {
    alias[0] = 9;
  }
  return c.read();
}
export function got(): number { return f(); }
";

#[test]
fn a_throw_after_a_rebind_in_a_loop_that_runs_still_refuses() {
    refuses(F2_LOOP_THAT_RUNS);
}

const F3_MERGED_CLOSURE_CALL: &str = r"
function f(): number {
  const items: number[] = [1, 2];
  const other: number[] = [9, 9];
  let alias: number[] = other;
  let flag: boolean = true;
  let cb: () => void = (): void => {};
  if (flag) {
    cb();
  } else {
    cb = (): void => { alias = items; };
  }
  class C { read(): number { return other[0]; } }
  const c = new C();
  cb();
  alias[0] = 7;
  return c.read();
}
export function got(): number { return f(); }
";

#[test]
fn a_call_through_a_merged_closure_keeps_the_pre_if_alias_roots() {
    let message = refuses(F3_MERGED_CLOSURE_CALL);
    assert!(
        message.contains("the object it holds is modified"),
        "the later call must keep the `other` root that was live before the `if`; got {message:?}"
    );
}

const F3_UNCONDITIONAL_CLOSURE_CALL_IS_NOT_WIDENED: &str = r"
function f(): number {
  const items: number[] = [1, 2];
  const other: number[] = [9, 9];
  let alias: number[] = other;
  const cb = (): void => { alias = items; };
  class C { read(): number { return other[0]; } }
  const c = new C();
  cb();
  alias[0] = 7;
  return c.read();
}
export function got(): number { return f(); }
";

#[test]
fn a_closure_call_that_always_rebinds_does_not_keep_the_old_root() {
    check(
        "unconditional_closure_rebind",
        F3_UNCONDITIONAL_CLOSURE_CALL_IS_NOT_WIDENED,
        &[("got", 9.0)],
    );
}

const F4_STRING_STATIC_FIELD_EXPRESSION: &str = r#"
export function got(): number {
  const C = class { static k: string = "ab"; };
  return C.k.length;
}
"#;

const F4_STRING_STATIC_FIELD_DECLARATION: &str = r#"
export function got(): number {
  class C { static k: string = "ab"; }
  return C.k.length;
}
"#;

#[test]
fn a_string_static_field_of_a_local_class_expression_is_refused() {
    refuses(F4_STRING_STATIC_FIELD_EXPRESSION);
}

#[test]
fn a_string_static_field_of_a_local_class_declaration_is_refused() {
    refuses(F4_STRING_STATIC_FIELD_DECLARATION);
}

const F5_NULL_STATIC_FIELD: &str = r"
export function got(): number {
  const C = class { static k: number | null = null; };
  return C.k === null ? 1 : 0;
}
";

const F5_UNDEFINED_STATIC_FIELD: &str = r"
export function got(): number {
  const C = class { static k: number | undefined = undefined; };
  return C.k === undefined ? 1 : 0;
}
";

const F5_VOID_ZERO_STATIC_FIELD: &str = r"
export function got(): number {
  const C = class { static k: number | undefined = void 0; };
  return C.k === undefined ? 1 : 0;
}
";

const F5_NULL_STATIC_FIELD_DECLARATION: &str = r"
export function got(): number {
  class C { static k: number | null = null; }
  return C.k === null ? 1 : 0;
}
";

#[test]
fn null_undefined_and_void_zero_static_field_initializers_keep_their_value() {
    check(
        "nullish_static_field_expression",
        F5_NULL_STATIC_FIELD,
        &[("got", 1.0)],
    );
    check(
        "undefined_static_field_expression",
        F5_UNDEFINED_STATIC_FIELD,
        &[("got", 1.0)],
    );
    check(
        "void_zero_static_field_expression",
        F5_VOID_ZERO_STATIC_FIELD,
        &[("got", 1.0)],
    );
    check(
        "nullish_static_field_declaration",
        F5_NULL_STATIC_FIELD_DECLARATION,
        &[("got", 1.0)],
    );
}

const F5_NEGATIVE_STATIC_FIELD: &str = r"
export function got(): number {
  const C = class { static k: number = -3; };
  return C.k;
}
";

#[test]
fn a_negative_numeric_static_field_initializer_carries_its_value() {
    check(
        "negative_static_field_expression",
        F5_NEGATIVE_STATIC_FIELD,
        &[("got", -3.0)],
    );
}

const F5_NULLISH_FOR_A_NON_NULLISH_TYPE: &str = r"
export function got(): number {
  const C = class { static k: number = null; };
  return C.k === null ? 1 : 0;
}
";

const F5_UNTYPED_NULL: &str = r"
export function got(): number {
  const C = class { static k = null; };
  return C.k === null ? 1 : 0;
}
";

const F5_UNRESOLVABLE_NULLISH_TYPE: &str = r"
export function got(): number {
  const C = class { static k: Missing | null = null; };
  return C.k === null ? 1 : 0;
}
";

const F5_NON_LITERAL: &str = r"
export function got(): number {
  const n = 3;
  const C = class { static k: number = n; };
  return C.k;
}
";

const F5_VOID_WITH_AN_OPERAND: &str = r"
function sideEffect(): number { return 1; }
export function got(): number {
  const C = class { static k: number | undefined = void sideEffect(); };
  return C.k === undefined ? 1 : 0;
}
";

#[test]
fn a_nullish_static_field_initializer_for_a_non_nullish_type_is_refused() {
    refuses(F5_NULLISH_FOR_A_NON_NULLISH_TYPE);
}

#[test]
fn an_untyped_null_static_field_initializer_is_refused() {
    refuses(F5_UNTYPED_NULL);
}

#[test]
fn an_unresolvable_nullish_static_field_initializer_is_refused() {
    refuses(F5_UNRESOLVABLE_NULLISH_TYPE);
}

#[test]
fn a_non_literal_static_field_initializer_is_still_refused() {
    refuses(F5_NON_LITERAL);
}

#[test]
fn a_void_static_field_initializer_with_an_operand_is_refused() {
    refuses(F5_VOID_WITH_AN_OPERAND);
}
