use std::collections::HashMap;

use ts_aot_core::{Atom, GenericParamId, Severity, Type, TypeId, TypeTable};
use ts_aot_ir_hir::HirDecl;

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
            assert!(f.body.is_empty(), "PR-16 foundation leaves body empty");
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
            assert!(c.methods[0].body.is_empty());
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
            assert_eq!(method.params.len(), 1);
            assert_eq!(
                method.params[0].name,
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
            assert_eq!(types.resolve(method.params[0].ty), Some(&u_type));
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
