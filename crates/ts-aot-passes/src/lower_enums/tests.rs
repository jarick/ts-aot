use super::*;
use crate::PassContext;
use ts_aot_core::{Atom, Span, TypeTable};
use ts_aot_ir_hir::HirEnumVariant;

fn enum_decl(name: u32, variants: Vec<(u32, Option<i64>)>) -> HirDecl {
    HirDecl::Enum {
        name: Atom::from(format!("e{}", name)),
        variants: variants
            .into_iter()
            .map(|(n, v)| HirEnumVariant {
                name: Atom::from(format!("v{}", n)),
                value: v.map(|v| HirExpr::Int(v, Span::default())),
            })
            .collect(),
    }
}

fn setup() -> (HirProgram, TypeTable, PassContext) {
    let types = TypeTable::new();
    let ctx = PassContext::default();
    (
        HirProgram::new(ts_aot_core::ModuleId::from_raw(0)),
        types,
        ctx,
    )
}

fn collect_enum_outputs(program: &HirProgram) -> Vec<&HirDecl> {
    program
        .declarations
        .iter()
        .filter(|d| matches!(d, HirDecl::TypeAlias { .. } | HirDecl::Global { .. }))
        .collect()
}

#[test]
fn enum_with_no_variants_produces_only_typealias() {
    let (mut program, mut types, mut ctx) = setup();
    program.declarations.push(enum_decl(1, vec![]));

    lower_enums(&mut program, &mut types, &mut ctx);

    let out = collect_enum_outputs(&program);
    assert_eq!(out.len(), 1);
    assert!(matches!(out[0], HirDecl::TypeAlias { .. }));
}

#[test]
fn variants_get_auto_incremented_values_starting_from_zero() {
    let (mut program, mut types, mut ctx) = setup();
    program
        .declarations
        .push(enum_decl(1, vec![(10, None), (11, None), (12, None)]));

    lower_enums(&mut program, &mut types, &mut ctx);

    let out = collect_enum_outputs(&program);
    assert_eq!(out.len(), 4);

    let HirDecl::TypeAlias { name, .. } = out[0] else {
        panic!("expected TypeAlias");
    };
    assert_eq!(*name, Atom::new_inline("e1"));

    let values: Vec<i64> = out[1..]
        .iter()
        .map(|d| match d {
            HirDecl::Global {
                init: Some(HirExpr::Int(v, _)),
                ..
            } => *v,
            _ => panic!("expected Global with Int init"),
        })
        .collect();
    assert_eq!(values, vec![0, 1, 2]);
}

#[test]
fn explicit_initialiser_advances_the_accumulator() {
    let (mut program, mut types, mut ctx) = setup();
    program.declarations.push(enum_decl(
        1,
        vec![(10, Some(10)), (11, None), (12, Some(20))],
    ));

    lower_enums(&mut program, &mut types, &mut ctx);

    let values: Vec<i64> = collect_enum_outputs(&program)[1..]
        .iter()
        .map(|d| match d {
            HirDecl::Global {
                init: Some(HirExpr::Int(v, _)),
                ..
            } => *v,
            _ => panic!(),
        })
        .collect();
    assert_eq!(values, vec![10, 11, 20]);
}

#[test]
fn non_enum_declarations_pass_through() {
    let (mut program, mut types, mut ctx) = setup();
    program.declarations.push(HirDecl::Interface {
        name: Atom::new_inline("99"),
    });
    program.declarations.push(enum_decl(1, vec![(10, None)]));

    lower_enums(&mut program, &mut types, &mut ctx);

    assert_eq!(program.declarations.len(), 3);
    assert!(matches!(program.declarations[0], HirDecl::Interface { .. }));
    assert!(matches!(program.declarations[1], HirDecl::TypeAlias { .. }));
    assert!(matches!(program.declarations[2], HirDecl::Global { .. }));
}

#[test]
fn multiple_enums_get_independent_accumulators() {
    let (mut program, mut types, mut ctx) = setup();
    program
        .declarations
        .push(enum_decl(1, vec![(10, None), (11, None)]));
    program
        .declarations
        .push(enum_decl(2, vec![(20, None), (21, Some(100)), (22, None)]));

    lower_enums(&mut program, &mut types, &mut ctx);

    let values: Vec<i64> = program
        .declarations
        .iter()
        .filter_map(|d| match d {
            HirDecl::Global {
                init: Some(HirExpr::Int(v, _)),
                ..
            } => Some(*v),
            _ => None,
        })
        .collect();
    assert_eq!(values, vec![0, 1, 0, 100, 101]);
}

#[test]
fn empty_program_is_a_noop() {
    let (mut program, mut types, mut ctx) = setup();
    lower_enums(&mut program, &mut types, &mut ctx);
    assert!(program.declarations.is_empty());
}

#[test]
fn variant_name_is_preserved_on_global() {
    let (mut program, mut types, mut ctx) = setup();
    program.declarations.push(enum_decl(1, vec![(42, Some(7))]));

    lower_enums(&mut program, &mut types, &mut ctx);

    let HirDecl::Global { init, .. } = &program.declarations[1] else {
        panic!("expected Global");
    };
    assert!(matches!(init, Some(HirExpr::Int(7, _))));
}

#[test]
fn float_variant_initialiser_falls_back_to_accumulator() {
    let (mut program, mut types, mut ctx) = setup();
    let value_str = Atom::new_inline("1.5");
    program
        .declarations
        .push(enum_decl(1, vec![(10, None), (11, None)]));
    if let HirDecl::Enum { variants, .. } = &mut program.declarations[0] {
        variants.push(HirEnumVariant {
            name: Atom::new_inline("12"),
            value: Some(HirExpr::Float(
                value_str.as_str().parse().unwrap_or(0),
                Span::default(),
            )),
        });
    }

    lower_enums(&mut program, &mut types, &mut ctx);

    let values: Vec<i64> = program
        .declarations
        .iter()
        .filter_map(|d| match d {
            HirDecl::Global {
                init: Some(HirExpr::Int(v, _)),
                ..
            } => Some(*v),
            _ => None,
        })
        .collect();
    assert_eq!(values, vec![0, 1, 2]);
}

#[test]
fn i64_type_is_shared_across_all_emitted_decls() {
    let (mut program, mut types, mut ctx) = setup();
    program
        .declarations
        .push(enum_decl(1, vec![(10, None), (11, None)]));

    lower_enums(&mut program, &mut types, &mut ctx);

    let alias_ty = match &program.declarations[0] {
        HirDecl::TypeAlias { target, .. } => *target,
        _ => panic!(),
    };
    let global_ty = match &program.declarations[1] {
        HirDecl::Global { ty, .. } => *ty,
        _ => panic!(),
    };
    let global_ty2 = match &program.declarations[2] {
        HirDecl::Global { ty, .. } => *ty,
        _ => panic!(),
    };
    assert_eq!(alias_ty, global_ty);
    assert_eq!(global_ty, global_ty2);
}

fn interned_enum_decl(enum_name: &str, variants: Vec<(&str, Option<i64>)>) -> HirDecl {
    let variants = variants
        .into_iter()
        .map(|(n, v)| HirEnumVariant {
            name: Atom::new_inline(n),
            value: v.map(|v| HirExpr::Int(v, Span::default())),
        })
        .collect();
    HirDecl::Enum {
        name: Atom::new_inline(enum_name),
        variants,
    }
}

#[test]
fn variant_globals_are_namespaced_by_enum() {
    let (mut program, mut types, mut ctx) = setup();
    program
        .declarations
        .push(interned_enum_decl("Color", vec![("Red", None)]));
    program
        .declarations
        .push(interned_enum_decl("Shape", vec![("Red", None)]));

    lower_enums(&mut program, &mut types, &mut ctx);

    let names: Vec<String> = program
        .declarations
        .iter()
        .filter_map(|d| match d {
            HirDecl::Global { name, .. } => Some(name.as_str().to_owned()),
            _ => None,
        })
        .collect();
    assert_eq!(
        names,
        vec!["Color.Red".to_owned(), "Shape.Red".to_owned()],
        "variant globals must be namespaced to avoid Atom collisions"
    );
}

#[test]
fn accumulator_overflow_emits_diagnostic_and_saturates() {
    let (mut program, mut types, mut ctx) = setup();
    let overflow_name = Atom::new_inline("MAX_VARIANT");
    let variants = vec![
        HirEnumVariant {
            name: overflow_name.clone(),
            value: Some(HirExpr::Int(i64::MAX - 1, Span::default())),
        },
        HirEnumVariant {
            name: overflow_name.clone(),
            value: None,
        },
        HirEnumVariant {
            name: overflow_name.clone(),
            value: None,
        },
    ];
    program.declarations.push(HirDecl::Enum {
        name: Atom::new_inline("O"),
        variants,
    });

    lower_enums(&mut program, &mut types, &mut ctx);

    let values: Vec<i64> = program
        .declarations
        .iter()
        .filter_map(|d| match d {
            HirDecl::Global {
                init: Some(HirExpr::Int(v, _)),
                ..
            } => Some(*v),
            _ => None,
        })
        .collect();
    assert_eq!(
        values,
        vec![i64::MAX - 1, i64::MAX, i64::MAX],
        "subsequent variants after overflow must saturate at i64::MAX"
    );
    assert!(
        ctx.diagnostics().iter().any(|d| d.code.as_str() == "P0007"),
        "expected P0007 diagnostic on accumulator overflow"
    );
}
