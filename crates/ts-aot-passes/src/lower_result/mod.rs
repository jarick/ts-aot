use ts_aot_core::TypeId;
use ts_aot_ir_mir::{MirBlock, MirDecl, MirExpr, MirProgram, MirStmt};

#[cfg(test)]
mod tests;

pub fn lower_result(program: &mut MirProgram) {
    for decl in &mut program.declarations {
        if let MirDecl::Function(f) = decl
            && let Some(err_ty) = f.throws
        {
            rewrite_block(&mut f.body.block, err_ty);
        }
    }
}

fn rewrite_block(block: &mut MirBlock, err_ty: TypeId) {
    for stmt in &mut block.stmts {
        rewrite_stmt(stmt, err_ty);
    }
}

fn rewrite_stmt(stmt: &mut MirStmt, err_ty: TypeId) {
    match stmt {
        MirStmt::Throw { error, error_ty } => {
            *error_ty = err_ty;
            let error = std::mem::replace(error, MirExpr::Unit);
            *stmt = MirStmt::ReturnResultErr { error, err_ty };
        }
        MirStmt::If {
            then_block,
            else_block,
            ..
        } => {
            rewrite_block(then_block, err_ty);
            if let Some(eb) = else_block {
                rewrite_block(eb, err_ty);
            }
        }
        MirStmt::While { body, .. } | MirStmt::ForOf { body, .. } | MirStmt::ForIn { body, .. } => {
            rewrite_block(body, err_ty)
        }
        MirStmt::Let { .. }
        | MirStmt::Assign { .. }
        | MirStmt::Expr(_)
        | MirStmt::Return(_)
        | MirStmt::ReturnResultErr { .. }
        | MirStmt::Break
        | MirStmt::Continue
        | MirStmt::Runtime { .. } => {}
    }
}
