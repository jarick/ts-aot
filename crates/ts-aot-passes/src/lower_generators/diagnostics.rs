use ts_aot_core::Span;
use ts_aot_ir_hir::{HirExpr, HirStmt, Visitor, walk_expr, walk_stmt};

pub(crate) const GENERATOR_DIAG_UNSUPPORTED_YIELD: &str = "E0501";
pub const GENERATOR_DIAG_DEFERRED_METHOD: &str = "E0502";

#[derive(Debug, Default, Clone, Copy)]
pub(super) struct BodyAnalysis {
    pub first_expression_yield_span: Option<Span>,
    pub first_valued_yield_span: Option<Span>,
    pub first_throw_span: Option<Span>,
    pub first_await_span: Option<Span>,
    pub first_try_body_yield_span: Option<Span>,
    pub first_catch_yield_span: Option<Span>,
    pub has_throw: bool,
    pub has_valued_yield: bool,
    pub has_await: bool,
}

pub(super) fn analyze_generator_body(body: &[HirStmt]) -> BodyAnalysis {
    let mut a = BodyAnalysis::default();
    let mut v = BodyAnalysisVisitor {
        a: &mut a,
        ctx: YieldContext::None,
    };
    for s in body {
        v.visit_stmt(s);
    }
    a
}

#[derive(Clone, Copy)]
enum YieldContext {
    None,
    TryBody,
    Catch,
}

struct BodyAnalysisVisitor<'a> {
    a: &'a mut BodyAnalysis,
    ctx: YieldContext,
}

impl Visitor for BodyAnalysisVisitor<'_> {
    fn visit_stmt(&mut self, stmt: &HirStmt) {
        match stmt {
            HirStmt::Expr {
                expr:
                    HirExpr::Yield {
                        expr: yield_expr,
                        span,
                        ..
                    },
            } => {
                match self.ctx {
                    YieldContext::TryBody => {
                        self.a.first_try_body_yield_span.get_or_insert(*span);
                    }
                    YieldContext::Catch => {
                        self.a.first_catch_yield_span.get_or_insert(*span);
                    }
                    YieldContext::None => {}
                }
                if yield_expr.is_some() {
                    self.a.has_valued_yield = true;
                    self.a.first_valued_yield_span.get_or_insert(*span);
                }
                if let Some(inner) = yield_expr {
                    let prev = self.ctx;
                    self.ctx = YieldContext::None;
                    self.visit_expr(inner);
                    self.ctx = prev;
                }
            }
            HirStmt::Throw { expr } => {
                self.a.has_throw = true;
                self.a.first_throw_span.get_or_insert(expr.span());
                self.visit_expr(expr);
            }
            HirStmt::Try {
                body,
                catch,
                finally,
            } => {
                let prev = self.ctx;
                self.ctx = YieldContext::TryBody;
                self.visit_stmt(body);
                if let Some(c) = catch {
                    self.ctx = YieldContext::Catch;
                    self.visit_stmt(&c.body);
                }
                if let Some(f) = finally {
                    self.ctx = prev;
                    self.visit_stmt(f);
                }
                self.ctx = prev;
            }
            _ => walk_stmt(self, stmt),
        }
    }

    fn visit_expr(&mut self, expr: &HirExpr) {
        match expr {
            HirExpr::Yield {
                expr: inner, span, ..
            } => {
                match self.ctx {
                    YieldContext::TryBody => {
                        self.a.first_try_body_yield_span.get_or_insert(*span);
                    }
                    YieldContext::Catch => {
                        self.a.first_catch_yield_span.get_or_insert(*span);
                    }
                    YieldContext::None => {
                        self.a.first_expression_yield_span.get_or_insert(*span);
                    }
                }
                if let Some(inner) = inner {
                    self.visit_expr(inner);
                }
            }
            HirExpr::Await {
                expr: inner, span, ..
            } => {
                self.a.has_await = true;
                self.a.first_await_span.get_or_insert(*span);
                self.visit_expr(inner);
            }
            HirExpr::Closure { captures, .. } => {
                for c in captures {
                    self.visit_expr(c);
                }
            }
            _ => walk_expr(self, expr),
        }
    }
}

pub(super) fn first_body_span(body: &[HirStmt]) -> Option<Span> {
    fn walk(stmts: &[HirStmt]) -> Option<Span> {
        for stmt in stmts {
            if let Some(s) = stmt_span(stmt) {
                return Some(s);
            }
        }
        None
    }
    fn stmt_span(stmt: &HirStmt) -> Option<Span> {
        match stmt {
            HirStmt::Expr { expr } => Some(expr.span()),
            HirStmt::Let {
                init: Some(init), ..
            } => Some(init.span()),
            HirStmt::If { cond, .. } => Some(cond.span()),
            HirStmt::While { cond, .. } => Some(cond.span()),
            HirStmt::DoWhile { body, cond } => stmt_span(body).or_else(|| Some(cond.span())),
            HirStmt::ForOf { iter, .. }
            | HirStmt::ForAwaitOf { iter, .. }
            | HirStmt::ForIn { iter, .. } => Some(iter.span()),
            HirStmt::Switch { disc, .. } => Some(disc.span()),
            HirStmt::Return {
                value: Some(value), ..
            } => Some(value.span()),
            HirStmt::Throw { expr } => Some(expr.span()),
            HirStmt::Try {
                body,
                catch,
                finally,
            } => stmt_span(body)
                .or_else(|| catch.as_ref().and_then(|c| stmt_span(&c.body)))
                .or_else(|| finally.as_deref().and_then(stmt_span)),
            HirStmt::Block(inner) => walk(inner),
            HirStmt::Let { init: None, .. }
            | HirStmt::Return { value: None, .. }
            | HirStmt::Break { .. }
            | HirStmt::Continue { .. }
            | HirStmt::Decl(_) => None,
        }
    }
    walk(body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ts_aot_core::LocalId;
    use ts_aot_ir_hir::{HirCallee, HirCatchClause, HirParam, HirSwitchCase};

    fn yield_stmt(value: Option<i64>, span_offset: u32) -> HirStmt {
        HirStmt::Expr {
            expr: HirExpr::Yield {
                expr: value
                    .map(|v| Box::new(HirExpr::Int(v, Span::new(span_offset, span_offset + 1)))),
                ty: ts_aot_core::TypeId::from_raw(7),
                span: Span::new(span_offset, span_offset + 1),
            },
        }
    }

    fn expr_yield_in_init(value: i64, span_offset: u32) -> HirStmt {
        HirStmt::Let {
            id: LocalId::from_raw(0),
            name: ts_aot_core::Atom::from("x"),
            ty: ts_aot_core::TypeId::from_raw(7),
            init: Some(HirExpr::Yield {
                expr: Some(Box::new(HirExpr::Int(value, Span::default()))),
                ty: ts_aot_core::TypeId::from_raw(7),
                span: Span::new(span_offset, span_offset + 1),
            }),
        }
    }

    fn throw_stmt() -> HirStmt {
        HirStmt::Throw {
            expr: HirExpr::Unit(Span::default()),
        }
    }

    fn throw_stmt_with_span(span_offset: u32) -> HirStmt {
        HirStmt::Throw {
            expr: HirExpr::Unit(Span::new(span_offset, span_offset + 1)),
        }
    }

    #[test]
    fn analyze_empty_body_returns_all_defaults() {
        let a = analyze_generator_body(&[]);
        assert!(a.first_expression_yield_span.is_none());
        assert!(a.first_valued_yield_span.is_none());
        assert!(a.first_throw_span.is_none());
        assert!(!a.has_throw);
        assert!(!a.has_valued_yield);
    }

    #[test]
    fn analyze_statement_level_valued_yield_marks_valued_only() {
        let a = analyze_generator_body(&[yield_stmt(Some(1), 0)]);
        assert!(a.first_expression_yield_span.is_none());
        assert!(!a.has_throw);
        assert!(a.has_valued_yield);
    }

    #[test]
    fn analyze_statement_level_bare_yield_marks_neither() {
        let a = analyze_generator_body(&[yield_stmt(None, 0)]);
        assert!(a.first_expression_yield_span.is_none());
        assert!(!a.has_throw);
        assert!(!a.has_valued_yield);
    }

    #[test]
    fn analyze_expression_position_yield_in_let_init_returns_span() {
        let a = analyze_generator_body(&[expr_yield_in_init(1, 42)]);
        assert_eq!(a.first_expression_yield_span, Some(Span::new(42, 43)));
        assert!(!a.has_throw);
        assert!(!a.has_valued_yield);
    }

    #[test]
    fn analyze_throw_stmt_marks_throw() {
        let a = analyze_generator_body(&[throw_stmt()]);
        assert!(a.first_expression_yield_span.is_none());
        assert!(a.has_throw);
        assert!(!a.has_valued_yield);
    }

    #[test]
    fn analyze_yield_inside_if_body() {
        let stmt = HirStmt::If {
            cond: HirExpr::Bool(true, Span::default()),
            then: Box::new(yield_stmt(Some(1), 7)),
            otherwise: None,
        };
        let a = analyze_generator_body(&[stmt]);
        assert!(a.has_valued_yield);
        assert!(!a.has_throw);
        assert!(a.first_expression_yield_span.is_none());
    }

    #[test]
    fn analyze_yield_inside_while_body() {
        let stmt = HirStmt::While {
            cond: HirExpr::Bool(true, Span::default()),
            body: Box::new(yield_stmt(Some(2), 9)),
        };
        let a = analyze_generator_body(&[stmt]);
        assert!(a.has_valued_yield);
    }

    #[test]
    fn analyze_yield_inside_forof_body() {
        let stmt = HirStmt::ForOf {
            binding: LocalId::from_raw(0),
            iter: HirExpr::Unit(Span::default()),
            body: Box::new(yield_stmt(Some(3), 11)),
        };
        let a = analyze_generator_body(&[stmt]);
        assert!(a.has_valued_yield);
    }

    #[test]
    fn analyze_yield_inside_try_catch_finally() {
        let stmt = HirStmt::Try {
            body: Box::new(yield_stmt(Some(4), 13)),
            catch: Some(HirCatchClause::new(None, Box::new(yield_stmt(Some(5), 17)))),
            finally: Some(Box::new(yield_stmt(Some(6), 19))),
        };
        let a = analyze_generator_body(&[stmt]);
        assert!(a.has_valued_yield);
    }

    #[test]
    fn analyze_yield_in_try_body_records_try_body_span() {
        let stmt = HirStmt::Try {
            body: Box::new(yield_stmt(Some(4), 13)),
            catch: None,
            finally: None,
        };
        let a = analyze_generator_body(&[stmt]);
        assert_eq!(a.first_try_body_yield_span, Some(Span::new(13, 14)));
        assert!(a.first_catch_yield_span.is_none());
    }

    #[test]
    fn analyze_yield_in_catch_records_catch_span() {
        let stmt = HirStmt::Try {
            body: Box::new(HirStmt::Return { value: None }),
            catch: Some(HirCatchClause::new(None, Box::new(yield_stmt(Some(5), 17)))),
            finally: None,
        };
        let a = analyze_generator_body(&[stmt]);
        assert_eq!(a.first_catch_yield_span, Some(Span::new(17, 18)));
        assert!(a.first_try_body_yield_span.is_none());
    }

    #[test]
    fn analyze_yield_in_finally_does_not_mark_try_or_catch() {
        let stmt = HirStmt::Try {
            body: Box::new(HirStmt::Return { value: None }),
            catch: None,
            finally: Some(Box::new(yield_stmt(Some(6), 19))),
        };
        let a = analyze_generator_body(&[stmt]);
        assert!(
            a.first_try_body_yield_span.is_none(),
            "finally is outside the catch_unwind closure, must not mark try-body"
        );
        assert!(
            a.first_catch_yield_span.is_none(),
            "no catch clause exists, must not mark catch"
        );
    }

    #[test]
    fn analyze_yield_in_nested_finally_records_try_body_span() {
        let nested_finally = HirStmt::Try {
            body: Box::new(HirStmt::Return { value: None }),
            catch: None,
            finally: Some(Box::new(yield_stmt(Some(7), 23))),
        };
        let outer = HirStmt::Try {
            body: Box::new(nested_finally),
            catch: Some(HirCatchClause::new(
                None,
                Box::new(HirStmt::Return { value: None }),
            )),
            finally: None,
        };
        let a = analyze_generator_body(&[outer]);
        assert_eq!(
            a.first_try_body_yield_span,
            Some(Span::new(23, 24)),
            "yield in nested try/finally must be attributed to the enclosing try-body context \
             (the finally arm preserves self.ctx, so the inner try's body context — TryBody — \
             carries through to the inner finally), got analysis: first_try_body_yield_span={:?}",
            a.first_try_body_yield_span
        );
        assert!(
            a.first_catch_yield_span.is_none(),
            "no yield in catch clause, must not mark catch"
        );
    }

    #[test]
    fn analyze_yield_expression_in_try_body_records_try_body_span() {
        let call = HirStmt::Try {
            body: Box::new(HirStmt::Expr {
                expr: HirExpr::Call {
                    callee: HirCallee::Indirect(Box::new(HirExpr::Global {
                        name: ts_aot_core::Atom::from("f"),
                        ty: ts_aot_core::TypeId::from_raw(0),
                        span: Span::default(),
                    })),
                    args: vec![HirExpr::Yield {
                        expr: Some(Box::new(HirExpr::Int(1, Span::default()))),
                        ty: ts_aot_core::TypeId::from_raw(7),
                        span: Span::new(21, 22),
                    }],
                    type_args: Vec::new(),
                    ty: ts_aot_core::TypeId::from_raw(0),
                    span: Span::default(),
                },
            }),
            catch: None,
            finally: None,
        };
        let a = analyze_generator_body(&[call]);
        assert_eq!(a.first_try_body_yield_span, Some(Span::new(21, 22)));
    }

    #[test]
    fn analyze_yield_in_nested_try_body_records_try_body_span() {
        let stmt = HirStmt::Try {
            body: Box::new(HirStmt::Try {
                body: Box::new(yield_stmt(Some(7), 33)),
                catch: None,
                finally: None,
            }),
            catch: None,
            finally: None,
        };
        let a = analyze_generator_body(&[stmt]);
        assert_eq!(a.first_try_body_yield_span, Some(Span::new(33, 34)));
        assert!(a.first_catch_yield_span.is_none());
    }

    #[test]
    fn analyze_first_try_body_yield_keeps_earliest_span() {
        let stmt = HirStmt::Try {
            body: Box::new(HirStmt::Block(vec![
                yield_stmt(Some(1), 5),
                yield_stmt(Some(2), 9),
            ])),
            catch: None,
            finally: None,
        };
        let a = analyze_generator_body(&[stmt]);
        assert_eq!(a.first_try_body_yield_span, Some(Span::new(5, 6)));
    }

    #[test]
    fn analyze_yield_inside_switch_case() {
        let case = HirSwitchCase::new(
            Some(HirExpr::Int(1, Span::default())),
            vec![yield_stmt(Some(7), 23)],
        );
        let stmt = HirStmt::Switch {
            disc: HirExpr::Int(0, Span::default()),
            cases: vec![case],
        };
        let a = analyze_generator_body(&[stmt]);
        assert!(a.has_valued_yield);
    }

    #[test]
    fn analyze_yield_in_switch_case_test_is_inspected() {
        let case = HirSwitchCase::new(
            Some(HirExpr::Yield {
                expr: Some(Box::new(HirExpr::Int(1, Span::new(33, 34)))),
                ty: ts_aot_core::TypeId::from_raw(7),
                span: Span::new(33, 34),
            }),
            Vec::new(),
        );
        let stmt = HirStmt::Switch {
            disc: HirExpr::Int(0, Span::default()),
            cases: vec![case],
        };
        let a = analyze_generator_body(&[stmt]);
        assert_eq!(
            a.first_expression_yield_span,
            Some(Span::new(33, 34)),
            "yield inside `case ...:` is in expression position (the case test IS an expression), \
             so it must be flagged as expression-position yield. \
             Without walking the case test, the walker would miss this yield entirely."
        );
        assert!(
            !a.has_valued_yield,
            "expression-position yield must not be counted as a statement-level valued yield"
        );
    }

    #[test]
    fn analyze_await_in_switch_case_test_is_inspected() {
        let case = HirSwitchCase::new(
            Some(HirExpr::Await {
                expr: Box::new(HirExpr::Unit(Span::new(40, 41))),
                ty: ts_aot_core::TypeId::from_raw(0),
                span: Span::new(40, 41),
            }),
            Vec::new(),
        );
        let stmt = HirStmt::Switch {
            disc: HirExpr::Int(0, Span::default()),
            cases: vec![case],
        };
        let a = analyze_generator_body(&[stmt]);
        assert!(
            a.has_await,
            "await inside a Switch case test must be inspected by the walker"
        );
        assert_eq!(a.first_await_span, Some(Span::new(40, 41)));
    }

    #[test]
    fn analyze_switch_case_with_no_test_still_walks_body() {
        let case = HirSwitchCase::new(None, vec![yield_stmt(Some(8), 60)]);
        let stmt = HirStmt::Switch {
            disc: HirExpr::Int(0, Span::default()),
            cases: vec![case],
        };
        let a = analyze_generator_body(&[stmt]);
        assert!(a.has_valued_yield);
    }

    #[test]
    fn analyze_throw_inside_if_body() {
        let stmt = HirStmt::If {
            cond: HirExpr::Bool(true, Span::default()),
            then: Box::new(throw_stmt()),
            otherwise: None,
        };
        let a = analyze_generator_body(&[stmt]);
        assert!(a.has_throw);
        assert!(!a.has_valued_yield);
    }

    #[test]
    fn analyze_expression_yield_inside_call_arg() {
        let call = HirStmt::Expr {
            expr: HirExpr::Call {
                callee: HirCallee::Indirect(Box::new(HirExpr::Global {
                    name: ts_aot_core::Atom::from("f"),
                    ty: ts_aot_core::TypeId::from_raw(0),
                    span: Span::default(),
                })),
                args: vec![HirExpr::Yield {
                    expr: Some(Box::new(HirExpr::Int(1, Span::default()))),
                    ty: ts_aot_core::TypeId::from_raw(7),
                    span: Span::new(31, 32),
                }],
                type_args: Vec::new(),
                ty: ts_aot_core::TypeId::from_raw(0),
                span: Span::default(),
            },
        };
        let a = analyze_generator_body(&[call]);
        assert_eq!(a.first_expression_yield_span, Some(Span::new(31, 32)));
    }

    #[test]
    fn analyze_nested_yield_in_return_value() {
        let stmt = HirStmt::Return {
            value: Some(HirExpr::Yield {
                expr: Some(Box::new(HirExpr::Int(1, Span::default()))),
                ty: ts_aot_core::TypeId::from_raw(7),
                span: Span::new(41, 42),
            }),
        };
        let a = analyze_generator_body(&[stmt]);
        assert_eq!(a.first_expression_yield_span, Some(Span::new(41, 42)));
    }

    fn throwing_closure() -> HirExpr {
        HirExpr::Closure {
            id: LocalId::from_raw(0),
            params: vec![HirParam {
                name: ts_aot_core::Atom::from("__closure_param__"),
                ty: ts_aot_core::TypeId::from_raw(0),
            }],
            captures: Vec::new(),
            body: vec![HirStmt::Throw {
                expr: HirExpr::Unit(Span::default()),
            }],
            ty: ts_aot_core::TypeId::from_raw(0),
            span: Span::default(),
        }
    }

    #[test]
    fn analyze_throw_in_nested_closure_does_not_affect_enclosing_generator() {
        let stmt = HirStmt::Expr {
            expr: throwing_closure(),
        };
        let a = analyze_generator_body(&[stmt]);
        assert!(
            !a.has_throw,
            "throw inside a nested closure must not be attributed to the enclosing generator's BodyAnalysis"
        );
        assert!(!a.has_valued_yield);
        assert!(a.first_expression_yield_span.is_none());
    }

    #[test]
    fn analyze_yield_in_nested_closure_does_not_affect_enclosing_generator() {
        let stmt = HirStmt::Expr {
            expr: HirExpr::Closure {
                id: LocalId::from_raw(0),
                params: Vec::new(),
                captures: Vec::new(),
                body: vec![yield_stmt(Some(99), 50)],
                ty: ts_aot_core::TypeId::from_raw(0),
                span: Span::default(),
            },
        };
        let a = analyze_generator_body(&[stmt]);
        assert!(!a.has_throw);
        assert!(
            !a.has_valued_yield,
            "valued yield inside a nested closure must not be attributed to the enclosing generator"
        );
        assert!(
            a.first_expression_yield_span.is_none(),
            "expression-position yield inside a nested closure must not be attributed to the enclosing generator"
        );
        assert!(
            a.first_try_body_yield_span.is_none(),
            "yield inside a nested closure must not be attributed to the enclosing try-body"
        );
        assert!(
            a.first_catch_yield_span.is_none(),
            "yield inside a nested closure must not be attributed to the enclosing catch"
        );
    }

    #[test]
    fn analyze_valued_yield_records_span() {
        let a = analyze_generator_body(&[yield_stmt(Some(7), 17)]);
        assert_eq!(a.first_valued_yield_span, Some(Span::new(17, 18)));
        assert!(a.has_valued_yield);
    }

    #[test]
    fn analyze_first_valued_yield_keeps_earliest_span() {
        let body = vec![yield_stmt(Some(1), 5), yield_stmt(Some(2), 9)];
        let a = analyze_generator_body(&body);
        assert_eq!(a.first_valued_yield_span, Some(Span::new(5, 6)));
    }

    #[test]
    fn analyze_throw_records_expr_span() {
        let stmt = throw_stmt_with_span(23);
        let a = analyze_generator_body(&[stmt]);
        assert!(a.has_throw);
        assert_eq!(a.first_throw_span, Some(Span::new(23, 24)));
    }

    #[test]
    fn analyze_throw_nested_in_if_records_inner_span() {
        let stmt = HirStmt::If {
            cond: HirExpr::Bool(true, Span::default()),
            then: Box::new(throw_stmt_with_span(31)),
            otherwise: None,
        };
        let a = analyze_generator_body(&[stmt]);
        assert!(a.has_throw);
        assert_eq!(a.first_throw_span, Some(Span::new(31, 32)));
    }

    #[test]
    fn analyze_first_throw_keeps_earliest_span() {
        let body = vec![throw_stmt_with_span(11), throw_stmt_with_span(19)];
        let a = analyze_generator_body(&body);
        assert_eq!(a.first_throw_span, Some(Span::new(11, 12)));
    }

    fn await_expr(span_offset: u32) -> HirExpr {
        HirExpr::Await {
            expr: Box::new(HirExpr::Unit(Span::new(span_offset, span_offset + 1))),
            ty: ts_aot_core::TypeId::from_raw(0),
            span: Span::new(span_offset, span_offset + 1),
        }
    }

    #[test]
    fn analyze_await_in_expression_marks_await() {
        let stmt = HirStmt::Expr {
            expr: await_expr(40),
        };
        let a = analyze_generator_body(&[stmt]);
        assert!(a.has_await);
        assert_eq!(a.first_await_span, Some(Span::new(40, 41)));
    }

    #[test]
    fn analyze_await_inside_if_body_marks_await() {
        let stmt = HirStmt::If {
            cond: HirExpr::Bool(true, Span::default()),
            then: Box::new(HirStmt::Expr {
                expr: await_expr(53),
            }),
            otherwise: None,
        };
        let a = analyze_generator_body(&[stmt]);
        assert!(a.has_await);
        assert_eq!(a.first_await_span, Some(Span::new(53, 54)));
    }

    #[test]
    fn analyze_first_await_keeps_earliest_span() {
        let body = vec![
            HirStmt::Expr {
                expr: await_expr(7),
            },
            HirStmt::Expr {
                expr: await_expr(13),
            },
        ];
        let a = analyze_generator_body(&body);
        assert_eq!(a.first_await_span, Some(Span::new(7, 8)));
    }

    #[test]
    fn analyze_await_in_nested_closure_does_not_affect_enclosing_generator() {
        let stmt = HirStmt::Expr {
            expr: HirExpr::Closure {
                id: LocalId::from_raw(0),
                params: Vec::new(),
                captures: Vec::new(),
                body: vec![HirStmt::Expr {
                    expr: await_expr(99),
                }],
                ty: ts_aot_core::TypeId::from_raw(0),
                span: Span::default(),
            },
        };
        let a = analyze_generator_body(&[stmt]);
        assert!(
            !a.has_await,
            "await inside a nested closure must not be attributed to the enclosing generator"
        );
        assert!(a.first_await_span.is_none());
    }

    #[test]
    fn first_body_span_returns_none_for_empty_body() {
        assert!(first_body_span(&[]).is_none());
    }

    #[test]
    fn first_body_span_finds_expr_in_stmt() {
        let stmt = HirStmt::Expr {
            expr: HirExpr::Int(1, Span::new(13, 14)),
        };
        assert_eq!(first_body_span(&[stmt]), Some(Span::new(13, 14)));
    }

    #[test]
    fn first_body_span_finds_nested_expr_span() {
        let stmt = HirStmt::If {
            cond: HirExpr::Bool(true, Span::new(1, 5)),
            then: Box::new(HirStmt::Expr {
                expr: HirExpr::Int(2, Span::new(99, 100)),
            }),
            otherwise: None,
        };
        assert_eq!(first_body_span(&[stmt]), Some(Span::new(1, 5)));
    }

    #[test]
    fn first_body_span_skips_stmts_without_spans() {
        let body = vec![
            HirStmt::Decl(ts_aot_ir_hir::HirDecl::Function(
                ts_aot_ir_hir::HirFunction {
                    name: ts_aot_core::Atom::from("__skip__"),
                    params: Vec::new(),
                    ret: ts_aot_core::TypeId::from_raw(0),
                    throws: None,
                    body: Vec::new(),
                    is_async: false,
                    is_generator: false,
                    is_exported: false,
                    type_params: Vec::new(),
                    async_info: None,
                },
            )),
            HirStmt::Expr {
                expr: HirExpr::Int(1, Span::new(7, 8)),
            },
        ];
        assert_eq!(first_body_span(&body), Some(Span::new(7, 8)));
    }
}
