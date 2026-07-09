use std::collections::HashMap;

use ts_aot_core::{Atom, GenericParamId, LocalId, Severity, Type, TypeId, TypeTable};
use ts_aot_ir_hir::{HirBinaryOp, HirCallee, HirDecl, HirExpr, HirFunction, HirStmt};

use super::*;
use crate::type_resolver::TypeParamMap;

const PARSE_ERROR_CODE: &str = "E0200";
const PARSE_PANIC_CODE: &str = "E0100";

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
            assert_eq!(
                f.body,
                vec![HirStmt::Return {
                    value: Some(HirExpr::Int(0)),
                }],
                "walker fills the body with the `return 0;` statement"
            );
            assert!(!f.is_async);
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
            assert_eq!(
                c.methods[0].body,
                vec![HirStmt::Return {
                    value: Some(HirExpr::Int(0)),
                }],
                "method bodies are walked now that `this` is the receiver param"
            );
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
    match &output.program.declarations[0] {
        HirDecl::Function(f) => f.clone(),
        other => panic!("expected Function, got {other:?}"),
    }
}

#[test]
fn body_walker_lets_binding_gets_local_id_and_init() {
    let f = sole_function("function f(): i32 { let x: i32 = 5; return x; }");
    let let_ty = match &f.body[0] {
        HirStmt::Let { ty, .. } => *ty,
        other => panic!("expected first stmt to be Let, got {other:?}"),
    };
    assert_eq!(
        f.body,
        vec![
            HirStmt::Let {
                id: LocalId::from_raw(0),
                name: Atom::from("x"),
                ty: let_ty,
                init: Some(HirExpr::Int(5)),
            },
            HirStmt::Return {
                value: Some(HirExpr::Local {
                    id: LocalId::from_raw(0),
                    ty: let_ty,
                }),
            },
        ],
    );
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
            assert_eq!(**rhs, HirExpr::Int(2));
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
            assert_eq!(**rhs, HirExpr::Int(1));
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
            assert_eq!(**rhs, HirExpr::Int(1));
            assert!(!*post, "pre-increment must be flagged post=false");
        }
        other => panic!("expected Expr(CompoundUpdate), got {other:?}"),
    }
}

#[test]
fn body_walker_compound_update_does_not_clone_target_side_effects() {
    let f = sole_function("function f(o: any, k: any): void { o[k()]++; }");
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
    assert_eq!(**rhs, HirExpr::Int(1));

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
    let output = FrontendPass::new().run("test.ts", "function f(): void { [1, 2, 3]; }");
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
