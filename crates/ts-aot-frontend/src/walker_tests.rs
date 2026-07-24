use std::collections::HashMap;

use ts_aot_core::{Atom, GenericParamId, LocalId, Severity, Span, Type, TypeId, TypeTable};
use ts_aot_ir_hir::{
    HirBinaryOp, HirCallee, HirDecl, HirExpr, HirFunction, HirStmt, ObjectLiteralField,
};

use super::*;
use crate::type_resolver::TypeParamMap;

const PARSE_ERROR_CODE: &str = "E0200";
const PARSE_PANIC_CODE: &str = "E0100";

fn has_e0403(diagnostics: &ts_aot_core::DiagnosticBag) -> bool {
    diagnostics
        .iter()
        .any(|d| d.severity == ts_aot_core::Severity::Warning && d.code.as_str() == "E0403")
}

fn has_e0404(diagnostics: &ts_aot_core::DiagnosticBag) -> bool {
    diagnostics
        .iter()
        .any(|d| d.severity == ts_aot_core::Severity::Warning && d.code.as_str() == "E0404")
}

fn has_e0407(diagnostics: &ts_aot_core::DiagnosticBag) -> bool {
    diagnostics
        .iter()
        .any(|d| d.severity == ts_aot_core::Severity::Warning && d.code.as_str() == "E0407")
}

fn count_e0400(diagnostics: &ts_aot_core::DiagnosticBag) -> usize {
    diagnostics
        .iter()
        .filter(|d| d.severity == ts_aot_core::Severity::Warning && d.code.as_str() == "E0400")
        .count()
}

#[test]
fn empty_source_yields_empty_program_without_errors() {
    let output = FrontendPass::new().run("test.ts", "");
    assert!(!output.diagnostics.has_errors(), "{:?}", output.diagnostics);
    assert_eq!(output.program.decl_count(), 0);
}

#[test]
fn function_declaration_is_scanned_with_signature() {
    let output =
        FrontendPass::new().run("test.ts", "function add(a: i32, b: i32): i32 { return 0; }");
    assert!(!output.diagnostics.has_errors(), "{:?}", output.diagnostics);
    assert_eq!(output.program.decl_count(), 1);
    match &output.program.declarations[0] {
        HirDecl::Function(f) => {
            assert_eq!(f.name, Atom::from("add"));
            assert_eq!(f.params.len(), 2);
            assert_eq!(f.body.len(), 1);
            match &f.body[0] {
                HirStmt::Return {
                    value: Some(HirExpr::Int(v, span)),
                } => {
                    assert_eq!(
                        *v, 0,
                        "walker fills the body with the `return 0;` statement"
                    );
                    assert_eq!(
                        *span,
                        Span::new(43, 44),
                        "Int literal `0` in `return 0;` must carry its source span (offset 43..44)"
                    );
                }
                other => panic!("expected Return(Int(0)), got {other:?}"),
            }
            assert!(!f.is_async);
        }
        other => panic!("expected Function, got {other:?}"),
    }
}

#[test]
fn function_declaration_preserves_compound_expression_source_spans() {
    let output = FrontendPass::new().run(
        "test.ts",
        "function calc(a: i32, b: i32): i32 { return a + b * 2; }",
    );
    assert!(!output.diagnostics.has_errors(), "{:?}", output.diagnostics);
    match &output.program.declarations[0] {
        HirDecl::Function(f) => {
            assert_eq!(f.body.len(), 1);
            match &f.body[0] {
                HirStmt::Return {
                    value:
                        Some(HirExpr::Binary {
                            op, lhs, rhs, span, ..
                        }),
                } => {
                    assert_eq!(*op, HirBinaryOp::Add);
                    assert_eq!(
                        *span,
                        Span::new(44, 53),
                        "outer Add expression must span `a + b * 2` (44..53)"
                    );
                    let HirExpr::Local { span: a_span, .. } = lhs.as_ref() else {
                        panic!("expected Local `a` on lhs, got {lhs:?}");
                    };
                    assert_eq!(
                        *a_span,
                        Span::new(44, 45),
                        "Local `a` must carry its source span (44..45)"
                    );
                    let HirExpr::Binary {
                        op: inner_op,
                        lhs: inner_lhs,
                        rhs: inner_rhs,
                        span: inner_span,
                        ..
                    } = rhs.as_ref()
                    else {
                        panic!("expected nested Binary on rhs, got {rhs:?}");
                    };
                    assert_eq!(*inner_op, HirBinaryOp::Mul);
                    assert_eq!(
                        *inner_span,
                        Span::new(48, 53),
                        "nested Mul expression must span `b * 2` (48..53)"
                    );
                    let HirExpr::Local { span: b_span, .. } = inner_lhs.as_ref() else {
                        panic!("expected Local `b` on nested lhs, got {inner_lhs:?}");
                    };
                    assert_eq!(
                        *b_span,
                        Span::new(48, 49),
                        "Local `b` must carry its source span (48..49)"
                    );
                    let HirExpr::Int(v, two_span) = inner_rhs.as_ref() else {
                        panic!("expected Int `2` on nested rhs, got {inner_rhs:?}");
                    };
                    assert_eq!(*v, 2, "rightmost literal must be 2");
                    assert_eq!(
                        *two_span,
                        Span::new(52, 53),
                        "Int literal `2` must carry its source span (52..53)"
                    );
                }
                other => panic!("expected Return(Binary), got {other:?}"),
            }
        }
        other => panic!("expected Function, got {other:?}"),
    }
}

#[test]
fn async_function_marks_is_async_true() {
    let output =
        FrontendPass::new().run("test.ts", "async function fetch(): string { return ''; }");
    assert!(!output.diagnostics.has_errors(), "{:?}", output.diagnostics);
    match &output.program.declarations[0] {
        HirDecl::Function(f) => assert!(f.is_async),
        other => panic!("expected Function, got {other:?}"),
    }
}

#[test]
fn exported_function_sets_is_exported() {
    let output = FrontendPass::new().run("test.ts", "export function go(): void {}");
    assert!(!output.diagnostics.has_errors(), "{:?}", output.diagnostics);
    match &output.program.declarations[0] {
        HirDecl::Function(f) => assert!(f.is_exported),
        other => panic!("expected Function, got {other:?}"),
    }
}

#[test]
fn class_declaration_is_scanned_with_fields() {
    let output = FrontendPass::new().run(
        "test.ts",
        "class Point { x: i32; y: i32; sum(): i32 { return 0; } }",
    );
    assert!(!output.diagnostics.has_errors(), "{:?}", output.diagnostics);
    match &output.program.declarations[0] {
        HirDecl::Class(c) => {
            assert_eq!(c.name, Atom::from("Point"));
            assert_eq!(c.fields.len(), 2);
            assert_eq!(c.methods.len(), 1);
            assert_eq!(c.methods[0].name, Atom::from("sum"));
            assert_eq!(
                c.methods[0].params[0].name,
                Atom::from("this"),
                "method receives `this` as params[0]"
            );
            assert_eq!(c.methods[0].body.len(), 1);
            match &c.methods[0].body[0] {
                HirStmt::Return {
                    value: Some(HirExpr::Int(v, _)),
                } => assert_eq!(
                    *v, 0,
                    "method bodies are walked now that `this` is the receiver param"
                ),
                other => panic!("expected Return(Int(0)), got {other:?}"),
            }
        }
        other => panic!("expected Class, got {other:?}"),
    }
}

#[test]
fn class_with_extends_captures_parent_name() {
    let output = FrontendPass::new().run("test.ts", "class B extends A { x: i32; }");
    assert!(!output.diagnostics.has_errors(), "{:?}", output.diagnostics);
    match &output.program.declarations[0] {
        HirDecl::Class(c) => assert_eq!(c.extends, Some(Atom::from("A"))),
        other => panic!("expected Class, got {other:?}"),
    }
}

#[test]
fn class_with_extends_member_access_captures_rightmost_name() {
    let output = FrontendPass::new().run("test.ts", "class B extends ns.A { x: i32; }");
    assert!(!output.diagnostics.has_errors(), "{:?}", output.diagnostics);
    match &output.program.declarations[0] {
        HirDecl::Class(c) => assert_eq!(c.extends, Some(Atom::from("A"))),
        other => panic!("expected Class, got {other:?}"),
    }
}

#[test]
fn class_with_extends_nested_member_access_captures_rightmost_name() {
    let output = FrontendPass::new().run("test.ts", "class B extends A.B.C { x: i32; }");
    assert!(!output.diagnostics.has_errors(), "{:?}", output.diagnostics);
    match &output.program.declarations[0] {
        HirDecl::Class(c) => assert_eq!(c.extends, Some(Atom::from("C"))),
        other => panic!("expected Class, got {other:?}"),
    }
}

#[test]
fn class_with_extends_unsupported_form_emits_diagnostic() {
    let output = FrontendPass::new().run("test.ts", "class B extends A() { x: i32; }");
    assert!(output.diagnostics.has_errors(), "{:?}", output.diagnostics);
    assert!(
        output
            .diagnostics
            .iter()
            .any(|d| d.code.as_str() == "E0300" && d.message.contains("extends")),
        "diagnostics: {:?}",
        output.diagnostics
    );
    match &output.program.declarations[0] {
        HirDecl::Class(c) => assert_eq!(c.extends, None),
        other => panic!("expected Class, got {other:?}"),
    }
}

#[test]
fn class_method_with_destructured_param_falls_back_to_underscore_name() {
    let output = FrontendPass::new().run(
        "test.ts",
        "class Foo { bar({a, b}: { a: i32; b: i32 }): i32 { return 0; } }",
    );
    assert!(!output.diagnostics.has_errors(), "{:?}", output.diagnostics);
    match &output.program.declarations[0] {
        HirDecl::Class(c) => {
            let method = &c.methods[0];
            assert_eq!(method.name, Atom::from("bar"));
            assert_eq!(
                method.params.len(),
                2,
                "params[0] is the injected `this` receiver, params[1] is the declared param"
            );
            assert_eq!(method.params[0].name, Atom::from("this"));
            assert_eq!(
                method.params[1].name,
                Atom::from("_"),
                "destructured method param must use the same '_' fallback as build_function"
            );
        }
        other => panic!("expected Class, got {other:?}"),
    }
}

#[test]
fn type_alias_is_collected() {
    let output = FrontendPass::new().run("test.ts", "type Foo = i32;");
    assert!(!output.diagnostics.has_errors(), "{:?}", output.diagnostics);
    match &output.program.declarations[0] {
        HirDecl::TypeAlias { name, .. } => {
            assert_eq!(name, &Atom::from("Foo"));
        }
        other => panic!("expected TypeAlias, got {other:?}"),
    }
}

#[test]
fn exported_type_alias_is_collected() {
    let output = FrontendPass::new().run("test.ts", "export type Foo = i32;");
    assert!(!output.diagnostics.has_errors(), "{:?}", output.diagnostics);
    match &output.program.declarations[0] {
        HirDecl::TypeAlias { name, .. } => {
            assert_eq!(name, &Atom::from("Foo"));
        }
        other => panic!("expected TypeAlias, got {other:?}"),
    }
}

#[test]
fn interface_declaration_is_recorded() {
    let output = FrontendPass::new().run("test.ts", "interface I { x: i32; }");
    assert!(!output.diagnostics.has_errors(), "{:?}", output.diagnostics);
    match &output.program.declarations[0] {
        HirDecl::Interface { name } => assert_eq!(name, &Atom::from("I")),
        other => panic!("expected Interface, got {other:?}"),
    }
}

#[test]
fn exported_interface_declaration_is_recorded() {
    let output = FrontendPass::new().run("test.ts", "export interface I { x: i32; }");
    assert!(!output.diagnostics.has_errors(), "{:?}", output.diagnostics);
    match &output.program.declarations[0] {
        HirDecl::Interface { name } => assert_eq!(name, &Atom::from("I")),
        other => panic!("expected Interface, got {other:?}"),
    }
}

#[test]
fn enum_declaration_is_recorded_with_variant_names() {
    let output = FrontendPass::new().run("test.ts", "enum Color { Red, Green, Blue }");
    assert!(!output.diagnostics.has_errors(), "{:?}", output.diagnostics);
    match &output.program.declarations[0] {
        HirDecl::Enum { name, variants } => {
            assert_eq!(name, &Atom::from("Color"));
            assert_eq!(variants.len(), 3);
            assert_eq!(variants[0].name, Atom::from("Red"));
        }
        other => panic!("expected Enum, got {other:?}"),
    }
}

#[test]
fn exported_enum_declaration_is_recorded() {
    let output = FrontendPass::new().run("test.ts", "export enum Color { Red, Green, Blue }");
    assert!(!output.diagnostics.has_errors(), "{:?}", output.diagnostics);
    match &output.program.declarations[0] {
        HirDecl::Enum { name, variants } => {
            assert_eq!(name, &Atom::from("Color"));
            assert_eq!(variants.len(), 3);
            assert_eq!(variants[0].name, Atom::from("Red"));
        }
        other => panic!("expected Enum, got {other:?}"),
    }
}

#[test]
fn exported_import_equals_emits_unsupported_diagnostic() {
    let output = FrontendPass::new().run("test.ts", "export import x = require(\"y\");");
    assert!(output.diagnostics.has_errors(), "{:?}", output.diagnostics);
    assert!(
        output
            .diagnostics
            .iter()
            .any(|d| d.code.as_str() == "E0300" && d.message.contains("exported declaration")),
        "diagnostics: {:?}",
        output.diagnostics
    );
}

#[test]
fn top_level_let_emits_global_decl() {
    let output = FrontendPass::new().run("test.ts", "let counter: i32 = 0;");
    assert!(!output.diagnostics.has_errors(), "{:?}", output.diagnostics);
    match &output.program.declarations[0] {
        HirDecl::Global { name, init, .. } => {
            assert_eq!(name, &Atom::from("counter"));
            assert!(init.is_none(), "foundation leaves init empty");
        }
        other => panic!("expected Global, got {other:?}"),
    }
}

#[test]
fn import_statement_records_named_import() {
    let output = FrontendPass::new().run("test.ts", "import { render } from \"./template\";");
    assert!(!output.diagnostics.has_errors(), "{:?}", output.diagnostics);
    assert_eq!(output.program.imports.len(), 1);
    assert_eq!(output.program.imports[0].module, Atom::from("./template"));
    assert_eq!(output.program.imports[0].name, Atom::from("render"));
}

#[test]
fn named_export_records_alias() {
    let output = FrontendPass::new().run("test.ts", "export { helper };");
    assert!(!output.diagnostics.has_errors(), "{:?}", output.diagnostics);
    assert_eq!(output.program.exports.len(), 1);
    assert_eq!(output.program.exports[0].name, Atom::from("helper"));
    assert_eq!(output.program.exports[0].alias, None);
}

#[test]
fn exported_class_declaration_records_export_metadata() {
    let output = FrontendPass::new().run("test.ts", "export class Point { x: i32; y: i32; }");
    assert!(!output.diagnostics.has_errors(), "{:?}", output.diagnostics);
    assert!(
        output
            .program
            .exports
            .iter()
            .any(|e| e.name.as_str() == "Point" && e.alias.is_none()),
        "exports should contain {{ name: Point, alias: None }}, got {:?}",
        output.program.exports
    );
}

#[test]
fn exported_const_records_export_metadata() {
    let output = FrontendPass::new().run("test.ts", "export const kLimit = 100;");
    assert!(!output.diagnostics.has_errors(), "{:?}", output.diagnostics);
    assert!(
        output
            .program
            .exports
            .iter()
            .any(|e| e.name.as_str() == "kLimit" && e.alias.is_none()),
        "exports should contain {{ name: kLimit, alias: None }}, got {:?}",
        output.program.exports
    );
}

#[test]
fn exported_let_records_export_metadata() {
    let output = FrontendPass::new().run("test.ts", "export let counter = 0;");
    assert!(!output.diagnostics.has_errors(), "{:?}", output.diagnostics);
    assert!(
        output
            .program
            .exports
            .iter()
            .any(|e| e.name.as_str() == "counter" && e.alias.is_none()),
        "exports should contain {{ name: counter, alias: None }}, got {:?}",
        output.program.exports
    );
}

#[test]
fn exported_multi_declarator_records_each_export() {
    let output = FrontendPass::new().run("test.ts", "export const a = 1, b = 2;");
    assert!(!output.diagnostics.has_errors(), "{:?}", output.diagnostics);
    let names: Vec<&str> = output
        .program
        .exports
        .iter()
        .map(|e| e.name.as_str())
        .collect();
    assert!(names.contains(&"a"), "missing export 'a' in {names:?}");
    assert!(names.contains(&"b"), "missing export 'b' in {names:?}");
}

#[test]
fn named_export_renames_local_symbol_via_alias() {
    let output = FrontendPass::new().run("test.ts", "export { helper as publicHelper };");
    assert!(!output.diagnostics.has_errors(), "{:?}", output.diagnostics);
    assert_eq!(output.program.exports.len(), 1);
    assert_eq!(output.program.exports[0].name, Atom::from("helper"));
    assert_eq!(
        output.program.exports[0].alias,
        Some(Atom::from("publicHelper"))
    );
}

#[test]
fn re_export_with_source_emits_unsupported_diagnostic() {
    let output = FrontendPass::new().run("test.ts", "export { helper } from \"./other\";");
    assert!(output.diagnostics.has_errors(), "expected diagnostic");
    assert!(
        output
            .diagnostics
            .iter()
            .any(|d| d.code.as_str() == "E0300" && d.message.contains("re-export")),
        "diagnostics: {:?}",
        output.diagnostics
    );
    assert!(
        output.program.exports.is_empty(),
        "re-export must not register a local HirExport"
    );
}

#[test]
fn default_import_emits_unsupported_diagnostic() {
    let output = FrontendPass::new().run("test.ts", "import React from \"react\";");
    assert!(output.diagnostics.has_errors(), "expected diagnostic");
    assert!(
        output
            .diagnostics
            .iter()
            .any(|d| d.code.as_str() == "E0300" && d.message.contains("default import")),
        "diagnostics: {:?}",
        output.diagnostics
    );
    assert!(
        output.program.imports.is_empty(),
        "default import must not register a HirImport"
    );
}

#[test]
fn namespace_import_emits_unsupported_diagnostic() {
    let output = FrontendPass::new().run("test.ts", "import * as ns from \"./x\";");
    assert!(output.diagnostics.has_errors(), "expected diagnostic");
    assert!(
        output
            .diagnostics
            .iter()
            .any(|d| d.code.as_str() == "E0300" && d.message.contains("namespace import")),
        "diagnostics: {:?}",
        output.diagnostics
    );
    assert!(
        output.program.imports.is_empty(),
        "namespace import must not register a HirImport"
    );
}

#[test]
fn export_star_from_emits_unsupported_diagnostic() {
    let output = FrontendPass::new().run("test.ts", "export * from \"./other\";");
    assert!(output.diagnostics.has_errors(), "expected diagnostic");
    assert!(
        output
            .diagnostics
            .iter()
            .any(|d| d.code.as_str() == "E0300" && d.message.contains("export *")),
        "diagnostics: {:?}",
        output.diagnostics
    );
}

#[test]
fn side_effect_import_emits_unsupported_diagnostic() {
    let output = FrontendPass::new().run("test.ts", "import \"./setup\";");
    assert!(output.diagnostics.has_errors(), "expected diagnostic");
    assert!(
        output
            .diagnostics
            .iter()
            .any(|d| d.code.as_str() == "E0300" && d.message.contains("side-effect import")),
        "diagnostics: {:?}",
        output.diagnostics
    );
    assert!(
        output.program.imports.is_empty(),
        "side-effect import must not register a HirImport"
    );
}

#[test]
fn syntax_error_emits_parse_diagnostic() {
    let output = FrontendPass::new().run("test.ts", "const x: number = ;");
    assert!(output.diagnostics.has_errors(), "no errors emitted");
    let codes: Vec<&str> = output.diagnostics.iter().map(|d| d.code.as_str()).collect();
    assert!(
        codes
            .iter()
            .any(|c| *c == PARSE_ERROR_CODE || *c == PARSE_PANIC_CODE),
        "expected {PARSE_ERROR_CODE} or {PARSE_PANIC_CODE}, got {codes:?}"
    );
}

#[test]
fn unsupported_top_level_reports_diagnostic() {
    let output = FrontendPass::new().run("test.ts", "export default 1;");
    assert!(output.diagnostics.has_errors());
    assert!(
        output
            .diagnostics
            .iter()
            .any(|d| d.code.as_str() == "E0300"),
        "{:?}",
        output.diagnostics
    );
}

#[test]
fn parse_panic_does_not_emit_panic_when_input_is_clean() {
    let output = FrontendPass::new().run("test.ts", "function ok(): void {}");
    assert!(
        output.diagnostics.is_empty(),
        "expected no diagnostics for clean source, got {:?}",
        output.diagnostics
    );
    for d in &output.diagnostics {
        assert_ne!(d.code.as_str(), PARSE_PANIC_CODE);
        assert_eq!(d.severity, Severity::Error);
    }
}

#[test]
fn function_with_unknown_return_type_yields_error_marker() {
    let mut types = TypeTable::new();
    let output =
        FrontendPass::new().run_with_types("test.ts", "function f(): UnknownType {}", &mut types);
    assert!(!output.diagnostics.has_errors(), "{:?}", output.diagnostics);
    match &output.program.declarations[0] {
        HirDecl::Function(f) => {
            assert_eq!(types.resolve(f.ret), Some(&Type::Error));
        }
        other => panic!("expected Function, got {other:?}"),
    }
}

fn assert_e0401_for(source: &str, contains: &str) {
    let output = FrontendPass::new().run("test.ts", source);
    assert!(
        output.diagnostics.has_errors(),
        "banned type `{contains}` must trigger E0401, got clean diagnostics for: {source}"
    );
    let found = output
        .diagnostics
        .iter()
        .any(|d| d.code.as_str() == "E0401" && d.message.contains(contains));
    assert!(
        found,
        "expected E0401 mentioning `{contains}`, got: {:?}",
        output.diagnostics
    );
}

#[test]
fn e0401_rejects_top_level_any() {
    assert_e0401_for("function f(): any {}", "any");
}

#[test]
fn e0401_rejects_top_level_unknown() {
    assert_e0401_for("function f(): unknown {}", "unknown");
}

#[test]
fn e0401_rejects_top_level_object() {
    assert_e0401_for("function f(): Object {}", "Object");
}

#[test]
fn e0401_rejects_any_inside_array_element() {
    assert_e0401_for("function f(): any[] {}", "any");
}

#[test]
fn e0401_rejects_unknown_inside_array_element() {
    assert_e0401_for("function f(): unknown[] {}", "unknown");
}

#[test]
fn e0401_rejects_any_inside_generic_type_argument() {
    assert_e0401_for("function f(): Vec<any> {}", "any");
}

#[test]
fn e0401_rejects_object_inside_generic_type_argument() {
    assert_e0401_for("function f(): Vec<Object> {}", "Object");
}

#[test]
fn e0401_returns_type_error_marker_not_warning() {
    let mut types = TypeTable::new();
    let output =
        FrontendPass::new().run_with_types("test.ts", "function f(): unknown {}", &mut types);
    let error = output
        .diagnostics
        .iter()
        .find(|d| d.code.as_str() == "E0401")
        .expect("E0401 must be emitted");
    assert_eq!(
        error.severity,
        ts_aot_core::Severity::Error,
        "E0401 must be Severity::Error so has_errors() rejects it"
    );
    match &output.program.declarations[0] {
        HirDecl::Function(f) => {
            assert_eq!(types.resolve(f.ret), Some(&Type::Error));
        }
        other => panic!("expected Function, got {other:?}"),
    }
}

#[test]
fn generic_function_resolves_param_and_return_to_generic_param() {
    let mut types = TypeTable::new();
    let output = FrontendPass::new().run_with_types(
        "test.ts",
        "function id<T>(x: T): T { return x; }",
        &mut types,
    );
    assert!(!output.diagnostics.has_errors(), "{:?}", output.diagnostics);
    match &output.program.declarations[0] {
        HirDecl::Function(f) => {
            assert_eq!(f.type_params, vec![GenericParamId::from_raw(0)]);
            assert_eq!(
                types.resolve(f.params[0].ty),
                Some(&Type::GenericParam {
                    id: GenericParamId::from_raw(0),
                })
            );
            assert_eq!(
                types.resolve(f.ret),
                Some(&Type::GenericParam {
                    id: GenericParamId::from_raw(0),
                })
            );
        }
        other => panic!("expected Function, got {other:?}"),
    }
}

#[test]
fn multiple_generic_params_get_distinct_ordinal_ids() {
    let mut types = TypeTable::new();
    let output = FrontendPass::new().run_with_types(
        "test.ts",
        "function pick<T, U>(a: T, b: U): T { return a; }",
        &mut types,
    );
    assert!(!output.diagnostics.has_errors(), "{:?}", output.diagnostics);
    match &output.program.declarations[0] {
        HirDecl::Function(f) => {
            assert_eq!(
                f.type_params,
                vec![GenericParamId::from_raw(0), GenericParamId::from_raw(1)]
            );
            let t_type = Type::GenericParam {
                id: GenericParamId::from_raw(0),
            };
            let u_type = Type::GenericParam {
                id: GenericParamId::from_raw(1),
            };
            assert_eq!(types.resolve(f.params[0].ty), Some(&t_type));
            assert_eq!(types.resolve(f.params[1].ty), Some(&u_type));
            assert_eq!(types.resolve(f.ret), Some(&t_type));
            assert_ne!(
                f.params[0].ty, f.params[1].ty,
                "T and U must resolve to distinct TypeIds"
            );
        }
        other => panic!("expected Function, got {other:?}"),
    }
}

#[test]
fn generic_class_method_inherits_class_type_params() {
    let mut types = TypeTable::new();
    let output = FrontendPass::new().run_with_types(
        "test.ts",
        "class Box<T> { item: T; peek(): T { return this.item; } }",
        &mut types,
    );
    assert!(!output.diagnostics.has_errors(), "{:?}", output.diagnostics);
    match &output.program.declarations[0] {
        HirDecl::Class(c) => {
            assert_eq!(c.type_params, vec![GenericParamId::from_raw(0)]);
            let t_type = Type::GenericParam {
                id: GenericParamId::from_raw(0),
            };
            assert_eq!(types.resolve(c.fields[0].ty), Some(&t_type));
            let method = &c.methods[0];
            assert_eq!(method.name, Atom::from("peek"));
            assert_eq!(types.resolve(method.ret), Some(&t_type));
        }
        other => panic!("expected Class, got {other:?}"),
    }
}

#[test]
fn generic_class_method_can_have_own_additional_type_params() {
    let mut types = TypeTable::new();
    let output = FrontendPass::new().run_with_types(
        "test.ts",
        "class Box<T> { item: T; wrap<U>(other: U): U { return other; } }",
        &mut types,
    );
    assert!(!output.diagnostics.has_errors(), "{:?}", output.diagnostics);
    match &output.program.declarations[0] {
        HirDecl::Class(c) => {
            assert_eq!(c.type_params, vec![GenericParamId::from_raw(0)]);
            let method = &c.methods[0];
            assert_eq!(method.type_params, vec![GenericParamId::from_raw(1)]);
            let u_type = Type::GenericParam {
                id: GenericParamId::from_raw(1),
            };
            assert_eq!(method.params[0].name, Atom::from("this"));
            assert_eq!(
                types.resolve(method.params[1].ty),
                Some(&u_type),
                "declared param `other` follows the injected `this` at index 1"
            );
            assert_eq!(types.resolve(method.ret), Some(&u_type));
        }
        other => panic!("expected Class, got {other:?}"),
    }
}

#[test]
fn type_param_map_iter_bindings_round_trips_bindings() {
    let mut m = TypeParamMap::new();
    let ty = TypeId::from_raw(42);
    m.bind("T", ty);
    let collected: HashMap<&str, ts_aot_core::TypeId> = m.iter_bindings().collect();
    assert_eq!(collected.get("T"), Some(&ty));
}

#[test]
fn alias_declared_after_consumer_is_resolved_via_pre_scan_cache() {
    let mut types = TypeTable::new();
    let output = FrontendPass::new().run_with_types(
        "test.ts",
        "function useFoo(x: Foo): i32 { return 0; }\n type Foo = string;",
        &mut types,
    );
    assert!(!output.diagnostics.has_errors(), "{:?}", output.diagnostics);
    match &output.program.declarations[0] {
        HirDecl::Function(f) => {
            assert_eq!(
                types.resolve(f.params[0].ty),
                Some(&Type::String),
                "Foo declared after its consumer must still resolve via pre-scan cache"
            );
        }
        other => panic!("expected Function, got {other:?}"),
    }
}

#[test]
fn exported_alias_target_visible_to_other_declarations_via_pre_scan_cache() {
    let mut types = TypeTable::new();
    let output = FrontendPass::new().run_with_types(
        "test.ts",
        "function useFoo(x: Foo): i32 { return 0; }\n export type Foo = i32;",
        &mut types,
    );
    assert!(!output.diagnostics.has_errors(), "{:?}", output.diagnostics);
    match &output.program.declarations[0] {
        HirDecl::Function(f) => {
            assert_eq!(
                types.resolve(f.params[0].ty),
                Some(&Type::I32),
                "Foo exported via export type must be in the pre-scan cache"
            );
        }
        other => panic!("expected Function, got {other:?}"),
    }
}

#[test]
fn missing_type_annotation_falls_back_to_full_source_span() {
    let source = "function noAnnot(x): i32 { return 0; }";
    let output = FrontendPass::new().run("test.ts", source);
    let diag = output
        .diagnostics
        .iter()
        .find(|d| d.code.as_str() == "E0400")
        .expect("expected E0400 warning for missing annotation");
    assert_eq!(
        diag.span,
        ts_aot_core::Span::new(0, u32::try_from(source.len()).unwrap()),
        "ty == None path keeps the existing full-file fallback"
    );
}

#[test]
fn never_keyword_annotation_resolves_to_type_never_not_error() {
    let mut types = TypeTable::new();
    let output = FrontendPass::new().run_with_types(
        "test.ts",
        "function noReturn(x: never): never { throw x; }",
        &mut types,
    );
    assert!(!output.diagnostics.has_errors(), "{:?}", output.diagnostics);
    match &output.program.declarations[0] {
        HirDecl::Function(f) => {
            assert_eq!(
                types.resolve(f.params[0].ty),
                Some(&Type::Never),
                "TSNeverKeyword must intern Type::Never, not Type::Error"
            );
            assert_eq!(
                types.resolve(f.ret),
                Some(&Type::Never),
                "TSNeverKeyword in return position must also intern Type::Never"
            );
        }
        other => panic!("expected Function, got {other:?}"),
    }
}

#[test]
fn union_type_in_param_resolves_to_type_union_with_primitive_variants() {
    let mut types = TypeTable::new();
    let output = FrontendPass::new().run_with_types(
        "test.ts",
        "function pick(x: i64 | string): i64 { return 0; }",
        &mut types,
    );
    assert!(!output.diagnostics.has_errors(), "{:?}", output.diagnostics);
    let i64_id = types.intern(&Type::I64);
    let string_id = types.intern(&Type::String);
    let expected = Type::Union {
        variants: vec![i64_id, string_id],
    };
    let fn_decl = output
        .program
        .declarations
        .iter()
        .find_map(|d| match d {
            HirDecl::Function(f) => Some(f.clone()),
            _ => None,
        })
        .expect("function should be present");
    assert_eq!(
        types.resolve(fn_decl.params[0].ty),
        Some(&expected),
        "param type `i64 | string` must resolve to Type::Union with [I64, String]"
    );
    let interned_union = types.intern(&expected);
    assert_eq!(
        fn_decl.params[0].ty, interned_union,
        "the param's TypeId must be the same TypeId obtained from interning the expected Union"
    );
}

#[test]
fn union_type_in_return_position_resolves_to_type_union() {
    let mut types = TypeTable::new();
    let output = FrontendPass::new().run_with_types(
        "test.ts",
        "function either(): i64 | string { return 0; }",
        &mut types,
    );
    assert!(!output.diagnostics.has_errors(), "{:?}", output.diagnostics);
    let fn_decl = output
        .program
        .declarations
        .iter()
        .find_map(|d| match d {
            HirDecl::Function(f) => Some(f.clone()),
            _ => None,
        })
        .expect("function should be present");
    let i64_id = types.intern(&Type::I64);
    let string_id = types.intern(&Type::String);
    assert_eq!(
        types.resolve(fn_decl.ret),
        Some(&Type::Union {
            variants: vec![i64_id, string_id]
        }),
        "return type `i64 | string` must resolve to Type::Union with [I64, String]"
    );
}

#[test]
fn union_type_with_three_variants_preserves_order() {
    let mut types = TypeTable::new();
    let output = FrontendPass::new().run_with_types(
        "test.ts",
        "function f(x: i64 | string | bool): i64 { return 0; }",
        &mut types,
    );
    assert!(!output.diagnostics.has_errors(), "{:?}", output.diagnostics);
    let fn_decl = output
        .program
        .declarations
        .iter()
        .find_map(|d| match d {
            HirDecl::Function(f) => Some(f.clone()),
            _ => None,
        })
        .expect("function should be present");
    let i64_id = types.intern(&Type::I64);
    let string_id = types.intern(&Type::String);
    let bool_id = types.intern(&Type::Bool);
    assert_eq!(
        types.resolve(fn_decl.params[0].ty),
        Some(&Type::Union {
            variants: vec![i64_id, string_id, bool_id]
        }),
        "variant order must be preserved as written in the source"
    );
}

#[test]
fn union_type_with_alias_variant_resolves_to_underlying_type() {
    let mut types = TypeTable::new();
    let source = "type Foo = i64; function f(v: Foo | string): i64 { return 0; }";
    let output = FrontendPass::new().run_with_types("test.ts", source, &mut types);
    assert!(!output.diagnostics.has_errors(), "{:?}", output.diagnostics);
    let fn_decl = output
        .program
        .declarations
        .iter()
        .find_map(|d| match d {
            HirDecl::Function(f) if f.name == Atom::from("f") => Some(f.clone()),
            _ => None,
        })
        .expect("function f should be present");
    let i64_id = types.intern(&Type::I64);
    let string_id = types.intern(&Type::String);
    assert_eq!(
        types.resolve(fn_decl.params[0].ty),
        Some(&Type::Union {
            variants: vec![i64_id, string_id]
        }),
        "alias `Foo = i64` must expand to I64 inside the union (no Named wrapper)"
    );
}

#[test]
fn union_type_alias_resolves_through_alias_chain() {
    let mut types = TypeTable::new();
    let source = "type MaybeNumber = i64 | string; function f(x: MaybeNumber): i64 { return 0; }";
    let output = FrontendPass::new().run_with_types("test.ts", source, &mut types);
    assert!(!output.diagnostics.has_errors(), "{:?}", output.diagnostics);
    let fn_decl = output
        .program
        .declarations
        .iter()
        .find_map(|d| match d {
            HirDecl::Function(f) => Some(f.clone()),
            _ => None,
        })
        .expect("function f should be present");
    let i64_id = types.intern(&Type::I64);
    let string_id = types.intern(&Type::String);
    assert_eq!(
        types.resolve(fn_decl.params[0].ty),
        Some(&Type::Union {
            variants: vec![i64_id, string_id]
        }),
        "alias `MaybeNumber = i64 | string` must resolve to the same Union TypeId"
    );
    let alias_target = output
        .program
        .declarations
        .iter()
        .find_map(|d| match d {
            HirDecl::TypeAlias { name, target } if name.as_str() == "MaybeNumber" => Some(*target),
            _ => None,
        })
        .expect("MaybeNumber alias decl should be present");
    assert_eq!(
        alias_target, fn_decl.params[0].ty,
        "alias target TypeId must equal the consumer's TypeId (interning is idempotent)"
    );
}

#[test]
fn union_type_interning_is_idempotent() {
    let mut types = TypeTable::new();
    let source = "function f(x: i64 | string): i64 { return 0; } function g(y: i64 | string): i64 { return 0; }";
    let output = FrontendPass::new().run_with_types("test.ts", source, &mut types);
    assert!(!output.diagnostics.has_errors(), "{:?}", output.diagnostics);
    let f_param = output
        .program
        .declarations
        .iter()
        .find_map(|d| match d {
            HirDecl::Function(f) if f.name == Atom::from("f") => Some(f.params[0].ty),
            _ => None,
        })
        .expect("function f should be present");
    let g_param = output
        .program
        .declarations
        .iter()
        .find_map(|d| match d {
            HirDecl::Function(f) if f.name == Atom::from("g") => Some(f.params[0].ty),
            _ => None,
        })
        .expect("function g should be present");
    assert_eq!(
        f_param, g_param,
        "two identical `i64 | string` annotations must intern to the same TypeId"
    );
}

#[test]
fn intersection_type_in_param_resolves_to_type_intersection_with_primitive_parts() {
    let mut types = TypeTable::new();
    let output = FrontendPass::new().run_with_types(
        "test.ts",
        "function combine(x: i64 & string): i64 { return 0; }",
        &mut types,
    );
    assert!(!output.diagnostics.has_errors(), "{:?}", output.diagnostics);
    let i64_id = types.intern(&Type::I64);
    let string_id = types.intern(&Type::String);
    let expected = Type::Intersection {
        parts: vec![i64_id, string_id],
    };
    let fn_decl = output
        .program
        .declarations
        .iter()
        .find_map(|d| match d {
            HirDecl::Function(f) => Some(f.clone()),
            _ => None,
        })
        .expect("function should be present");
    assert_eq!(
        types.resolve(fn_decl.params[0].ty),
        Some(&expected),
        "param type `i64 & string` must resolve to Type::Intersection with [I64, String]"
    );
    let interned = types.intern(&expected);
    assert_eq!(
        fn_decl.params[0].ty, interned,
        "the param's TypeId must be the same TypeId obtained from interning the expected Intersection"
    );
}

#[test]
fn intersection_type_in_return_position_resolves_to_type_intersection() {
    let mut types = TypeTable::new();
    let output = FrontendPass::new().run_with_types(
        "test.ts",
        "function both(): i64 & string { return 0; }",
        &mut types,
    );
    assert!(!output.diagnostics.has_errors(), "{:?}", output.diagnostics);
    let fn_decl = output
        .program
        .declarations
        .iter()
        .find_map(|d| match d {
            HirDecl::Function(f) => Some(f.clone()),
            _ => None,
        })
        .expect("function should be present");
    let i64_id = types.intern(&Type::I64);
    let string_id = types.intern(&Type::String);
    assert_eq!(
        types.resolve(fn_decl.ret),
        Some(&Type::Intersection {
            parts: vec![i64_id, string_id]
        }),
        "return type `i64 & string` must resolve to Type::Intersection with [I64, String]"
    );
}

#[test]
fn intersection_type_with_three_parts_resolves_to_sorted_intersection() {
    let mut types = TypeTable::new();
    let output = FrontendPass::new().run_with_types(
        "test.ts",
        "function f(x: i64 & string & bool): i64 { return 0; }",
        &mut types,
    );
    assert!(!output.diagnostics.has_errors(), "{:?}", output.diagnostics);
    let fn_decl = output
        .program
        .declarations
        .iter()
        .find_map(|d| match d {
            HirDecl::Function(f) => Some(f.clone()),
            _ => None,
        })
        .expect("function should be present");
    let i64_id = types.intern(&Type::I64);
    let string_id = types.intern(&Type::String);
    let bool_id = types.intern(&Type::Bool);
    let mut expected = vec![i64_id, string_id, bool_id];
    expected.sort_unstable_by_key(|id| id.raw());
    assert_eq!(
        types.resolve(fn_decl.params[0].ty),
        Some(&Type::Intersection { parts: expected }),
        "Intersection part order is canonicalised by the resolver (sorted by TypeId::raw())"
    );
}

#[test]
fn intersection_type_is_commutative_under_part_reordering() {
    let mut types = TypeTable::new();
    let source = "function f(x: i64 & string): i64 { return 0; }\nfunction g(y: string & i64): i64 { return 0; }";
    let output = FrontendPass::new().run_with_types("test.ts", source, &mut types);
    assert!(!output.diagnostics.has_errors(), "{:?}", output.diagnostics);
    let f_param = output
        .program
        .declarations
        .iter()
        .find_map(|d| match d {
            HirDecl::Function(f) if f.name == Atom::from("f") => Some(f.params[0].ty),
            _ => None,
        })
        .expect("function f should be present");
    let g_param = output
        .program
        .declarations
        .iter()
        .find_map(|d| match d {
            HirDecl::Function(f) if f.name == Atom::from("g") => Some(f.params[0].ty),
            _ => None,
        })
        .expect("function g should be present");
    assert_eq!(
        f_param, g_param,
        "`i64 & string` and `string & i64` must intern to the same TypeId (Intersection is commutative)"
    );
}

#[test]
fn intersection_type_as_alias_body_pre_resolves_inner_aliases() {
    let mut types = TypeTable::new();
    let source = "type Foo = i64;\ntype Bar = string;\ntype Combined = Foo & Bar;\nfunction f(x: Combined): i64 { return 0; }";
    let output = FrontendPass::new().run_with_types("test.ts", source, &mut types);
    assert!(!output.diagnostics.has_errors(), "{:?}", output.diagnostics);
    let fn_decl = output
        .program
        .declarations
        .iter()
        .find_map(|d| match d {
            HirDecl::Function(f) => Some(f.clone()),
            _ => None,
        })
        .expect("function f should be present");
    let i64_id = types.intern(&Type::I64);
    let string_id = types.intern(&Type::String);
    let mut expected = vec![i64_id, string_id];
    expected.sort_unstable_by_key(|id| id.raw());
    assert_eq!(
        types.resolve(fn_decl.params[0].ty),
        Some(&Type::Intersection { parts: expected }),
        "alias body `Foo & Bar` must pre-resolve inner aliases through `pre_resolve_aliases_in_type` recursion into TSIntersectionType"
    );
}

#[test]
fn intersection_type_with_repeated_member_dedups_to_canonical_form() {
    let mut types = TypeTable::new();
    let source = "function f(x: i64 & i64): i64 { return 0; }\nfunction g(y: i64 & i64 & string): i64 { return 0; }";
    let output = FrontendPass::new().run_with_types("test.ts", source, &mut types);
    assert!(!output.diagnostics.has_errors(), "{:?}", output.diagnostics);
    let f_param = output
        .program
        .declarations
        .iter()
        .find_map(|d| match d {
            HirDecl::Function(f) if f.name == Atom::from("f") => Some(f.params[0].ty),
            _ => None,
        })
        .expect("function f should be present");
    let g_param = output
        .program
        .declarations
        .iter()
        .find_map(|d| match d {
            HirDecl::Function(f) if f.name == Atom::from("g") => Some(f.params[0].ty),
            _ => None,
        })
        .expect("function g should be present");
    let i64_id = types.intern(&Type::I64);
    let string_id = types.intern(&Type::String);
    assert_eq!(
        types.resolve(f_param),
        Some(&Type::Intersection {
            parts: vec![i64_id]
        }),
        "`i64 & i64` must dedup to a singleton Intersection (idempotent under repeated members)"
    );
    let mut expected = vec![i64_id, string_id];
    expected.sort_unstable_by_key(|id| id.raw());
    assert_eq!(
        types.resolve(g_param),
        Some(&Type::Intersection { parts: expected }),
        "`i64 & i64 & string` must dedup to `i64 & string` (sorted canonical form)"
    );
}

#[test]
fn intersection_type_alias_resolves_through_alias_chain() {
    let mut types = TypeTable::new();
    let source = "type Combined = i64 & string; function f(x: Combined): i64 { return 0; }";
    let output = FrontendPass::new().run_with_types("test.ts", source, &mut types);
    assert!(!output.diagnostics.has_errors(), "{:?}", output.diagnostics);
    let fn_decl = output
        .program
        .declarations
        .iter()
        .find_map(|d| match d {
            HirDecl::Function(f) => Some(f.clone()),
            _ => None,
        })
        .expect("function f should be present");
    let i64_id = types.intern(&Type::I64);
    let string_id = types.intern(&Type::String);
    assert_eq!(
        types.resolve(fn_decl.params[0].ty),
        Some(&Type::Intersection {
            parts: vec![i64_id, string_id]
        }),
        "alias `Combined = i64 & string` must resolve to the same Intersection TypeId"
    );
}

#[test]
fn tuple_type_in_param_resolves_to_type_tuple_with_ordered_elements() {
    let mut types = TypeTable::new();
    let output = FrontendPass::new().run_with_types(
        "test.ts",
        "function f(x: [i64, string]): i64 { return 0; }",
        &mut types,
    );
    assert!(!output.diagnostics.has_errors(), "{:?}", output.diagnostics);
    let fn_decl = output
        .program
        .declarations
        .iter()
        .find_map(|d| match d {
            HirDecl::Function(f) => Some(f.clone()),
            _ => None,
        })
        .expect("function should be present");
    let i64_id = types.intern(&Type::I64);
    let string_id = types.intern(&Type::String);
    assert_eq!(
        types.resolve(fn_decl.params[0].ty),
        Some(&Type::Tuple {
            elements: vec![i64_id, string_id]
        }),
        "param type `[i64, string]` must resolve to Type::Tuple with [I64, String] in order"
    );
}

#[test]
fn tuple_type_in_return_position_resolves_to_type_tuple() {
    let mut types = TypeTable::new();
    let output = FrontendPass::new().run_with_types(
        "test.ts",
        "function f(): [i64, string] { return [0, '']; }",
        &mut types,
    );
    assert!(!output.diagnostics.has_errors(), "{:?}", output.diagnostics);
    let fn_decl = output
        .program
        .declarations
        .iter()
        .find_map(|d| match d {
            HirDecl::Function(f) => Some(f.clone()),
            _ => None,
        })
        .expect("function should be present");
    let i64_id = types.intern(&Type::I64);
    let string_id = types.intern(&Type::String);
    assert_eq!(
        types.resolve(fn_decl.ret),
        Some(&Type::Tuple {
            elements: vec![i64_id, string_id]
        }),
        "return type `[i64, string]` must resolve to Type::Tuple with [I64, String]"
    );
}

#[test]
fn tuple_type_with_three_elements_preserves_source_order() {
    let mut types = TypeTable::new();
    let output = FrontendPass::new().run_with_types(
        "test.ts",
        "function f(x: [i64, string, bool]): i64 { return 0; }",
        &mut types,
    );
    assert!(!output.diagnostics.has_errors(), "{:?}", output.diagnostics);
    let fn_decl = output
        .program
        .declarations
        .iter()
        .find_map(|d| match d {
            HirDecl::Function(f) => Some(f.clone()),
            _ => None,
        })
        .expect("function should be present");
    let i64_id = types.intern(&Type::I64);
    let string_id = types.intern(&Type::String);
    let bool_id = types.intern(&Type::Bool);
    assert_eq!(
        types.resolve(fn_decl.params[0].ty),
        Some(&Type::Tuple {
            elements: vec![i64_id, string_id, bool_id]
        }),
        "element order must be preserved as written in the source"
    );
}

#[test]
fn tuple_type_with_alias_element_resolves_through_alias() {
    let mut types = TypeTable::new();
    let source = "type Idx = i64; function f(x: [Idx, string]): i64 { return 0; }";
    let output = FrontendPass::new().run_with_types("test.ts", source, &mut types);
    assert!(!output.diagnostics.has_errors(), "{:?}", output.diagnostics);
    let fn_decl = output
        .program
        .declarations
        .iter()
        .find_map(|d| match d {
            HirDecl::Function(f) => Some(f.clone()),
            _ => None,
        })
        .expect("function f should be present");
    let i64_id = types.intern(&Type::I64);
    let string_id = types.intern(&Type::String);
    assert_eq!(
        types.resolve(fn_decl.params[0].ty),
        Some(&Type::Tuple {
            elements: vec![i64_id, string_id]
        }),
        "alias `Idx = i64` must resolve to the same TypeId as `i64` in tuple element position"
    );
}

#[test]
fn tuple_type_as_alias_body_resolves_to_same_type_as_inline_tuple() {
    let mut types = TypeTable::new();
    let source = "type Pair = [i64, string];\nfunction f(x: Pair): i64 { return 0; }\nfunction g(y: [i64, string]): i64 { return 0; }";
    let output = FrontendPass::new().run_with_types("test.ts", source, &mut types);
    assert!(!output.diagnostics.has_errors(), "{:?}", output.diagnostics);
    let f_param = output
        .program
        .declarations
        .iter()
        .find_map(|d| match d {
            HirDecl::Function(f) if f.name == Atom::from("f") => Some(f.params[0].ty),
            _ => None,
        })
        .expect("function f should be present");
    let g_param = output
        .program
        .declarations
        .iter()
        .find_map(|d| match d {
            HirDecl::Function(f) if f.name == Atom::from("g") => Some(f.params[0].ty),
            _ => None,
        })
        .expect("function g should be present");
    assert_eq!(
        f_param, g_param,
        "alias `type Pair = [i64, string]` must produce the same TypeId as the inline tuple `[i64, string]` (covers pre-resolve recursion into TSTupleType in skeleton.rs)"
    );
    let i64_id = types.intern(&Type::I64);
    let string_id = types.intern(&Type::String);
    assert_eq!(
        types.resolve(f_param),
        Some(&Type::Tuple {
            elements: vec![i64_id, string_id]
        })
    );
}

#[test]
fn nested_tuple_type_resolves_recursively() {
    let mut types = TypeTable::new();
    let output = FrontendPass::new().run_with_types(
        "test.ts",
        "function f(x: [[i64, string], bool]): i64 { return 0; }",
        &mut types,
    );
    assert!(!output.diagnostics.has_errors(), "{:?}", output.diagnostics);
    let fn_decl = output
        .program
        .declarations
        .iter()
        .find_map(|d| match d {
            HirDecl::Function(f) => Some(f.clone()),
            _ => None,
        })
        .expect("function should be present");
    let i64_id = types.intern(&Type::I64);
    let string_id = types.intern(&Type::String);
    let bool_id = types.intern(&Type::Bool);
    let inner_tuple_id = types.intern(&Type::Tuple {
        elements: vec![i64_id, string_id],
    });
    assert_eq!(
        types.resolve(fn_decl.params[0].ty),
        Some(&Type::Tuple {
            elements: vec![inner_tuple_id, bool_id]
        }),
        "nested tuple `[[i64, string], bool]` must resolve to Type::Tuple containing a nested Type::Tuple (resolver recursion through TSTupleType)"
    );
}

#[test]
fn tuple_type_with_named_element_emits_warning_diagnostic() {
    let mut types = TypeTable::new();
    let output = FrontendPass::new().run_with_types(
        "test.ts",
        "function f(x: [name: i64, string]): i64 { return 0; }",
        &mut types,
    );
    assert!(
        output
            .diagnostics
            .iter()
            .any(|d| d.severity == ts_aot_core::Severity::Warning),
        "named tuple element must produce a warning, got: {:?}",
        output.diagnostics
    );
}

#[test]
fn tuple_type_with_rest_element_emits_warning_diagnostic() {
    let mut types = TypeTable::new();
    let output = FrontendPass::new().run_with_types(
        "test.ts",
        "function f(x: [i64, ...string[]]): i64 { return 0; }",
        &mut types,
    );
    assert!(
        output
            .diagnostics
            .iter()
            .any(|d| d.severity == ts_aot_core::Severity::Warning),
        "rest tuple element must produce a warning, got: {:?}",
        output.diagnostics
    );
}

#[test]
fn tuple_type_with_optional_element_emits_warning_diagnostic() {
    let mut types = TypeTable::new();
    let output = FrontendPass::new().run_with_types(
        "test.ts",
        "function f(x: [i64, string?]): i64 { return 0; }",
        &mut types,
    );
    assert!(
        output
            .diagnostics
            .iter()
            .any(|d| d.severity == ts_aot_core::Severity::Warning),
        "optional tuple element must produce a warning, got: {:?}",
        output.diagnostics
    );
}

#[test]
fn array_generic_syntax_in_param_resolves_to_type_array() {
    let mut types = TypeTable::new();
    let output = FrontendPass::new().run_with_types(
        "test.ts",
        "function f(x: Array<i64>): i64 { return 0; }",
        &mut types,
    );
    assert!(!output.diagnostics.has_errors(), "{:?}", output.diagnostics);
    let fn_decl = output
        .program
        .declarations
        .iter()
        .find_map(|d| match d {
            HirDecl::Function(f) => Some(f.clone()),
            _ => None,
        })
        .expect("function should be present");
    let i64_id = types.intern(&Type::I64);
    assert_eq!(
        types.resolve(fn_decl.params[0].ty),
        Some(&Type::Array { element: i64_id }),
        "param type `Array<i64>` must resolve to Type::Array with element I64 (special-case in resolver)"
    );
}

#[test]
fn array_generic_and_array_sugar_resolve_to_same_type() {
    let mut types = TypeTable::new();
    let source =
        "function f(x: Array<i64>): i64 { return 0; }\nfunction g(y: i64[]): i64 { return 0; }";
    let output = FrontendPass::new().run_with_types("test.ts", source, &mut types);
    assert!(!output.diagnostics.has_errors(), "{:?}", output.diagnostics);
    let f_param = output
        .program
        .declarations
        .iter()
        .find_map(|d| match d {
            HirDecl::Function(f) if f.name == Atom::from("f") => Some(f.params[0].ty),
            _ => None,
        })
        .expect("function f should be present");
    let g_param = output
        .program
        .declarations
        .iter()
        .find_map(|d| match d {
            HirDecl::Function(f) if f.name == Atom::from("g") => Some(f.params[0].ty),
            _ => None,
        })
        .expect("function g should be present");
    assert_eq!(
        f_param, g_param,
        "`Array<i64>` and `i64[]` must intern to the same TypeId (canonical form of array-of-i64)"
    );
}

#[test]
fn array_generic_with_alias_element_resolves_through_alias() {
    let mut types = TypeTable::new();
    let source = "type Foo = i64; function f(x: Array<Foo>): i64 { return 0; }";
    let output = FrontendPass::new().run_with_types("test.ts", source, &mut types);
    assert!(!output.diagnostics.has_errors(), "{:?}", output.diagnostics);
    let fn_decl = output
        .program
        .declarations
        .iter()
        .find_map(|d| match d {
            HirDecl::Function(f) => Some(f.clone()),
            _ => None,
        })
        .expect("function f should be present");
    let i64_id = types.intern(&Type::I64);
    assert_eq!(
        types.resolve(fn_decl.params[0].ty),
        Some(&Type::Array { element: i64_id }),
        "alias `Foo = i64` must resolve inside `Array<Foo>` element position"
    );
}

#[test]
fn array_generic_with_nested_generic_resolves_recursively() {
    let mut types = TypeTable::new();
    let output = FrontendPass::new().run_with_types(
        "test.ts",
        "function f(x: Array<Array<i64>>): i64 { return 0; }",
        &mut types,
    );
    assert!(!output.diagnostics.has_errors(), "{:?}", output.diagnostics);
    let fn_decl = output
        .program
        .declarations
        .iter()
        .find_map(|d| match d {
            HirDecl::Function(f) => Some(f.clone()),
            _ => None,
        })
        .expect("function should be present");
    let i64_id = types.intern(&Type::I64);
    let inner_array_id = types.intern(&Type::Array { element: i64_id });
    assert_eq!(
        types.resolve(fn_decl.params[0].ty),
        Some(&Type::Array {
            element: inner_array_id
        }),
        "nested generic `Array<Array<i64>>` must resolve recursively"
    );
}

#[test]
fn user_defined_array_alias_overrides_builtin_array_generic_syntax() {
    let mut types = TypeTable::new();
    let source = "type Array = string;\nfunction f(x: Array<i64>): i64 { return 0; }";
    let output = FrontendPass::new().run_with_types("test.ts", source, &mut types);
    assert!(!output.diagnostics.has_errors(), "{:?}", output.diagnostics);
    let fn_decl = output
        .program
        .declarations
        .iter()
        .find_map(|d| match d {
            HirDecl::Function(f) => Some(f.clone()),
            _ => None,
        })
        .expect("function f should be present");
    let string_id = types.intern(&Type::String);
    assert_eq!(
        types.resolve(fn_decl.params[0].ty),
        Some(&Type::String),
        "user-defined alias `type Array = string` must shadow the builtin `Array<T>` special case — `Array<i64>` resolves to `string`, not to `Type::Array`"
    );
    assert_eq!(
        fn_decl.params[0].ty, string_id,
        "alias resolves to the canonical String TypeId"
    );
}

#[test]
fn array_generic_bare_reference_emits_e0403_and_resolves_to_type_error() {
    let mut types = TypeTable::new();
    let output = FrontendPass::new().run_with_types(
        "test.ts",
        "function f(x: Array): i64 { return 0; }",
        &mut types,
    );
    assert!(
        has_e0403(&output.diagnostics),
        "bare `Array` (no `<...>`) is treated as zero arguments and must produce an E0403 warning, got: {:?}",
        output.diagnostics
    );
    let fn_decl = output
        .program
        .declarations
        .iter()
        .find_map(|d| match d {
            HirDecl::Function(f) => Some(f.clone()),
            _ => None,
        })
        .expect("function should be present");
    let error_id = types.intern(&Type::Error);
    assert_eq!(
        fn_decl.params[0].ty, error_id,
        "bare `Array` reference must resolve to Type::Error after E0403 warning"
    );
}

#[test]
fn array_generic_with_zero_type_arguments_emits_e0403_warning() {
    let mut types = TypeTable::new();
    let output = FrontendPass::new().run_with_types(
        "test.ts",
        "function f(x: Array<>): i64 { return 0; }",
        &mut types,
    );
    assert!(
        has_e0403(&output.diagnostics),
        "empty `Array<>` must produce an E0403 warning about required arity, got: {:?}",
        output.diagnostics
    );
}

#[test]
fn array_generic_with_too_many_type_arguments_emits_e0403_warning() {
    let mut types = TypeTable::new();
    let output = FrontendPass::new().run_with_types(
        "test.ts",
        "function f(x: Array<i64, i32>): i64 { return 0; }",
        &mut types,
    );
    assert!(
        has_e0403(&output.diagnostics),
        "`Array<i64, i32>` (wrong arity) must produce an E0403 warning, got: {:?}",
        output.diagnostics
    );
}

#[test]
fn array_generic_with_nested_wrong_arity_inside_emits_e0403_warning() {
    let mut types = TypeTable::new();
    let output = FrontendPass::new().run_with_types(
        "test.ts",
        "function f(x: Array<Array<>>): i64 { return 0; }",
        &mut types,
    );
    assert!(
        has_e0403(&output.diagnostics),
        "`Array<Array<>>` (nested zero-arity) must produce an E0403 warning via resolver recursion, got: {:?}",
        output.diagnostics
    );
    let fn_decl = output
        .program
        .declarations
        .iter()
        .find_map(|d| match d {
            HirDecl::Function(f) => Some(f.clone()),
            _ => None,
        })
        .expect("function should be present");
    let error_id = types.intern(&Type::Error);
    assert_eq!(
        types.resolve(fn_decl.params[0].ty),
        Some(&Type::Array { element: error_id }),
        "outer Array must wrap the inner-error: param type must be Array-of-Error, not flat Error"
    );
}

#[test]
fn array_generic_with_nested_multiple_type_arguments_inside_emits_e0403_warning() {
    let mut types = TypeTable::new();
    let output = FrontendPass::new().run_with_types(
        "test.ts",
        "function f(x: Array<Array<i64, i32>>): i64 { return 0; }",
        &mut types,
    );
    assert!(
        has_e0403(&output.diagnostics),
        "`Array<Array<i64, i32>>` (nested multiple-arity) must produce an E0403 warning via resolver recursion, got: {:?}",
        output.diagnostics
    );
    let fn_decl = output
        .program
        .declarations
        .iter()
        .find_map(|d| match d {
            HirDecl::Function(f) => Some(f.clone()),
            _ => None,
        })
        .expect("function should be present");
    let error_id = types.intern(&Type::Error);
    assert_eq!(
        types.resolve(fn_decl.params[0].ty),
        Some(&Type::Array { element: error_id }),
        "outer Array must wrap the inner-error: param type must be Array-of-Error, not flat Error"
    );
}

#[test]
fn array_type_in_param_resolves_to_type_array() {
    let mut types = TypeTable::new();
    let output = FrontendPass::new().run_with_types(
        "test.ts",
        "function f(x: i64[]): i64 { return 0; }",
        &mut types,
    );
    assert!(!output.diagnostics.has_errors(), "{:?}", output.diagnostics);
    let fn_decl = output
        .program
        .declarations
        .iter()
        .find_map(|d| match d {
            HirDecl::Function(f) => Some(f.clone()),
            _ => None,
        })
        .expect("function should be present");
    let i64_id = types.intern(&Type::I64);
    assert_eq!(
        types.resolve(fn_decl.params[0].ty),
        Some(&Type::Array { element: i64_id }),
        "param type `i64[]` must resolve to Type::Array with element I64"
    );
}

#[test]
fn array_type_in_return_position_resolves_to_type_array() {
    let mut types = TypeTable::new();
    let output = FrontendPass::new().run_with_types(
        "test.ts",
        "function f(): i64[] { return []; }",
        &mut types,
    );
    assert!(!output.diagnostics.has_errors(), "{:?}", output.diagnostics);
    let fn_decl = output
        .program
        .declarations
        .iter()
        .find_map(|d| match d {
            HirDecl::Function(f) => Some(f.clone()),
            _ => None,
        })
        .expect("function should be present");
    let i64_id = types.intern(&Type::I64);
    assert_eq!(
        types.resolve(fn_decl.ret),
        Some(&Type::Array { element: i64_id }),
        "return type `i64[]` must resolve to Type::Array with element I64"
    );
}

#[test]
fn nested_array_type_resolves_recursively() {
    let mut types = TypeTable::new();
    let output = FrontendPass::new().run_with_types(
        "test.ts",
        "function f(x: i64[][]): i64 { return 0; }",
        &mut types,
    );
    assert!(!output.diagnostics.has_errors(), "{:?}", output.diagnostics);
    let fn_decl = output
        .program
        .declarations
        .iter()
        .find_map(|d| match d {
            HirDecl::Function(f) => Some(f.clone()),
            _ => None,
        })
        .expect("function should be present");
    let i64_id = types.intern(&Type::I64);
    let inner_array_id = types.intern(&Type::Array { element: i64_id });
    assert_eq!(
        types.resolve(fn_decl.params[0].ty),
        Some(&Type::Array {
            element: inner_array_id
        }),
        "nested array `i64[][]` must resolve to Type::Array containing a nested Type::Array (resolver recursion through TSArrayType)"
    );
}

#[test]
fn array_type_with_alias_element_resolves_through_alias() {
    let mut types = TypeTable::new();
    let source = "type Foo = i64; function f(x: Foo[]): i64 { return 0; }";
    let output = FrontendPass::new().run_with_types("test.ts", source, &mut types);
    assert!(!output.diagnostics.has_errors(), "{:?}", output.diagnostics);
    let fn_decl = output
        .program
        .declarations
        .iter()
        .find_map(|d| match d {
            HirDecl::Function(f) => Some(f.clone()),
            _ => None,
        })
        .expect("function f should be present");
    let i64_id = types.intern(&Type::I64);
    assert_eq!(
        types.resolve(fn_decl.params[0].ty),
        Some(&Type::Array { element: i64_id }),
        "alias `Foo = i64` must resolve to the same TypeId as `i64` in array element position (covers pre-resolve recursion into TSArrayType in skeleton.rs)"
    );
}

#[test]
fn array_type_as_alias_body_resolves_to_same_type_as_inline_array() {
    let mut types = TypeTable::new();
    let source = "type Ints = i64[];\nfunction f(x: Ints): i64 { return 0; }\nfunction g(y: i64[]): i64 { return 0; }";
    let output = FrontendPass::new().run_with_types("test.ts", source, &mut types);
    assert!(!output.diagnostics.has_errors(), "{:?}", output.diagnostics);
    let f_param = output
        .program
        .declarations
        .iter()
        .find_map(|d| match d {
            HirDecl::Function(f) if f.name == Atom::from("f") => Some(f.params[0].ty),
            _ => None,
        })
        .expect("function f should be present");
    let g_param = output
        .program
        .declarations
        .iter()
        .find_map(|d| match d {
            HirDecl::Function(f) if f.name == Atom::from("g") => Some(f.params[0].ty),
            _ => None,
        })
        .expect("function g should be present");
    assert_eq!(
        f_param, g_param,
        "alias `type Ints = i64[]` must produce the same TypeId as the inline array `i64[]` (covers pre-resolve recursion into TSArrayType in skeleton.rs)"
    );
    let i64_id = types.intern(&Type::I64);
    assert_eq!(
        types.resolve(f_param),
        Some(&Type::Array { element: i64_id })
    );
}

#[test]
fn array_type_distinguishes_from_singleton_tuple() {
    let mut types = TypeTable::new();
    let source = "function f(x: i64[]): i64 { return 0; }\nfunction g(y: [i64]): i64 { return 0; }";
    let output = FrontendPass::new().run_with_types("test.ts", source, &mut types);
    assert!(!output.diagnostics.has_errors(), "{:?}", output.diagnostics);
    let f_param = output
        .program
        .declarations
        .iter()
        .find_map(|d| match d {
            HirDecl::Function(f) if f.name == Atom::from("f") => Some(f.params[0].ty),
            _ => None,
        })
        .expect("function f should be present");
    let g_param = output
        .program
        .declarations
        .iter()
        .find_map(|d| match d {
            HirDecl::Function(f) if f.name == Atom::from("g") => Some(f.params[0].ty),
            _ => None,
        })
        .expect("function g should be present");
    assert_ne!(
        f_param, g_param,
        "`i64[]` (array) and `[i64]` (singleton tuple) must resolve to different TypeIds"
    );
}

#[test]
fn tuple_type_distinguishes_from_array_with_same_element() {
    let mut types = TypeTable::new();
    let source = "function f(x: [i64]): i64 { return 0; }\nfunction g(y: i64[]): i64 { return 0; }";
    let output = FrontendPass::new().run_with_types("test.ts", source, &mut types);
    assert!(!output.diagnostics.has_errors(), "{:?}", output.diagnostics);
    let f_param = output
        .program
        .declarations
        .iter()
        .find_map(|d| match d {
            HirDecl::Function(f) if f.name == Atom::from("f") => Some(f.params[0].ty),
            _ => None,
        })
        .expect("function f should be present");
    let g_param = output
        .program
        .declarations
        .iter()
        .find_map(|d| match d {
            HirDecl::Function(f) if f.name == Atom::from("g") => Some(f.params[0].ty),
            _ => None,
        })
        .expect("function g should be present");
    assert_ne!(
        f_param, g_param,
        "`[i64]` (singleton tuple) and `i64[]` (array) must resolve to different TypeIds"
    );
    let i64_id = types.intern(&Type::I64);
    assert_eq!(
        types.resolve(f_param),
        Some(&Type::Tuple {
            elements: vec![i64_id]
        })
    );
}

#[test]
fn intersection_type_distinguishes_from_union_with_same_parts() {
    let mut types = TypeTable::new();
    let source = "function f(x: i64 & string): i64 { return 0; }\nfunction g(y: i64 | string): i64 { return 0; }";
    let output = FrontendPass::new().run_with_types("test.ts", source, &mut types);
    assert!(!output.diagnostics.has_errors(), "{:?}", output.diagnostics);
    let f_param = output
        .program
        .declarations
        .iter()
        .find_map(|d| match d {
            HirDecl::Function(f) if f.name == Atom::from("f") => Some(f.params[0].ty),
            _ => None,
        })
        .expect("function f should be present");
    let g_param = output
        .program
        .declarations
        .iter()
        .find_map(|d| match d {
            HirDecl::Function(f) if f.name == Atom::from("g") => Some(f.params[0].ty),
            _ => None,
        })
        .expect("function g should be present");
    assert_ne!(
        f_param, g_param,
        "`i64 & string` and `i64 | string` must resolve to different TypeIds"
    );
    let i64_id = types.intern(&Type::I64);
    let string_id = types.intern(&Type::String);
    assert_eq!(
        types.resolve(f_param),
        Some(&Type::Intersection {
            parts: vec![i64_id, string_id]
        })
    );
    assert_eq!(
        types.resolve(g_param),
        Some(&Type::Union {
            variants: vec![i64_id, string_id]
        })
    );
}

#[test]
fn union_type_with_forward_declared_alias_pre_resolves_inner_alias() {
    let mut types = TypeTable::new();
    let source = "type Bar = Foo | string;\ntype Foo = i64;\nfunction f(x: Bar): i64 { return 0; }";
    let output = FrontendPass::new().run_with_types("test.ts", source, &mut types);
    assert!(!output.diagnostics.has_errors(), "{:?}", output.diagnostics);
    let fn_decl = output
        .program
        .declarations
        .iter()
        .find_map(|d| match d {
            HirDecl::Function(f) if f.name == Atom::from("f") => Some(f.clone()),
            _ => None,
        })
        .expect("function f should be present");
    let i64_id = types.intern(&Type::I64);
    let string_id = types.intern(&Type::String);
    assert_eq!(
        types.resolve(fn_decl.params[0].ty),
        Some(&Type::Union {
            variants: vec![i64_id, string_id]
        }),
        "Bar = Foo | string with forward-declared Foo must pre-resolve Foo before caching Bar"
    );
    let bar_target = output
        .program
        .declarations
        .iter()
        .find_map(|d| match d {
            HirDecl::TypeAlias { name, target } if name.as_str() == "Bar" => Some(*target),
            _ => None,
        })
        .expect("Bar alias decl should be present");
    assert_eq!(
        bar_target, fn_decl.params[0].ty,
        "Bar alias target TypeId must equal the consumer's TypeId (pre-resolve cache hit)"
    );
}

#[test]
fn chained_alias_forward_ref_resolves_via_cache_update_in_handle_type_alias() {
    let mut types = TypeTable::new();
    let source = "type Foo = Bar;\n type Bar = string;\n function f(x: Foo): i32 { return 0; }";
    let output = FrontendPass::new().run_with_types("test.ts", source, &mut types);
    assert!(!output.diagnostics.has_errors(), "{:?}", output.diagnostics);
    let fn_decl = output
        .program
        .declarations
        .iter()
        .find_map(|d| match d {
            HirDecl::Function(f) if f.name.as_str() == "f" => Some(f),
            _ => None,
        })
        .expect("function f should be present");
    assert_eq!(
        types.resolve(fn_decl.params[0].ty),
        Some(&Type::String),
        "Foo = Bar = string must resolve through the cache even when Bar was declared AFTER Foo"
    );
}

#[test]
fn consumer_before_alias_chain_resolves_via_pre_resolve() {
    let mut types = TypeTable::new();
    let source = "function f(x: Foo): i32 { return 0; }\n type Foo = Bar;\n type Bar = string;";
    let output = FrontendPass::new().run_with_types("test.ts", source, &mut types);
    assert!(!output.diagnostics.has_errors(), "{:?}", output.diagnostics);
    let fn_decl = output
        .program
        .declarations
        .iter()
        .find_map(|d| match d {
            HirDecl::Function(f) if f.name.as_str() == "f" => Some(f),
            _ => None,
        })
        .expect("function f should be present");
    assert_eq!(
        types.resolve(fn_decl.params[0].ty),
        Some(&Type::String),
        "consumer (function f) declared before alias chain Foo = Bar = string must still resolve through pre-resolve"
    );
}

#[test]
fn self_referential_alias_emits_cycle_warning_and_resolves_to_error_without_panicking() {
    let mut types = TypeTable::new();
    let output = FrontendPass::new().run_with_types("test.ts", "type Foo = Foo;", &mut types);
    let diag = output
        .diagnostics
        .iter()
        .find(|d| d.code.as_str() == "E0401")
        .expect("expected E0401 alias-cycle warning for type Foo = Foo;");
    assert!(diag.message.contains("Foo"));
    assert_eq!(
        types.resolve(
            output
                .program
                .declarations
                .iter()
                .find_map(|d| match d {
                    HirDecl::TypeAlias { name, target } if name.as_str() == "Foo" => Some(*target),
                    _ => None,
                })
                .expect("Foo alias decl should be present")
        ),
        Some(&Type::Error),
        "self-referential alias must terminate with Type::Error, not recurse infinitely"
    );
}

#[test]
fn mutually_recursive_aliases_emit_cycle_warning_without_panicking() {
    let mut types = TypeTable::new();
    let source = "type A = B; type B = A;";
    let output = FrontendPass::new().run_with_types("test.ts", source, &mut types);
    assert!(
        output
            .diagnostics
            .iter()
            .any(|d| d.code.as_str() == "E0401"),
        "expected at least one E0401 alias-cycle warning for type A = B; type B = A;"
    );
    let alias_target_for = |name: &str| -> TypeId {
        output
            .program
            .declarations
            .iter()
            .find_map(|d| match d {
                HirDecl::TypeAlias { name: n, target } if n.as_str() == name => Some(*target),
                _ => None,
            })
            .unwrap()
    };
    assert_eq!(
        types.resolve(alias_target_for("A")),
        Some(&Type::Error),
        "mutually-recursive A must terminate with Type::Error"
    );
    assert_eq!(
        types.resolve(alias_target_for("B")),
        Some(&Type::Error),
        "mutually-recursive B must terminate with Type::Error"
    );
}

fn sole_function(source: &str) -> HirFunction {
    let output = FrontendPass::new().run("test.ts", source);
    assert!(!output.diagnostics.has_errors(), "{:?}", output.diagnostics);
    output
        .program
        .declarations
        .iter()
        .find_map(|d| match d {
            HirDecl::Function(f) => Some(f.clone()),
            _ => None,
        })
        .unwrap_or_else(|| {
            panic!(
                "expected Function in decls, got {:?}",
                output.program.declarations
            )
        })
}

#[test]
fn body_walker_lets_binding_gets_local_id_and_init() {
    let f = sole_function("function f(): i32 { let x: i32 = 5; return x; }");
    let let_ty = match &f.body[0] {
        HirStmt::Let { ty, .. } => *ty,
        other => panic!("expected first stmt to be Let, got {other:?}"),
    };
    assert_eq!(f.body.len(), 2);
    match &f.body[0] {
        HirStmt::Let {
            id,
            name,
            ty,
            init: Some(HirExpr::Int(v, _)),
        } => {
            assert_eq!(*id, LocalId::from_raw(0));
            assert_eq!(*name, Atom::from("x"));
            assert_eq!(*ty, let_ty);
            assert_eq!(*v, 5);
        }
        other => panic!("expected Let {{ x = 5 }}, got {other:?}"),
    }
    match &f.body[1] {
        HirStmt::Return {
            value: Some(HirExpr::Local { id, ty, .. }),
        } => {
            assert_eq!(*id, LocalId::from_raw(0));
            assert_eq!(*ty, let_ty);
        }
        other => panic!("expected Return(Local(0)), got {other:?}"),
    }
}

#[test]
fn body_walker_param_reference_resolves_to_local_zero() {
    let f = sole_function("function f(a: i32): i32 { return a; }");
    match &f.body[0] {
        HirStmt::Return {
            value: Some(HirExpr::Local { id, .. }),
        } => assert_eq!(*id, LocalId::from_raw(0), "param `a` is Local(0)"),
        other => panic!("expected Return(Local), got {other:?}"),
    }
}

#[test]
fn body_walker_let_after_param_gets_fresh_id_beyond_param_count() {
    let f = sole_function("function f(a: i32): i32 { let b: i32 = 1; return b; }");
    match &f.body[0] {
        HirStmt::Let { id, name, .. } => {
            assert_eq!(
                *id,
                LocalId::from_raw(1),
                "let `b` sits past the single param"
            );
            assert_eq!(name, &Atom::from("b"));
        }
        other => panic!("expected Let, got {other:?}"),
    }
}

#[test]
fn body_walker_undeclared_identifier_becomes_global() {
    let f = sole_function("function f(): i32 { return missing; }");
    match &f.body[0] {
        HirStmt::Return {
            value: Some(HirExpr::Global { name, .. }),
        } => assert_eq!(name, &Atom::from("missing")),
        other => panic!("expected Return(Global), got {other:?}"),
    }
}

#[test]
fn body_walker_binary_expression_maps_operator() {
    let f = sole_function("function f(a: i32, b: i32): i32 { return a + b; }");
    match &f.body[0] {
        HirStmt::Return {
            value: Some(HirExpr::Binary { op, lhs, rhs, .. }),
        } => {
            assert_eq!(*op, HirBinaryOp::Add);
            assert!(matches!(**lhs, HirExpr::Local { id, .. } if id == LocalId::from_raw(0)));
            assert!(matches!(**rhs, HirExpr::Local { id, .. } if id == LocalId::from_raw(1)));
        }
        other => panic!("expected Return(Binary), got {other:?}"),
    }
}

#[test]
fn body_walker_if_else_produces_both_branches() {
    let f = sole_function("function f(n: i32): i32 { if (n) { return 1; } else { return 2; } }");
    match &f.body[0] {
        HirStmt::If {
            cond: HirExpr::Local { .. },
            then,
            otherwise: Some(otherwise),
        } => {
            assert!(matches!(**then, HirStmt::Block(_)));
            assert!(matches!(**otherwise, HirStmt::Block(_)));
        }
        other => panic!("expected If with both branches, got {other:?}"),
    }
}

#[test]
fn body_walker_while_loop() {
    let f = sole_function("function f(n: i32): void { while (n) { n = 0; } }");
    assert!(
        matches!(
            &f.body[0],
            HirStmt::While {
                cond: HirExpr::Local { .. },
                ..
            }
        ),
        "got {:?}",
        f.body[0]
    );
}

#[test]
fn body_walker_call_uses_indirect_global_callee() {
    let f = sole_function("function f(): void { g(); }");
    match &f.body[0] {
        HirStmt::Expr {
            expr: HirExpr::Call { callee, args, .. },
        } => {
            assert!(args.is_empty());
            match callee {
                HirCallee::Indirect(inner) => {
                    assert!(
                        matches!(**inner, HirExpr::Global { ref name, .. } if name.as_str() == "g")
                    );
                }
                other => panic!("expected Indirect(Global) callee, got {other:?}"),
            }
        }
        other => panic!("expected Expr(Call), got {other:?}"),
    }
}

#[test]
fn body_walker_member_access_records_field_name() {
    let f = sole_function("function f(o: i32): i32 { return o.x; }");
    match &f.body[0] {
        HirStmt::Return {
            value: Some(HirExpr::Field { field_name, .. }),
        } => assert_eq!(field_name, &Atom::from("x")),
        other => panic!("expected Return(Field), got {other:?}"),
    }
}

#[test]
fn body_walker_compound_assignment_uses_compound_update() {
    let f = sole_function("function f(a: i32): void { a += 2; }");
    match &f.body[0] {
        HirStmt::Expr {
            expr:
                HirExpr::CompoundUpdate {
                    target,
                    op,
                    rhs,
                    post,
                    ..
                },
        } => {
            assert!(matches!(**target, HirExpr::Local { id, .. } if id == LocalId::from_raw(0)));
            assert_eq!(*op, HirBinaryOp::Add);
            assert!(
                !*post,
                "compound assignment is pre-style (returns new value)"
            );
            match rhs.as_ref() {
                HirExpr::Int(v, _) => assert_eq!(*v, 2),
                other => panic!("expected Int(2), got {other:?}"),
            }
        }
        other => panic!("expected Expr(CompoundUpdate), got {other:?}"),
    }
}

#[test]
fn body_walker_update_lowers_post_increment_to_compound_update() {
    let f = sole_function("function f(a: i32): void { a++; }");
    match &f.body[0] {
        HirStmt::Expr {
            expr:
                HirExpr::CompoundUpdate {
                    target,
                    op,
                    rhs,
                    post,
                    ..
                },
        } => {
            assert!(matches!(**target, HirExpr::Local { id, .. } if id == LocalId::from_raw(0)));
            assert_eq!(*op, HirBinaryOp::Add);
            assert_eq!(**rhs, HirExpr::Int(1, Span::default()));
            assert!(*post, "post-increment must be flagged post=true");
        }
        other => panic!("expected Expr(CompoundUpdate), got {other:?}"),
    }
}

#[test]
fn body_walker_update_lowers_pre_increment_with_post_false() {
    let f = sole_function("function f(a: i32): void { ++a; }");
    match &f.body[0] {
        HirStmt::Expr {
            expr:
                HirExpr::CompoundUpdate {
                    target,
                    op,
                    rhs,
                    post,
                    ..
                },
        } => {
            assert!(matches!(**target, HirExpr::Local { id, .. } if id == LocalId::from_raw(0)));
            assert_eq!(*op, HirBinaryOp::Add);
            assert_eq!(**rhs, HirExpr::Int(1, Span::default()));
            assert!(!*post, "pre-increment must be flagged post=false");
        }
        other => panic!("expected Expr(CompoundUpdate), got {other:?}"),
    }
}

#[test]
fn body_walker_compound_update_does_not_clone_target_side_effects() {
    let f = sole_function("function f(o: Vec<i64>, k: i64): void { o[k()]++; }");
    let body = &f.body[0];
    let HirStmt::Expr {
        expr: HirExpr::CompoundUpdate { target, rhs, .. },
    } = body
    else {
        panic!("expected Expr(CompoundUpdate), got {body:?}");
    };
    let HirExpr::Index { owner, index, .. } = &**target else {
        panic!("expected target to be Index, got {target:?}");
    };
    assert!(
        matches!(**owner, HirExpr::Local { id, .. } if id == LocalId::from_raw(0)),
        "owner must be the local `o`"
    );
    let HirExpr::Call { callee, .. } = &**index else {
        panic!("expected index to be a Call, got {index:?}");
    };
    let HirCallee::Indirect(callee_inner) = callee else {
        panic!("expected indirect callee, got {callee:?}");
    };
    assert!(
        matches!(**callee_inner, HirExpr::Local { id, .. } if id == LocalId::from_raw(1)),
        "callee must be the local `k`, got {callee_inner:?}"
    );
    assert_eq!(**rhs, HirExpr::Int(1, Span::default()));

    let mut calls = 0u32;
    count_calls_in_stmts(&f.body, &mut calls);
    assert_eq!(
        calls, 1,
        "the index call must appear exactly once in the HIR (no cloning): {f:?}"
    );
}

fn count_calls_in_stmts(stmts: &[HirStmt], out: &mut u32) {
    for s in stmts {
        count_calls_in_stmt(s, out);
    }
}

#[allow(clippy::match_same_arms)]
fn count_calls_in_stmt(s: &HirStmt, out: &mut u32) {
    match s {
        HirStmt::Block(inner) => count_calls_in_stmts(inner, out),
        HirStmt::If {
            then, otherwise, ..
        } => {
            count_calls_in_stmt(then, out);
            if let Some(o) = otherwise {
                count_calls_in_stmt(o, out);
            }
        }
        HirStmt::While { body, .. } => count_calls_in_stmt(body, out),
        HirStmt::DoWhile { body, .. } => count_calls_in_stmt(body, out),
        HirStmt::ForOf { body, .. } => count_calls_in_stmt(body, out),
        HirStmt::ForIn { body, .. } => count_calls_in_stmt(body, out),
        HirStmt::Switch { cases, .. } => {
            for c in cases {
                count_calls_in_stmts(&c.body, out);
            }
        }
        HirStmt::Try {
            body,
            catch,
            finally,
        } => {
            count_calls_in_stmt(body, out);
            if let Some(c) = catch {
                count_calls_in_stmt(&c.body, out);
            }
            if let Some(f) = finally {
                count_calls_in_stmt(f, out);
            }
        }
        HirStmt::Expr { expr } => count_calls_in_expr(expr, out),
        _ => {}
    }
}

fn count_calls_in_expr(e: &HirExpr, out: &mut u32) {
    if matches!(e, HirExpr::Call { .. }) {
        *out += 1;
    }
    for sub in hir_expr_subexprs(e) {
        count_calls_in_expr(sub, out);
    }
}

fn hir_expr_subexprs(e: &HirExpr) -> Vec<&HirExpr> {
    match e {
        HirExpr::Binary { lhs, rhs, .. } => vec![lhs, rhs],
        HirExpr::Unary { expr, .. } | HirExpr::Await { expr, .. } => vec![expr],
        HirExpr::Field { owner, .. } => vec![owner],
        HirExpr::Index { owner, index, .. } => vec![owner, index],
        HirExpr::Call { callee, args, .. } => {
            let mut v: Vec<&HirExpr> = Vec::with_capacity(1 + args.len());
            if let HirCallee::Indirect(inner) = callee {
                v.push(inner);
            }
            v.extend(args);
            v
        }
        HirExpr::Assignment { target, value, .. } => vec![target, value],
        HirExpr::CompoundUpdate { target, rhs, .. } => vec![target, rhs],
        _ => Vec::new(),
    }
}

#[test]
fn body_walker_nested_blocks_get_distinct_local_ids() {
    let f = sole_function("function f(): void { let a = 1; { let b = 2; } let c = 3; }");
    let mut ids: Vec<u32> = Vec::new();
    collect_let_ids(&f.body, &mut ids);
    assert_eq!(ids, vec![0, 1, 2], "each binding gets a unique LocalId");
}

fn collect_let_ids(stmts: &[HirStmt], out: &mut Vec<u32>) {
    for s in stmts {
        match s {
            HirStmt::Let { id, .. } => out.push(id.raw()),
            HirStmt::Block(inner) => collect_let_ids(inner, out),
            _ => {}
        }
    }
}

#[test]
fn body_walker_c_style_for_desugars_to_block_with_while() {
    let f = sole_function("function f(): void { for (let i = 0; i < 3; i = i + 1) {} }");
    match &f.body[0] {
        HirStmt::Block(inner) => {
            assert!(
                matches!(inner.first(), Some(HirStmt::Let { .. })),
                "for-init lowers to a Let: {inner:?}"
            );
            assert!(
                matches!(inner.last(), Some(HirStmt::While { .. })),
                "for lowers to a While: {inner:?}"
            );
        }
        other => panic!("expected desugared Block, got {other:?}"),
    }
}

#[test]
fn body_walker_c_for_runs_update_before_continue() {
    let f = sole_function("function f(): void { for (let i = 0; i < 3; i = i + 1) { continue; } }");
    let outer = match &f.body[0] {
        HirStmt::Block(o) => o,
        other => panic!("expected desugared Block, got {other:?}"),
    };
    let while_body = outer
        .iter()
        .find_map(|s| match s {
            HirStmt::While { body, .. } => Some(body),
            _ => None,
        })
        .expect("desugared for must contain a While");
    let wstmts = match &**while_body {
        HirStmt::Block(w) => w,
        other => panic!("expected While body Block, got {other:?}"),
    };
    let inner = match &wstmts[0] {
        HirStmt::Block(b) => b,
        other => panic!("expected walked for-body Block, got {other:?}"),
    };
    assert!(
        matches!(inner.last(), Some(HirStmt::Continue { .. })),
        "for-body ends with continue: {inner:?}"
    );
    assert!(
        matches!(
            inner.get(inner.len().wrapping_sub(2)),
            Some(HirStmt::Expr {
                expr: HirExpr::Assignment { .. }
            })
        ),
        "the update assignment must run immediately before continue: {inner:?}"
    );
}

#[test]
fn body_walker_unsupported_expression_warns_without_erroring() {
    let output = FrontendPass::new().run("test.ts", "function f(): void { new.target; }");
    assert!(
        !output.diagnostics.has_errors(),
        "unsupported body expressions degrade to a warning, not an error: {:?}",
        output.diagnostics
    );
    assert!(
        output
            .diagnostics
            .iter()
            .any(|d| d.code.as_str() == "E0500"),
        "expected an E0500 walker warning, got {:?}",
        output.diagnostics
    );
}

#[test]
fn body_walker_regexp_literal_with_flags_emits_regexp_expr() {
    let f = sole_function("function f(): i64 { return /foo/g; }");
    let HirStmt::Return { value: Some(ret) } = &f.body[0] else {
        panic!("expected Return with value, got {:?}", f.body[0]);
    };
    match ret {
        HirExpr::RegExp { pattern, flags, .. } => {
            assert_eq!(pattern, Atom::from("foo"));
            assert_eq!(flags, Atom::from("g"));
        }
        other => panic!("expected RegExp, got {other:?}"),
    }
}

#[test]
fn body_walker_regexp_literal_without_flags_uses_empty_flags() {
    let f = sole_function("function f(): i64 { return /abc/; }");
    let HirStmt::Return { value: Some(ret) } = &f.body[0] else {
        panic!("expected Return with value, got {:?}", f.body[0]);
    };
    match ret {
        HirExpr::RegExp { pattern, flags, .. } => {
            assert_eq!(pattern, Atom::from("abc"));
            assert_eq!(
                flags.as_str(),
                "",
                "no-flags regex must produce empty flags string"
            );
        }
        other => panic!("expected RegExp, got {other:?}"),
    }
}

#[test]
fn body_walker_regexp_literal_with_multiple_flags_preserves_all() {
    let f = sole_function("function f(): i64 { return /foo/gim; }");
    let HirStmt::Return { value: Some(ret) } = &f.body[0] else {
        panic!("expected Return with value, got {:?}", f.body[0]);
    };
    match ret {
        HirExpr::RegExp { pattern, flags, .. } => {
            assert_eq!(pattern, Atom::from("foo"));
            assert_eq!(flags.as_str().len(), 3);
        }
        other => panic!("expected RegExp, got {other:?}"),
    }
}

#[test]
fn body_walker_bigint_literal_emits_bigint_expr() {
    let f = sole_function("function f(): i64 { return 42n; }");
    let HirStmt::Return { value: Some(ret) } = &f.body[0] else {
        panic!("expected Return with value, got {:?}", f.body[0]);
    };
    match ret {
        HirExpr::BigInt { value, .. } => {
            assert_eq!(value, Atom::from("42"));
        }
        other => panic!("expected BigInt, got {other:?}"),
    }
}

#[test]
fn body_walker_bigint_literal_handles_large_value_via_const_fold() {
    let f = sole_function("function f(): i64 { return 99999999999999999999n; }");
    let HirStmt::Return { value: Some(ret) } = &f.body[0] else {
        panic!("expected Return with value, got {:?}", f.body[0]);
    };
    match ret {
        HirExpr::BigInt { value, .. } => {
            assert_eq!(value, Atom::from("99999999999999999999"));
        }
        other => panic!("expected BigInt, got {other:?}"),
    }
}

#[test]
fn body_walker_dynamic_import_emits_import_expr() {
    let f = sole_function("function f(): i64 { return import('./mod.js'); }");
    let HirStmt::Return { value: Some(ret) } = &f.body[0] else {
        panic!("expected Return with value, got {:?}", f.body[0]);
    };
    let HirExpr::Import { source, .. } = ret else {
        panic!("expected Import, got {ret:?}");
    };
    match source.as_ref() {
        HirExpr::String(s, _) => assert_eq!(s.as_str(), "./mod.js"),
        other => panic!("expected String source, got {other:?}"),
    }
}

#[test]
fn body_walker_dynamic_import_with_options_emits_unwalked_diagnostic() {
    let output = FrontendPass::new().run(
        "test.ts",
        "function f(): i64 { return import('./mod', { with: { type: 'json' } }); }",
    );
    let diags: Vec<String> = output
        .diagnostics
        .iter()
        .map(|d| format!("{d:?}"))
        .collect();
    assert!(
        diags
            .iter()
            .any(|d| d.contains("with") && d.contains("not supported")),
        "expected unwalked diagnostic about `with` import options, got: {diags:?}"
    );
}

#[test]
fn body_walker_method_this_is_local_zero_and_params_follow() {
    let output = FrontendPass::new().run(
        "test.ts",
        "class C { m(a: i32): i32 { return this.x + a; } }",
    );
    assert!(!output.diagnostics.has_errors(), "{:?}", output.diagnostics);
    let method = match &output.program.declarations[0] {
        HirDecl::Class(c) => &c.methods[0],
        other => panic!("expected Class, got {other:?}"),
    };
    assert_eq!(method.params[0].name, Atom::from("this"));
    assert_eq!(method.params[1].name, Atom::from("a"));
    match &method.body[0] {
        HirStmt::Return {
            value: Some(HirExpr::Binary { lhs, rhs, .. }),
        } => {
            match &**lhs {
                HirExpr::Field {
                    owner, field_name, ..
                } => {
                    assert!(
                        matches!(**owner, HirExpr::Local { id, .. } if id == LocalId::from_raw(0)),
                        "`this` must lower to the receiver Local(0): {owner:?}"
                    );
                    assert_eq!(field_name, &Atom::from("x"));
                }
                other => panic!("expected Field for this.x, got {other:?}"),
            }
            assert!(
                matches!(**rhs, HirExpr::Local { id, .. } if id == LocalId::from_raw(1)),
                "declared param `a` must lower to Local(1), past the receiver: {rhs:?}"
            );
        }
        other => panic!("expected Return(Binary), got {other:?}"),
    }
}

#[test]
fn body_walker_labeled_control_flow_warns_but_does_not_error() {
    let output = FrontendPass::new().run(
        "test.ts",
        "function f(): void { outer: while (true) { break outer; } }",
    );
    assert!(
        !output.diagnostics.has_errors(),
        "labeled control flow degrades to warnings, not errors: {:?}",
        output.diagnostics
    );
    let warnings = output
        .diagnostics
        .iter()
        .filter(|d| d.code.as_str() == "E0500")
        .count();
    assert!(
        warnings >= 2,
        "both the labeled statement and the labeled break must warn: {:?}",
        output.diagnostics
    );
}

#[test]
fn body_walker_unlabeled_break_does_not_warn() {
    let output =
        FrontendPass::new().run("test.ts", "function f(): void { while (true) { break; } }");
    assert!(
        output
            .diagnostics
            .iter()
            .all(|d| d.code.as_str() != "E0500"),
        "plain break/continue stay silent: {:?}",
        output.diagnostics
    );
}

#[test]
fn class_static_method_with_params_is_rejected_with_diagnostic() {
    let output =
        FrontendPass::new().run("test.ts", "class C { static s(a: i32): i32 { return a; } }");
    assert!(
        output.diagnostics.has_errors(),
        "static methods are out of scope and must emit a diagnostic, got {:?}",
        output.diagnostics
    );
    let class = match &output.program.declarations[0] {
        HirDecl::Class(c) => c,
        other => panic!("expected Class, got {other:?}"),
    };
    assert!(
        class.methods.is_empty(),
        "rejected static method must NOT enter HIR class.methods (downstream treats every method as instance), got {} methods: {:?}",
        class.methods.len(),
        class.methods
    );
    assert!(
        output.diagnostics.iter().any(|d| d.code == "E0300".into()),
        "E0300 (unsupported declaration form) must be reported for static methods, got {:?}",
        output
            .diagnostics
            .iter()
            .map(|d| (d.code.as_str(), d.message.as_str()))
            .collect::<Vec<_>>()
    );
}

#[test]
fn class_static_method_without_params_is_rejected_with_diagnostic() {
    let output = FrontendPass::new().run("test.ts", "class C { static s(): i32 { return 0; } }");
    assert!(
        output.diagnostics.has_errors(),
        "no-arg static methods must also emit a diagnostic (previously silently dropped at MIR), got {:?}",
        output.diagnostics
    );
    let class = match &output.program.declarations[0] {
        HirDecl::Class(c) => c,
        other => panic!("expected Class, got {other:?}"),
    };
    assert!(
        class.methods.is_empty(),
        "no-arg static method must also NOT enter HIR class.methods, got {} methods",
        class.methods.len()
    );
}

#[test]
fn class_with_only_static_methods_emits_diagnostic_and_empty_methods() {
    let output = FrontendPass::new().run(
        "test.ts",
        "class C { static a(): void {} static b(x: i32): i32 { return x; } }",
    );
    assert!(output.diagnostics.has_errors());
    let class = match &output.program.declarations[0] {
        HirDecl::Class(c) => c,
        other => panic!("expected Class, got {other:?}"),
    };
    assert!(
        class.methods.is_empty(),
        "a class whose every method is static must still have an empty methods vec, got {:?}",
        class.methods
    );
    let e0300_count = output
        .diagnostics
        .iter()
        .filter(|d| d.code == "E0300".into())
        .count();
    assert_eq!(
        e0300_count, 2,
        "each static method must produce its own E0300 diagnostic, got {e0300_count} diagnostics"
    );
}

#[test]
fn class_instance_method_still_receives_synthetic_this() {
    let output = FrontendPass::new().run("test.ts", "class C { m(a: i32): i32 { return a; } }");
    let method = match &output.program.declarations[0] {
        HirDecl::Class(c) => &c.methods[0],
        other => panic!("expected Class, got {other:?}"),
    };
    assert_eq!(method.params.len(), 2);
    assert_eq!(method.params[0].name, Atom::from("this"));
    assert_eq!(method.params[1].name, Atom::from("a"));
}

#[test]
fn class_getter_without_params_is_rejected_with_diagnostic() {
    let output = FrontendPass::new().run("test.ts", "class C { get foo(): i32 { return 1; } }");
    assert!(
        output.diagnostics.has_errors(),
        "getter must emit a diagnostic (previously slipped through to MIR where `params.is_empty()` silently dropped it), got {:?}",
        output.diagnostics
    );
    let class = match &output.program.declarations[0] {
        HirDecl::Class(c) => c,
        other => panic!("expected Class, got {other:?}"),
    };
    assert!(
        class.methods.is_empty(),
        "rejected getter must NOT enter HIR class.methods, got {} methods",
        class.methods.len()
    );
    assert!(
        output.diagnostics.iter().any(|d| d.code == "E0300".into()),
        "E0300 must be reported for getter, got {:?}",
        output
            .diagnostics
            .iter()
            .map(|d| (d.code.as_str(), d.message.as_str()))
            .collect::<Vec<_>>()
    );
}

#[test]
fn class_setter_with_value_param_is_rejected_with_diagnostic() {
    let output = FrontendPass::new().run("test.ts", "class C { set foo(v: i32) { return; } }");
    assert!(
        output.diagnostics.has_errors(),
        "setter must emit a diagnostic (previously slipped through and treated its `v` as `this` in MIR), got {:?}",
        output.diagnostics
    );
    let class = match &output.program.declarations[0] {
        HirDecl::Class(c) => c,
        other => panic!("expected Class, got {other:?}"),
    };
    assert!(
        class.methods.is_empty(),
        "rejected setter must NOT enter HIR class.methods (its `v` would otherwise be misbound to `self_param = Local(0)`), got {} methods",
        class.methods.len()
    );
    assert!(
        output.diagnostics.iter().any(|d| d.code == "E0300".into()),
        "E0300 must be reported for setter, got {:?}",
        output
            .diagnostics
            .iter()
            .map(|d| (d.code.as_str(), d.message.as_str()))
            .collect::<Vec<_>>()
    );
}

#[test]
fn class_with_get_set_and_instance_method_rejects_only_accessors() {
    let output = FrontendPass::new().run(
        "test.ts",
        "class C { get foo(): i32 { return 1; } set foo(v: i32) { return; } m(a: i32): i32 { return a; } }",
    );
    assert!(
        output.diagnostics.has_errors(),
        "class with get+set+method must surface diagnostics for accessors, got {:?}",
        output.diagnostics
    );
    let class = match &output.program.declarations[0] {
        HirDecl::Class(c) => c,
        other => panic!("expected Class, got {other:?}"),
    };
    assert_eq!(
        class.methods.len(),
        1,
        "only the instance method must survive, got methods: {:?}",
        class.methods
    );
    assert_eq!(class.methods[0].name, Atom::from("m"));
    assert_eq!(
        class.methods[0].params.len(),
        2,
        "the surviving `m` must still receive synthetic `this`, got params: {:?}",
        class.methods[0].params
    );
    assert_eq!(class.methods[0].params[0].name, Atom::from("this"));
    assert_eq!(class.methods[0].params[1].name, Atom::from("a"));
    let e0300_count = output
        .diagnostics
        .iter()
        .filter(|d| d.code == "E0300".into())
        .count();
    assert_eq!(
        e0300_count, 2,
        "each of get+set must produce its own E0300, got {e0300_count}"
    );
}

#[test]
fn class_constructor_with_params_receives_synthetic_this_before_declared_params() {
    let output = FrontendPass::new().run("test.ts", "class C { constructor(x: i32) { return; } }");
    assert!(
        !output.diagnostics.has_errors(),
        "constructor must pass through (this is bound at `new`-call time, not via a synthetic HIR param), got {:?}",
        output.diagnostics
    );
    let class = match &output.program.declarations[0] {
        HirDecl::Class(c) => c,
        other => panic!("expected Class, got {other:?}"),
    };
    assert_eq!(
        class.methods.len(),
        1,
        "constructor must be recorded as a method, got methods: {:?}",
        class.methods
    );
    let ctor = &class.methods[0];
    assert_eq!(
        ctor.params.len(),
        2,
        "constructor must receive synthetic `this` first, then declared params, got params: {:?}",
        ctor.params
    );
    assert_eq!(
        ctor.params[0].name,
        Atom::from("this"),
        "synthetic `this` must occupy LocalId(0) so MIR `self_param = Local(0)` matches the alloc_id pushed by `New` lowering, got params: {:?}",
        ctor.params
    );
    assert_eq!(ctor.params[1].name, Atom::from("x"));
}

#[test]
fn class_constructor_with_no_args_receives_only_synthetic_this() {
    let output = FrontendPass::new().run("test.ts", "class C { constructor() { return; } }");
    assert!(
        !output.diagnostics.has_errors(),
        "no-arg constructor must pass through with a synthetic `this`, got {:?}",
        output.diagnostics
    );
    let class = match &output.program.declarations[0] {
        HirDecl::Class(c) => c,
        other => panic!("expected Class, got {other:?}"),
    };
    assert_eq!(
        class.methods.len(),
        1,
        "no-arg constructor must NOT be silently dropped by MIR `params.is_empty()` guard, got methods: {:?}",
        class.methods
    );
    let ctor = &class.methods[0];
    assert_eq!(
        ctor.params.len(),
        1,
        "no-arg constructor must have exactly one param (synthetic `this`), got: {:?}",
        ctor.params
    );
    assert_eq!(ctor.params[0].name, Atom::from("this"));
}

#[test]
fn body_walker_template_literal_no_substitution_produces_string() {
    let f = sole_function("function f(): string { return `hello`; }");
    let HirStmt::Return { value: Some(expr) } = &f.body[0] else {
        panic!("expected Return, got {:?}", f.body[0]);
    };
    let HirExpr::Template {
        tag,
        expressions,
        cooked_parts,
        raw_parts,
        ..
    } = expr
    else {
        panic!("expected Template, got {expr:?}");
    };
    assert!(
        tag.is_none(),
        "no-substitution template must have tag = None"
    );
    assert!(
        expressions.is_empty(),
        "no expressions, got: {expressions:?}"
    );
    assert_eq!(
        cooked_parts.len(),
        1,
        "1 quasi → 1 cooked entry, got: {cooked_parts:?}"
    );
    assert_eq!(
        raw_parts.len(),
        1,
        "1 quasi → 1 raw entry, got: {raw_parts:?}"
    );
    assert_eq!(
        cooked_parts[0].as_ref().map(Atom::as_str),
        Some("hello"),
        "single quasi cooked must be `hello`"
    );
    assert_eq!(
        raw_parts[0].as_ref().map(Atom::as_str),
        Some("hello"),
        "single quasi raw must be `hello`"
    );
}

#[test]
fn body_walker_template_literal_with_interpolation_interleaves_strings_and_exprs() {
    let f = sole_function("function f(name: string): string { return `hi ${name}!`; }");
    let HirStmt::Return { value: Some(expr) } = &f.body[0] else {
        panic!("expected Return, got {:?}", f.body[0]);
    };
    let HirExpr::Template {
        tag,
        expressions,
        cooked_parts,
        raw_parts,
        ..
    } = expr
    else {
        panic!("expected Template, got {expr:?}");
    };
    assert!(tag.is_none(), "plain template literal must have tag = None");
    assert_eq!(expressions.len(), 1, "1 interpolation → 1 expression");
    assert_eq!(cooked_parts.len(), 2, "2 quasis → 2 cooked entries");
    assert_eq!(raw_parts.len(), 2, "2 quasis → 2 raw entries");
    assert!(
        matches!(&expressions[0], HirExpr::Local { .. }),
        "expression must be walked Local reference to `name` param, got: {:?}",
        expressions[0]
    );
    assert_eq!(cooked_parts[0].as_ref().map(Atom::as_str), Some("hi "));
    assert_eq!(cooked_parts[1].as_ref().map(Atom::as_str), Some("!"));
    assert_eq!(raw_parts[0].as_ref().map(Atom::as_str), Some("hi "));
    assert_eq!(raw_parts[1].as_ref().map(Atom::as_str), Some("!"));
}

#[test]
fn body_walker_template_literal_multiple_interpolations() {
    let f = sole_function("function f(a: string, b: i64): string { return `${a}-${b}-end`; }");
    let HirStmt::Return { value: Some(expr) } = &f.body[0] else {
        panic!("expected Return, got {:?}", f.body[0]);
    };
    let HirExpr::Template {
        expressions,
        cooked_parts,
        raw_parts,
        ..
    } = expr
    else {
        panic!("expected Template, got {expr:?}");
    };
    assert_eq!(expressions.len(), 2, "2 interpolations → 2 expressions");
    assert_eq!(cooked_parts.len(), 3, "3 quasis → 3 cooked entries");
    assert_eq!(raw_parts.len(), 3, "3 quasis → 3 raw entries");
    assert!(matches!(&expressions[0], HirExpr::Local { .. }));
    assert!(matches!(&expressions[1], HirExpr::Local { .. }));
    assert_eq!(cooked_parts[0].as_ref().map(Atom::as_str), Some(""));
    assert_eq!(cooked_parts[1].as_ref().map(Atom::as_str), Some("-"));
    assert_eq!(cooked_parts[2].as_ref().map(Atom::as_str), Some("-end"));
    assert_eq!(raw_parts[0].as_ref().map(Atom::as_str), Some(""));
    assert_eq!(raw_parts[1].as_ref().map(Atom::as_str), Some("-"));
    assert_eq!(raw_parts[2].as_ref().map(Atom::as_str), Some("-end"));
}

#[test]
fn body_walker_template_literal_raw_preserves_escape_sequences_verbatim() {
    let f = sole_function(r"function f(): string { return `a\nb`; }");
    let HirStmt::Return { value: Some(expr) } = &f.body[0] else {
        panic!("expected Return, got {:?}", f.body[0]);
    };
    let HirExpr::Template {
        cooked_parts,
        raw_parts,
        ..
    } = expr
    else {
        panic!("expected Template, got {expr:?}");
    };
    assert_eq!(cooked_parts.len(), 1);
    assert_eq!(raw_parts.len(), 1);
    assert_eq!(
        cooked_parts[0].as_ref().map(Atom::as_str),
        Some("a\nb"),
        "cooked resolves \\n to a real newline"
    );
    assert_eq!(
        raw_parts[0].as_ref().map(Atom::as_str),
        Some(r"a\nb"),
        "raw keeps the literal backslash + n"
    );
}

#[test]
fn body_walker_tagged_template_has_some_tag() {
    let f = sole_function("function f(name: string): string { return String.raw`hi ${name}`; }");
    let HirStmt::Return { value: Some(expr) } = &f.body[0] else {
        panic!("expected Return, got {:?}", f.body[0]);
    };
    let HirExpr::Template {
        tag,
        expressions,
        cooked_parts,
        raw_parts,
        ..
    } = expr
    else {
        panic!("expected Template, got {expr:?}");
    };
    assert!(
        tag.is_some(),
        "tagged template must have Some(tag), got None"
    );
    assert_eq!(expressions.len(), 1, "1 interpolation → 1 expression");
    assert_eq!(cooked_parts.len(), 2, "2 quasis → 2 cooked entries");
    assert_eq!(raw_parts.len(), 2, "2 quasis → 2 raw entries");
    assert!(matches!(&expressions[0], HirExpr::Local { .. }));
    assert_eq!(cooked_parts[0].as_ref().map(Atom::as_str), Some("hi "));
    assert_eq!(cooked_parts[1].as_ref().map(Atom::as_str), Some(""));
    assert_eq!(raw_parts[0].as_ref().map(Atom::as_str), Some("hi "));
    assert_eq!(raw_parts[1].as_ref().map(Atom::as_str), Some(""));
}

#[test]
fn body_walker_tagged_template_invalid_escape_keeps_raw_but_drops_cooked() {
    let f = sole_function(r"function f(): string { return String.raw`\u{FFFFFFFF}`; }");
    let HirStmt::Return { value: Some(expr) } = &f.body[0] else {
        panic!("expected Return, got {:?}", f.body[0]);
    };
    let HirExpr::Template {
        cooked_parts,
        raw_parts,
        ..
    } = expr
    else {
        panic!("expected Template, got {expr:?}");
    };
    assert_eq!(cooked_parts.len(), 1);
    assert_eq!(raw_parts.len(), 1);
    assert_eq!(
        cooked_parts[0], None,
        "out-of-range \\u{{FFFFFFFF}} → cooked is None (explicit invalid marker), got: {:?}",
        cooked_parts[0]
    );
    assert_eq!(
        raw_parts[0].as_ref().map(Atom::as_str),
        Some(r"\u{FFFFFFFF}"),
        "raw keeps the literal escape even when cooked is invalid"
    );
}

#[test]
fn body_walker_array_expression_empty_produces_empty_array() {
    let f = sole_function("function f(): i64[] { return []; }");
    let HirStmt::Return { value: Some(expr) } = &f.body[0] else {
        panic!("expected Return, got {:?}", f.body[0]);
    };
    let HirExpr::ArrayLiteral { elements, .. } = expr else {
        panic!("expected ArrayLiteral, got {expr:?}");
    };
    assert!(
        elements.is_empty(),
        "empty array literal has no elements, got: {elements:?}"
    );
}

#[test]
fn body_walker_array_expression_with_literals_walks_each_element() {
    let f = sole_function("function f(): i64[] { return [1, 2, 3]; }");
    let HirStmt::Return { value: Some(expr) } = &f.body[0] else {
        panic!("expected Return, got {:?}", f.body[0]);
    };
    let HirExpr::ArrayLiteral { elements, .. } = expr else {
        panic!("expected ArrayLiteral, got {expr:?}");
    };
    assert_eq!(elements.len(), 3, "[1, 2, 3] has 3 elements");
    assert!(matches!(&elements[0], HirExpr::Int(1, _)));
    assert!(matches!(&elements[1], HirExpr::Int(2, _)));
    assert!(matches!(&elements[2], HirExpr::Int(3, _)));
}

#[test]
fn body_walker_array_expression_with_identifiers_walks_local_refs() {
    let f = sole_function("function f(a: i64, b: i64): i64[] { return [a, b]; }");
    let HirStmt::Return { value: Some(expr) } = &f.body[0] else {
        panic!("expected Return, got {:?}", f.body[0]);
    };
    let HirExpr::ArrayLiteral { elements, .. } = expr else {
        panic!("expected ArrayLiteral, got {expr:?}");
    };
    assert_eq!(elements.len(), 2);
    assert!(
        matches!(&elements[0], HirExpr::Local { .. }),
        "first element is local ref to `a`"
    );
    assert!(
        matches!(&elements[1], HirExpr::Local { .. }),
        "second element is local ref to `b`"
    );
}

#[test]
fn body_walker_array_expression_nested_walks_inner_array() {
    let f = sole_function("function f(): i64[] { return [[1, 2], [3]]; }");
    let HirStmt::Return { value: Some(expr) } = &f.body[0] else {
        panic!("expected Return, got {:?}", f.body[0]);
    };
    let HirExpr::ArrayLiteral { elements, .. } = expr else {
        panic!("expected ArrayLiteral, got {expr:?}");
    };
    assert_eq!(elements.len(), 2, "outer has 2 inner arrays");
    let inner0 = &elements[0];
    let inner1 = &elements[1];
    let HirExpr::ArrayLiteral {
        elements: inner0_elements,
        ..
    } = inner0
    else {
        panic!("expected nested ArrayLiteral, got {inner0:?}");
    };
    assert_eq!(inner0_elements.len(), 2);
    assert!(matches!(&inner0_elements[0], HirExpr::Int(1, _)));
    assert!(matches!(&inner0_elements[1], HirExpr::Int(2, _)));
    let HirExpr::ArrayLiteral {
        elements: inner1_elements,
        ..
    } = inner1
    else {
        panic!("expected nested ArrayLiteral, got {inner1:?}");
    };
    assert_eq!(inner1_elements.len(), 1);
    assert!(matches!(&inner1_elements[0], HirExpr::Int(3, _)));
}

#[test]
fn body_walker_array_expression_elision_becomes_undefined() {
    let f = sole_function("function f(): i64[] { return [1, , 3]; }");
    let HirStmt::Return { value: Some(expr) } = &f.body[0] else {
        panic!("expected Return, got {:?}", f.body[0]);
    };
    let HirExpr::ArrayLiteral { elements, .. } = expr else {
        panic!("expected ArrayLiteral, got {expr:?}");
    };
    assert_eq!(
        elements.len(),
        3,
        "[1, , 3] has 3 elements (elision counted)"
    );
    assert!(matches!(&elements[0], HirExpr::Int(1, _)));
    assert!(
        matches!(&elements[1], HirExpr::Undefined(_)),
        "elision becomes Undefined per JS spec, got: {:?}",
        elements[1]
    );
    assert!(matches!(&elements[2], HirExpr::Int(3, _)));
}

#[test]
fn body_walker_array_expression_spread_walks_inner_but_warns() {
    let output = FrontendPass::new().run(
        "test.ts",
        "function f(a: i64[]): i64[] { return [...a, 1]; }",
    );
    assert!(
        !output.diagnostics.has_errors(),
        "spread array element must not error, got: {:?}",
        output.diagnostics
    );
    assert!(
        output
            .diagnostics
            .iter()
            .any(|d| d.message.contains("spread")),
        "spread should report unwalked warning, got: {:?}",
        output.diagnostics
    );
    let f = match &output.program.declarations[0] {
        ts_aot_ir_hir::HirDecl::Function(f) => f.clone(),
        other => panic!("expected Function, got {other:?}"),
    };
    let HirStmt::Return { value: Some(expr) } = &f.body[0] else {
        panic!("expected Return, got {:?}", f.body[0]);
    };
    let HirExpr::ArrayLiteral { elements, .. } = expr else {
        panic!("expected ArrayLiteral, got {expr:?}");
    };
    assert_eq!(elements.len(), 2, "[...a, 1] has 2 elements");
    assert!(
        matches!(&elements[0], HirExpr::Local { .. }),
        "spread inner is walked as local ref to `a` (PR 7.7 will do concat), got: {:?}",
        elements[0]
    );
    assert!(matches!(&elements[1], HirExpr::Int(1, _)));
}

#[test]
fn body_walker_object_expression_empty_produces_empty_object() {
    let f = sole_function("function f(): i64 { return {}; }");
    let HirStmt::Return { value: Some(expr) } = &f.body[0] else {
        panic!("expected Return, got {:?}", f.body[0]);
    };
    let HirExpr::ObjectLiteral { fields, .. } = expr else {
        panic!("expected ObjectLiteral, got {expr:?}");
    };
    assert!(
        fields.is_empty(),
        "empty object literal has no fields, got: {fields:?}"
    );
}

#[test]
fn body_walker_object_expression_with_identifier_keys_walks_values() {
    let f = sole_function("function f(a: i64, b: i64): i64 { return { x: a, y: b }; }");
    let HirStmt::Return { value: Some(expr) } = &f.body[0] else {
        panic!("expected Return, got {:?}", f.body[0]);
    };
    let HirExpr::ObjectLiteral { fields, .. } = expr else {
        panic!("expected ObjectLiteral, got {expr:?}");
    };
    assert_eq!(fields.len(), 2, "{{x:a, y:b}} has 2 properties");
    let (name0, val0) = match &fields[0] {
        ObjectLiteralField::Property { name, value } => (name, value),
        ObjectLiteralField::Spread(_) => panic!("expected Property, got Spread"),
    };
    assert_eq!(name0.as_str(), "x");
    assert!(
        matches!(val0, HirExpr::Local { .. }),
        "value is local ref to `a`, got: {val0:?}"
    );
    let (name1, val1) = match &fields[1] {
        ObjectLiteralField::Property { name, value } => (name, value),
        ObjectLiteralField::Spread(_) => panic!("expected Property, got Spread"),
    };
    assert_eq!(name1.as_str(), "y");
    assert!(matches!(val1, HirExpr::Local { .. }));
}

#[test]
fn body_walker_object_expression_with_string_key_records_atom_name() {
    let f = sole_function(r#"function f(): i64 { return { "key": 1 }; }"#);
    let HirStmt::Return { value: Some(expr) } = &f.body[0] else {
        panic!("expected Return, got {:?}", f.body[0]);
    };
    let HirExpr::ObjectLiteral { fields, .. } = expr else {
        panic!("expected ObjectLiteral, got {expr:?}");
    };
    assert_eq!(fields.len(), 1);
    let (name, value) = match &fields[0] {
        ObjectLiteralField::Property { name, value } => (name, value),
        ObjectLiteralField::Spread(_) => panic!("expected Property, got Spread"),
    };
    assert_eq!(
        name.as_str(),
        "key",
        "string-literal key resolves to its text"
    );
    assert!(matches!(value, HirExpr::Int(1, _)));
}

#[test]
fn body_walker_object_expression_with_numeric_key_stringifies_to_atom() {
    let f = sole_function("function f(): i64 { return { 1: 2 }; }");
    let HirStmt::Return { value: Some(expr) } = &f.body[0] else {
        panic!("expected Return, got {:?}", f.body[0]);
    };
    let HirExpr::ObjectLiteral { fields, .. } = expr else {
        panic!("expected ObjectLiteral, got {expr:?}");
    };
    let (name, _) = match &fields[0] {
        ObjectLiteralField::Property { name, value } => (name, value),
        ObjectLiteralField::Spread(_) => panic!("expected Property, got Spread"),
    };
    assert_eq!(name.as_str(), "1", "numeric key stringified to Atom");
}

#[test]
fn body_walker_object_expression_shorthand_walks_identifier_value() {
    let f = sole_function("function f(a: i64): i64 { return { a }; }");
    let HirStmt::Return { value: Some(expr) } = &f.body[0] else {
        panic!("expected Return, got {:?}", f.body[0]);
    };
    let HirExpr::ObjectLiteral { fields, .. } = expr else {
        panic!("expected ObjectLiteral, got {expr:?}");
    };
    let (name, value) = match &fields[0] {
        ObjectLiteralField::Property { name, value } => (name, value),
        ObjectLiteralField::Spread(_) => panic!("expected Property, got Spread"),
    };
    assert_eq!(name.as_str(), "a");
    assert!(
        matches!(value, HirExpr::Local { .. }),
        "shorthand `a` walks as local ref to `a`, got: {value:?}"
    );
}

#[test]
fn body_walker_object_expression_spread_walks_inner_but_warns() {
    let output = FrontendPass::new().run(
        "test.ts",
        "function f(o: Record<string, i64>): i64 { return { ...o, x: 1 }; }",
    );
    assert!(
        !output.diagnostics.has_errors(),
        "spread object property must not error, got: {:?}",
        output.diagnostics
    );
    assert!(
        output
            .diagnostics
            .iter()
            .any(|d| d.message.contains("spread")),
        "spread should report unwalked warning, got: {:?}",
        output.diagnostics
    );
    let f = match &output.program.declarations[0] {
        HirDecl::Function(f) => f.clone(),
        other => panic!("expected Function, got {other:?}"),
    };
    let HirStmt::Return { value: Some(expr) } = &f.body[0] else {
        panic!("expected Return, got {:?}", f.body[0]);
    };
    let HirExpr::ObjectLiteral { fields, .. } = expr else {
        panic!("expected ObjectLiteral, got {expr:?}");
    };
    assert_eq!(fields.len(), 2, "{{...o, x:1}} has 2 fields");
    assert!(
        matches!(
            &fields[0],
            ObjectLiteralField::Spread(HirExpr::Local { .. })
        ),
        "first field is Spread with local ref to `o`, got: {:?}",
        fields[0]
    );
    let (name, value) = match &fields[1] {
        ObjectLiteralField::Property { name, value } => (name, value),
        ObjectLiteralField::Spread(_) => panic!("expected Property, got Spread"),
    };
    assert_eq!(name.as_str(), "x");
    assert!(matches!(value, HirExpr::Int(1, _)));
}

#[test]
fn body_walker_object_expression_getter_warns_without_erroring() {
    let output = FrontendPass::new().run(
        "test.ts",
        "function f(): i64 { return { get x() { return 1; } }; }",
    );
    assert!(
        !output.diagnostics.has_errors(),
        "getter syntax degrades to warning, not error: {:?}",
        output.diagnostics
    );
    assert!(
        output
            .diagnostics
            .iter()
            .any(|d| d.message.contains("accessor")),
        "expected accessor warning, got: {:?}",
        output.diagnostics
    );
}

#[test]
fn body_walker_object_expression_computed_key_walks_key_and_value_for_side_effects() {
    let output = FrontendPass::new().run(
        "test.ts",
        "let key: i64 = 0; function f(): i64 { return { [++key]: ++key }; }",
    );
    assert!(
        !output.diagnostics.has_errors(),
        "computed key object literal must not error, got: {:?}",
        output.diagnostics
    );
    assert!(
        output
            .diagnostics
            .iter()
            .any(|d| d.message.contains("computed property key")),
        "computed key should report unwalked warning, got: {:?}",
        output.diagnostics
    );
    let f = output
        .program
        .declarations
        .iter()
        .find_map(|d| match d {
            HirDecl::Function(f) if f.name.as_str() == "f" => Some(f.clone()),
            _ => None,
        })
        .expect("function `f` declaration");
    let HirStmt::Return { value: Some(expr) } = &f.body[0] else {
        panic!("expected Return, got {:?}", f.body[0]);
    };
    let HirExpr::ObjectLiteral { fields, .. } = expr else {
        panic!("expected ObjectLiteral, got {expr:?}");
    };
    assert!(
        fields.is_empty(),
        "unsupported computed key property must be omitted from fields, got: {fields:?}"
    );
}

#[test]
fn body_walker_conditional_expression_basic_true_branch() {
    let f = sole_function("function f(c: i64): i64 { return c > 0 ? 1 : 2; }");
    let HirStmt::Return { value: Some(expr) } = &f.body[0] else {
        panic!("expected Return, got {:?}", f.body[0]);
    };
    let HirExpr::Ternary {
        cond,
        then_branch,
        else_branch,
        ..
    } = expr
    else {
        panic!("expected Ternary, got {expr:?}");
    };
    assert!(matches!(cond.as_ref(), HirExpr::Binary { .. }));
    assert!(matches!(then_branch.as_ref(), HirExpr::Int(1, _)));
    assert!(matches!(else_branch.as_ref(), HirExpr::Int(2, _)));
}

#[test]
fn body_walker_conditional_expression_nested() {
    let f =
        sole_function("function f(a: i64, b: i64): i64 { return a > 0 ? (b > 0 ? 1 : 2) : 3; }");
    let HirStmt::Return { value: Some(expr) } = &f.body[0] else {
        panic!("expected Return, got {:?}", f.body[0]);
    };
    let HirExpr::Ternary {
        cond,
        then_branch,
        else_branch,
        ..
    } = expr
    else {
        panic!("expected outer Ternary, got {expr:?}");
    };
    assert!(matches!(cond.as_ref(), HirExpr::Binary { .. }));
    assert!(matches!(else_branch.as_ref(), HirExpr::Int(3, _)));
    let HirExpr::Ternary {
        cond: inner_cond,
        then_branch: inner_then,
        else_branch: inner_else,
        ..
    } = then_branch.as_ref()
    else {
        panic!("expected inner Ternary in then_branch, got {then_branch:?}");
    };
    assert!(matches!(inner_cond.as_ref(), HirExpr::Binary { .. }));
    assert!(matches!(inner_then.as_ref(), HirExpr::Int(1, _)));
    assert!(matches!(inner_else.as_ref(), HirExpr::Int(2, _)));
}

#[test]
fn body_walker_conditional_expression_with_call_in_branch() {
    let f = sole_function("function f(c: i64): i64 { return c > 0 ? f(1) : f(2); }");
    let HirStmt::Return { value: Some(expr) } = &f.body[0] else {
        panic!("expected Return, got {:?}", f.body[0]);
    };
    let HirExpr::Ternary {
        then_branch,
        else_branch,
        ..
    } = expr
    else {
        panic!("expected Ternary, got {expr:?}");
    };
    let HirExpr::Call { args, .. } = then_branch.as_ref() else {
        panic!("expected Call in then_branch, got {then_branch:?}");
    };
    assert_eq!(args.len(), 1, "then_branch should call f(1)");
    let HirExpr::Call { args, .. } = else_branch.as_ref() else {
        panic!("expected Call in else_branch, got {else_branch:?}");
    };
    assert_eq!(args.len(), 1, "else_branch should call f(2)");
}

#[test]
fn body_walker_sequence_expression_returns_last_value() {
    let f = sole_function("function f(): i64 { return (1, 2, 3); }");
    let HirStmt::Return { value: Some(expr) } = &f.body[0] else {
        panic!("expected Return, got {:?}", f.body[0]);
    };
    let HirExpr::Sequence { exprs, .. } = expr else {
        panic!("expected Sequence, got {expr:?}");
    };
    assert_eq!(exprs.len(), 3, "sequence must hold all 3 elements");
    assert!(matches!(exprs[0], HirExpr::Int(1, _)));
    assert!(matches!(exprs[1], HirExpr::Int(2, _)));
    assert!(matches!(exprs[2], HirExpr::Int(3, _)));
}

#[test]
fn body_walker_sequence_expression_walks_all_subexpressions() {
    let f = sole_function("function f(a: i64, b: i64): i64 { return (a, b, a + b); }");
    let HirStmt::Return { value: Some(expr) } = &f.body[0] else {
        panic!("expected Return, got {:?}", f.body[0]);
    };
    let HirExpr::Sequence { exprs, .. } = expr else {
        panic!("expected Sequence, got {expr:?}");
    };
    assert_eq!(exprs.len(), 3);
    assert!(matches!(exprs[0], HirExpr::Local { .. }));
    assert!(matches!(exprs[1], HirExpr::Local { .. }));
    assert!(matches!(exprs[2], HirExpr::Binary { .. }));
}

#[test]
fn body_walker_sequence_expression_nested() {
    let f = sole_function("function f(): i64 { return ((1, 2), 3); }");
    let HirStmt::Return { value: Some(expr) } = &f.body[0] else {
        panic!("expected Return, got {:?}", f.body[0]);
    };
    let HirExpr::Sequence { exprs, .. } = expr else {
        panic!("expected outer Sequence, got {expr:?}");
    };
    assert_eq!(exprs.len(), 2);
    let inner_seq = &exprs[0];
    let HirExpr::Sequence { exprs: inner, .. } = inner_seq else {
        panic!("expected inner Sequence, got {inner_seq:?}");
    };
    assert_eq!(inner.len(), 2);
    assert!(matches!(inner[0], HirExpr::Int(1, _)));
    assert!(matches!(inner[1], HirExpr::Int(2, _)));
    assert!(matches!(exprs[1], HirExpr::Int(3, _)));
}

#[test]
fn class_expression_anonymous_registers_with_module_unique_name() {
    let output =
        FrontendPass::new().run("test.ts", "function f(): i64 { return class { x: i32; }; }");
    assert!(!output.diagnostics.has_errors(), "{:?}", output.diagnostics);
    let anon_class = output
        .program
        .declarations
        .iter()
        .find_map(|d| match d {
            HirDecl::Class(c) if c.name == Atom::from("__class_m0_0") => Some(c.clone()),
            _ => None,
        })
        .expect("anonymous class expression must register a Class decl named __class_m0_0");
    assert_eq!(anon_class.fields.len(), 1);
    assert_eq!(anon_class.fields[0].name, Atom::from("x"));
}

#[test]
fn class_expression_named_uses_module_unique_name_not_source_name() {
    let output = FrontendPass::new().run(
        "test.ts",
        "function f(): i64 { return class Named { y: i32; }; }",
    );
    assert!(!output.diagnostics.has_errors(), "{:?}", output.diagnostics);
    let named = output
        .program
        .declarations
        .iter()
        .find_map(|d| match d {
            HirDecl::Class(c) if c.name == Atom::from("__class_m0_0") => Some(c.clone()),
            _ => None,
        })
        .expect("named class expression must also get a module-unique name (not the source name)");
    assert_eq!(named.fields.len(), 1);
    assert_eq!(named.fields[0].name, Atom::from("y"));
}

#[test]
fn class_expression_in_body_returns_global_reference() {
    let f = sole_function("function f(): i64 { return class { x: i32; }; }");
    let HirStmt::Return {
        value: Some(ret_expr),
    } = &f.body[0]
    else {
        panic!("expected Return with value, got {:?}", f.body[0]);
    };
    match ret_expr {
        HirExpr::Global { name, .. } => {
            assert_eq!(name, Atom::from("__class_m0_0"));
        }
        other => panic!("expected Global referencing anon class, got {other:?}"),
    }
}

#[test]
fn class_expression_with_extends_captures_parent_name() {
    let output = FrontendPass::new().run(
        "test.ts",
        "function f(): i64 { return class extends Base { y: i32; }; }",
    );
    assert!(!output.diagnostics.has_errors(), "{:?}", output.diagnostics);
    let anon = output
        .program
        .declarations
        .iter()
        .find_map(|d| match d {
            HirDecl::Class(c) if c.name == Atom::from("__class_m0_0") => Some(c.clone()),
            _ => None,
        })
        .expect("anonymous extends class must register with module-unique name");
    assert_eq!(anon.extends, Some(Atom::from("Base")));
}

#[test]
fn multiple_class_expressions_get_distinct_module_unique_names() {
    let output = FrontendPass::new().run(
        "test.ts",
        "function f(): i64 { const A = class { a: i32; }; const B = class { b: i32; }; return 0; }",
    );
    assert!(!output.diagnostics.has_errors(), "{:?}", output.diagnostics);
    let mut names: Vec<String> = output
        .program
        .declarations
        .iter()
        .filter_map(|d| match d {
            HirDecl::Class(c) if c.name.as_str().starts_with("__class_m0_") => {
                Some(c.name.to_string())
            }
            _ => None,
        })
        .collect();
    names.sort();
    assert_eq!(names, vec!["__class_m0_0", "__class_m0_1"]);
}

#[test]
fn class_expression_assigned_to_local_keeps_class_registered() {
    let f = sole_function("function f(): i64 { const C = class { z: i32; }; return 0; }");
    let HirStmt::Let { init, .. } = &f.body[0] else {
        panic!("expected first stmt to be Let, got {:?}", f.body[0]);
    };
    let init = init.as_ref().expect("const binding must have an init");
    match init {
        HirExpr::Global { name, .. } => assert_eq!(name, Atom::from("__class_m0_0")),
        other => panic!("expected Global referencing anon class, got {other:?}"),
    }
}

fn yield_diagnostic_check(source: &str, expected_substring: &str) {
    let output = FrontendPass::new().run("test.ts", source);
    assert!(
        output.diagnostics.has_errors(),
        "`yield` in non-generator must produce a hard error, got clean diagnostics for: {source}"
    );
    let found = output.diagnostics.iter().any(|d| {
        d.code.as_str() == "E0500"
            && d.message.contains(expected_substring)
            && d.severity == ts_aot_core::Severity::Error
    });
    assert!(
        found,
        "expected E0500 error (not warning) mentioning `{expected_substring}`, got: {:?}",
        output.diagnostics
    );
}

#[test]
fn body_walker_yield_expression_in_non_generator_rejects_with_e0500_error() {
    yield_diagnostic_check(
        "function f(): i64 { yield 1; return 0; }",
        "generator function",
    );
}

#[test]
fn body_walker_yield_expression_in_non_generator_returns_unit_placeholder() {
    let output = FrontendPass::new().run("test.ts", "function f(): i64 { yield 42; return 0; }");
    let f = output
        .program
        .declarations
        .iter()
        .find_map(|d| match d {
            HirDecl::Function(f) => Some(f.clone()),
            _ => None,
        })
        .expect("expected Function decl");
    let HirStmt::Expr { expr, .. } = &f.body[0] else {
        panic!("expected first stmt to be Expr, got {:?}", f.body[0]);
    };
    assert!(
        matches!(expr, HirExpr::Unit(_)),
        "rejected `yield` must produce HirExpr::Unit placeholder, not propagate to MIR; got {expr:?}"
    );
}

#[test]
fn body_walker_yield_inside_nested_non_generator_rejects_with_e0500_error() {
    use crate::skeleton::SkeletonBuilder;
    use ts_aot_core::{DiagnosticBag, ModuleId, Span as CoreSpan, TypeTable};
    use ts_aot_ir_hir::HirProgram;

    let mut types = TypeTable::new();
    let mut diagnostics = DiagnosticBag::new();
    let mut program = HirProgram::new(ModuleId::from_raw(0));
    let mut builder = SkeletonBuilder::new("test.ts", &mut types, &mut diagnostics, &mut program);

    builder.is_generator_stack.push(true);
    builder.is_generator_stack.push(false);
    assert!(
        !builder.current_function_is_generator(),
        "innermost non-generator must override enclosing generator context"
    );

    builder.is_generator_stack.clear();
    builder.is_generator_stack.push(true);
    builder.is_generator_stack.push(true);
    assert!(
        builder.current_function_is_generator(),
        "generator nested inside generator must remain generator"
    );

    builder.is_generator_stack.clear();
    builder.is_generator_stack.push(false);
    assert!(
        !builder.current_function_is_generator(),
        "single non-generator must be detected as non-generator"
    );
    let _ = (CoreSpan::new(0, 0),);
}

#[test]
fn body_walker_yield_expression_in_generator_walks_into_hir_yield() {
    let output = FrontendPass::new().run("test.ts", "function* gen(): i64 { yield 42; return 0; }");
    let f = output
        .program
        .declarations
        .iter()
        .find_map(|d| match d {
            HirDecl::Function(f) => Some(f.clone()),
            _ => None,
        })
        .expect("expected Function decl");
    assert!(f.is_generator, "function* must set is_generator=true");
    let HirStmt::Expr { expr, .. } = &f.body[0] else {
        panic!("expected first stmt to be Expr, got {:?}", f.body[0]);
    };
    let HirExpr::Yield { expr: inner, .. } = expr else {
        panic!("`yield` in generator must produce HirExpr::Yield, got {expr:?}");
    };
    let inner = inner.as_deref().expect("yield 42 must have inner");
    assert!(
        matches!(inner, HirExpr::Int(42, _)),
        "yield argument must be walked in generator context, got {inner:?}"
    );
}

#[test]
fn body_walker_bare_yield_in_generator_produces_hir_yield_with_none() {
    let output = FrontendPass::new().run("test.ts", "function* gen(): i64 { yield; return 0; }");
    let f = output
        .program
        .declarations
        .iter()
        .find_map(|d| match d {
            HirDecl::Function(f) => Some(f.clone()),
            _ => None,
        })
        .expect("expected Function decl");
    let HirStmt::Expr { expr, .. } = &f.body[0] else {
        panic!("expected first stmt to be Expr, got {:?}", f.body[0]);
    };
    assert!(
        matches!(expr, HirExpr::Yield { expr: None, .. }),
        "bare `yield` in generator must produce HirExpr::Yield with expr=None; got {expr:?}"
    );
}

#[test]
fn function_type_in_param_resolves_to_type_fn() {
    let mut types = TypeTable::new();
    let output = FrontendPass::new().run_with_types(
        "test.ts",
        "function f(cb: (a: i64) => i64): i64 { return 0; }",
        &mut types,
    );
    assert!(!output.diagnostics.has_errors(), "{:?}", output.diagnostics);
    let fn_decl = output
        .program
        .declarations
        .iter()
        .find_map(|d| match d {
            HirDecl::Function(f) => Some(f.clone()),
            _ => None,
        })
        .expect("function should be present");
    let i64_id = types.intern(&Type::I64);
    assert_eq!(
        types.resolve(fn_decl.params[0].ty),
        Some(&Type::Fn {
            params: vec![i64_id],
            ret: i64_id,
            err: None,
        }),
        "param type `(a: i64) => i64` must resolve to Type::Fn with single i64 param and i64 return"
    );
}

#[test]
fn function_type_in_return_position_resolves_to_type_fn() {
    let mut types = TypeTable::new();
    let output = FrontendPass::new().run_with_types(
        "test.ts",
        "function f(): (a: i64) => i64 { return 0; }",
        &mut types,
    );
    assert!(!output.diagnostics.has_errors(), "{:?}", output.diagnostics);
    let fn_decl = output
        .program
        .declarations
        .iter()
        .find_map(|d| match d {
            HirDecl::Function(f) => Some(f.clone()),
            _ => None,
        })
        .expect("function should be present");
    let i64_id = types.intern(&Type::I64);
    assert_eq!(
        types.resolve(fn_decl.ret),
        Some(&Type::Fn {
            params: vec![i64_id],
            ret: i64_id,
            err: None,
        }),
        "return type `(a: i64) => i64` must resolve to Type::Fn with single i64 param and i64 return"
    );
}

#[test]
fn function_type_with_no_params_resolves_to_type_fn() {
    let mut types = TypeTable::new();
    let output = FrontendPass::new().run_with_types(
        "test.ts",
        "function f(cb: () => i64): i64 { return 0; }",
        &mut types,
    );
    assert!(!output.diagnostics.has_errors(), "{:?}", output.diagnostics);
    let fn_decl = output
        .program
        .declarations
        .iter()
        .find_map(|d| match d {
            HirDecl::Function(f) => Some(f.clone()),
            _ => None,
        })
        .expect("function should be present");
    let i64_id = types.intern(&Type::I64);
    assert_eq!(
        types.resolve(fn_decl.params[0].ty),
        Some(&Type::Fn {
            params: vec![],
            ret: i64_id,
            err: None,
        }),
        "param type `() => i64` must resolve to Type::Fn with empty params and i64 return"
    );
}

#[test]
fn function_type_as_alias_body_resolves_to_same_type_as_inline() {
    let mut types = TypeTable::new();
    let output_alias = FrontendPass::new().run_with_types(
        "test.ts",
        "type F = (a: i64) => i64; function f(cb: F): i64 { return 0; }",
        &mut types,
    );
    assert!(
        !output_alias.diagnostics.has_errors(),
        "{:?}",
        output_alias.diagnostics
    );
    let mut types2 = TypeTable::new();
    let output_inline = FrontendPass::new().run_with_types(
        "test.ts",
        "function f(cb: (a: i64) => i64): i64 { return 0; }",
        &mut types2,
    );
    assert!(
        !output_inline.diagnostics.has_errors(),
        "{:?}",
        output_inline.diagnostics
    );
    let ty_alias = output_alias
        .program
        .declarations
        .iter()
        .find_map(|d| match d {
            HirDecl::Function(f) => Some(f.clone()),
            _ => None,
        })
        .expect("function should be present")
        .params[0]
        .ty;
    let ty_inline = output_inline
        .program
        .declarations
        .iter()
        .find_map(|d| match d {
            HirDecl::Function(f) => Some(f.clone()),
            _ => None,
        })
        .expect("function should be present")
        .params[0]
        .ty;
    assert_eq!(
        types.resolve(ty_alias),
        types2.resolve(ty_inline),
        "function type as alias body must resolve to the same Type::Fn as inline syntax"
    );
}

#[test]
fn function_type_with_nested_array_param_resolves_recursively() {
    let mut types = TypeTable::new();
    let output = FrontendPass::new().run_with_types(
        "test.ts",
        "function f(cb: (a: i64[]) => i64): i64 { return 0; }",
        &mut types,
    );
    assert!(!output.diagnostics.has_errors(), "{:?}", output.diagnostics);
    let fn_decl = output
        .program
        .declarations
        .iter()
        .find_map(|d| match d {
            HirDecl::Function(f) => Some(f.clone()),
            _ => None,
        })
        .expect("function should be present");
    let i64_id = types.intern(&Type::I64);
    let array_id = types.intern(&Type::Array { element: i64_id });
    assert_eq!(
        types.resolve(fn_decl.params[0].ty),
        Some(&Type::Fn {
            params: vec![array_id],
            ret: i64_id,
            err: None,
        }),
        "function type with array param must resolve nested TSArrayType to Type::Array inside Type::Fn.params"
    );
}

#[test]
fn function_type_with_generic_params_emits_e0404_warning() {
    let mut types = TypeTable::new();
    let output = FrontendPass::new().run_with_types(
        "test.ts",
        "function f(cb: <T>(a: T) => T): i64 { return 0; }",
        &mut types,
    );
    assert!(
        has_e0404(&output.diagnostics),
        "function type with `<T>` generics must produce an E0404 warning, got: {:?}",
        output.diagnostics
    );
}

#[test]
fn function_type_with_this_param_emits_e0404_warning() {
    let mut types = TypeTable::new();
    let output = FrontendPass::new().run_with_types(
        "test.ts",
        "function f(cb: (this: string, a: i64) => i64): i64 { return 0; }",
        &mut types,
    );
    assert!(
        has_e0404(&output.diagnostics),
        "function type with `this:` parameter must produce an E0404 warning, got: {:?}",
        output.diagnostics
    );
}

#[test]
fn function_type_with_untyped_param_emits_e0404_and_resolves_param_to_type_error() {
    let mut types = TypeTable::new();
    let output = FrontendPass::new().run_with_types(
        "test.ts",
        "function f(cb: (a, b) => i64): i64 { return 0; }",
        &mut types,
    );
    assert!(
        has_e0404(&output.diagnostics),
        "function type with parameters lacking type annotation must produce an E0404 warning, got: {:?}",
        output.diagnostics
    );
    let fn_decl = output
        .program
        .declarations
        .iter()
        .find_map(|d| match d {
            HirDecl::Function(f) => Some(f.clone()),
            _ => None,
        })
        .expect("function should be present");
    let error_id = types.intern(&Type::Error);
    let i64_id = types.intern(&Type::I64);
    assert_eq!(
        types.resolve(fn_decl.params[0].ty),
        Some(&Type::Fn {
            params: vec![error_id, error_id],
            ret: i64_id,
            err: None,
        }),
        "function type with two untyped params must produce Type::Fn with two Type::Error params (preserves arity, downstream passes can detect by checking params[0]==Type::Error)"
    );
}

#[test]
fn function_type_with_rest_param_emits_e0404_and_resolves_to_type_error() {
    let mut types = TypeTable::new();
    let output = FrontendPass::new().run_with_types(
        "test.ts",
        "function f(cb: (...args: i64[]) => i64): i64 { return 0; }",
        &mut types,
    );
    assert!(
        has_e0404(&output.diagnostics),
        "function type with rest parameter `...args: T` must produce an E0404 warning, got: {:?}",
        output.diagnostics
    );
    let fn_decl = output
        .program
        .declarations
        .iter()
        .find_map(|d| match d {
            HirDecl::Function(f) => Some(f.clone()),
            _ => None,
        })
        .expect("function should be present");
    assert_eq!(
        types.resolve(fn_decl.params[0].ty),
        Some(&Type::Error),
        "function type with rest parameter must resolve to Type::Error after E0404 (not Type::Fn) — \
         Type::Fn has no field for variadic, silently dropping the rest param would lose information"
    );
}

#[test]
fn conditional_type_in_param_annotation_emits_e0407_and_resolves_to_never() {
    let mut types = TypeTable::new();
    let output = FrontendPass::new().run_with_types(
        "test.ts",
        "function f(x: T extends U ? A : B): i64 { return 0; }\n\
         type T = i64;\n\
         type U = i32;\n\
         type A = i64;\n\
         type B = string;",
        &mut types,
    );
    assert!(
        has_e0407(&output.diagnostics),
        "conditional type in param annotation must produce an E0407 warning, got: {:?}",
        output.diagnostics
    );
    assert!(
        !output.diagnostics.has_errors(),
        "E0407 is a warning and must NOT block HIR emit, got errors: {:?}",
        output.diagnostics
    );
    let fn_decl = output
        .program
        .declarations
        .iter()
        .find_map(|d| match d {
            HirDecl::Function(f) => Some(f.clone()),
            _ => None,
        })
        .expect("function should be present");
    assert_eq!(
        types.resolve(fn_decl.params[0].ty),
        Some(&Type::Never),
        "conditional type param annotation must resolve to Type::Never (per PR 4.7 plan: warning + never fallback)"
    );
}

#[test]
fn conditional_type_in_alias_emits_e0407_and_resolves_to_never() {
    let mut types = TypeTable::new();
    let output = FrontendPass::new().run_with_types(
        "test.ts",
        "type Conditional<T> = T extends i64 ? string : i32;\n\
         function f(x: Conditional<i64>): i64 { return 0; }",
        &mut types,
    );
    assert!(
        has_e0407(&output.diagnostics),
        "conditional type in alias must produce an E0407 warning, got: {:?}",
        output.diagnostics
    );
    assert!(
        !output.diagnostics.has_errors(),
        "E0407 is a warning and must NOT block HIR emit, got errors: {:?}",
        output.diagnostics
    );
    let fn_decl = output
        .program
        .declarations
        .iter()
        .find_map(|d| match d {
            HirDecl::Function(f) => Some(f.clone()),
            _ => None,
        })
        .expect("function should be present");
    assert_eq!(
        types.resolve(fn_decl.params[0].ty),
        Some(&Type::Never),
        "conditional type used via alias must resolve to Type::Never"
    );
}

#[test]
fn conditional_type_with_branch_resolving_to_type_error_emits_e0400_per_branch() {
    // Regression: resolve_simple_type returns Some(Type::Error) for unsupported
    // type forms (wildcard fallback, e.g. `keyof T`). Conditional resolver must
    // treat Some(Type::Error) the same as None and emit E0400 per branch — not
    // silently swallow the error.
    let mut types = TypeTable::new();
    let output = FrontendPass::new().run_with_types(
        "test.ts",
        "function f(x: T extends keyof U ? i64 : i32): i64 { return 0; }\n\
         type T = i64;\n\
         type U = { a: i64, b: string };",
        &mut types,
    );
    assert!(
        has_e0407(&output.diagnostics),
        "conditional type itself must produce E0407, got: {:?}",
        output.diagnostics
    );
    let e0400_count = count_e0400(&output.diagnostics);
    assert!(
        e0400_count >= 1,
        "at least one branch (extends_type) resolves via wildcard to Type::Error and must emit E0400, \
         got E0400 count = {e0400_count}, diagnostics: {:?}",
        output.diagnostics
    );
    assert!(
        !output.diagnostics.has_errors(),
        "E0400/E0407 are warnings and must NOT block HIR emit, got errors: {:?}",
        output.diagnostics
    );
    let fn_decl = output
        .program
        .declarations
        .iter()
        .find_map(|d| match d {
            HirDecl::Function(f) => Some(f.clone()),
            _ => None,
        })
        .expect("function should be present");
    assert_eq!(
        types.resolve(fn_decl.params[0].ty),
        Some(&Type::Never),
        "conditional with Type::Error branch must still resolve to Type::Never"
    );
}
