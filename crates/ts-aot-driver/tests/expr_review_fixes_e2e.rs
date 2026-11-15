use ts_aot_driver::{CompileOptions, Driver, EmitStage};
use ts_aot_test_support::check_generated_crate;

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

fn codes(source: &str) -> Vec<String> {
    compile_module(source)
        .diagnostics
        .iter()
        .map(|d| d.code.as_str().to_owned())
        .collect()
}

#[test]
fn a_literal_operand_types_an_inferred_closure_return() {
    const SOURCE: &str = "export function mk(x: i64): () => i64 { return () => x && 1; } \
         export function mkOr(x: i64): () => i64 { return () => x || 1; }";
    let out = compile_module(SOURCE);
    assert!(
        !out.has_errors(),
        "`x && 1` over `i64` must be typed from the other operand; got {:?}",
        out.diagnostics
    );
    assert!(
        !out.diagnostics.iter().any(|d| d.code.as_str() == "E0415"),
        "the closure return type must be inferred from `x && 1`; got {:?}",
        out.diagnostics
    );
    let rust = out
        .rust_source
        .expect("emit-rust must populate rust_source");
    check_generated_crate("literal_operand_closure", &rust);
}

#[test]
fn await_and_yield_withdraw_written_captures() {
    for source in [
        "export async function f(): Promise<number> { let x: number | null = null; \
         const set = () => { x = null; }; x = 1; await 0; return x; }",
        "export function* g(): Generator<number> { let x: number | null = null; \
         const set = () => { x = null; }; x = 1; yield 0; return x; }",
    ] {
        let diagnostics = codes(source);
        assert!(
            diagnostics.iter().any(|c| c == "P0018"),
            "`{source}` must be rejected with P0018 (await/yield may run the closure \
             that writes the captured variable); got {diagnostics:?}"
        );
    }
    for source in [
        "export async function f(): Promise<number> { let x: number | null = null; \
         const set = () => { x = null; }; x = 1; return x; }",
        "export function* g(): Generator<number> { let x: number | null = null; \
         const set = () => { x = null; }; x = 1; return x; }",
    ] {
        let diagnostics = codes(source);
        assert!(
            !diagnostics.iter().any(|c| c == "P0018"),
            "without `await`/`yield` the narrowing is still sound; got {diagnostics:?}"
        );
    }
}

#[test]
fn an_optional_element_flattens_into_one_optional() {
    let out = compile_module(
        "export function f(a: (number | null)[] | null, i: number): number | null \
         { return a?.[i]; }",
    );
    let rejection = out
        .diagnostics
        .iter()
        .find(|d| d.code.as_str() == "P0019")
        .unwrap_or_else(|| {
            panic!(
                "`a?.[i]` over a nullable element must be rejected against a `null`-only \
                 target; got {:?}",
                out.diagnostics
            )
        });
    assert!(
        rejection.message.contains("null | undefined"),
        "the rejected value is the flattened `null | undefined`, not a bare `undefined`; \
         got {:?}",
        rejection.message
    );

    let out = compile_module(
        "export function f(a: (number | null)[] | null, i: number): bool \
         { const v = a?.[i]; return v === null; }",
    );
    assert!(
        out.diagnostics.iter().any(|d| d.code.as_str() == "P0020"),
        "`v === null` over the flattened `number | null | undefined` cannot be decided; \
         got {:?}",
        out.diagnostics
    );

    let rust = rust_of(
        "export function f(a: (number | null)[] | null, i: number): number | null | undefined \
         { return a?.[i]; }",
    );
    let flat: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        flat.contains("->Option<f64>{"),
        "`a?.[i]` must flatten to a single `Option<f64>`; got:\n{rust}"
    );
    assert!(
        !flat.contains("Option<Option<"),
        "`a?.[i]` must not nest `Option<Option<_>>`; got:\n{rust}"
    );
    check_generated_crate("optional_index_flatten", &rust);
}
