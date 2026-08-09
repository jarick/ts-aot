use std::collections::HashMap;

use ts_aot_core::{Span, StructId, Type, TypeId, TypeTable};
use ts_aot_ir_hir::HirExpr;
use ts_aot_ir_mir::{MirExpr, MirStmt, RuntimeOp};

use crate::PassContext;
use crate::hir_to_mir::PLACEHOLDER_FUNCTION;
use crate::hir_to_mir::convert_expr::call::is_string_typed_source;
use crate::hir_to_mir::convert_expr::util::{has_potential_side_effects, hir_expr_type_id};
use crate::hir_to_mir::converter::ExprConverter;

fn is_numeric_type_for_array_len(arg: &HirExpr, types: &TypeTable) -> bool {
    if matches!(arg, HirExpr::Int(_, _) | HirExpr::Float(_, _)) {
        return true;
    }
    let Some(ty) = hir_expr_type_id(arg) else {
        return false;
    };
    matches!(
        types.resolve(ty),
        Some(
            Type::I8
                | Type::I16
                | Type::I32
                | Type::I64
                | Type::U8
                | Type::U16
                | Type::U32
                | Type::U64
                | Type::F32
                | Type::F64
        )
    )
}

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
        if matches!(callee, HirExpr::Global { name, .. } if name.as_str() == "Array") {
            return self.convert_new_array(
                callee,
                args,
                ty,
                out,
                shared_struct_ids,
                shared_next_struct,
                types,
                ctx,
            );
        }
        if matches!(callee, HirExpr::Global { name, .. } if name.as_str() == "Date") {
            return self.convert_new_date(
                callee,
                args,
                ty,
                out,
                shared_struct_ids,
                shared_next_struct,
                types,
                ctx,
            );
        }
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

    fn convert_new_array(
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
        let _ = callee;
        let alloc_id = self.fresh_local();
        self.push_temp_local(alloc_id, ty);
        if args.is_empty() {
            out.push(MirStmt::Runtime {
                op: RuntimeOp::ArrayCreate,
                args: Vec::new(),
                dest: Some(alloc_id),
                ty,
                target_ty: None,
            });
            return MirExpr::Local(alloc_id);
        }
        if args.len() == 1 && is_numeric_type_for_array_len(&args[0], types) {
            let len_mir = self.convert_expr(
                &args[0],
                out,
                shared_struct_ids,
                shared_next_struct,
                types,
                ctx,
            );
            out.push(MirStmt::Runtime {
                op: RuntimeOp::ArrayCreateWithLen,
                args: vec![len_mir],
                dest: Some(alloc_id),
                ty,
                target_ty: None,
            });
            return MirExpr::Local(alloc_id);
        }
        out.push(MirStmt::Runtime {
            op: RuntimeOp::ArrayCreate,
            args: Vec::new(),
            dest: Some(alloc_id),
            ty,
            target_ty: None,
        });
        for a in args {
            let item_mir =
                self.convert_expr(a, out, shared_struct_ids, shared_next_struct, types, ctx);
            out.push(MirStmt::Runtime {
                op: RuntimeOp::ArrayPush,
                args: vec![MirExpr::Local(alloc_id), item_mir],
                dest: None,
                ty: TypeId::from_raw(0),
                target_ty: None,
            });
        }
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

    fn convert_new_date(
        &mut self,
        _callee: &HirExpr,
        args: &[HirExpr],
        ty: TypeId,
        out: &mut Vec<MirStmt>,
        shared_struct_ids: &mut HashMap<TypeId, StructId>,
        shared_next_struct: &mut u32,
        types: &mut TypeTable,
        ctx: &mut PassContext,
    ) -> MirExpr {
        if args.is_empty() {
            let dest = self.fresh_local();
            self.push_temp_local(dest, ty);
            out.push(MirStmt::Runtime {
                op: RuntimeOp::DateNow,
                args: Vec::new(),
                dest: Some(dest),
                ty,
                target_ty: None,
            });
            return MirExpr::Local(dest);
        }
        if args.len() == 1 {
            let op = if is_string_typed_source(&args[0], types) {
                RuntimeOp::DateParse
            } else {
                RuntimeOp::DateNewFromMs
            };
            let arg = self.convert_expr(
                &args[0],
                out,
                shared_struct_ids,
                shared_next_struct,
                types,
                ctx,
            );
            let dest = self.fresh_local();
            self.push_temp_local(dest, ty);
            out.push(MirStmt::Runtime {
                op,
                args: vec![arg],
                dest: Some(dest),
                ty,
                target_ty: None,
            });
            return MirExpr::Local(dest);
        }
        ctx.error(
            "E0406",
            format!(
                "new Date(year, month, ...rest) constructor with {} positional args is not yet \
                 supported in this AOT target; only `new Date()` and `new Date(value: number | string)` \
                 are currently lowered. Pass a single epoch-millisecond value (number) or an ISO 8601 \
                 string to construct a Date from components or a timestamp.",
                args.len()
            ),
            Span::new(0, 0),
        );
        MirExpr::Unit
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
