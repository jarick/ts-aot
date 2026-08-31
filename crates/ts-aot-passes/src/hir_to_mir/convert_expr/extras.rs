use std::collections::HashMap;

use ts_aot_core::{LocalId, Span, StructId, Type, TypeId, TypeTable};
use ts_aot_ir_hir::HirExpr;
use ts_aot_ir_mir::{MirExpr, MirLocalDecl, MirParam, MirStmt, RuntimeOp};

use crate::PassContext;
use crate::hir_to_mir::PLACEHOLDER_FUNCTION;
use crate::hir_to_mir::convert_expr::globals::is_string_typed_source;
use crate::hir_to_mir::convert_expr::util::{
    has_potential_side_effects, is_numeric_type_for_array_len,
};
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
        if matches!(callee, HirExpr::Global { name, .. } if name.as_str() == "ArrayBuffer") {
            return self.convert_new_array_buffer(
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
        if let Some(mir) = self.try_typed_array_new_dispatch(
            callee,
            args,
            ty,
            out,
            shared_struct_ids,
            shared_next_struct,
            types,
            ctx,
        ) {
            return mir;
        }
        if let Some(mir) = self.try_weakmap_new_dispatch(
            callee,
            args,
            ty,
            out,
            shared_struct_ids,
            shared_next_struct,
            types,
            ctx,
        ) {
            return mir;
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

    fn convert_new_array_buffer(
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
        if args.len() != 1 {
            ctx.error(
                "E0406",
                format!(
                    "new ArrayBuffer(byteLength) requires exactly 1 argument; got {}",
                    args.len()
                ),
                Span::new(0, 0),
            );
            return MirExpr::Unit;
        }
        if !is_numeric_type_for_array_len(&args[0], types) {
            ctx.error(
                "E0406",
                "new ArrayBuffer(byteLength) argument must be a non-negative integer (per \
                 ECMAScript spec); got non-numeric expression. Coerce the length to an integer \
                 (e.g. Math.floor(n)) before calling new ArrayBuffer(n)."
                    .to_string(),
                Span::new(0, 0),
            );
            return MirExpr::Unit;
        }
        let byte_length_mir = self.convert_expr(
            &args[0],
            out,
            shared_struct_ids,
            shared_next_struct,
            types,
            ctx,
        );
        let array_buffer_ty = types.intern(&Type::ArrayBuffer);
        let dest = self.fresh_local();
        self.push_temp_local(dest, array_buffer_ty);
        out.push(MirStmt::Runtime {
            op: RuntimeOp::ArrayBufferNew,
            args: vec![byte_length_mir],
            dest: Some(dest),
            ty: array_buffer_ty,
            target_ty: None,
        });
        let _ = ty;
        MirExpr::Local(dest)
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

    pub(super) fn convert_closure(
        &mut self,
        hir: &HirExpr,
        ty: TypeId,
        out: &mut Vec<MirStmt>,
        shared_struct_ids: &mut HashMap<TypeId, StructId>,
        shared_next_struct: &mut u32,
        types: &mut TypeTable,
        ctx: &mut PassContext,
    ) -> MirExpr {
        let HirExpr::Closure {
            id: _id,
            params,
            captures,
            body,
            ty: closure_ty,
            span,
        } = hir
        else {
            ctx.error(
                "P0005",
                "convert_closure called with non-closure",
                Span::new(0, 0),
            );
            return MirExpr::Unit;
        };
        if !captures.is_empty() {
            ctx.error(
                "P0005",
                "capturing closures are not supported in this PR; \
                 only no-capture arrow functions are accepted",
                *span,
            );
            return MirExpr::Unit;
        }
        let ret_ty = types
            .resolve(*closure_ty)
            .and_then(|t| match t {
                ts_aot_core::Type::Fn { ret, .. } => Some(*ret),
                _ => None,
            })
            .unwrap_or(ty);
        let mut mir_params: Vec<MirParam> = Vec::with_capacity(params.len());
        let param_ids: Vec<LocalId> = (0..params.len())
            .map(|i| LocalId::from_raw(u32::try_from(i).unwrap_or(u32::MAX)))
            .collect();
        let snapshot: std::collections::HashMap<LocalId, LocalId> = self.local_map.clone();
        let temp_locals_snapshot: Vec<MirLocalDecl> = std::mem::take(&mut self.temp_locals);
        for (hir_param_id, p) in param_ids.iter().zip(params.iter()) {
            let new_id = self.fresh_local();
            self.map_local_id_inplace(*hir_param_id, new_id);
            let name = self.unique_synth_local_name(new_id, "__closure_param");
            mir_params.push(MirParam {
                id: new_id,
                name,
                ty: p.ty,
            });
        }
        let (mir_body, body_locals) = self.convert_block_with_shared_struct_ids(
            body,
            shared_struct_ids,
            shared_next_struct,
            types,
            ctx,
        );
        self.local_map = snapshot;
        self.temp_locals = temp_locals_snapshot;
        let closure_expr = MirExpr::Closure {
            params: mir_params,
            captures: Vec::new(),
            locals: body_locals,
            body: mir_body,
            ret_ty,
            fn_ty: *closure_ty,
        };
        let dest = self.fresh_local();
        self.push_temp_local_with_mut(dest, *closure_ty, false);
        out.push(MirStmt::Let {
            local: dest,
            ty: *closure_ty,
            init: Some(closure_expr),
            mutable: false,
        });
        MirExpr::Local(dest)
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
