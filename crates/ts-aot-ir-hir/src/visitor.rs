use crate::expr::{HirCallee, HirExpr};
use crate::stmt::{HirStmt, HirSwitchCase};

pub trait Visitor {
    fn visit_stmt(&mut self, stmt: &HirStmt) {
        walk_stmt(self, stmt);
    }
    fn visit_expr(&mut self, expr: &HirExpr) {
        walk_expr(self, expr);
    }
    fn visit_block(&mut self, stmts: &[HirStmt]) {
        for s in stmts {
            self.visit_stmt(s);
        }
    }
    fn visit_switch_case(&mut self, case: &HirSwitchCase) {
        if let Some(test) = &case.test {
            self.visit_expr(test);
        }
        for s in &case.body {
            self.visit_stmt(s);
        }
    }
}

pub trait VisitorMut {
    fn visit_stmt_mut(&mut self, stmt: &mut HirStmt) {
        walk_stmt_mut(self, stmt);
    }
    fn visit_expr_mut(&mut self, expr: &mut HirExpr) {
        walk_expr_mut(self, expr);
    }
    fn visit_block_mut(&mut self, stmts: &mut [HirStmt]) {
        for s in stmts {
            self.visit_stmt_mut(s);
        }
    }
    fn visit_switch_case_mut(&mut self, case: &mut HirSwitchCase) {
        if let Some(test) = case.test.as_mut() {
            self.visit_expr_mut(test);
        }
        for s in &mut case.body {
            self.visit_stmt_mut(s);
        }
    }
}

pub fn walk_stmt<V: Visitor + ?Sized>(v: &mut V, stmt: &HirStmt) {
    match stmt {
        HirStmt::Block(stmts) => v.visit_block(stmts),
        HirStmt::Let { init, .. } => {
            if let Some(e) = init {
                v.visit_expr(e);
            }
        }
        HirStmt::Expr { expr } => v.visit_expr(expr),
        HirStmt::If {
            cond,
            then,
            otherwise,
        } => {
            v.visit_expr(cond);
            v.visit_stmt(then);
            if let Some(b) = otherwise {
                v.visit_stmt(b);
            }
        }
        HirStmt::While { cond, body } => {
            v.visit_expr(cond);
            v.visit_stmt(body);
        }
        HirStmt::DoWhile { body, cond } => {
            v.visit_stmt(body);
            v.visit_expr(cond);
        }
        HirStmt::ForOf { iter, body, .. }
        | HirStmt::ForAwaitOf { iter, body, .. }
        | HirStmt::ForIn { iter, body, .. } => {
            v.visit_expr(iter);
            v.visit_stmt(body);
        }
        HirStmt::Switch { disc, cases } => {
            v.visit_expr(disc);
            for c in cases {
                v.visit_switch_case(c);
            }
        }
        HirStmt::Return { value } => {
            if let Some(e) = value {
                v.visit_expr(e);
            }
        }
        HirStmt::Break { .. } | HirStmt::Continue { .. } => {}
        HirStmt::Throw { expr } => v.visit_expr(expr),
        HirStmt::Try {
            body,
            catch,
            finally,
        } => {
            v.visit_stmt(body);
            if let Some(c) = catch {
                v.visit_stmt(&c.body);
            }
            if let Some(f) = finally {
                v.visit_stmt(f);
            }
        }
        HirStmt::Decl(_) => {}
    }
}

pub fn walk_expr<V: Visitor + ?Sized>(v: &mut V, expr: &HirExpr) {
    match expr {
        HirExpr::Unit(_)
        | HirExpr::Bool(_, _)
        | HirExpr::Int(_, _)
        | HirExpr::Float(_, _)
        | HirExpr::String(_, _)
        | HirExpr::Null(_)
        | HirExpr::Undefined(_)
        | HirExpr::RegExp { .. }
        | HirExpr::BigInt { .. }
        | HirExpr::Local { .. }
        | HirExpr::Global { .. } => {}
        HirExpr::Field { owner, .. } => v.visit_expr(owner),
        HirExpr::Index { owner, index, .. } => {
            v.visit_expr(owner);
            v.visit_expr(index);
        }
        HirExpr::Call { callee, args, .. } => {
            for a in args {
                v.visit_expr(a);
            }
            if let HirCallee::Indirect(inner) = callee {
                v.visit_expr(inner);
            }
        }
        HirExpr::Binary { lhs, rhs, .. } => {
            v.visit_expr(lhs);
            v.visit_expr(rhs);
        }
        HirExpr::Unary { expr, .. } => v.visit_expr(expr),
        HirExpr::StructLiteral { fields, .. } => {
            for (_, e) in fields {
                v.visit_expr(e);
            }
        }
        HirExpr::ObjectLiteral { fields, .. } => {
            for f in fields {
                v.visit_expr(f.value());
            }
        }
        HirExpr::Ternary {
            cond,
            then_branch,
            else_branch,
            ..
        } => {
            v.visit_expr(cond);
            v.visit_expr(then_branch);
            v.visit_expr(else_branch);
        }
        HirExpr::ArrayLiteral { elements, .. } => {
            for e in elements {
                v.visit_expr(e);
            }
        }
        HirExpr::Closure { captures, body, .. } => {
            for c in captures {
                v.visit_expr(c);
            }
            v.visit_block(body);
        }
        HirExpr::Await { expr, .. } => v.visit_expr(expr),
        HirExpr::Yield { expr, .. } => {
            if let Some(e) = expr {
                v.visit_expr(e);
            }
        }
        HirExpr::Template {
            tag, expressions, ..
        } => {
            if let Some(t) = tag {
                v.visit_expr(t);
            }
            for e in expressions {
                v.visit_expr(e);
            }
        }
        HirExpr::New { callee, args, .. } => {
            v.visit_expr(callee);
            for a in args {
                v.visit_expr(a);
            }
        }
        HirExpr::OptionalChain { base, .. } => v.visit_expr(base),
        HirExpr::TypeAssertion { expr, .. } => v.visit_expr(expr),
        HirExpr::Assignment { target, value, .. } => {
            v.visit_expr(target);
            v.visit_expr(value);
        }
        HirExpr::CompoundUpdate { target, rhs, .. } => {
            v.visit_expr(target);
            v.visit_expr(rhs);
        }
        HirExpr::Sequence { exprs, .. } => {
            for e in exprs {
                v.visit_expr(e);
            }
        }
        HirExpr::Import { source, .. } => v.visit_expr(source),
    }
}

pub fn walk_stmt_mut<V: VisitorMut + ?Sized>(v: &mut V, stmt: &mut HirStmt) {
    match stmt {
        HirStmt::Block(stmts) => v.visit_block_mut(stmts),
        HirStmt::Let { init, .. } => {
            if let Some(e) = init {
                v.visit_expr_mut(e);
            }
        }
        HirStmt::Expr { expr } => v.visit_expr_mut(expr),
        HirStmt::If {
            cond,
            then,
            otherwise,
        } => {
            v.visit_expr_mut(cond);
            v.visit_stmt_mut(then);
            if let Some(b) = otherwise {
                v.visit_stmt_mut(b);
            }
        }
        HirStmt::While { cond, body } => {
            v.visit_expr_mut(cond);
            v.visit_stmt_mut(body);
        }
        HirStmt::DoWhile { body, cond } => {
            v.visit_stmt_mut(body);
            v.visit_expr_mut(cond);
        }
        HirStmt::ForOf { iter, body, .. }
        | HirStmt::ForAwaitOf { iter, body, .. }
        | HirStmt::ForIn { iter, body, .. } => {
            v.visit_expr_mut(iter);
            v.visit_stmt_mut(body);
        }
        HirStmt::Switch { disc, cases } => {
            v.visit_expr_mut(disc);
            for c in cases {
                v.visit_switch_case_mut(c);
            }
        }
        HirStmt::Return { value } => {
            if let Some(e) = value {
                v.visit_expr_mut(e);
            }
        }
        HirStmt::Break { .. } | HirStmt::Continue { .. } => {}
        HirStmt::Throw { expr } => v.visit_expr_mut(expr),
        HirStmt::Try {
            body,
            catch,
            finally,
        } => {
            v.visit_stmt_mut(body);
            if let Some(c) = catch {
                v.visit_stmt_mut(&mut c.body);
            }
            if let Some(f) = finally {
                v.visit_stmt_mut(f);
            }
        }
        HirStmt::Decl(_) => {}
    }
}

pub fn walk_expr_mut<V: VisitorMut + ?Sized>(v: &mut V, expr: &mut HirExpr) {
    match expr {
        HirExpr::Unit(_)
        | HirExpr::Bool(_, _)
        | HirExpr::Int(_, _)
        | HirExpr::Float(_, _)
        | HirExpr::String(_, _)
        | HirExpr::Null(_)
        | HirExpr::Undefined(_)
        | HirExpr::RegExp { .. }
        | HirExpr::BigInt { .. }
        | HirExpr::Local { .. }
        | HirExpr::Global { .. } => {}
        HirExpr::Field { owner, .. } => v.visit_expr_mut(owner),
        HirExpr::Index { owner, index, .. } => {
            v.visit_expr_mut(owner);
            v.visit_expr_mut(index);
        }
        HirExpr::Call { callee, args, .. } => {
            for a in args {
                v.visit_expr_mut(a);
            }
            if let HirCallee::Indirect(inner) = callee {
                v.visit_expr_mut(inner);
            }
        }
        HirExpr::Binary { lhs, rhs, .. } => {
            v.visit_expr_mut(lhs);
            v.visit_expr_mut(rhs);
        }
        HirExpr::Unary { expr, .. } => v.visit_expr_mut(expr),
        HirExpr::StructLiteral { fields, .. } => {
            for (_, e) in fields {
                v.visit_expr_mut(e);
            }
        }
        HirExpr::ObjectLiteral { fields, .. } => {
            for f in fields {
                v.visit_expr_mut(f.value_mut());
            }
        }
        HirExpr::Ternary {
            cond,
            then_branch,
            else_branch,
            ..
        } => {
            v.visit_expr_mut(cond);
            v.visit_expr_mut(then_branch);
            v.visit_expr_mut(else_branch);
        }
        HirExpr::ArrayLiteral { elements, .. } => {
            for e in elements {
                v.visit_expr_mut(e);
            }
        }
        HirExpr::Closure { captures, body, .. } => {
            for c in captures {
                v.visit_expr_mut(c);
            }
            v.visit_block_mut(body);
        }
        HirExpr::Await { expr, .. } => v.visit_expr_mut(expr),
        HirExpr::Yield { expr, .. } => {
            if let Some(e) = expr {
                v.visit_expr_mut(e);
            }
        }
        HirExpr::Template {
            tag, expressions, ..
        } => {
            if let Some(t) = tag {
                v.visit_expr_mut(t);
            }
            for e in expressions {
                v.visit_expr_mut(e);
            }
        }
        HirExpr::New { callee, args, .. } => {
            v.visit_expr_mut(callee);
            for a in args {
                v.visit_expr_mut(a);
            }
        }
        HirExpr::OptionalChain { base, .. } => v.visit_expr_mut(base),
        HirExpr::TypeAssertion { expr, .. } => v.visit_expr_mut(expr),
        HirExpr::Assignment { target, value, .. } => {
            v.visit_expr_mut(target);
            v.visit_expr_mut(value);
        }
        HirExpr::CompoundUpdate { target, rhs, .. } => {
            v.visit_expr_mut(target);
            v.visit_expr_mut(rhs);
        }
        HirExpr::Sequence { exprs, .. } => {
            for e in exprs {
                v.visit_expr_mut(e);
            }
        }
        HirExpr::Import { source, .. } => v.visit_expr_mut(source),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ts_aot_core::{LocalId, Span, TypeId};

    fn int_lit(v: i64) -> HirExpr {
        HirExpr::Int(v, Span::new(0, 1))
    }

    struct SpanCollector {
        spans: Vec<Span>,
    }

    impl Visitor for SpanCollector {
        fn visit_stmt(&mut self, stmt: &HirStmt) {
            self.spans.push(stmt_first_span(stmt));
            walk_stmt(self, stmt);
        }
        fn visit_expr(&mut self, expr: &HirExpr) {
            self.spans.push(expr.span());
            walk_expr(self, expr);
        }
    }

    struct LocalTyAssigner {
        target: TypeId,
    }

    impl VisitorMut for LocalTyAssigner {
        fn visit_expr_mut(&mut self, expr: &mut HirExpr) {
            if let HirExpr::Local { ty, .. } = expr {
                *ty = self.target;
            }
            walk_expr_mut(self, expr);
        }
    }

    fn stmt_first_span(stmt: &HirStmt) -> Span {
        match stmt {
            HirStmt::Expr { expr } => expr.span(),
            HirStmt::Let { init: Some(e), .. } => e.span(),
            HirStmt::Let { .. } => Span::new(0, 0),
            HirStmt::If { cond, .. } => cond.span(),
            HirStmt::While { cond, .. } => cond.span(),
            HirStmt::DoWhile { body, .. } => stmt_first_span(body),
            HirStmt::ForOf { iter, .. }
            | HirStmt::ForAwaitOf { iter, .. }
            | HirStmt::ForIn { iter, .. } => iter.span(),
            HirStmt::Switch { disc, .. } => disc.span(),
            HirStmt::Return { value: Some(e), .. } => e.span(),
            HirStmt::Throw { expr } => expr.span(),
            HirStmt::Try { body, .. } => stmt_first_span(body),
            HirStmt::Block(stmts) => stmts
                .first()
                .map(stmt_first_span)
                .unwrap_or_else(|| Span::new(0, 0)),
            HirStmt::Return { value: None, .. }
            | HirStmt::Break { .. }
            | HirStmt::Continue { .. }
            | HirStmt::Decl(_) => Span::new(0, 0),
        }
    }

    #[test]
    fn walks_all_expr_variants() {
        let ty = TypeId::from_raw(0);
        let span = Span::new(0, 1);
        let local = HirExpr::Local {
            id: LocalId::from_raw(0),
            ty,
            span,
        };
        let global = HirExpr::Global {
            name: ts_aot_core::Atom::from("g"),
            ty,
            span,
        };
        let field = HirExpr::Field {
            owner: Box::new(local.clone()),
            field: ts_aot_core::FieldId::from_raw(0),
            field_name: ts_aot_core::Atom::from("f"),
            ty,
            span,
        };
        let exprs: Vec<HirExpr> = vec![
            HirExpr::Unit(span),
            HirExpr::Bool(true, span),
            HirExpr::Int(1, span),
            HirExpr::Float(0, span),
            HirExpr::String(ts_aot_core::Atom::from(""), span),
            HirExpr::Null(span),
            HirExpr::Undefined(span),
            local.clone(),
            global.clone(),
            field.clone(),
            HirExpr::Index {
                owner: Box::new(local.clone()),
                index: Box::new(int_lit(1)),
                ty,
                span,
            },
            HirExpr::Call {
                callee: HirCallee::Function(ts_aot_core::FunctionId::from_raw(0)),
                args: vec![int_lit(1), int_lit(2)],
                type_args: Vec::new(),
                ty,
                span,
            },
            HirExpr::Binary {
                op: crate::expr::HirBinaryOp::Add,
                lhs: Box::new(int_lit(1)),
                rhs: Box::new(int_lit(2)),
                ty,
                span,
            },
            HirExpr::Unary {
                op: crate::expr::HirUnaryOp::Not,
                expr: Box::new(int_lit(0)),
                ty,
                span,
            },
            HirExpr::StructLiteral {
                ty,
                fields: vec![(ts_aot_core::FieldId::from_raw(0), int_lit(1))],
                span,
            },
            HirExpr::ObjectLiteral {
                fields: vec![crate::expr::ObjectLiteralField::Property {
                    name: ts_aot_core::Atom::from("k"),
                    value: int_lit(1),
                }],
                ty,
                span,
            },
            HirExpr::Ternary {
                cond: Box::new(int_lit(1)),
                then_branch: Box::new(int_lit(2)),
                else_branch: Box::new(int_lit(3)),
                ty,
                span,
            },
            HirExpr::ArrayLiteral {
                elements: vec![int_lit(1), int_lit(2)],
                ty,
                span,
            },
            HirExpr::Closure {
                id: LocalId::from_raw(0),
                params: Vec::new(),
                captures: vec![local.clone()],
                body: vec![HirStmt::Expr { expr: int_lit(1) }],
                ty,
                span,
            },
            HirExpr::Await {
                expr: Box::new(int_lit(1)),
                ty,
                span,
            },
            HirExpr::Yield {
                expr: Some(Box::new(int_lit(1))),
                ty,
                span,
            },
            HirExpr::Template {
                tag: None,
                expressions: vec![int_lit(1)],
                cooked_parts: vec![Some(ts_aot_core::Atom::from(""))],
                raw_parts: vec![Some(ts_aot_core::Atom::from(""))],
                ty,
                span,
            },
            HirExpr::New {
                callee: Box::new(global.clone()),
                args: vec![int_lit(1)],
                ty,
                span,
            },
            HirExpr::OptionalChain {
                base: Box::new(field.clone()),
                ty,
                span,
            },
            HirExpr::TypeAssertion {
                expr: Box::new(int_lit(1)),
                target: ty,
                span,
            },
            HirExpr::Assignment {
                target: Box::new(local.clone()),
                value: Box::new(int_lit(1)),
                ty,
                span,
            },
            HirExpr::CompoundUpdate {
                target: Box::new(local.clone()),
                op: crate::expr::HirBinaryOp::Add,
                rhs: Box::new(int_lit(1)),
                post: false,
                ty,
                span,
            },
            HirExpr::Sequence {
                exprs: vec![int_lit(1), int_lit(2)],
                ty,
                span,
            },
            HirExpr::RegExp {
                pattern: ts_aot_core::Atom::from(""),
                flags: ts_aot_core::Atom::from(""),
                ty,
                span,
            },
            HirExpr::BigInt {
                value: ts_aot_core::Atom::from("0"),
                ty,
                span,
            },
            HirExpr::Import {
                source: Box::new(int_lit(1)),
                ty,
                span,
            },
        ];
        let expected_per_variant: Vec<(&str, usize)> = vec![
            ("unit", 1),
            ("bool", 1),
            ("int", 1),
            ("float", 1),
            ("string", 1),
            ("null", 1),
            ("undefined", 1),
            ("local", 1),
            ("global", 1),
            ("field", 2),
            ("index", 3),
            ("call", 3),
            ("binary", 3),
            ("unary", 2),
            ("struct_literal", 2),
            ("object_literal", 2),
            ("ternary", 4),
            ("array_literal", 3),
            ("closure", 4),
            ("await", 2),
            ("yield", 2),
            ("template", 2),
            ("new", 3),
            ("optional_chain", 3),
            ("type_assertion", 2),
            ("assignment", 3),
            ("compound_update", 3),
            ("sequence", 3),
            ("regexp", 1),
            ("bigint", 1),
            ("import", 2),
        ];
        assert_eq!(
            exprs.len(),
            expected_per_variant.len(),
            "fixture and expected table must have the same length; update the table when adding a new variant"
        );
        for (i, (expr, (label, expected))) in
            exprs.iter().zip(expected_per_variant.iter()).enumerate()
        {
            assert_eq!(
                expected_per_variant[i].0, *label,
                "label/index mismatch at position {i}"
            );
            let mut v = SpanCollector { spans: Vec::new() };
            v.visit_expr(expr);
            assert_eq!(
                v.spans.len(),
                *expected,
                "{label}: expected {expected} span visits, got {}",
                v.spans.len()
            );
        }
    }

    #[test]
    fn walks_into_block_and_if() {
        let span = Span::new(0, 1);
        let stmts = vec![
            HirStmt::Expr { expr: int_lit(1) },
            HirStmt::If {
                cond: HirExpr::Bool(true, span),
                then: Box::new(HirStmt::Expr { expr: int_lit(2) }),
                otherwise: Some(Box::new(HirStmt::Expr { expr: int_lit(3) })),
            },
        ];
        let block = HirStmt::Block(stmts);
        let mut v = SpanCollector { spans: Vec::new() };
        v.visit_stmt(&block);
        assert_eq!(v.spans.len(), 9);
    }

    #[test]
    fn walks_try_catch_finally() {
        let stmts = vec![HirStmt::Try {
            body: Box::new(HirStmt::Expr { expr: int_lit(1) }),
            catch: Some(crate::stmt::HirCatchClause::new(
                None,
                Box::new(HirStmt::Expr { expr: int_lit(2) }),
            )),
            finally: Some(Box::new(HirStmt::Expr { expr: int_lit(3) })),
        }];
        let mut v = SpanCollector { spans: Vec::new() };
        v.visit_block(&stmts);
        assert_eq!(v.spans.len(), 7);
    }

    #[test]
    fn mut_walks_all_stmt_variants() {
        let target = TypeId::from_raw(7);
        let mut stmts: Vec<HirStmt> = vec![
            HirStmt::Expr { expr: int_lit(1) },
            HirStmt::If {
                cond: int_lit(0),
                then: Box::new(HirStmt::Expr { expr: int_lit(2) }),
                otherwise: Some(Box::new(HirStmt::Expr { expr: int_lit(3) })),
            },
            HirStmt::While {
                cond: int_lit(0),
                body: Box::new(HirStmt::Expr { expr: int_lit(4) }),
            },
            HirStmt::DoWhile {
                body: Box::new(HirStmt::Expr { expr: int_lit(5) }),
                cond: int_lit(0),
            },
            HirStmt::ForOf {
                binding: LocalId::from_raw(0),
                iter: int_lit(0),
                body: Box::new(HirStmt::Expr { expr: int_lit(6) }),
            },
            HirStmt::ForIn {
                binding: LocalId::from_raw(0),
                iter: int_lit(0),
                body: Box::new(HirStmt::Expr { expr: int_lit(7) }),
            },
            HirStmt::Switch {
                disc: int_lit(0),
                cases: vec![crate::stmt::HirSwitchCase::new(
                    Some(int_lit(1)),
                    vec![HirStmt::Expr { expr: int_lit(8) }],
                )],
            },
            HirStmt::Return {
                value: Some(int_lit(9)),
            },
            HirStmt::Break { label: None },
            HirStmt::Continue { label: None },
            HirStmt::Throw { expr: int_lit(10) },
            HirStmt::Try {
                body: Box::new(HirStmt::Expr { expr: int_lit(11) }),
                catch: Some(crate::stmt::HirCatchClause::new(
                    None,
                    Box::new(HirStmt::Expr { expr: int_lit(12) }),
                )),
                finally: Some(Box::new(HirStmt::Expr { expr: int_lit(13) })),
            },
            HirStmt::Block(vec![HirStmt::Expr { expr: int_lit(14) }]),
            HirStmt::Let {
                id: LocalId::from_raw(0),
                name: ts_aot_core::Atom::from("x"),
                ty: TypeId::from_raw(0),
                init: Some(int_lit(15)),
            },
        ];
        let mut v = LocalTyAssigner { target };
        v.visit_block_mut(&mut stmts);
        let mut int_count = 0;
        for s in &stmts {
            count_ints_mut(s, &mut int_count);
        }
        assert_eq!(int_count, 22);
    }

    fn count_ints_mut(stmt: &HirStmt, count: &mut usize) {
        match stmt {
            HirStmt::Block(stmts) => {
                for s in stmts {
                    count_ints_mut(s, count);
                }
            }
            HirStmt::Let { init: Some(e), .. } => count_ints_in_expr_mut(e, count),
            HirStmt::Let { .. } => {}
            HirStmt::Expr { expr } => count_ints_in_expr_mut(expr, count),
            HirStmt::If {
                cond,
                then,
                otherwise,
            } => {
                count_ints_in_expr_mut(cond, count);
                count_ints_mut(then, count);
                if let Some(b) = otherwise {
                    count_ints_mut(b, count);
                }
            }
            HirStmt::While { cond, body } | HirStmt::DoWhile { body, cond } => {
                count_ints_in_expr_mut(cond, count);
                count_ints_mut(body, count);
            }
            HirStmt::ForOf { iter, body, .. }
            | HirStmt::ForAwaitOf { iter, body, .. }
            | HirStmt::ForIn { iter, body, .. } => {
                count_ints_in_expr_mut(iter, count);
                count_ints_mut(body, count);
            }
            HirStmt::Switch { disc, cases } => {
                count_ints_in_expr_mut(disc, count);
                for c in cases {
                    if let Some(t) = &c.test {
                        count_ints_in_expr_mut(t, count);
                    }
                    for s in &c.body {
                        count_ints_mut(s, count);
                    }
                }
            }
            HirStmt::Return { value: Some(e) } => count_ints_in_expr_mut(e, count),
            HirStmt::Return { value: None }
            | HirStmt::Break { .. }
            | HirStmt::Continue { .. }
            | HirStmt::Decl(_) => {}
            HirStmt::Throw { expr } => count_ints_in_expr_mut(expr, count),
            HirStmt::Try {
                body,
                catch,
                finally,
            } => {
                count_ints_mut(body, count);
                if let Some(c) = catch {
                    count_ints_mut(&c.body, count);
                }
                if let Some(f) = finally {
                    count_ints_mut(f, count);
                }
            }
        }
    }

    fn count_ints_in_expr_mut(expr: &HirExpr, count: &mut usize) {
        if let HirExpr::Int(_, _) = expr {
            *count += 1;
        }
    }

    #[test]
    fn mut_assigns_local_ty() {
        let target = TypeId::from_raw(99);
        let mut local = HirExpr::Local {
            id: LocalId::from_raw(0),
            ty: TypeId::from_raw(0),
            span: Span::new(0, 1),
        };
        let mut v = LocalTyAssigner { target };
        v.visit_expr_mut(&mut local);
        if let HirExpr::Local { ty, .. } = local {
            assert_eq!(ty, target);
        } else {
            panic!("expected Local");
        }
    }

    #[test]
    fn mut_walks_into_block_via_default() {
        let target = TypeId::from_raw(11);
        let mut stmts = vec![
            HirStmt::Expr {
                expr: HirExpr::Local {
                    id: LocalId::from_raw(0),
                    ty: TypeId::from_raw(0),
                    span: Span::new(0, 1),
                },
            },
            HirStmt::Expr { expr: int_lit(1) },
        ];
        let mut v = LocalTyAssigner { target };
        v.visit_block_mut(&mut stmts);
        if let HirStmt::Expr {
            expr: HirExpr::Local { ty, .. },
            ..
        } = &stmts[0]
        {
            assert_eq!(*ty, target);
        } else {
            panic!("expected Expr(Local)");
        }
    }
}
