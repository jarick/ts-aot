use ts_aot_core::TypeTable;
use ts_aot_ir_hir::{HirCatchClause, HirStmt, HirSwitchCase};

use super::substitute::{TypeParamMap, TypeSubstitutionResult};
use super::substitute_decl::{substitute_body, substitute_decl as substitute_decl_inner};
use super::substitute_expr::substitute_expr;
use super::substitute_ty::substitute_type;

pub fn substitute_stmt(
    stmt: &HirStmt,
    mapping: &TypeParamMap,
    types: &mut TypeTable,
    result: &mut TypeSubstitutionResult,
) -> HirStmt {
    match stmt {
        HirStmt::Block(stmts) => HirStmt::Block(substitute_body(stmts, mapping, types, result)),
        HirStmt::Let { id, name, ty, init } => HirStmt::Let {
            id: *id,
            name: name.clone(),
            ty: substitute_type(*ty, mapping, types, result),
            init: init
                .as_ref()
                .map(|e| substitute_expr(e, mapping, types, result)),
        },
        HirStmt::Expr { expr } => HirStmt::Expr {
            expr: substitute_expr(expr, mapping, types, result),
        },
        HirStmt::If {
            cond,
            then,
            otherwise,
        } => HirStmt::If {
            cond: substitute_expr(cond, mapping, types, result),
            then: Box::new(substitute_stmt(then, mapping, types, result)),
            otherwise: otherwise
                .as_ref()
                .map(|e| Box::new(substitute_stmt(e, mapping, types, result))),
        },
        HirStmt::While { cond, body } => HirStmt::While {
            cond: substitute_expr(cond, mapping, types, result),
            body: Box::new(substitute_stmt(body, mapping, types, result)),
        },
        HirStmt::DoWhile { body, cond } => HirStmt::DoWhile {
            body: Box::new(substitute_stmt(body, mapping, types, result)),
            cond: substitute_expr(cond, mapping, types, result),
        },
        HirStmt::ForOf {
            binding,
            iter,
            body,
        } => HirStmt::ForOf {
            binding: *binding,
            iter: substitute_expr(iter, mapping, types, result),
            body: Box::new(substitute_stmt(body, mapping, types, result)),
        },
        HirStmt::ForIn {
            binding,
            iter,
            body,
        } => HirStmt::ForIn {
            binding: *binding,
            iter: substitute_expr(iter, mapping, types, result),
            body: Box::new(substitute_stmt(body, mapping, types, result)),
        },
        HirStmt::Switch { disc, cases } => HirStmt::Switch {
            disc: substitute_expr(disc, mapping, types, result),
            cases: cases
                .iter()
                .map(|c| HirSwitchCase {
                    test: c
                        .test
                        .as_ref()
                        .map(|t| substitute_expr(t, mapping, types, result)),
                    body: substitute_body(&c.body, mapping, types, result),
                })
                .collect(),
        },
        HirStmt::Return { value } => HirStmt::Return {
            value: value
                .as_ref()
                .map(|e| substitute_expr(e, mapping, types, result)),
        },
        HirStmt::Break { label } => HirStmt::Break {
            label: label.clone(),
        },
        HirStmt::Continue { label } => HirStmt::Continue {
            label: label.clone(),
        },
        HirStmt::Throw { expr } => HirStmt::Throw {
            expr: substitute_expr(expr, mapping, types, result),
        },
        HirStmt::Try {
            body,
            catch,
            finally,
        } => HirStmt::Try {
            body: Box::new(substitute_stmt(body, mapping, types, result)),
            catch: catch.as_ref().map(|c| HirCatchClause {
                binding: c.binding.clone(),
                body: Box::new(substitute_stmt(&c.body, mapping, types, result)),
            }),
            finally: finally
                .as_ref()
                .map(|f| Box::new(substitute_stmt(f, mapping, types, result))),
        },
        HirStmt::Decl(decl) => HirStmt::Decl(substitute_decl_inner(decl, mapping, types, result)),
    }
}
