use ts_aot_core::{Type, TypeId, TypeTable};
use ts_aot_ir_mir::{MirBlock, MirDecl, MirExpr, MirProgram, MirStmt};

#[cfg(test)]
mod tests;

pub fn lower_result(program: &mut MirProgram, types: &mut TypeTable) {
    for decl in &mut program.declarations {
        if let MirDecl::Function(f) = decl
            && let Some(throws_ty) = f.throws
        {
            let (result_ty, err_ty) = if is_already_result(f.ret, types) {
                let Type::Result { err, .. } =
                    types.resolve(f.ret).expect("checked by is_already_result")
                else {
                    unreachable!()
                };
                (f.ret, *err)
            } else {
                let new_ret = types.intern(&Type::Result {
                    ok: f.ret,
                    err: throws_ty,
                });
                f.ret = new_ret;
                (new_ret, throws_ty)
            };
            rewrite_block(&mut f.body.block, result_ty, err_ty);
        }
    }
}

fn is_already_result(ty: TypeId, types: &TypeTable) -> bool {
    matches!(types.resolve(ty), Some(Type::Result { .. }))
}

fn rewrite_block(block: &mut MirBlock, result_ty: TypeId, err_ty: TypeId) {
    for stmt in &mut block.stmts {
        rewrite_stmt(stmt, result_ty, err_ty);
    }
}

fn rewrite_stmt(stmt: &mut MirStmt, result_ty: TypeId, err_ty: TypeId) {
    match stmt {
        MirStmt::Throw { error, error_ty } => {
            *error_ty = err_ty;
            let error = std::mem::replace(error, MirExpr::Unit);
            *stmt = MirStmt::ReturnResultErr { error, err_ty };
        }
        MirStmt::Return(slot) => {
            let needs_wrap = match slot {
                Some(expr) => !is_result_expr(expr),
                None => true,
            };
            if needs_wrap {
                let old = slot.take();
                let value = old.unwrap_or(MirExpr::Unit);
                *slot = Some(MirExpr::ResultOk {
                    value: Box::new(value),
                    ty: result_ty,
                });
            }
        }
        MirStmt::If {
            then_block,
            else_block,
            ..
        } => {
            rewrite_block(then_block, result_ty, err_ty);
            if let Some(eb) = else_block {
                rewrite_block(eb, result_ty, err_ty);
            }
        }
        MirStmt::While { body, .. } | MirStmt::ForOf { body, .. } | MirStmt::ForIn { body, .. } => {
            rewrite_block(body, result_ty, err_ty)
        }
        MirStmt::Switch { cases, default, .. } => {
            for case in cases {
                rewrite_block(&mut case.body, result_ty, err_ty);
            }
            if let Some(def) = default {
                rewrite_block(def, result_ty, err_ty);
            }
        }
        MirStmt::Try {
            body,
            catch,
            finally,
            ..
        } => {
            rewrite_block(body, result_ty, err_ty);
            if let Some(catch_block) = catch {
                rewrite_block(catch_block, result_ty, err_ty);
            }
            if let Some(fin) = finally {
                rewrite_block(fin, result_ty, err_ty);
            }
        }
        MirStmt::Let { .. }
        | MirStmt::Assign { .. }
        | MirStmt::Expr(_)
        | MirStmt::ReturnResultErr { .. }
        | MirStmt::Break
        | MirStmt::Continue
        | MirStmt::Runtime { .. } => {}
    }
}

fn is_result_expr(expr: &MirExpr) -> bool {
    matches!(expr, MirExpr::ResultOk { .. } | MirExpr::ResultErr { .. })
}
