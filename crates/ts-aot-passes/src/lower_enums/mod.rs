use std::collections::HashMap;

use ts_aot_core::{Atom, Span, Type, TypeTable};
use ts_aot_ir_hir::{HirDecl, HirExpr, HirProgram};

mod expr;
#[cfg(test)]
mod tests;
mod walk;

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
                    let (value, init_span): (i64, Span) = match variant.value {
                        Some(HirExpr::Int(v, span)) => {
                            next_value = i128::from(v) + 1;
                            (v, span)
                        }
                        Some(_) | None => {
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
                            (v, Span::default())
                        }
                    };
                    let raw = variant.name.as_str();
                    let namespaced = format!("{enum_raw}.{raw}");
                    let namespaced_sym = Atom::from(namespaced);
                    variant_map.insert((name.clone(), variant.name), namespaced_sym.clone());
                    rewritten.push(HirDecl::Global {
                        name: namespaced_sym,
                        ty: i64_ty,
                        init: Some(HirExpr::Int(value, init_span)),
                    });
                }
            }
            other => rewritten.push(other),
        }
    }

    program.declarations = rewritten;

    if !variant_map.is_empty() {
        for decl in &mut program.declarations {
            walk::rewrite_decl(decl, &variant_map);
        }
    }
}
