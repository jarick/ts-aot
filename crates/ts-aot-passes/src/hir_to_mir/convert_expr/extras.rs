#![allow(clippy::too_many_arguments)]

use std::collections::HashMap;

use ts_aot_core::{Span, StructId, Type, TypeId, TypeTable};
use ts_aot_ir_hir::HirExpr;
use ts_aot_ir_mir::{MirExpr, MirStmt};

use crate::PassContext;
use crate::hir_to_mir::PLACEHOLDER_FUNCTION;
use crate::hir_to_mir::convert_expr::util::has_potential_side_effects;
use crate::hir_to_mir::converter::ExprConverter;

impl ExprConverter {
    pub(super) fn convert_new(
        &mut self,
        callee: &HirExpr,
        args: &[HirExpr],
        ty: TypeId,
        out: &mut Vec<MirStmt>,
        shared_struct_ids: &mut HashMap<TypeId, StructId>,
        shared_next_struct: &mut u32,
        types: &mut TypeTable,
        ctx: &mut PassContext,
    ) -> MirExpr {
        let callee_mir = self.convert_expr(
            callee,
            out,
            shared_struct_ids,
            shared_next_struct,
            types,
            ctx,
        );
        if has_potential_side_effects(&callee_mir) {
            out.push(MirStmt::Expr(callee_mir));
        }
        let struct_id = self.lookup_or_alloc_struct_id(ty, shared_struct_ids, shared_next_struct);
        let alloc_id = self.fresh_local();
        self.push_temp_local(alloc_id, ty);
        out.push(MirStmt::Let {
            local: alloc_id,
            ty,
            init: Some(MirExpr::StructLiteral {
                struct_id,
                fields: Vec::new(),
                ty,
            }),
            mutable: true,
        });
        let ctor_callee = PLACEHOLDER_FUNCTION;
        let mut ctor_args: Vec<MirExpr> = Vec::with_capacity(args.len() + 1);
        ctor_args.push(MirExpr::Local(alloc_id));
        for a in args {
            ctor_args.push(self.convert_expr(
                a,
                out,
                shared_struct_ids,
                shared_next_struct,
                types,
                ctx,
            ));
        }
        out.push(MirStmt::Expr(MirExpr::Call {
            callee: ctor_callee,
            args: ctor_args,
            ty,
        }));
        MirExpr::Local(alloc_id)
    }

    pub(super) fn convert_optional_chain(
        &mut self,
        base: &HirExpr,
        out: &mut Vec<MirStmt>,
        shared_struct_ids: &mut HashMap<TypeId, StructId>,
        shared_next_struct: &mut u32,
        types: &mut TypeTable,
        ctx: &mut PassContext,
    ) -> MirExpr {
        let inner = self.convert_expr(base, out, shared_struct_ids, shared_next_struct, types, ctx);
        let base_ty = crate::monomorphize::hir_expr_ty(base, types)
            .unwrap_or_else(|| crate::hir_to_mir::convert_expr::util::mir_expr_ty(&inner));
        let inner_ty = match types.resolve(base_ty) {
            Some(Type::Optional { inner }) => *inner,
            _ => base_ty,
        };
        let opt_ty = types.intern(&Type::Optional { inner: inner_ty });
        MirExpr::OptionalChain {
            base: Box::new(inner),
            ty: opt_ty,
        }
    }

    pub(super) fn convert_type_assertion(
        &mut self,
        expr: &HirExpr,
        target: TypeId,
        out: &mut Vec<MirStmt>,
        shared_struct_ids: &mut HashMap<TypeId, StructId>,
        shared_next_struct: &mut u32,
        types: &mut TypeTable,
        ctx: &mut PassContext,
    ) -> MirExpr {
        let _ = target;
        self.convert_expr(expr, out, shared_struct_ids, shared_next_struct, types, ctx)
    }

    pub(super) fn convert_closure(&mut self, ty: TypeId, ctx: &mut PassContext) -> MirExpr {
        let _ = ty;
        ctx.error(
            "P0005",
            "closure expressions are not yet supported in HIR→MIR",
            Span::new(0, 0),
        );
        MirExpr::Unit
    }

    pub(super) fn convert_await(
        &mut self,
        expr: &HirExpr,
        ty: TypeId,
        out: &mut Vec<MirStmt>,
        shared_struct_ids: &mut HashMap<TypeId, StructId>,
        shared_next_struct: &mut u32,
        types: &mut TypeTable,
        ctx: &mut PassContext,
    ) -> MirExpr {
        let inner = self.convert_expr(expr, out, shared_struct_ids, shared_next_struct, types, ctx);
        MirExpr::Await {
            expr: Box::new(inner),
            ty,
        }
    }

    pub(super) fn convert_yield(
        &mut self,
        expr: Option<&HirExpr>,
        ty: TypeId,
        out: &mut Vec<MirStmt>,
        shared_struct_ids: &mut HashMap<TypeId, StructId>,
        shared_next_struct: &mut u32,
        types: &mut TypeTable,
        ctx: &mut PassContext,
    ) -> MirExpr {
        let inner = expr
            .map(|e| self.convert_expr(e, out, shared_struct_ids, shared_next_struct, types, ctx))
            .map(Box::new);
        MirExpr::Yield { expr: inner, ty }
    }
}
