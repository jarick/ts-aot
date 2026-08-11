use std::collections::HashMap;

use ts_aot_core::{Span, StructId, Type, TypeId, TypeTable};
use ts_aot_ir_hir::{HirCallee, HirExpr};
use ts_aot_ir_mir::{MirExpr, MirStmt, RuntimeOp};

use crate::PassContext;
use crate::hir_to_mir::convert_expr::util::hir_expr_type_id;
use crate::hir_to_mir::converter::ExprConverter;

use super::globals::{
    SymbolArgKind, is_global_symbol_reference, is_string_typed_source, type_label,
};

impl ExprConverter {
    pub(in crate::hir_to_mir::convert_expr) fn try_symbol_global_dispatch(
        &mut self,
        callee: &HirCallee,
        args: &[HirExpr],
        ty: TypeId,
        out: &mut Vec<MirStmt>,
        shared_struct_ids: &mut HashMap<TypeId, StructId>,
        shared_next_struct: &mut u32,
        types: &mut TypeTable,
        ctx: &mut PassContext,
    ) -> Option<MirExpr> {
        let HirCallee::Indirect(inner) = callee else {
            return None;
        };
        let HirExpr::Global {
            name: sym_global, ..
        } = inner.as_ref()
        else {
            return None;
        };
        if sym_global.as_str() != "Symbol" {
            return None;
        }
        if args.len() > 1 {
            ctx.error(
                "E0406",
                format!(
                    "Symbol() takes 0 or 1 argument (description); got {}",
                    args.len()
                ),
                Span::new(0, 0),
            );
            return Some(MirExpr::Unit);
        }
        if args.len() == 1
            && !is_string_typed_source(&args[0], types)
            && !matches!(
                &args[0],
                HirExpr::Unit(_) | HirExpr::Null(_) | HirExpr::Undefined(_)
            )
        {
            ctx.error(
                "E0406",
                "Symbol(description) argument must be a string, undefined, or null \
                 (per ECMAScript spec); got non-string expression. The runtime call \
                 __ts_aot_symbol_new was not emitted. Coerce the description to a string \
                 (e.g. String(v)) before calling Symbol(v)."
                    .to_string(),
                Span::new(0, 0),
            );
            return Some(MirExpr::Unit);
        }
        let mir_args: Vec<MirExpr> = args
            .iter()
            .map(|a| self.convert_expr(a, out, shared_struct_ids, shared_next_struct, types, ctx))
            .collect();
        let dest = self.fresh_local();
        self.push_temp_local(dest, ty);
        out.push(MirStmt::Runtime {
            op: RuntimeOp::SymbolNew,
            args: mir_args,
            dest: Some(dest),
            ty,
            target_ty: None,
        });
        Some(MirExpr::Local(dest))
    }

    pub(in crate::hir_to_mir::convert_expr) fn try_symbol_static_method_dispatch(
        &mut self,
        callee: &HirCallee,
        args: &[HirExpr],
        ty: TypeId,
        out: &mut Vec<MirStmt>,
        shared_struct_ids: &mut HashMap<TypeId, StructId>,
        shared_next_struct: &mut u32,
        types: &mut TypeTable,
        ctx: &mut PassContext,
    ) -> Option<MirExpr> {
        let HirCallee::Indirect(inner) = callee else {
            return None;
        };
        let HirExpr::Field {
            owner: symbol_owner,
            field_name: symbol_field,
            ..
        } = inner.as_ref()
        else {
            return None;
        };
        if !is_global_symbol_reference(symbol_owner) {
            return None;
        }
        let (op, expected_arity, arg_kind) = match symbol_field.as_str() {
            "for" => (RuntimeOp::SymbolFor, 1usize, SymbolArgKind::String),
            "keyFor" => (RuntimeOp::SymbolKeyFor, 1usize, SymbolArgKind::Symbol),
            _ => return None,
        };
        if args.len() != expected_arity {
            ctx.error(
                "E0406",
                format!(
                    "Symbol.{} requires exactly {} argument(s); got {}",
                    symbol_field.as_str(),
                    expected_arity,
                    args.len()
                ),
                Span::new(0, 0),
            );
            return Some(MirExpr::Unit);
        }
        match arg_kind {
            SymbolArgKind::String => {
                if !is_string_typed_source(&args[0], types) {
                    ctx.error(
                        "E0406",
                        "Symbol.for() argument must be a string (per ECMAScript spec); got \
                         non-string expression. Coerce the key to a string (e.g. String(k)) \
                         before calling Symbol.for(k)."
                            .to_string(),
                        Span::new(0, 0),
                    );
                    return Some(MirExpr::Unit);
                }
            }
            SymbolArgKind::Symbol => {
                let Some(arg_ty) = hir_expr_type_id(&args[0]) else {
                    ctx.error(
                        "E0406",
                        "Symbol.keyFor() argument must be a Symbol (per ECMAScript spec); \
                         got expression with unresolvable type. The runtime call was not emitted."
                            .to_string(),
                        Span::new(0, 0),
                    );
                    return Some(MirExpr::Unit);
                };
                if !matches!(types.resolve(arg_ty), Some(Type::Symbol)) {
                    ctx.error(
                        "E0406",
                        format!(
                            "Symbol.keyFor() argument must be a Symbol (per ECMAScript spec); \
                             got expression of type `{}`. The runtime call was not emitted.",
                            type_label(types, arg_ty)
                        ),
                        Span::new(0, 0),
                    );
                    return Some(MirExpr::Unit);
                }
            }
        }
        let mir_args: Vec<MirExpr> = args
            .iter()
            .map(|a| self.convert_expr(a, out, shared_struct_ids, shared_next_struct, types, ctx))
            .collect();
        let result_ty = if op == RuntimeOp::SymbolKeyFor {
            let string_ty = types.intern(&Type::String);
            types.intern(&Type::Optional { inner: string_ty })
        } else {
            ty
        };
        let dest = self.fresh_local();
        self.push_temp_local(dest, result_ty);
        out.push(MirStmt::Runtime {
            op,
            args: mir_args,
            dest: Some(dest),
            ty: result_ty,
            target_ty: None,
        });
        Some(MirExpr::Local(dest))
    }
}
