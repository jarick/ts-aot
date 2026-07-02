use std::collections::HashMap;

use ts2zig_core::{Atom, Span, Type, TypeTable};
use ts2zig_ir_hir::{HirDecl, HirExpr, HirProgram, HirStmt};

use crate::PassContext;

pub fn lower_enums(program: &mut HirProgram, types: &mut TypeTable, ctx: &mut PassContext) {
    let i64_ty = types.intern(&Type::I64);
    let mut rewritten: Vec<HirDecl> = Vec::with_capacity(program.declarations.len());
    let mut variant_map: HashMap<(Atom, Atom), Atom> = HashMap::new();

    for decl in program.declarations.drain(..) {
        match decl {
            HirDecl::Enum { name, variants } => {
                let enum_raw = name.as_str().to_owned();
                rewritten.push(HirDecl::TypeAlias {
                    name: name.clone(),
                    target: i64_ty,
                });
                let mut next_value: i128 = 0;
                for variant in variants {
                    let value: i64 = match variant.value {
                        Some(HirExpr::Int(v)) => {
                            next_value = i128::from(v) + 1;
                            v
                        }
                        _ => {
                            let v: i64 = match i64::try_from(next_value) {
                                Ok(v) => v,
                                Err(_) => {
                                    ctx.error(
                                        "P0007",
                                        format!(
                                            "enum variant accumulator overflows i64 (current: {next_value})"
                                        ),
                                        Span::new(0, 0),
                                    );
                                    i64::MAX
                                }
                            };
                            next_value = next_value.saturating_add(1);
                            v
                        }
                    };
                    let raw = variant.name.as_str();
                    let namespaced = format!("{enum_raw}.{raw}");
                    let namespaced_sym = Atom::from(namespaced);
                    variant_map.insert((name.clone(), variant.name), namespaced_sym.clone());
                    rewritten.push(HirDecl::Global {
                        name: namespaced_sym,
                        ty: i64_ty,
                        init: Some(HirExpr::Int(value)),
                    });
                }
            }
            other => rewritten.push(other),
        }
    }

    program.declarations = rewritten;

    if !variant_map.is_empty() {
        for decl in &mut program.declarations {
            rewrite_decl(decl, &variant_map);
        }
    }
}

fn rewrite_decl(decl: &mut HirDecl, map: &HashMap<(Atom, Atom), Atom>) {
    match decl {
        HirDecl::Function(f) => rewrite_body(&mut f.body, map),
        HirDecl::Class(c) => {
            for m in &mut c.methods {
                rewrite_body(&mut m.body, map);
            }
        }
        HirDecl::Global { init, .. } => {
            if let Some(e) = init.as_mut() {
                rewrite_expr(e, map);
            }
        }
        HirDecl::TypeAlias { .. }
        | HirDecl::Enum { .. }
        | HirDecl::Interface { .. }
        | HirDecl::Namespace { .. } => {}
    }
}

fn rewrite_body(body: &mut [HirStmt], map: &HashMap<(Atom, Atom), Atom>) {
    for stmt in body.iter_mut() {
        rewrite_stmt(stmt, map);
    }
}

fn rewrite_stmt(stmt: &mut HirStmt, map: &HashMap<(Atom, Atom), Atom>) {
    match stmt {
        HirStmt::Block(stmts) => rewrite_body(stmts, map),
        HirStmt::Let { init, .. } => {
            if let Some(e) = init.as_mut() {
                rewrite_expr(e, map);
            }
        }
        HirStmt::Expr { expr } => rewrite_expr(expr, map),
        HirStmt::Return { value } => {
            if let Some(e) = value.as_mut() {
                rewrite_expr(e, map);
            }
        }
        HirStmt::Throw { expr } => rewrite_expr(expr, map),
        HirStmt::If {
            cond,
            then,
            otherwise,
        } => {
            rewrite_expr(cond, map);
            rewrite_stmt(then, map);
            if let Some(o) = otherwise.as_mut() {
                rewrite_stmt(o, map);
            }
        }
        HirStmt::While { cond, body } | HirStmt::DoWhile { body, cond } => {
            rewrite_expr(cond, map);
            rewrite_stmt(body, map);
        }
        HirStmt::ForOf { iter, body, .. } | HirStmt::ForIn { iter, body, .. } => {
            rewrite_expr(iter, map);
            rewrite_stmt(body, map);
        }
        HirStmt::Switch { disc, cases } => {
            rewrite_expr(disc, map);
            for case in cases {
                if let Some(test) = case.test.as_mut() {
                    rewrite_expr(test, map);
                }
                rewrite_body(&mut case.body, map);
            }
        }
        HirStmt::Try {
            body,
            catch,
            finally,
        } => {
            rewrite_stmt(body, map);
            if let Some(c) = catch.as_mut() {
                rewrite_stmt(&mut c.body, map);
            }
            if let Some(f) = finally.as_mut() {
                rewrite_stmt(f, map);
            }
        }
        HirStmt::Decl(decl) => rewrite_decl(decl, map),
        HirStmt::Break { .. } | HirStmt::Continue { .. } => {}
    }
}

fn rewrite_expr(expr: &mut HirExpr, map: &HashMap<(Atom, Atom), Atom>) {
    match expr {
        HirExpr::Field {
            owner,
            field_name,
            ty,
            ..
        } => {
            let enum_name = match owner.as_ref() {
                HirExpr::Global { name, .. } => Some(name.clone()),
                _ => None,
            };
            if let Some(enum_name) = enum_name
                && let Some(namespaced) = map.get(&(enum_name, field_name.clone()))
            {
                *expr = HirExpr::Global {
                    name: namespaced.clone(),
                    ty: *ty,
                };
            } else {
                rewrite_expr(owner, map);
            }
        }
        HirExpr::Call { callee, args, .. } => {
            rewrite_callee(callee, map);
            for a in args {
                rewrite_expr(a, map);
            }
        }
        HirExpr::Binary { lhs, rhs, .. } => {
            rewrite_expr(lhs, map);
            rewrite_expr(rhs, map);
        }
        HirExpr::Unary { expr: e, .. } => rewrite_expr(e, map),
        HirExpr::Index { owner, index, .. } => {
            rewrite_expr(owner, map);
            rewrite_expr(index, map);
        }
        HirExpr::Assignment { target, value, .. } => {
            rewrite_expr(target, map);
            rewrite_expr(value, map);
        }
        HirExpr::StructLiteral { fields, .. } => {
            for (_, e) in fields {
                rewrite_expr(e, map);
            }
        }
        HirExpr::ArrayLiteral { elements, .. } => {
            for e in elements {
                rewrite_expr(e, map);
            }
        }
        HirExpr::Closure { captures, .. } => {
            for c in captures {
                rewrite_expr(c, map);
            }
        }
        HirExpr::Await { expr, .. } => rewrite_expr(expr, map),
        HirExpr::Yield { expr, .. } => {
            if let Some(e) = expr.as_mut() {
                rewrite_expr(e, map);
            }
        }
        HirExpr::Template { tag, parts, .. } => {
            if let Some(t) = tag.as_mut() {
                rewrite_expr(t, map);
            }
            for p in parts {
                rewrite_expr(p, map);
            }
        }
        HirExpr::New { callee, args, .. } => {
            rewrite_expr(callee, map);
            for a in args {
                rewrite_expr(a, map);
            }
        }
        HirExpr::OptionalChain { base, .. } => rewrite_expr(base, map),
        HirExpr::TypeAssertion { expr, .. } => rewrite_expr(expr, map),
        HirExpr::Global { .. }
        | HirExpr::Local { .. }
        | HirExpr::Unit
        | HirExpr::Bool(_)
        | HirExpr::Int(_)
        | HirExpr::Float(_)
        | HirExpr::String(_)
        | HirExpr::Null
        | HirExpr::Undefined => {}
    }
}

fn rewrite_callee(callee: &mut ts2zig_ir_hir::HirCallee, map: &HashMap<(Atom, Atom), Atom>) {
    match callee {
        ts2zig_ir_hir::HirCallee::Function(_)
        | ts2zig_ir_hir::HirCallee::Closure(_)
        | ts2zig_ir_hir::HirCallee::Runtime { .. } => {}
        ts2zig_ir_hir::HirCallee::Indirect(expr) => rewrite_expr(expr, map),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PassContext;
    use ts2zig_core::{Atom, TypeTable};
    use ts2zig_ir_hir::HirEnumVariant;

    fn enum_decl(name: u32, variants: Vec<(u32, Option<i64>)>) -> HirDecl {
        HirDecl::Enum {
            name: Atom::from(format!("e{}", name)),
            variants: variants
                .into_iter()
                .map(|(n, v)| HirEnumVariant {
                    name: Atom::from(format!("v{}", n)),
                    value: v.map(HirExpr::Int),
                })
                .collect(),
        }
    }

    fn setup() -> (HirProgram, TypeTable, PassContext) {
        let types = TypeTable::new();
        let ctx = PassContext::default();
        (
            HirProgram::new(ts2zig_core::ModuleId::from_raw(0)),
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
                    init: Some(HirExpr::Int(v)),
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
                    init: Some(HirExpr::Int(v)),
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
                    init: Some(HirExpr::Int(v)),
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
        assert!(matches!(init, Some(HirExpr::Int(7))));
    }

    #[test]
    fn float_variant_initialiser_falls_back_to_accumulator() {
        let (mut program, mut types, mut ctx) = setup();
        let value_str = Atom::new_inline("1.5");
        program
            .declarations
            .push(enum_decl(1, vec![(10, None), (11, None)]));
        // Mutate the just-pushed enum to inject a Float initialiser.
        if let HirDecl::Enum { variants, .. } = &mut program.declarations[0] {
            variants.push(HirEnumVariant {
                name: Atom::new_inline("12"),
                value: Some(HirExpr::Float(value_str.as_str().parse().unwrap_or(0))),
            });
        }

        lower_enums(&mut program, &mut types, &mut ctx);

        let values: Vec<i64> = program
            .declarations
            .iter()
            .filter_map(|d| match d {
                HirDecl::Global {
                    init: Some(HirExpr::Int(v)),
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
                value: v.map(HirExpr::Int),
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
                value: Some(HirExpr::Int(i64::MAX - 1)),
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
                    init: Some(HirExpr::Int(v)),
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
}
