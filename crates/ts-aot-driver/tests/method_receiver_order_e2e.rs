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

fn normalized(rust: &str) -> String {
    rust.chars().filter(|c| !c.is_whitespace()).collect()
}

fn codes(out: &ts_aot_driver::DriverOutput) -> Vec<&str> {
    out.diagnostics.iter().map(|d| d.code.as_str()).collect()
}

fn compile_hir(source: &str) -> ts_aot_driver::DriverOutput {
    Driver::new().compile_source(
        "test.ts",
        source,
        &CompileOptions {
            emit: EmitStage::Hir,
            module: true,
        },
    )
}

fn function_body(rust: &str, name: &str) -> String {
    let norm = normalized(rust);
    let signature = format!("pubfn{name}<'a>(");
    let start = norm
        .find(&signature)
        .unwrap_or_else(|| panic!("no `{name}` in generated code:\n{norm}"));
    let tail = &norm[start..];
    let end = tail[1..].find("pubfn").map_or(tail.len(), |i| i + 1);
    tail[..end].to_owned()
}

const ORDER_SOURCE: &str = "class Counter { n: number = 0; add(x: number): void { this.n = this.n + x; } } \
     let order: number = 0; \
     function make(): Counter { order = order * 10 + 1; return new Counter(); } \
     function arg(): number | null { order = order * 10 + 2; return 5; } \
     export function got(): number { make().add(arg() ?? 0); return order; }";

#[test]
fn a_call_receiver_is_materialized_before_the_argument_statements() {
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(receiver_order::got(&ctx), 12.0);
    println!("ok");
}
"#;
    let rust = rust_of(ORDER_SOURCE);
    let body = function_body(&rust, "got");
    let receiver_at = body
        .find("make(__ctx)")
        .unwrap_or_else(|| panic!("the receiver call must survive; got:\n{body}"));
    let argument_at = body
        .find("arg(__ctx)")
        .unwrap_or_else(|| panic!("the argument call must survive; got:\n{body}"));
    assert!(
        receiver_at < argument_at,
        "the side effects of the receiver must be materialized ahead of the argument's \
         statements; got:\n{body}"
    );
    assert!(
        !body.contains("&mut(make(__ctx))"),
        "the receiver call must not be borrowed inline, because that moves its evaluation \
         behind the argument statements; got:\n{body}"
    );
    let stdout = run_generated_crate("receiver_order", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_place_receiver_keeps_the_method_mutations() {
    const SOURCE: &str = "class Counter { n: number = 0; add(x: number): void { this.n = this.n + x; } } \
         let hits: number = 0; \
         function arg(): number { hits = hits + 1; return 5; } \
         export function got(): number { const c = new Counter(); c.add(arg()); return c.n * 10 + hits; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(receiver_place::got(&ctx), 51.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate("receiver_place", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn an_index_receiver_with_a_call_index_keeps_the_method_mutations() {
    const SOURCE: &str = "class Counter { n: number = 0; add(x: number): void { this.n = this.n + x; } } \
         let other: number = 0; \
         function idx(): number { return 0; } \
         function arg(): number | null { other = other + 1; return 5; } \
         export function got(): number { \
             const arr: Counter[] = [new Counter()]; \
             arr[idx()].add(arg() ?? 0); \
             return arr[0].n * 100 + other; \
         }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(index_receiver::got(&ctx), 501.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate("index_receiver", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn an_index_receiver_computes_the_index_before_the_argument_statements() {
    const SOURCE: &str = "class Counter { n: number = 0; add(x: number): void { this.n = this.n + x; } } \
         let order: number = 0; \
         function idx(): number { order = order * 10 + 1; return 0; } \
         function arg(): number | null { order = order * 10 + 2; return 5; } \
         export function got(): number { \
             const arr: Counter[] = [new Counter()]; \
             arr[idx()].add(arg() ?? 0); \
             return arr[0].n * 100 + order; \
         }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(index_order::got(&ctx), 512.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let body = function_body(&rust, "got");
    let index_at = body
        .find("idx(__ctx)")
        .unwrap_or_else(|| panic!("the index call must survive; got:\n{body}"));
    let argument_at = body
        .find("arg(__ctx)")
        .unwrap_or_else(|| panic!("the argument call must survive; got:\n{body}"));
    assert!(
        index_at < argument_at,
        "the index of a place receiver must be materialized ahead of the argument's statements; \
         got:\n{body}"
    );
    assert!(
        body.contains("&mut(arr["),
        "the receiver must stay a mutable place of the original element; got:\n{body}"
    );
    let stdout = run_generated_crate("index_order", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_reassigned_alias_target_is_not_resolved_to_one_function() {
    const REASSIGNED_BY_A_FUNCTION: &str = "class Counter { n: number = 0; add(x: number): void { this.n = this.n + x; } } \
         let counter: Counter = new Counter(); \
         function bump(x: number): number { counter = new Counter(); return x; } \
         function quiet(x: number): number { return x; } \
         let h: (x: number) => number; \
         function arm(): void { h = bump; } \
         h = quiet; \
         arm(); \
         export function got(): number { counter.add(h(1)); return counter.n; }";
    const REASSIGNED_DIRECTLY: &str = "class Counter { n: number = 0; add(x: number): void { this.n = this.n + x; } } \
         let counter: Counter = new Counter(); \
         function bump(x: number): number { counter = new Counter(); return x; } \
         function quiet(x: number): number { return x; } \
         let h: (x: number) => number; \
         h = bump; \
         h = quiet; \
         export function got(): number { counter.add(h(1)); return counter.n; }";
    const REASSIGNED_DIRECTLY_REVERSED: &str = "class Counter { n: number = 0; add(x: number): void { this.n = this.n + x; } } \
         let counter: Counter = new Counter(); \
         function bump(x: number): number { counter = new Counter(); return x; } \
         function quiet(x: number): number { return x; } \
         let h: (x: number) => number; \
         h = quiet; \
         h = bump; \
         export function got(): number { counter.add(h(1)); return counter.n; }";
    const UNAMBIGUOUS: &str = "class Counter { n: number = 0; add(x: number): void { this.n = this.n + x; } } \
         let counter: Counter = new Counter(); \
         function quiet(x: number): number { return x; } \
         const h: (x: number) => number = quiet; \
         export function got(): number { counter.add(h(1)); return counter.n; }";
    let ambiguous = [
        (
            "`h` is reassigned inside a function body",
            REASSIGNED_BY_A_FUNCTION,
        ),
        ("`h` is reassigned directly", REASSIGNED_DIRECTLY),
        (
            "`h` is reassigned directly, with the harmless target written first",
            REASSIGNED_DIRECTLY_REVERSED,
        ),
    ];
    for (what, source) in ambiguous {
        let out = compile_hir(source);
        assert!(
            codes(&out).contains(&"E0419"),
            "{what} and ends up holding another target, so a call through it may still be the one \
             that reassigns the module binding `counter`; the receiver order cannot be proven, so \
             the call must be refused; got {:?}",
            out.diagnostics
        );
    }

    let out = compile_hir(UNAMBIGUOUS);
    assert!(
        !out.has_errors(),
        "a single alias target must still resolve to its function, so this call stays accepted; \
         got {:?}",
        out.diagnostics
    );
}
