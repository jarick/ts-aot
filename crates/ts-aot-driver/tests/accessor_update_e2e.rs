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

fn rust_of(source: &str) -> String {
    let out = compile(source);
    assert!(
        !out.has_errors(),
        "must compile through the full pipeline; got {:?}",
        out.diagnostics
    );
    out.rust_source
        .expect("emit-rust must populate rust_source")
}

fn mir_of(source: &str) -> String {
    let out = Driver::new().compile_source(
        "test.ts",
        source,
        &CompileOptions {
            emit: EmitStage::Mir,
            module: true,
        },
    );
    assert!(
        !out.has_errors(),
        "must compile through the full pipeline; got {:?}",
        out.diagnostics
    );
    out.mir_text.expect("emit-mir must populate mir_text")
}

fn main_with(body: &str) -> String {
    format!(
        "use ts_aot_runtime::{{Bump, Ctx, Host}};\n\nstruct NoHost;\nimpl Host for NoHost {{}}\n\n\
         fn main() {{\n    let host = NoHost;\n    let bump = Bump::new();\n    \
         let ctx = Ctx {{ arena: &bump, host: &host }};\n{body}    println!(\"ok\");\n}}\n"
    )
}

fn expect_type_error(package: &str, rust: &str, call: &str) {
    let body = format!(
        "    let thrown = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| \
         {package}::{call}(&ctx)));\n    \
         let payload = thrown.expect_err(\"{call} must throw a TypeError at run time\");\n    \
         let error = payload\n        \
         .downcast_ref::<ts_aot_runtime::TypeErrorPayload>()\n        \
         .expect(\"the payload must keep the TypeError kind instead of a bare string\");\n    \
         assert!(error.to_string().starts_with(\"TypeError\"), \"got {{}}\", error);\n"
    );
    let stdout = run_generated_crate(package, rust, &main_with(&body));
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

fn run_body(package: &str, rust: &str, body: &str) {
    let stdout = run_generated_crate(package, rust, &main_with(body));
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_private_get_only_accessor_compound_update_throws_a_type_error() {
    const SOURCE: &str = r"
class C {
  get #field(): i64 { return 1; }
  bump(): i64 { return this.#field += 1; }
}
export function run(): i64 { return new C().bump(); }
";
    let rust = rust_of(SOURCE);
    assert!(
        rust.contains("__ts_aot_throw_type_error"),
        "a compound update of a get-only accessor must call the runtime TypeError helper; got:\n{rust}"
    );
    assert!(
        !rust.contains("return Err"),
        "the accessor update must throw at run time, not return a Result; got:\n{rust}"
    );
    let mir = mir_of(SOURCE);
    assert!(
        mir.contains("throw_type_error"),
        "the MIR must carry the runtime error, got:\n{mir}"
    );
    expect_type_error("acc_update_private", &rust, "run");
}

#[test]
fn a_public_get_only_accessor_compound_update_throws_a_type_error() {
    const SOURCE: &str = r"
class C {
  get y(): i64 { return 1; }
  bump(): i64 { return this.y += 1; }
}
export function run(): i64 { return new C().bump(); }
";
    let rust = rust_of(SOURCE);
    assert!(
        rust.contains("__ts_aot_throw_type_error"),
        "a compound update of a public get-only accessor must call the runtime TypeError helper; got:\n{rust}"
    );
    expect_type_error("acc_update_public", &rust, "run");
}

#[test]
fn a_postfix_update_of_a_get_only_accessor_throws_a_type_error() {
    const SOURCE: &str = r"
class C {
  get #field(): i64 { return 1; }
  bump(): i64 { return this.#field++; }
}
export function run(): i64 { return new C().bump(); }
";
    let rust = rust_of(SOURCE);
    assert!(
        rust.contains("__ts_aot_throw_type_error"),
        "a postfix update of a get-only accessor must call the runtime TypeError helper; got:\n{rust}"
    );
    expect_type_error("acc_update_postfix", &rust, "run");
}

#[test]
fn a_static_get_only_accessor_compound_update_throws_a_type_error() {
    const SOURCE: &str = r"
class C {
  static get s(): i64 { return 1; }
  static bump(): i64 { return C.s += 1; }
}
export function run(): i64 { return C.bump(); }
";
    let rust = rust_of(SOURCE);
    assert!(
        rust.contains("__ts_aot_throw_type_error"),
        "a static get-only accessor must call the runtime TypeError helper; got:\n{rust}"
    );
    expect_type_error("acc_update_static", &rust, "run");
}

#[test]
fn a_logical_update_that_must_write_a_get_only_accessor_throws_a_type_error() {
    const SOURCE: &str = r"
class C {
  get #field(): i64 { return 0; }
  bump(): i64 { return this.#field ||= 5; }
}
export function run(): i64 { return new C().bump(); }
";
    let rust = rust_of(SOURCE);
    assert!(
        rust.contains("__ts_aot_throw_type_error"),
        "a non-short-circuiting logical update must call the runtime TypeError helper; got:\n{rust}"
    );
    expect_type_error("acc_update_logical", &rust, "run");
}

#[test]
fn a_compound_update_of_a_get_only_accessor_is_catchable() {
    const SOURCE: &str = r"
class C {
  get #field(): i64 { return 1; }
  bump(): i64 { return this.#field += 1; }
}
export function run(): i64 {
  try { new C().bump(); return 0; } catch (e) { return 1; }
}
";
    let rust = rust_of(SOURCE);
    run_body(
        "acc_update_catch",
        &rust,
        "    assert_eq!(acc_update_catch::run(&ctx), 1);\n",
    );
}

#[test]
fn a_short_circuited_logical_and_update_of_a_get_only_accessor_does_not_throw() {
    const SOURCE: &str = r"
class C {
  #field: bool = false;
  #rhsCalls: i64 = 0;
  get #f(): bool { return this.#field; }
  rhs(): bool { this.#rhsCalls = this.#rhsCalls + 1; return true; }
  bump(): bool { return this.#f &&= this.rhs(); }
  rhsCalls(): i64 { return this.#rhsCalls; }
}
const c = new C();
export function runBool(): bool { return c.bump(); }
export function rhsCalls(): i64 { return c.rhsCalls(); }
";
    let rust = rust_of(SOURCE);
    run_body(
        "acc_update_and",
        &rust,
        "    assert_eq!(acc_update_and::runBool(&ctx), false);\n    \
         assert_eq!(acc_update_and::rhsCalls(&ctx), 0);\n",
    );
}

#[test]
fn a_short_circuited_nullish_update_of_a_get_only_accessor_does_not_throw() {
    const SOURCE: &str = r"
class C {
  #rhsCalls: i64 = 0;
  get #f(): i64 { return 7; }
  rhs(): i64 { this.#rhsCalls = this.#rhsCalls + 1; return 0; }
  bump(): i64 { return this.#f ??= this.rhs(); }
  rhsCalls(): i64 { return this.#rhsCalls; }
}
const c = new C();
export function runNum(): i64 { return c.bump(); }
export function rhsCalls(): i64 { return c.rhsCalls(); }
";
    let rust = rust_of(SOURCE);
    run_body(
        "acc_update_nullish",
        &rust,
        "    assert_eq!(acc_update_nullish::runNum(&ctx), 7);\n    \
         assert_eq!(acc_update_nullish::rhsCalls(&ctx), 0);\n",
    );
}

#[test]
fn a_compound_update_of_an_accessor_with_a_setter_still_reads_and_writes() {
    const SOURCE: &str = r"
class C {
  #v: i64 = 0;
  get #f(): i64 { return this.#v; }
  set #f(value: i64) { this.#v = value; }
  bump(): i64 { return this.#f += 2; }
  stored(): i64 { return this.#v; }
}
const c = new C();
export function bumped(): i64 { return c.bump(); }
export function stored(): i64 { return c.stored(); }
";
    let rust = rust_of(SOURCE);
    assert!(
        !rust.contains("__ts_aot_throw_type_error"),
        "an accessor with a setter must keep calling the setter; got:\n{rust}"
    );
    run_body(
        "acc_update_setter",
        &rust,
        "    assert_eq!(acc_update_setter::bumped(&ctx), 2);\n    \
         assert_eq!(acc_update_setter::stored(&ctx), 2);\n",
    );
}

#[test]
fn an_operator_error_in_a_get_only_compound_update_is_preserved() {
    const SOURCE: &str = r"
class C {
  get #field(): bigint { return 10n; }
  bump(): bigint { return this.#field /= 0n; }
}
export function run(): bigint { return new C().bump(); }
";
    let rust = rust_of(SOURCE);
    assert!(
        rust.contains("__ts_aot_bigint_div"),
        "the bigint division operator must stay in the generated code; got:\n{rust}"
    );
    assert!(
        rust.contains("__ts_aot_throw_type_error"),
        "the failed write must still throw a TypeError; got:\n{rust}"
    );
    assert!(
        rust.find("__ts_aot_bigint_div").unwrap() < rust.find("__ts_aot_throw_type_error").unwrap(),
        "the operator must be evaluated before the write-failure throw; got:\n{rust}"
    );
    let body = "    let thrown = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| \
         bigint_divzero::run(&ctx)));\n    \
         let err = thrown.expect_err(\"division by zero must throw before the failed write\");\n    \
         assert!(\n        \
         err.downcast_ref::<ts_aot_runtime::TypeErrorPayload>().is_none(),\n        \
         \"the operator error must not be swallowed by the write-failure TypeError\"\n    );\n    \
         let error = err\n        \
         .downcast_ref::<ts_aot_runtime::RangeErrorPayload>()\n        \
         .expect(\"the bigint error must keep the RangeError kind instead of a bare string\");\n    \
         assert_eq!(error.message, \"Division by zero\");\n    \
         assert_eq!(error.to_string(), \"RangeError: Division by zero\");\n";
    let stdout = run_generated_crate("bigint_divzero", &rust, &main_with(body));
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_plain_write_to_a_get_only_accessor_keeps_its_existing_diagnostic() {
    const SOURCE: &str = r"
class C {
  get #field(): i64 { return 1; }
  write(): i64 { return this.#field = 5; }
}
";
    let out = compile(SOURCE);
    assert!(
        !out.has_errors(),
        "a plain write to a get-only accessor must not become a hard error; got {:?}",
        out.diagnostics
    );
    assert!(
        out.diagnostics.iter().any(|d| d.code.as_str() == "P0006"),
        "a plain write to a get-only accessor must keep its P0006 diagnostic; got {:?}",
        out.diagnostics
    );
}
