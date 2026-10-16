use ts_aot_ir_hir::{HirDecl, HirExpr, HirStmt};

use super::ClosureLowerer;

impl ClosureLowerer<'_> {
    pub(super) fn walk_decl(&mut self, decl: &mut HirDecl) {
        match decl {
            HirDecl::Function(f) => self.process_scope(&mut f.body),
            HirDecl::Class(c) => {
                for method in &mut c.methods {
                    self.process_scope(&mut method.body);
                }
            }
            HirDecl::Global { init: Some(e), .. } => self.process_global_init(e),
            HirDecl::Namespace { members, .. } => {
                for m in members {
                    self.walk_decl(m);
                }
            }
            HirDecl::Global { init: None, .. }
            | HirDecl::Enum { .. }
            | HirDecl::TypeAlias { .. }
            | HirDecl::Interface { .. } => {}
        }
    }

    fn process_scope(&mut self, body: &mut [HirStmt]) {
        let outer = std::mem::take(&mut self.closure_names);
        self.walk_body(body);
        if !self.closure_names.is_empty() {
            super::rewrite::rewrite_in_body(body, &self.closure_names, self.ctx);
        }
        self.closure_names = outer;
    }

    fn process_global_init(&mut self, expr: &mut HirExpr) {
        let outer = std::mem::take(&mut self.closure_names);
        self.walk_expr(expr);
        if !self.closure_names.is_empty() {
            super::rewrite::rewrite_in_global_init(expr, &self.closure_names, self.ctx);
        }
        self.closure_names = outer;
    }
}
