mod body;
mod decl;
mod dump;
mod expr;
mod frontend;
mod module;
mod ops;
mod scope;
mod semantic_analyze;
mod skeleton;
mod stmt;
mod type_resolver;
mod util;

#[cfg(test)]
mod walker_tests;

pub use frontend::{FrontendOutput, FrontendPass};
pub use semantic_analyze::{analyze_semantic, with_semantic};

#[cfg(test)]
mod tests {
    use ts_aot_core::Severity;

    use super::*;

    #[test]
    fn analyze_semantic_and_frontend_produce_same_empty_program() {
        let src = "function noop(): void {}";
        let sem = analyze_semantic("test.ts", src);
        assert!(!sem.has_errors(), "{sem:?}");

        let output = FrontendPass::new().run("test.ts", src);
        assert!(!output.diagnostics.has_errors(), "{:?}", output.diagnostics);
        assert_eq!(output.program.decl_count(), 1);
    }

    #[test]
    fn with_semantic_yields_semantic_handle_for_valid_source() {
        let src = "const x: i32 = 1;";
        let found = with_semantic("test.ts", src, |sem| sem.symbols().len());
        assert!(found.is_some());
    }

    #[test]
    fn analyze_semantic_flags_invalid_syntax_severity() {
        let bag = analyze_semantic("test.ts", "const = 1;");
        let diag = bag.iter().next().expect("at least one diagnostic");
        assert_eq!(diag.severity, Severity::Error);
    }

    #[test]
    fn frontend_pass_walks_function_body() {
        let output = FrontendPass::new().run(
            "test.ts",
            "function greet(name: string): string { return name; }",
        );
        assert!(!output.diagnostics.has_errors(), "{:?}", output.diagnostics);
        assert_eq!(output.program.decl_count(), 1);
        match &output.program.declarations[0] {
            ts_aot_ir_hir::HirDecl::Function(f) => {
                assert_eq!(f.name, ts_aot_core::Atom::from("greet"));
                assert_eq!(f.params.len(), 1);
                match f.body.as_slice() {
                    [
                        ts_aot_ir_hir::HirStmt::Return {
                            value: Some(ts_aot_ir_hir::HirExpr::Local { id, .. }),
                        },
                    ] => assert_eq!(
                        *id,
                        ts_aot_core::LocalId::from_raw(0),
                        "param `name` is Local(0)"
                    ),
                    other => panic!("expected body `return name;`, got {other:?}"),
                }
            }
            other => panic!("expected Function, got {other:?}"),
        }
    }
}
