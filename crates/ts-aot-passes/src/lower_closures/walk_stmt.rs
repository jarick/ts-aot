use ts_aot_ir_hir::HirStmt;

use super::ClosureLowerer;

impl ClosureLowerer<'_> {
    pub(super) fn walk_body(&mut self, body: &mut [HirStmt]) {
        for stmt in body.iter_mut() {
            self.walk_stmt(stmt);
        }
    }

    pub(super) fn walk_stmt(&mut self, stmt: &mut HirStmt) {
        match stmt {
            HirStmt::Block(stmts) => self.walk_body(stmts),
            HirStmt::Let { init: Some(e), .. } | HirStmt::Expr { expr: e } => self.walk_expr(e),
            HirStmt::Let { init: None, .. } => {}
            HirStmt::If {
                cond,
                then,
                otherwise,
            } => {
                self.walk_expr(cond);
                self.walk_stmt(then);
                if let Some(else_stmt) = otherwise {
                    self.walk_stmt(else_stmt);
                }
            }
            HirStmt::While { cond, body } | HirStmt::DoWhile { body, cond } => {
                self.walk_expr(cond);
                self.walk_stmt(body);
            }
            HirStmt::ForOf { iter, body, .. }
            | HirStmt::ForAwaitOf { iter, body, .. }
            | HirStmt::ForIn { iter, body, .. } => {
                self.walk_expr(iter);
                self.walk_stmt(body);
            }
            HirStmt::Switch { disc, cases } => {
                self.walk_expr(disc);
                for case in cases {
                    if let Some(test) = &mut case.test {
                        self.walk_expr(test);
                    }
                    self.walk_body(&mut case.body);
                }
            }
            HirStmt::Return { value: Some(e) } => self.walk_expr(e),
            HirStmt::Return { value: None } => {}
            HirStmt::Throw { expr } => self.walk_expr(expr),
            HirStmt::Try {
                body,
                catch,
                finally,
            } => {
                self.walk_stmt(body);
                if let Some(c) = catch {
                    self.walk_stmt(&mut c.body);
                }
                if let Some(f) = finally {
                    self.walk_stmt(f);
                }
            }
            HirStmt::Decl(d) => self.walk_decl(d),
            HirStmt::Break { .. } | HirStmt::Continue { .. } => {}
        }
    }
}
