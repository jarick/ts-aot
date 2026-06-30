use std::collections::HashMap;

use ts2zig_core::{LocalId, Span, StructId, SymbolId, TypeId};
use ts2zig_ir_hir::HirStmt;
use ts2zig_ir_mir::{BinaryOp, MirBlock, MirExpr, MirLocalDecl, MirPlace, MirStmt};

use crate::PassContext;
use crate::hir_to_mir::converter::ExprConverter;

impl ExprConverter {
    pub fn convert_block(
        &mut self,
        block: &[HirStmt],
        ctx: &mut PassContext,
    ) -> (MirBlock, Vec<MirLocalDecl>) {
        let mut out = MirBlock::new();
        let mut final_locals: Vec<MirLocalDecl> = Vec::new();
        let mut interim: Vec<MirStmt> = Vec::new();
        let mut shared_struct_ids: HashMap<TypeId, StructId> = HashMap::new();
        let mut shared_next_struct: u32 = 0;
        for s in block {
            self.convert_stmt_into(
                s,
                &mut interim,
                &mut final_locals,
                &mut shared_struct_ids,
                &mut shared_next_struct,
                ctx,
            );
        }
        out.stmts.extend(interim);
        final_locals.extend(self.take_temp_locals());
        (out, final_locals)
    }

    pub fn convert_block_with_shared_struct_ids(
        &mut self,
        block: &[HirStmt],
        shared_struct_ids: &mut HashMap<TypeId, StructId>,
        shared_next_struct: &mut u32,
        ctx: &mut PassContext,
    ) -> (MirBlock, Vec<MirLocalDecl>) {
        let mut out = MirBlock::new();
        let mut final_locals: Vec<MirLocalDecl> = Vec::new();
        let mut interim: Vec<MirStmt> = Vec::new();
        for s in block {
            self.convert_stmt_into(
                s,
                &mut interim,
                &mut final_locals,
                shared_struct_ids,
                shared_next_struct,
                ctx,
            );
        }
        out.stmts.extend(interim);
        final_locals.extend(self.take_temp_locals());
        (out, final_locals)
    }

    pub(super) fn convert_stmt_into(
        &mut self,
        s: &HirStmt,
        out: &mut Vec<MirStmt>,
        final_locals: &mut Vec<MirLocalDecl>,
        shared_struct_ids: &mut HashMap<TypeId, StructId>,
        shared_next_struct: &mut u32,
        ctx: &mut PassContext,
    ) {
        match s {
            HirStmt::Block(stmts) => {
                for inner in stmts {
                    self.convert_stmt_into(
                        inner,
                        out,
                        final_locals,
                        shared_struct_ids,
                        shared_next_struct,
                        ctx,
                    );
                }
            }
            HirStmt::Let { id, name, ty, init } => {
                let new_id = self.map_local_id(*id);
                self.register_local_name(new_id, *name);
                final_locals.push(MirLocalDecl {
                    id: new_id,
                    name: *name,
                    ty: *ty,
                    mutable: false,
                });
                let init_mir = init
                    .as_ref()
                    .map(|e| self.convert_expr(e, out, shared_struct_ids, shared_next_struct, ctx));
                out.push(MirStmt::Let {
                    local: new_id,
                    ty: *ty,
                    init: init_mir,
                    mutable: false,
                });
            }
            HirStmt::Expr { expr } => {
                let mir = self.convert_expr(expr, out, shared_struct_ids, shared_next_struct, ctx);
                out.push(MirStmt::Expr(mir));
            }
            HirStmt::If {
                cond,
                then,
                otherwise,
            } => {
                let cond_mir =
                    self.convert_expr(cond, out, shared_struct_ids, shared_next_struct, ctx);
                let (then_mir, then_locals) =
                    self.convert_stmt_block(then, shared_struct_ids, shared_next_struct, ctx);
                final_locals.extend(then_locals);
                let else_mir = otherwise.as_ref().map(|b| {
                    let (m, l) =
                        self.convert_stmt_block(b, shared_struct_ids, shared_next_struct, ctx);
                    final_locals.extend(l);
                    m
                });
                out.push(MirStmt::If {
                    cond: cond_mir,
                    then_block: then_mir,
                    else_block: else_mir,
                });
            }
            HirStmt::While { cond, body } => {
                let mut cond_stmts: Vec<MirStmt> = Vec::new();
                let cond_mir = self.convert_expr(
                    cond,
                    &mut cond_stmts,
                    shared_struct_ids,
                    shared_next_struct,
                    ctx,
                );
                out.extend(cond_stmts.iter().cloned());
                let (body_mir, body_locals) =
                    self.convert_stmt_block(body, shared_struct_ids, shared_next_struct, ctx);
                final_locals.extend(body_locals);

                let is_break = self.fresh_local();
                final_locals.push(MirLocalDecl {
                    id: is_break,
                    name: SymbolId::from_raw(0),
                    ty: TypeId::from_raw(0),
                    mutable: true,
                });

                let mut inner_stmts = rewrite_break_continue_for_loop(body_mir.stmts, is_break, 0);
                inner_stmts.push(MirStmt::Break);

                let mut loop_body = Vec::with_capacity(inner_stmts.len() + cond_stmts.len() + 2);
                loop_body.push(MirStmt::While {
                    cond: MirExpr::Bool(true),
                    body: MirBlock { stmts: inner_stmts },
                });
                loop_body.push(MirStmt::If {
                    cond: MirExpr::Local(is_break),
                    then_block: MirBlock::with(MirStmt::Break),
                    else_block: None,
                });
                loop_body.extend(cond_stmts);

                out.push(MirStmt::Let {
                    local: is_break,
                    ty: TypeId::from_raw(0),
                    init: Some(MirExpr::Bool(false)),
                    mutable: true,
                });
                out.push(MirStmt::While {
                    cond: cond_mir,
                    body: MirBlock { stmts: loop_body },
                });
            }
            HirStmt::DoWhile { body, cond } => {
                let (body_mir, body_locals) =
                    self.convert_stmt_block(body, shared_struct_ids, shared_next_struct, ctx);
                final_locals.extend(body_locals);
                let mut cond_stmts: Vec<MirStmt> = Vec::new();
                let cond_mir = self.convert_expr(
                    cond,
                    &mut cond_stmts,
                    shared_struct_ids,
                    shared_next_struct,
                    ctx,
                );

                let first_id = self.fresh_local();
                final_locals.push(MirLocalDecl {
                    id: first_id,
                    name: SymbolId::from_raw(0),
                    ty: TypeId::from_raw(0),
                    mutable: true,
                });
                let is_break = self.fresh_local();
                final_locals.push(MirLocalDecl {
                    id: is_break,
                    name: SymbolId::from_raw(0),
                    ty: TypeId::from_raw(0),
                    mutable: true,
                });

                let mut inner_stmts = vec![MirStmt::Assign {
                    target: MirPlace::Local { id: first_id },
                    value: MirExpr::Bool(false),
                }];
                inner_stmts.extend(rewrite_break_continue_for_loop(body_mir.stmts, is_break, 0));
                inner_stmts.push(MirStmt::Break);

                let continue_cond = MirExpr::Binary {
                    op: BinaryOp::Or,
                    left: Box::new(MirExpr::Local(first_id)),
                    right: Box::new(cond_mir),
                    ty: TypeId::from_raw(0),
                };

                let mut loop_body = Vec::with_capacity(inner_stmts.len() + cond_stmts.len() + 2);
                loop_body.push(MirStmt::While {
                    cond: MirExpr::Bool(true),
                    body: MirBlock { stmts: inner_stmts },
                });
                loop_body.push(MirStmt::If {
                    cond: MirExpr::Local(is_break),
                    then_block: MirBlock::with(MirStmt::Break),
                    else_block: None,
                });
                loop_body.extend(cond_stmts);

                out.push(MirStmt::Let {
                    local: first_id,
                    ty: TypeId::from_raw(0),
                    init: Some(MirExpr::Bool(true)),
                    mutable: true,
                });
                out.push(MirStmt::Let {
                    local: is_break,
                    ty: TypeId::from_raw(0),
                    init: Some(MirExpr::Bool(false)),
                    mutable: true,
                });
                out.push(MirStmt::While {
                    cond: continue_cond,
                    body: MirBlock { stmts: loop_body },
                });
            }
            HirStmt::ForOf {
                binding,
                iter,
                body,
            } => {
                let iter_mir =
                    self.convert_expr(iter, out, shared_struct_ids, shared_next_struct, ctx);
                let new_binding = self.map_local_id(*binding);
                final_locals.push(MirLocalDecl {
                    id: new_binding,
                    name: SymbolId::from_raw(0),
                    ty: TypeId::from_raw(0),
                    mutable: false,
                });
                let (body_mir, body_locals) =
                    self.convert_stmt_block(body, shared_struct_ids, shared_next_struct, ctx);
                final_locals.extend(body_locals);
                out.push(MirStmt::ForOf {
                    item: new_binding,
                    iterable: iter_mir,
                    body: body_mir,
                });
            }
            HirStmt::ForIn {
                binding,
                iter,
                body,
            } => {
                let iter_mir =
                    self.convert_expr(iter, out, shared_struct_ids, shared_next_struct, ctx);
                let new_binding = self.map_local_id(*binding);
                final_locals.push(MirLocalDecl {
                    id: new_binding,
                    name: SymbolId::from_raw(0),
                    ty: TypeId::from_raw(0),
                    mutable: false,
                });
                let (body_mir, body_locals) =
                    self.convert_stmt_block(body, shared_struct_ids, shared_next_struct, ctx);
                final_locals.extend(body_locals);
                out.push(MirStmt::ForIn {
                    key: new_binding,
                    object: iter_mir,
                    body: body_mir,
                });
            }
            HirStmt::Switch { disc, cases } => {
                let _ = (disc, cases);
                ctx.error(
                    "P0005",
                    "switch statement is not yet implemented in HIR→MIR",
                    Span::new(0, 0),
                );
            }
            HirStmt::Return { value } => {
                let value_mir = value
                    .as_ref()
                    .map(|e| self.convert_expr(e, out, shared_struct_ids, shared_next_struct, ctx));
                out.push(MirStmt::Return(value_mir));
            }
            HirStmt::Break { .. } => out.push(MirStmt::Break),
            HirStmt::Continue { .. } => out.push(MirStmt::Continue),
            HirStmt::Throw { expr } => {
                let err_mir =
                    self.convert_expr(expr, out, shared_struct_ids, shared_next_struct, ctx);
                out.push(MirStmt::Throw {
                    error: err_mir,
                    error_ty: TypeId::from_raw(0),
                });
            }
            HirStmt::Try { .. } => {
                ctx.error(
                    "P0005",
                    "try statement is not yet implemented in HIR→MIR",
                    Span::new(0, 0),
                );
            }
            HirStmt::Decl(_) => {}
        }
    }

    pub(super) fn convert_stmt_block(
        &mut self,
        s: &HirStmt,
        shared_struct_ids: &mut HashMap<TypeId, StructId>,
        shared_next_struct: &mut u32,
        ctx: &mut PassContext,
    ) -> (MirBlock, Vec<MirLocalDecl>) {
        let mut out = MirBlock::new();
        let mut final_locals: Vec<MirLocalDecl> = Vec::new();
        self.convert_stmt_into(
            s,
            &mut out.stmts,
            &mut final_locals,
            shared_struct_ids,
            shared_next_struct,
            ctx,
        );
        (out, final_locals)
    }
}

fn rewrite_break_continue_for_loop(
    stmts: Vec<MirStmt>,
    is_break_local: LocalId,
    our_depth: usize,
) -> Vec<MirStmt> {
    let mut out = Vec::with_capacity(stmts.len());
    for s in stmts {
        match s {
            MirStmt::Continue if our_depth == 0 => {
                out.push(MirStmt::Break);
            }
            MirStmt::Break if our_depth == 0 => {
                out.push(MirStmt::Assign {
                    target: MirPlace::Local { id: is_break_local },
                    value: MirExpr::Bool(true),
                });
                out.push(MirStmt::Break);
            }
            MirStmt::While { cond, body } => {
                let new_body =
                    rewrite_break_continue_for_loop(body.stmts, is_break_local, our_depth + 1);
                out.push(MirStmt::While {
                    cond,
                    body: MirBlock { stmts: new_body },
                });
            }
            MirStmt::If {
                cond,
                then_block,
                else_block,
            } => {
                let new_then =
                    rewrite_break_continue_for_loop(then_block.stmts, is_break_local, our_depth);
                let new_else = else_block.map(|b| MirBlock {
                    stmts: rewrite_break_continue_for_loop(b.stmts, is_break_local, our_depth),
                });
                out.push(MirStmt::If {
                    cond,
                    then_block: MirBlock { stmts: new_then },
                    else_block: new_else,
                });
            }
            MirStmt::ForOf {
                item,
                iterable,
                body,
            } => {
                let new_body =
                    rewrite_break_continue_for_loop(body.stmts, is_break_local, our_depth + 1);
                out.push(MirStmt::ForOf {
                    item,
                    iterable,
                    body: MirBlock { stmts: new_body },
                });
            }
            MirStmt::ForIn { key, object, body } => {
                let new_body =
                    rewrite_break_continue_for_loop(body.stmts, is_break_local, our_depth + 1);
                out.push(MirStmt::ForIn {
                    key,
                    object,
                    body: MirBlock { stmts: new_body },
                });
            }
            other => out.push(other),
        }
    }
    out
}
