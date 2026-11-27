use ts_aot_driver::{CompileOptions, Driver, DriverOutput, EmitStage};

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

fn e0500(out: &DriverOutput) -> bool {
    out.diagnostics.errors().any(|d| d.code.as_str() == "E0500")
}

#[test]
fn a_referenced_local_generic_generator_is_a_hard_error() {
    const SOURCE: &str = "export function got(): i64 { \
         function* gen<T>(v: T): i64 { yield 1; return 0; } gen(1); return 0; }";
    let out = compile(SOURCE);
    assert!(
        out.has_errors(),
        "a local generic generator declaration must be a hard error, not a warning; got {:?}",
        out.diagnostics
    );
    assert!(
        e0500(&out),
        "a local generic generator declaration must report E0500 at error severity; got {:?}",
        out.diagnostics
    );
    assert!(
        out.rust_source.is_none(),
        "no Rust may be emitted for a local generic generator declaration, otherwise the \
         reference lowers to an undefined `gen_`; got:\n{:?}",
        out.rust_source
    );
    assert!(
        out.diagnostics
            .iter()
            .all(|d| !d.message.contains("function expression form")),
        "the diagnostic must not call a declaration a function expression; got {:?}",
        out.diagnostics
    );
}

#[test]
fn an_unused_local_generic_generator_is_a_hard_error() {
    const SOURCE: &str = "export function got(): i64 { \
         function* gen<T>(v: T): i64 { yield 1; return 0; } return 0; }";
    let out = compile(SOURCE);
    assert!(
        out.has_errors(),
        "an unused local generic generator declaration must not compile silently; got {:?}",
        out.diagnostics
    );
    assert!(
        e0500(&out),
        "an unused local generic generator declaration must report E0500 at error severity; got \
         {:?}",
        out.diagnostics
    );
    assert!(
        out.rust_source.is_none(),
        "no Rust may be emitted for an unused local generic generator declaration; got:\n{:?}",
        out.rust_source
    );
}

#[test]
fn a_non_generic_local_generator_still_lifts() {
    const SOURCE: &str = "export function got(): i64 { \
         function* gen(): i64 { yield 1; return 0; } const it = gen(); return 0; }";
    let out = compile(SOURCE);
    assert!(
        !out.has_errors(),
        "a non-generic local generator declaration must keep lifting; got {:?}",
        out.diagnostics
    );
    assert!(
        out.rust_source.is_some(),
        "a non-generic local generator declaration must still emit Rust"
    );
}
