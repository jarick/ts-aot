use ts_aot_driver::{CompileOptions, Driver, EmitStage};

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

const ANNOTATED_FOR_OF: &str = "export function f(): void { for (const x of xs: number) { x; } }";

#[test]
fn a_fatal_parse_error_reports_the_underlying_syntax_message() {
    let out = compile_module(ANNOTATED_FOR_OF);
    let message = out
        .diagnostics
        .iter()
        .find(|d| d.code.as_str() == "E0100")
        .map_or_else(
            || panic!("expected a fatal parse error, got {:?}", out.diagnostics),
            |d| d.message.clone(),
        );
    assert!(
        message.contains("Expected `)` but found `:`"),
        "the fatal parse error must carry the oxc message, not a bare `internal parser panic`; got: {message}"
    );
}

#[test]
fn a_fatal_parse_error_points_at_the_offending_token() {
    let out = compile_module(ANNOTATED_FOR_OF);
    let span = out
        .diagnostics
        .iter()
        .find(|d| d.code.as_str() == "E0100")
        .map_or_else(
            || panic!("expected a fatal parse error, got {:?}", out.diagnostics),
            |d| d.span,
        );
    let start = usize::try_from(span.start).unwrap_or(usize::MAX);
    let end = usize::try_from(span.end).unwrap_or(usize::MAX);
    assert!(
        end <= ANNOTATED_FOR_OF.len(),
        "the span must stay inside the source; got {start}..{end}"
    );
    assert_eq!(
        ANNOTATED_FOR_OF.get(start..end),
        Some(":"),
        "the span must point at the offending `:`, got {start}..{end}"
    );
}

#[test]
fn a_shifted_annotation_reports_the_token_that_breaks_the_statement() {
    let out = compile_module(
        "export async function f(): Promise<void> { for await (const x of xs: number) { x; } }",
    );
    let message = out
        .diagnostics
        .iter()
        .find(|d| d.code.as_str() == "E0100")
        .map_or_else(
            || panic!("expected a fatal parse error, got {:?}", out.diagnostics),
            |d| d.message.clone(),
        );
    assert!(
        message.contains("found `:`"),
        "a `for await` head annotated on the operand must be reported with the offending token; got: {message}"
    );
}

#[test]
fn a_set_accessor_initializer_stays_a_ts_incompatible_diagnostic() {
    let out = compile_module(
        "export function f(): number { const o = { set a(x: number = 1) {} }; return 0; }",
    );
    assert!(out.has_errors(), "got {:?}", out.diagnostics);
    assert!(
        out.diagnostics.iter().any(|d| d.code.as_str() == "E0200"
            && d.message
                .contains("A 'set' accessor cannot have an initializer.")),
        "the setter-initializer diagnostic is what the harness classifies as TS-incompatible; got {:?}",
        out.diagnostics
    );
}
