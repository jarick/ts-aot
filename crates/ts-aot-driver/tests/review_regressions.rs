use ts_aot_driver::{CompileOptions, Driver, EmitStage};
use ts_aot_test_support::run_generated_crate;

fn compile(source: &str) -> ts_aot_driver::DriverOutput {
    Driver::new().compile_source(
        "test.ts",
        source,
        &CompileOptions {
            emit: EmitStage::Rust,
            module: true,
        },
    )
}

fn run(source: &str, assertions: &str) {
    let out = compile(source);
    assert!(!out.has_errors(), "{:?}", out.diagnostics);
    let main = format!(
        r"
use ts_aot_runtime::{{Bump, Ctx, Host}};
struct NoHost;
impl Host for NoHost {{}}
fn main() {{
    let bump = Bump::new();
    let host = NoHost;
    let ctx = Ctx {{ arena: &bump, host: &host }};
    {assertions}
}}
"
    );
    let _ = run_generated_crate("review_regressions", &out.rust_source.unwrap(), &main);
}

#[test]
fn optional_mutating_method_updates_the_original_receiver() {
    run(
        r"
class Counter { value: number = 0; bump(): void { this.value += 1; } }
export function run(present: boolean): number {
    let c: Counter | null = present ? new Counter() : null;
    c?.bump();
    return c?.value ?? 9;
}
",
        "assert_eq!(review_regressions::run(&ctx, true), 1.0); assert_eq!(review_regressions::run(&ctx, false), 9.0);",
    );
}

#[test]
fn logical_accessor_setter_runs_only_on_assignment() {
    run(
        r"
class Counter {
    value: number | null = null;
    writes: number = 0;
    get x(): number | null { return this.value; }
    set x(v: number) { this.writes += 1; this.value = v; }
    test(): number {
        this.x &&= 2;
        this.x ??= 3;
        this.x ||= 4;
        this.x &&= 0;
        this.x ||= 5;
        this.x ??= 6;
        return this.writes;
    }
}
export function run(): number { let c = new Counter(); return c.test(); }
",
        "assert_eq!(review_regressions::run(&ctx), 3.0);",
    );
}

#[test]
fn undefined_optional_argument_selects_nullable_default() {
    run(
        r"
function fallback(x: number | null = 7): number | null { return x; }
export function run(x: number | undefined): number | null { return fallback(x); }
",
        "assert_eq!(review_regressions::run(&ctx, None), Some(7.0)); assert_eq!(review_regressions::run(&ctx, Some(3.0)), Some(3.0));",
    );
}

#[test]
fn ambiguous_optional_argument_is_reported() {
    let out = compile(
        r"
function fallback(x: number | null = 7): number | null { return x; }
export function run(x: number | null | undefined): number | null { return fallback(x); }
",
    );
    assert!(
        out.has_errors(),
        "ambiguous nullish arguments must be rejected"
    );
}

#[test]
fn borrowed_this_owned_reads_compile() {
    run(
        r"
class Counter {
    value: number = 0;
    constructor() { this.copy(); }
    copy(): Counter { return this; }
    bump(): void { this.value += 1; }
    forward(): Counter { this.bump(); return this; }
}
export function run(): number { let c = new Counter(); return c.forward().value; }
",
        "assert_eq!(review_regressions::run(&ctx), 1.0);",
    );
}

#[test]
fn deferred_calls_do_not_make_the_enclosing_function_throw() {
    run(
        r"
function boom(): number { throw 2; }
export function deferred(): number { let f = () => boom(); return 4; }
",
        "assert_eq!(review_regressions::deferred(&ctx), 4.0);",
    );
}

#[test]
fn generator_method_reborrows_its_owned_receiver() {
    let out = compile(
        r"
class Counter {
    value: number = 0;
    bump(): void { this.value += 1; }
    *values(): Generator<number> { this.bump(); yield this.value; }
}
",
    );
    assert!(!out.has_errors(), "{:?}", out.diagnostics);
    let rust: String = out
        .rust_source
        .unwrap()
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    assert!(
        rust.contains("Counter::Counter__bump(&mut((self)),"),
        "{rust}"
    );
    assert!(!rust.contains("&mut((*self))"), "{rust}");
}
