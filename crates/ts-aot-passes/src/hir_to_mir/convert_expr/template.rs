#![allow(clippy::too_many_arguments)]

use std::collections::HashMap;

use ts_aot_core::{Atom, StructId, TypeId, TypeTable};
use ts_aot_ir_hir::HirExpr;
use ts_aot_ir_mir::{MirExpr, MirStmt, RuntimeOp};

use crate::PassContext;
use crate::hir_to_mir::converter::ExprConverter;

impl ExprConverter {
    pub(super) fn convert_template(
        &mut self,
        tag: Option<&HirExpr>,
        expressions: &[HirExpr],
        cooked_parts: &[Option<Atom>],
        ty: TypeId,
        out: &mut Vec<MirStmt>,
        shared_struct_ids: &mut HashMap<TypeId, StructId>,
        shared_next_struct: &mut u32,
        types: &mut TypeTable,
        ctx: &mut PassContext,
    ) -> MirExpr {
        if let Some(tag_expr) = tag {
            let tag_mir = self.convert_expr(
                tag_expr,
                out,
                shared_struct_ids,
                shared_next_struct,
                types,
                ctx,
            );
            let mut cooked_atoms: Vec<Atom> = Vec::with_capacity(cooked_parts.len());
            for cooked_opt in cooked_parts.iter() {
                let cooked_text = cooked_opt.as_ref().map_or("", ts_aot_core::Atom::as_str);
                cooked_atoms.push(Atom::from(cooked_text));
            }
            let mut call_args: Vec<MirExpr> = Vec::with_capacity(1 + expressions.len());
            call_args.push(MirExpr::TemplateStringsArray {
                cooked: cooked_atoms,
                ty,
            });
            for e in expressions.iter() {
                call_args.push(self.convert_expr(
                    e,
                    out,
                    shared_struct_ids,
                    shared_next_struct,
                    types,
                    ctx,
                ));
            }
            let dest = self.fresh_local();
            self.push_temp_local(dest, ty);
            out.push(MirStmt::Let {
                local: dest,
                ty,
                init: Some(MirExpr::IndirectCall {
                    callee: Box::new(tag_mir),
                    args: call_args,
                    ty,
                }),
                mutable: false,
            });
            MirExpr::Local(dest)
        } else {
            let mut parts: Vec<MirExpr> = Vec::with_capacity(expressions.len() * 2 + 1);
            for (i, cooked_opt) in cooked_parts.iter().enumerate() {
                let cooked_text = cooked_opt.as_ref().map_or("", ts_aot_core::Atom::as_str);
                parts.push(MirExpr::String {
                    id: Atom::from(cooked_text),
                    ty,
                });
                if let Some(e) = expressions.get(i) {
                    parts.push(self.convert_expr(
                        e,
                        out,
                        shared_struct_ids,
                        shared_next_struct,
                        types,
                        ctx,
                    ));
                }
            }
            if parts.is_empty() {
                MirExpr::Unit
            } else if parts.len() == 1 {
                let part = parts.into_iter().next().expect("len 1");
                let dest = self.fresh_local();
                self.push_temp_local(dest, ty);
                out.push(MirStmt::Let {
                    local: dest,
                    ty,
                    init: Some(part),
                    mutable: false,
                });
                MirExpr::Local(dest)
            } else {
                let (first, rest) = parts.split_first().expect("len >= 2");
                let first_dest = self.fresh_local();
                self.push_temp_local(first_dest, ty);
                out.push(MirStmt::Let {
                    local: first_dest,
                    ty,
                    init: Some(first.clone()),
                    mutable: false,
                });
                let mut current = MirExpr::Local(first_dest);
                for part in rest.iter() {
                    let dest = self.fresh_local();
                    self.push_temp_local(dest, ty);
                    out.push(MirStmt::Runtime {
                        op: RuntimeOp::StringConcat,
                        args: vec![current, part.clone()],
                        dest: Some(dest),
                        ty,
                    });
                    current = MirExpr::Local(dest);
                }
                current
            }
        }
    }
}
