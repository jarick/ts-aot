use std::fmt::Write as _;

use ts_aot_driver::{CompileOptions, Driver, DriverOutput, EmitStage};
use ts_aot_test_support::{check_generated_crate, run_generated_crate};

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

fn errors_of(source: &str) -> Vec<(String, String)> {
    let out = compile(source);
    assert!(
        out.has_errors(),
        "expected an error, got {:?}",
        out.diagnostics
    );
    out.diagnostics
        .errors()
        .map(|d| (d.code.as_str().to_owned(), d.message.clone()))
        .collect()
}

fn has_error(source: &str, code: &str, fragment: &str) {
    let errors = errors_of(source);
    assert!(
        errors
            .iter()
            .any(|(found_code, message)| found_code == code && message.contains(fragment)),
        "expected {code} mentioning `{fragment}`, got {errors:?}"
    );
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

#[test]
fn a_local_generator_is_lifted_and_consumed_by_for_of() {
    const SOURCE: &str = r"
export function got0(): number {
  function* g() { yield 1; yield 2; }
  let sum = 0;
  for (const x of g()) { sum = sum + x; }
  return sum;
}
";
    check("local_gen_forof", SOURCE, &[3.0]);
}

#[test]
fn a_local_generator_takes_arguments() {
    const SOURCE: &str = r"
export function got0(): number {
  function* g(n: number) { yield n; yield n + 1; }
  let sum = 0;
  for (const x of g(40)) { sum = sum + x; }
  return sum;
}
";
    check("local_gen_args", SOURCE, &[81.0]);
}

#[test]
fn a_local_generator_delegating_with_yield_star_reports_the_known_delegation_gap() {
    let out = compile(
        "export function got0(): number { \
           function* h(): i64 { yield 1; } \
           function* g(): i64 { yield* h(); yield 2; } \
           let sum = 0; \
           for (const x of g()) { sum = sum + x; } \
           return sum; \
         }",
    );
    assert!(!out.has_errors(), "got {:?}", out.diagnostics);
    assert!(
        out.diagnostics
            .iter()
            .any(|d| d.code.as_str() == "E0501" && d.message.contains("delegating yield")),
        "yield* must keep reporting that delegation is not forwarded, got {:?}",
        out.diagnostics
    );
}

#[test]
fn two_local_generators_in_one_function_stay_distinct() {
    const SOURCE: &str = r"
export function got0(): number {
  function* a() { yield 1; }
  function* b() { yield 2; }
  let sum = 0;
  for (const x of a()) { sum = sum + x; }
  for (const y of b()) { sum = sum + y; }
  return sum;
}
";
    check("local_gen_two", SOURCE, &[3.0]);
}

#[test]
fn a_local_generator_can_be_called_from_a_closure() {
    const SOURCE: &str = r"
export function got0(): number {
  function* g() { yield 7; }
  const read = () => {
    let sum = 0;
    for (const x of g()) { sum = sum + x; }
    return sum;
  };
  return read();
}
";
    check("local_gen_closure", SOURCE, &[7.0]);
}

#[test]
fn a_reference_before_the_local_generator_declaration_is_refused() {
    has_error(
        "export function got0(): number { \
           let sum = 0; \
           for (const x of g()) { sum = sum + x; } \
           function* g() { yield 1; } \
           return sum; \
         }",
        "E0500",
        "is referenced before its declaration",
    );
}

#[test]
fn a_local_generator_next_result_is_a_struct_with_value_and_done_fields() {
    let out = compile(
        "export function got0(): number { \
           function* g(): i64 { yield 1; } \
           const it = g(); \
           const step = it.next(); \
           return step.value ?? 0; \
         }",
    );
    assert!(
        !out.has_errors(),
        "a next() result must lower to a real struct field access; got {:?}",
        out.diagnostics
    );
    let rust = out
        .rust_source
        .expect("emit-rust must populate rust_source");
    assert!(
        rust.contains("GeneratorResult"),
        "the next() result must be typed as GeneratorResult, got:\n{rust}"
    );
    assert!(
        rust.contains(". value"),
        "`.value` must lower to a struct field access, got:\n{rust}"
    );
    check_generated_crate("local_gen_next_struct", &rust);
    let stdout = run_generated_crate(
        "local_gen_next_struct",
        &rust,
        &numbers_main("local_gen_next_struct", &[1.0]),
    );
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_module_level_generator_next_result_is_a_struct() {
    let out = compile(
        "function* g(): i64 { yield 1; } \
         export function got0(): number { const it = g(); const step = it.next(); return step.value ?? 0; }",
    );
    assert!(!out.has_errors(), "got {:?}", out.diagnostics);
    let rust = out
        .rust_source
        .expect("emit-rust must populate rust_source");
    assert!(
        rust.contains("GeneratorResult") && rust.contains(". value"),
        "a module-level generator's next() result must expose `.value` as a struct field, got:\n{rust}"
    );
    check_generated_crate("module_gen_next_struct", &rust);
    let stdout = run_generated_crate(
        "module_gen_next_struct",
        &rust,
        &numbers_main("module_gen_next_struct", &[1.0]),
    );
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_module_generator_next_result_reads_value_and_done() {
    check(
        "localgen_result_module",
        "function* g(): i64 { yield 1; } \
         export function got0(): number { const it = g(); const step = it.next(); return (step.value ?? 0) * 100; } \
         export function got1(): number { \
           const it = g(); \
           it.next(); \
           const last = it.next(); \
           let sum = 0; \
           if (last.done) { sum = sum + 7; } \
           return sum; \
         }",
        &[100.0, 7.0],
    );
}

#[test]
fn a_local_generator_next_result_reads_value_and_done() {
    check(
        "localgen_result_local",
        "export function got0(): number { \
           function* g(): i64 { yield 1; } \
           const it = g(); \
           const step = it.next(); \
           return (step.value ?? 0) * 100; \
         } \
         export function got1(): number { \
           function* g(): i64 { yield 1; } \
           const it = g(); \
           it.next(); \
           const last = it.next(); \
           let sum = 0; \
           if (last.done) { sum = sum + 7; } \
           return sum; \
         }",
        &[100.0, 7.0],
    );
}

#[test]
fn a_generator_result_done_is_usable_as_a_branch_condition() {
    check(
        "localgen_result_branch",
        "export function got0(): number { \
           function* g(): i64 { yield 1; yield 2; } \
           const it = g(); \
           const step = it.next(); \
           let sum = 0; \
           if (step.done) { sum = sum + 999; } else { sum = sum + (step.value ?? 0) * 100; } \
           return sum; \
         } \
         export function got1(): number { \
           function* g(): i64 { yield 1; yield 2; } \
           const it = g(); \
           const step = it.next(); \
           let sum = 0; \
           if (step.done) { sum = 999; } else { sum = (step.value ?? 0) * 11; } \
           return sum; \
         }",
        &[100.0, 11.0],
    );
}

#[test]
fn a_generator_declared_in_a_nested_block_is_refused() {
    has_error(
        "export function got0(): number { \
           let sum = 0; \
           { function* g() { yield 1; } for (const x of g()) { sum = sum + x; } } \
           return sum; \
         }",
        "E0500",
        "inside a nested block",
    );
}

#[test]
fn a_generator_declaration_shadowing_an_outer_generator_is_refused() {
    has_error(
        "export function got0(): number { \
           function* g() { yield 1; } \
           const read = () => { \
             function* g() { yield 2; } \
             let sum = 0; \
             for (const x of g()) { sum = sum + x; } \
             return sum; \
           }; \
           return read() + 0; \
         }",
        "E0500",
        "shadows a generator of an enclosing function",
    );
}

#[test]
fn a_local_generator_that_reads_an_enclosing_local_is_refused() {
    has_error(
        "export function got0(): number { \
           let base = 41; \
           function* g() { yield base; } \
           let sum = 0; \
           for (const x of g()) { sum = sum + x; } \
           return sum; \
         }",
        "E0500",
        "reads a variable of the enclosing",
    );
}
