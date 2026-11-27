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

fn numbers_main(package: &str, expectations: &[f64]) -> String {
    let mut checks = String::new();
    for (index, value) in expectations.iter().enumerate() {
        let _ = writeln!(
            checks,
            "    assert_eq!({package}::got{index}(&ctx), {value}_f64);"
        );
    }
    format!(
        "use ts_aot_runtime::{{Bump, Ctx, Host}};\n\nstruct NoHost;\nimpl Host for NoHost {{}}\n\n\
         fn main() {{\n    let host = NoHost;\n    let bump = Bump::new();\n    \
         let ctx = Ctx {{ arena: &bump, host: &host }};\n{checks}    println!(\"ok\");\n}}\n"
    )
}

fn check(package: &str, source: &str, expectations: &[f64]) {
    let out = compile(source);
    assert!(
        !out.has_errors(),
        "must compile through the full pipeline; got {:?}",
        out.diagnostics
    );
    let rust = out
        .rust_source
        .expect("emit-rust must populate rust_source");
    let main = numbers_main(package, expectations);
    let stdout = run_generated_crate(package, &rust, &main);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

fn refuses(source: &str) {
    let out = compile(source);
    assert!(
        out.diagnostics.errors().any(|d| d.code.as_str() == "E0419"),
        "expected the E0419 mutation refusal; got {:?}",
        out.diagnostics
    );
    assert!(
        out.rust_source.is_none(),
        "a refused source must not emit rust; got:\n{}",
        out.rust_source.unwrap_or_default()
    );
}

const TRY_CATCH_REBIND_BEFORE_THROW: &str = r"
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
    alias[0] = 7;
  }
  return c.read() * 1000 + items[0] * 100 + alias[0];
}
export function got0(): number { return f(); }
";

const TRY_CATCH_REBIND_BEFORE_THROW_CAPTURES_OTHER: &str = r"
function f(): number {
  const items: number[] = [1, 2];
  const other: number[] = [9, 9];
  let alias: number[] = items;
  class C { read(): number { return other[0]; } }
  const c = new C();
  try {
    alias = other;
    throw 1;
  } catch {
    alias[0] = 7;
  }
  return c.read();
}
export function got0(): number { return f(); }
";

#[test]
fn a_catch_that_only_runs_after_the_rebind_sees_the_rebound_alias() {
    check(
        "try_catch_rebind_before_throw",
        TRY_CATCH_REBIND_BEFORE_THROW,
        &[1107.0],
    );
}

#[test]
fn a_catch_mutation_of_the_rebound_alias_is_refused() {
    refuses(TRY_CATCH_REBIND_BEFORE_THROW_CAPTURES_OTHER);
}

const TRY_CATCH_THROW_BEFORE_REBIND: &str = r"
function f(): number {
  const items: number[] = [1, 2];
  const other: number[] = [9, 9];
  let alias: number[] = items;
  class C { read(): number { return items[0]; } }
  const c = new C();
  try {
    throw 1;
  } catch {
    alias[0] = 7;
  }
  return c.read();
}
export function got0(): number { return f(); }
";

#[test]
fn a_catch_reachable_before_the_rebind_still_refuses() {
    refuses(TRY_CATCH_THROW_BEFORE_REBIND);
}

const TRY_CATCH_UNREACHABLE_THROW_AFTER_REBIND: &str = r"
function f(): number {
  const items: number[] = [1, 2];
  const other: number[] = [9, 9];
  let alias: number[] = other;
  class C { read(): number { return items[0]; } }
  const c = new C();
  try {
    alias = other;
    throw 1;
    alias = items;
    throw 2;
  } catch {
    alias[0] = 7;
  }
  return c.read() * 1000 + items[0] * 100 + alias[0];
}
export function got0(): number { return f(); }
";

#[test]
fn an_unreachable_throw_does_not_reach_the_catch_join() {
    check(
        "try_catch_unreachable_throw_after_rebind",
        TRY_CATCH_UNREACHABLE_THROW_AFTER_REBIND,
        &[1107.0],
    );
}

const TRY_CATCH_THROW_AFTER_AN_IF_THAT_ALWAYS_THROWS: &str = r"
function f(flag: boolean): number {
  const items: number[] = [1, 2];
  const other: number[] = [9, 9];
  let alias: number[] = other;
  class C { read(): number { return items[0]; } }
  const c = new C();
  try {
    if (flag) {
      throw 1;
    } else {
      throw 2;
    }
    alias = items;
    throw 3;
  } catch {
    alias[0] = 7;
  }
  return c.read() * 1000 + items[0] * 100 + alias[0];
}
export function got0(): number { return f(true); }
";

const TRY_CATCH_THROW_AFTER_A_RETURN: &str = r"
function f(flag: boolean): number {
  const items: number[] = [1, 2];
  const other: number[] = [9, 9];
  let alias: number[] = other;
  class C { read(): number { return items[0]; } }
  const c = new C();
  try {
    if (flag) {
      return c.read();
      alias = items;
      throw 2;
    }
    alias = other;
    throw 1;
  } catch {
    alias[0] = 7;
  }
  return c.read() * 1000 + items[0] * 100 + alias[0];
}
export function got0(): number { return f(false); }
";

const TRY_CATCH_THROW_INSIDE_A_LOOP_AFTER_THE_REBIND: &str = r"
function f(flag: boolean): number {
  const items: number[] = [1, 2];
  const other: number[] = [9, 9];
  let alias: number[] = other;
  class C { read(): number { return items[0]; } }
  const c = new C();
  try {
    while (flag) {
      alias = items;
      throw 1;
    }
  } catch {
    alias[0] = 9;
  }
  return c.read();
}
export function got0(): number { return f(true); }
";

const TRY_CATCH_THROW_INSIDE_A_NESTED_IF: &str = r"
function f(flag: boolean): number {
  const items: number[] = [1, 2];
  const other: number[] = [9, 9];
  let alias: number[] = other;
  class C { read(): number { return items[0]; } }
  const c = new C();
  try {
    alias = items;
    if (flag) {
      throw 1;
    }
  } catch {
    alias[0] = 9;
  }
  return c.read();
}
export function got0(): number { return f(true); }
";

const TRY_CATCH_REACHABLE_THROW_AFTER_A_CONDITIONAL_REBIND: &str = r"
function f(flag: boolean): number {
  const items: number[] = [1, 2];
  const other: number[] = [9, 9];
  let alias: number[] = other;
  class C { read(): number { return items[0]; } }
  const c = new C();
  try {
    if (flag) {
      alias = items;
    }
    throw 1;
  } catch {
    alias[0] = 9;
  }
  return c.read();
}
export function got0(): number { return f(true); }
";

#[test]
fn a_throw_after_an_if_that_always_throws_is_unreachable() {
    check(
        "try_catch_throw_after_always_throwing_if",
        TRY_CATCH_THROW_AFTER_AN_IF_THAT_ALWAYS_THROWS,
        &[1107.0],
    );
}

#[test]
fn a_throw_after_a_return_is_unreachable() {
    check(
        "try_catch_throw_after_return",
        TRY_CATCH_THROW_AFTER_A_RETURN,
        &[1107.0],
    );
}

#[test]
fn a_throw_inside_a_loop_after_the_rebind_still_refuses() {
    refuses(TRY_CATCH_THROW_INSIDE_A_LOOP_AFTER_THE_REBIND);
}

#[test]
fn a_throw_inside_a_nested_if_still_refuses() {
    refuses(TRY_CATCH_THROW_INSIDE_A_NESTED_IF);
}

#[test]
fn a_reachable_throw_after_a_conditional_rebind_still_refuses() {
    refuses(TRY_CATCH_REACHABLE_THROW_AFTER_A_CONDITIONAL_REBIND);
}

const IF_ELSE_BOTH_ARMS_REBIND_THE_CLOSURE: &str = r"
function f(flag: boolean): number {
  const items: number[] = [1, 2];
  const other: number[] = [9, 9];
  let alias: number[] = other;
  let cb: () => void = (): void => {};
  if (flag) {
    cb = (): void => { alias = items; };
  } else {
    cb = (): void => { alias = other; };
  }
  class C { read(): number { return items[0]; } }
  const c = new C();
  cb();
  alias[0] = 9;
  return c.read();
}
export function got0(): number { return f(true); }
";

const IF_ELSE_ELSE_ARM_CALLS_THE_OLD_CLOSURE: &str = r"
function f(flag: boolean): number {
  const items: number[] = [1, 2];
  const other: number[] = [9, 9];
  let alias: number[] = other;
  let cb: () => void = (): void => {};
  if (flag) {
    cb = (): void => { alias = items; };
  } else {
    cb();
  }
  class C { read(): number { return items[0]; } }
  const c = new C();
  alias[0] = 9;
  return c.read();
}
export function got0(): number { return f(true); }
";

#[test]
fn both_arms_of_an_if_keep_their_closure_effects() {
    refuses(IF_ELSE_BOTH_ARMS_REBIND_THE_CLOSURE);
}

#[test]
fn the_else_arm_does_not_see_the_then_arms_closure_effects() {
    check(
        "if_else_else_only",
        IF_ELSE_ELSE_ARM_CALLS_THE_OLD_CLOSURE,
        &[1.0],
    );
}

const LOOP_ASSIGNS_THE_CAPTURE_AFTER_THE_MUTATION: &str = r"
function f(flag: boolean): number {
  const items: number[] = [1, 2];
  const other: number[] = [9, 9];
  let alias: number[] = other;
  class C { read(): number { return items[0]; } }
  const c = new C();
  while (flag) {
    alias[0] = 9;
    alias = items;
  }
  return c.read();
}
export function got0(): number { return f(true); }
";

#[test]
fn a_loop_back_observation_reaches_the_mutation_site() {
    refuses(LOOP_ASSIGNS_THE_CAPTURE_AFTER_THE_MUTATION);
}

const LOOP_BODY_LEAVES_THE_STATE_UNCHANGED: &str = r"
function f(flag: boolean): number {
  const items: number[] = [1, 2];
  const other: number[] = [9, 9];
  let alias: number[] = other;
  class C { read(): number { return items[0]; } }
  const c = new C();
  while (flag) {
    if (flag) {
      alias = other;
    }
    alias[0] = 7;
    flag = false;
  }
  return c.read() * 1000 + items[0] * 100 + alias[0];
}
export function got0(): number { return f(true); }
";

#[test]
fn a_loop_body_that_keeps_its_alias_state_is_not_refused() {
    check(
        "loop_stable_alias_state",
        LOOP_BODY_LEAVES_THE_STATE_UNCHANGED,
        &[1107.0],
    );
}

#[test]
fn the_loop_fixpoint_analysis_terminates_within_a_bounded_time() {
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let stable = compile(LOOP_BODY_LEAVES_THE_STATE_UNCHANGED);
        let loop_back = compile(LOOP_ASSIGNS_THE_CAPTURE_AFTER_THE_MUTATION);
        let _ = sender.send((stable.has_errors(), loop_back.has_errors()));
    });
    let (stable_errors, loop_back_errors) = receiver
        .recv_timeout(std::time::Duration::from_secs(120))
        .expect("the loop fixpoint analysis must terminate");
    assert!(
        !stable_errors,
        "a loop body that keeps its alias facts must not be refused"
    );
    assert!(
        loop_back_errors,
        "a mutation observed from the loop-back state must be refused"
    );
}
