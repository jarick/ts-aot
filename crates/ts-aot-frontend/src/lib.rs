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
        let found = with_semantic("test.ts", src, |sem| sem.scoping().symbols_len());
        assert!(found.is_some());
    }

    #[test]
    fn analyze_semantic_flags_invalid_syntax_severity() {
        let bag = analyze_semantic("test.ts", "const = 1;");
        let diag = bag.iter().next().expect("at least one diagnostic");
        assert_eq!(diag.severity, Severity::Error);
    }

    #[test]
    fn frontend_rejects_duplicate_arrow_parameter_names() {
        let output = FrontendPass::new().run("test.ts", "const f = (a, a) => 0;");
        let codes: Vec<&str> = output
            .diagnostics
            .errors()
            .map(|d| d.code.as_str())
            .collect();
        assert!(
            codes.contains(&"E0200"),
            "duplicate arrow parameters must be a syntax error, got {codes:?}"
        );
    }

    #[test]
    fn frontend_rejects_duplicate_destructured_arrow_parameters() {
        let output = FrontendPass::new().run("test.ts", "const f = ({ a }, { a }) => 0;");
        let codes: Vec<&str> = output
            .diagnostics
            .errors()
            .map(|d| d.code.as_str())
            .collect();
        assert!(
            codes.contains(&"E0200"),
            "duplicate destructured arrow params must be a syntax error, got {codes:?}"
        );
    }

    #[test]
    fn frontend_rejects_repeated_underscore_arrow_parameters() {
        let output = FrontendPass::new().run("test.ts", "const f = (_, _) => 0;");
        let codes: Vec<&str> = output
            .diagnostics
            .errors()
            .map(|d| d.code.as_str())
            .collect();
        assert!(
            codes.contains(&"E0200"),
            "repeated `_` arrow params must be a syntax error, got {codes:?}"
        );
    }

    #[test]
    fn frontend_allows_distinct_destructured_arrow_parameters() {
        let output = FrontendPass::new().run("test.ts", "const f = ({ a }, { b }) => 0;");
        let codes: Vec<&str> = output
            .diagnostics
            .errors()
            .map(|d| d.code.as_str())
            .collect();
        assert!(
            !codes.contains(&"E0200"),
            "distinct destructured arrow params must not be flagged as duplicates, got {codes:?}"
        );
    }

    #[test]
    fn frontend_rejects_duplicate_params_in_use_strict_function() {
        let output =
            FrontendPass::new().run("test.ts", "function f(a, a) { \"use strict\"; return a; }");
        let codes: Vec<&str> = output
            .diagnostics
            .errors()
            .map(|d| d.code.as_str())
            .collect();
        assert!(
            codes.contains(&"E0200"),
            "duplicate params in a `use strict` function must be a syntax error, got {codes:?}"
        );
    }

    #[test]
    fn frontend_allows_duplicate_params_in_non_strict_function() {
        let output = FrontendPass::new().run("test.ts", "function f(a, a) { return a; }");
        let codes: Vec<&str> = output
            .diagnostics
            .errors()
            .map(|d| d.code.as_str())
            .collect();
        assert!(
            !codes.contains(&"E0200"),
            "sloppy duplicate params must not be rejected, got {codes:?}"
        );
    }

    #[test]
    fn frontend_rejects_duplicate_params_in_class_method() {
        let output = FrontendPass::new().run("test.ts", "class C { m(a, a) { return a; } }");
        let codes: Vec<&str> = output
            .diagnostics
            .errors()
            .map(|d| d.code.as_str())
            .collect();
        assert!(
            codes.contains(&"E0200"),
            "class methods are always strict; duplicate params must be rejected, got {codes:?}"
        );
    }

    #[test]
    fn frontend_inherits_use_strict_into_nested_function_expression() {
        let output = FrontendPass::new().run(
            "test.ts",
            "function outer() { \"use strict\"; var g = function (a, a) { return a; }; return g; }",
        );
        let codes: Vec<&str> = output
            .diagnostics
            .errors()
            .map(|d| d.code.as_str())
            .collect();
        assert!(
            codes.contains(&"E0200"),
            "a nested function expression inherits strict from the outer directive, got {codes:?}"
        );
    }

    #[test]
    fn frontend_module_rejects_duplicate_params_without_use_strict() {
        let mut types = ts_aot_core::TypeTable::new();
        let pass = FrontendPass::new();
        let output = pass.run_with_types(
            "test.ts",
            "function f(a, a) { return a; }",
            &mut types,
            true,
        );
        let codes: Vec<&str> = output
            .diagnostics
            .errors()
            .map(|d| d.code.as_str())
            .collect();
        assert!(
            codes.contains(&"E0200"),
            "module code is strict and must reject duplicate parameter names even without a directive, got {codes:?}"
        );
    }

    #[test]
    fn frontend_script_use_strict_makes_top_level_functions_strict() {
        let output =
            FrontendPass::new().run("test.ts", "\"use strict\"; function f(a, a) { return a; }");
        let codes: Vec<&str> = output
            .diagnostics
            .errors()
            .map(|d| d.code.as_str())
            .collect();
        assert!(
            codes.contains(&"E0200"),
            "a script with a `use strict` prologue is strict, so top-level functions must reject duplicate parameter names, got {codes:?}"
        );
    }

    #[test]
    fn frontend_lowers_try_statement() {
        let src = "function f(): i64 { try { throw \"x\"; } catch (e) { } return 0; }";
        let output = FrontendPass::new().run("test.ts", src);
        let ts_aot_ir_hir::HirDecl::Function(func) = &output.program.declarations[0] else {
            panic!(
                "expected a function declaration, got {:?}",
                output.program.declarations
            );
        };
        let has_try = func
            .body
            .iter()
            .any(|s| matches!(s, ts_aot_ir_hir::HirStmt::Try { .. }));
        assert!(
            has_try,
            "a try statement must lower to HirStmt::Try (no longer dropped), got: {:?}",
            func.body
        );
    }

    #[test]
    fn frontend_rejects_duplicate_params_in_non_simple_function() {
        let output = FrontendPass::new().run("test.ts", "function f(a, b = 1, a) { return a; }");
        let codes: Vec<&str> = output
            .diagnostics
            .errors()
            .map(|d| d.code.as_str())
            .collect();
        assert!(
            codes.contains(&"E0200"),
            "non-simple parameter lists reject duplicate names even when sloppy, got {codes:?}"
        );
    }

    #[test]
    fn frontend_rejects_duplicate_params_in_non_simple_function_expression() {
        let output =
            FrontendPass::new().run("test.ts", "var g = function (a, { a }) { return a; };");
        let codes: Vec<&str> = output
            .diagnostics
            .errors()
            .map(|d| d.code.as_str())
            .collect();
        assert!(
            codes.contains(&"E0200"),
            "non-simple function-expression parameter lists reject duplicate names even when sloppy, got {codes:?}"
        );
    }

    #[test]
    fn frontend_nested_function_in_sloppy_arrow_inherits_sloppiness() {
        let output = FrontendPass::new().run(
            "test.ts",
            "var g = () => { return function (a, a) { return a; }; };",
        );
        let codes: Vec<&str> = output
            .diagnostics
            .errors()
            .map(|d| d.code.as_str())
            .collect();
        assert!(
            !codes.contains(&"E0200"),
            "a nested function expression inherits the sloppy strictness of its surrounding arrow, got {codes:?}"
        );
    }

    #[test]
    fn frontend_nested_function_in_use_strict_arrow_is_strict() {
        let output = FrontendPass::new().run(
            "test.ts",
            "var g = () => { \"use strict\"; return function (a, a) { return a; }; };",
        );
        let codes: Vec<&str> = output
            .diagnostics
            .errors()
            .map(|d| d.code.as_str())
            .collect();
        assert!(
            codes.contains(&"E0200"),
            "a `use strict` directive in an arrow body makes nested function expressions strict, got {codes:?}"
        );
    }

    #[test]
    fn frontend_rejects_use_strict_with_non_simple_parameters() {
        let output =
            FrontendPass::new().run("test.ts", "const f = (a = 0) => { \"use strict\"; };");
        let codes: Vec<&str> = output
            .diagnostics
            .errors()
            .map(|d| d.code.as_str())
            .collect();
        assert!(
            codes.contains(&"E0200"),
            "use strict with non-simple parameters must be a syntax error, got {codes:?}"
        );
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
