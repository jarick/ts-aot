use crate::decl::HirDecl;
use crate::expr::HirExpr;
use ts_aot_core::{Atom, LocalId, TypeId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Completion {
    MayFallThrough,
    Returns,
    Throws,
}

impl Completion {
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Returns | Self::Throws)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum HirStmt {
    Block(Vec<HirStmt>),

    Let {
        id: LocalId,
        name: Atom,
        ty: TypeId,
        init: Option<HirExpr>,
    },

    Expr {
        expr: HirExpr,
    },

    If {
        cond: HirExpr,
        then: Box<HirStmt>,
        otherwise: Option<Box<HirStmt>>,
    },
    While {
        cond: HirExpr,
        body: Box<HirStmt>,
    },
    DoWhile {
        body: Box<HirStmt>,
        cond: HirExpr,
    },
    ForOf {
        binding: LocalId,
        iter: HirExpr,
        body: Box<HirStmt>,
    },
    ForIn {
        binding: LocalId,
        iter: HirExpr,
        body: Box<HirStmt>,
    },
    Switch {
        disc: HirExpr,
        cases: Vec<HirSwitchCase>,
    },

    Return {
        value: Option<HirExpr>,
    },
    Break {
        label: Option<Atom>,
    },
    Continue {
        label: Option<Atom>,
    },
    Throw {
        expr: HirExpr,
    },
    Try {
        body: Box<HirStmt>,
        catch: Option<HirCatchClause>,
        finally: Option<Box<HirStmt>>,
    },

    Decl(HirDecl),
}

impl HirStmt {
    #[must_use]
    pub const fn block(stmts: Vec<HirStmt>) -> Self {
        Self::Block(stmts)
    }

    #[must_use]
    pub const fn ret(value: Option<HirExpr>) -> Self {
        Self::Return { value }
    }

    #[must_use]
    pub fn expr(expr: HirExpr) -> Self {
        Self::Expr { expr }
    }

    #[must_use]
    pub fn let_(id: LocalId, name: Atom, ty: TypeId, init: Option<HirExpr>) -> Self {
        Self::Let { id, name, ty, init }
    }

    #[must_use]
    pub fn completion(&self) -> Completion {
        match self {
            Self::Return { value: None } => Completion::Returns,
            Self::Return { value: Some(_) } => Completion::Returns,
            Self::Throw { .. } => Completion::Throws,
            Self::Break { .. } | Self::Continue { .. } => Completion::MayFallThrough,
            Self::Block(stmts) => stmts
                .last()
                .map_or(Completion::MayFallThrough, HirStmt::completion),
            Self::If {
                then,
                otherwise: Some(otherwise),
                ..
            } => combine_completion(then.completion(), otherwise.completion()),
            Self::If {
                then,
                otherwise: None,
                ..
            } => combine_completion(then.completion(), Completion::MayFallThrough),
            Self::Switch { cases, .. } => switch_completion(cases),
            Self::Try {
                body,
                catch,
                finally,
            } => try_completion(body, catch.as_ref(), finally.as_deref()),
            Self::DoWhile { body, .. } => body.completion(),
            Self::While { .. }
            | Self::ForOf { .. }
            | Self::ForIn { .. }
            | Self::Let { .. }
            | Self::Expr { .. }
            | Self::Decl(_) => Completion::MayFallThrough,
        }
    }

    #[must_use]
    pub fn is_terminal(&self) -> bool {
        self.completion().is_terminal()
    }
}

fn combine_completion(a: Completion, b: Completion) -> Completion {
    if a == Completion::MayFallThrough || b == Completion::MayFallThrough {
        Completion::MayFallThrough
    } else if a == Completion::Throws || b == Completion::Throws {
        Completion::Throws
    } else {
        Completion::Returns
    }
}

fn try_completion(
    body: &HirStmt,
    catch: Option<&HirCatchClause>,
    finally: Option<&HirStmt>,
) -> Completion {
    let body_c = body.completion();
    let finally_c = finally.map(HirStmt::completion);

    if let Some(fc) = finally_c
        && fc.is_terminal()
    {
        return fc;
    }

    match body_c {
        Completion::Returns => Completion::Returns,
        Completion::Throws => catch.map_or(Completion::Throws, |c| c.body.completion()),
        Completion::MayFallThrough => Completion::MayFallThrough,
    }
}

fn switch_completion(cases: &[HirSwitchCase]) -> Completion {
    if !cases.iter().any(HirSwitchCase::is_default) {
        return Completion::MayFallThrough;
    }
    let comps: Vec<Completion> = cases
        .iter()
        .enumerate()
        .map(|(i, c)| c.completion_chain(cases, i))
        .collect();

    if comps.contains(&Completion::MayFallThrough) {
        Completion::MayFallThrough
    } else if comps.iter().all(|c| *c == Completion::Returns) {
        Completion::Returns
    } else {
        Completion::Throws
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct HirSwitchCase {
    pub test: Option<HirExpr>,
    pub body: Vec<HirStmt>,
}

impl HirSwitchCase {
    #[must_use]
    pub const fn new(test: Option<HirExpr>, body: Vec<HirStmt>) -> Self {
        Self { test, body }
    }

    #[must_use]
    pub fn is_default(&self) -> bool {
        self.test.is_none()
    }

    #[must_use]
    pub fn is_terminal(&self) -> bool {
        matches!(
            tail_class(&self.body),
            TailClass::Returns | TailClass::Throws
        )
    }

    #[must_use]
    pub fn is_terminal_in_switch(&self, cases: &[HirSwitchCase], index: usize) -> bool {
        self.completion_chain(cases, index).is_terminal()
    }

    #[must_use]
    pub fn completion_chain(&self, cases: &[HirSwitchCase], index: usize) -> Completion {
        match tail_class(&self.body) {
            TailClass::Returns => Completion::Returns,
            TailClass::Throws => Completion::Throws,
            TailClass::ExitsSwitch => Completion::MayFallThrough,
            TailClass::FallsThrough => {
                let next = index + 1;
                if next < cases.len() {
                    cases[next].completion_chain(cases, next)
                } else {
                    Completion::MayFallThrough
                }
            }
        }
    }
}

enum TailClass {
    Returns,
    Throws,
    ExitsSwitch,
    FallsThrough,
}

fn tail_class(stmts: &[HirStmt]) -> TailClass {
    let Some(last) = stmts.last() else {
        return TailClass::FallsThrough;
    };
    match last {
        HirStmt::Return { .. } => TailClass::Returns,
        HirStmt::Throw { .. } => TailClass::Throws,
        HirStmt::Break { .. } | HirStmt::Continue { .. } => TailClass::ExitsSwitch,
        HirStmt::Block(inner) => tail_class(inner),
        other => match other.completion() {
            Completion::Returns => TailClass::Returns,
            Completion::Throws => TailClass::Throws,
            Completion::MayFallThrough => TailClass::FallsThrough,
        },
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct HirCatchClause {
    pub binding: Option<(LocalId, Atom)>,
    pub body: Box<HirStmt>,
}

impl HirCatchClause {
    #[must_use]
    pub const fn new(binding: Option<(LocalId, Atom)>, body: Box<HirStmt>) -> Self {
        Self { binding, body }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expr::{HirBinaryOp, HirExpr};

    #[test]
    fn block_holds_statements() {
        let stmts = vec![HirStmt::ret(None), HirStmt::expr(HirExpr::Int(0))];
        let block = HirStmt::block(stmts.clone());
        match block {
            HirStmt::Block(b) => assert_eq!(b.len(), 2),
            _ => panic!("expected Block"),
        }
    }

    #[test]
    fn let_stmt_preserves_name_and_id() {
        let id = LocalId::from_raw(3);
        let name = Atom::new_inline("7");
        let ty = TypeId::from_raw(0);
        let s = HirStmt::let_(id, name.clone(), ty, Some(HirExpr::Int(42)));
        match s {
            HirStmt::Let {
                id: got_id,
                name: got_name,
                init: Some(HirExpr::Int(v)),
                ..
            } => {
                assert_eq!(got_id, id);
                assert_eq!(got_name, name);
                assert_eq!(v, 42);
            }
            _ => panic!("expected Let with Int init"),
        }
    }

    #[test]
    fn if_stmt_supports_optional_else() {
        let cond = HirExpr::Bool(true);
        let then = Box::new(HirStmt::ret(None));
        let otherwise = Box::new(HirStmt::ret(None));
        let with_else = HirStmt::If {
            cond: cond.clone(),
            then: then.clone(),
            otherwise: Some(otherwise),
        };
        match with_else {
            HirStmt::If {
                otherwise: Some(_), ..
            } => {}
            _ => panic!("expected If with otherwise"),
        }
        let no_else = HirStmt::If {
            cond,
            then,
            otherwise: None,
        };
        match no_else {
            HirStmt::If {
                otherwise: None, ..
            } => {}
            _ => panic!("expected If without otherwise"),
        }
    }

    #[test]
    fn while_and_dowhile_are_distinct() {
        let cond = HirExpr::Bool(true);
        let body = Box::new(HirStmt::expr(HirExpr::Unit));
        let w = HirStmt::While {
            cond: cond.clone(),
            body: body.clone(),
        };
        let d = HirStmt::DoWhile { body, cond };
        assert_ne!(w, d);
    }

    #[test]
    fn forof_carries_binding_and_iter() {
        let binding = LocalId::from_raw(2);
        let iter = HirExpr::String(ts_aot_core::Atom::new_inline("9"));
        let body = Box::new(HirStmt::expr(HirExpr::Unit));
        let s = HirStmt::ForOf {
            binding,
            iter: iter.clone(),
            body,
        };
        match s {
            HirStmt::ForOf {
                binding: b,
                iter: i,
                ..
            } => {
                assert_eq!(b, binding);
                assert_eq!(i, iter);
            }
            _ => panic!("expected ForOf"),
        }
    }

    #[test]
    fn return_with_and_without_value() {
        let r1 = HirStmt::ret(None);
        let r2 = HirStmt::ret(Some(HirExpr::Int(1)));
        assert_ne!(r1, r2);
        match r1 {
            HirStmt::Return { value: None } => {}
            _ => panic!("expected Return without value"),
        }
    }

    #[test]
    fn break_and_continue_carry_optional_label() {
        let labeled = HirStmt::Break {
            label: Some(Atom::new_inline("1")),
        };
        let plain = HirStmt::Break { label: None };
        assert_ne!(labeled, plain);
        let cont = HirStmt::Continue {
            label: Some(Atom::new_inline("1")),
        };
        assert_ne!(labeled, cont);
    }

    #[test]
    fn throw_holds_expression() {
        let s = HirStmt::Throw {
            expr: HirExpr::String(ts_aot_core::Atom::new_inline("11")),
        };
        match s {
            HirStmt::Throw {
                expr: HirExpr::String(id),
            } => {
                assert_eq!(id, ts_aot_core::Atom::new_inline("11"));
            }
            _ => panic!("expected Throw with String"),
        }
    }

    #[test]
    fn switch_case_default_detection() {
        let case = HirSwitchCase::new(Some(HirExpr::Int(1)), vec![]);
        let default = HirSwitchCase::new(None, vec![]);
        assert!(!case.is_default());
        assert!(default.is_default());
    }

    #[test]
    fn try_with_catch_and_finally() {
        let body = Box::new(HirStmt::expr(HirExpr::Unit));
        let binding = (LocalId::from_raw(0), Atom::new_inline("1"));
        let catch_body = Box::new(HirStmt::ret(None));
        let catch = HirCatchClause::new(Some(binding), catch_body);
        let finally = Box::new(HirStmt::ret(None));
        let s = HirStmt::Try {
            body,
            catch: Some(catch),
            finally: Some(finally),
        };
        match s {
            HirStmt::Try {
                catch: Some(c),
                finally: Some(_),
                ..
            } => {
                assert!(c.binding.is_some());
            }
            _ => panic!("expected Try with catch and finally"),
        }
    }

    #[test]
    fn is_terminal_recognises_control_flow() {
        assert!(HirStmt::ret(None).is_terminal());
        assert!(
            HirStmt::Throw {
                expr: HirExpr::Unit
            }
            .is_terminal()
        );
        assert!(!HirStmt::expr(HirExpr::Unit).is_terminal());
        assert!(HirStmt::ret(Some(HirExpr::Int(0))).is_terminal());
    }

    #[test]
    fn break_and_continue_are_not_function_level_terminal() {
        assert!(!HirStmt::Break { label: None }.is_terminal());
        assert!(!HirStmt::Continue { label: None }.is_terminal());
        assert!(
            !HirStmt::Break {
                label: Some(Atom::new_inline("1"))
            }
            .is_terminal()
        );
    }

    #[test]
    fn block_ending_in_break_is_not_terminal() {
        let block = HirStmt::block(vec![
            HirStmt::expr(HirExpr::Int(1)),
            HirStmt::Break { label: None },
        ]);
        assert!(!block.is_terminal());
    }

    #[test]
    fn if_with_break_branch_is_not_terminal() {
        let s = HirStmt::If {
            cond: HirExpr::Bool(true),
            then: Box::new(HirStmt::ret(None)),
            otherwise: Some(Box::new(HirStmt::Break { label: None })),
        };
        assert!(!s.is_terminal());
    }

    #[test]
    fn nested_block_and_if() {
        let inner_if = HirStmt::If {
            cond: HirExpr::Bool(true),
            then: Box::new(HirStmt::ret(None)),
            otherwise: None,
        };
        let outer = HirStmt::block(vec![inner_if.clone()]);
        match outer {
            HirStmt::Block(stmts) => {
                assert_eq!(stmts.len(), 1);
                assert_eq!(stmts[0], inner_if);
            }
            _ => panic!("expected Block"),
        }
    }

    #[test]
    fn stmt_supports_equality_and_hash() {
        use std::collections::HashSet;
        let a = HirStmt::ret(Some(HirExpr::Int(1)));
        let b = HirStmt::ret(Some(HirExpr::Int(1)));
        let c = HirStmt::ret(Some(HirExpr::Int(2)));
        assert_eq!(a, b);
        assert_ne!(a, c);

        let mut set = HashSet::new();
        set.insert(a.clone());
        set.insert(b);
        set.insert(c);
        assert_eq!(set.len(), 2);
    }

    #[test]
    fn binary_expr_inside_expr_stmt() {
        let expr = HirExpr::Binary {
            op: HirBinaryOp::Add,
            lhs: Box::new(HirExpr::Int(1)),
            rhs: Box::new(HirExpr::Int(2)),
            ty: TypeId::from_raw(0),
        };
        let s = HirStmt::expr(expr.clone());
        match s {
            HirStmt::Expr { expr: e } => assert_eq!(e, expr),
            _ => panic!("expected Expr"),
        }
    }

    #[test]
    fn block_with_terminal_tail_is_terminal() {
        let block = HirStmt::block(vec![HirStmt::expr(HirExpr::Int(1)), HirStmt::ret(None)]);
        assert!(block.is_terminal());
    }

    #[test]
    fn block_with_non_terminal_tail_is_not_terminal() {
        let block = HirStmt::block(vec![HirStmt::ret(None), HirStmt::expr(HirExpr::Int(1))]);
        assert!(!block.is_terminal());
    }

    #[test]
    fn empty_block_is_not_terminal() {
        let block = HirStmt::block(vec![]);
        assert!(!block.is_terminal());
    }

    #[test]
    fn if_with_both_branches_terminal_is_terminal() {
        let s = HirStmt::If {
            cond: HirExpr::Bool(true),
            then: Box::new(HirStmt::ret(None)),
            otherwise: Some(Box::new(HirStmt::Throw {
                expr: HirExpr::Unit,
            })),
        };
        assert!(s.is_terminal());
    }

    #[test]
    fn if_without_else_is_not_terminal_even_with_terminal_then() {
        let s = HirStmt::If {
            cond: HirExpr::Bool(true),
            then: Box::new(HirStmt::ret(None)),
            otherwise: None,
        };
        assert!(!s.is_terminal());
    }

    #[test]
    fn if_with_non_terminal_otherwise_is_not_terminal() {
        let s = HirStmt::If {
            cond: HirExpr::Bool(true),
            then: Box::new(HirStmt::ret(None)),
            otherwise: Some(Box::new(HirStmt::expr(HirExpr::Int(0)))),
        };
        assert!(!s.is_terminal());
    }

    #[test]
    fn switch_with_all_terminal_cases_is_terminal() {
        let cases = vec![
            HirSwitchCase::new(Some(HirExpr::Int(1)), vec![HirStmt::ret(None)]),
            HirSwitchCase::new(None, vec![HirStmt::ret(None)]),
        ];
        let s = HirStmt::Switch {
            disc: HirExpr::Int(0),
            cases,
        };
        assert!(s.is_terminal());
    }

    #[test]
    fn switch_with_non_terminal_case_is_not_terminal() {
        let cases = vec![
            HirSwitchCase::new(Some(HirExpr::Int(1)), vec![HirStmt::ret(None)]),
            HirSwitchCase::new(None, vec![HirStmt::expr(HirExpr::Unit)]),
        ];
        let s = HirStmt::Switch {
            disc: HirExpr::Int(0),
            cases,
        };
        assert!(!s.is_terminal());
    }

    #[test]
    fn try_with_terminal_body_and_no_catch_is_terminal() {
        let s = HirStmt::Try {
            body: Box::new(HirStmt::ret(None)),
            catch: None,
            finally: None,
        };
        assert!(s.is_terminal());
    }

    #[test]
    fn try_with_return_body_and_non_terminal_catch_is_terminal() {
        let s = HirStmt::Try {
            body: Box::new(HirStmt::ret(None)),
            catch: Some(HirCatchClause::new(
                None,
                Box::new(HirStmt::expr(HirExpr::Int(0))),
            )),
            finally: None,
        };
        assert!(s.is_terminal());
        assert_eq!(s.completion(), Completion::Returns);
    }

    #[test]
    fn try_with_throw_body_and_non_terminal_catch_is_not_terminal() {
        let s = HirStmt::Try {
            body: Box::new(HirStmt::Throw {
                expr: HirExpr::Unit,
            }),
            catch: Some(HirCatchClause::new(
                None,
                Box::new(HirStmt::expr(HirExpr::Int(0))),
            )),
            finally: None,
        };
        assert!(!s.is_terminal());
        assert_eq!(s.completion(), Completion::MayFallThrough);
    }

    #[test]
    fn try_with_throw_body_and_return_catch_is_terminal() {
        let s = HirStmt::Try {
            body: Box::new(HirStmt::Throw {
                expr: HirExpr::Unit,
            }),
            catch: Some(HirCatchClause::new(None, Box::new(HirStmt::ret(None)))),
            finally: None,
        };
        assert!(s.is_terminal());
        assert_eq!(s.completion(), Completion::Returns);
    }

    #[test]
    fn return_with_value_completes_as_returns() {
        let s = HirStmt::ret(Some(HirExpr::Int(1)));
        assert_eq!(s.completion(), Completion::Returns);
    }

    #[test]
    fn bare_return_completes_as_returns() {
        assert_eq!(HirStmt::ret(None).completion(), Completion::Returns);
    }

    #[test]
    fn throw_completes_as_throws() {
        let s = HirStmt::Throw {
            expr: HirExpr::Unit,
        };
        assert_eq!(s.completion(), Completion::Throws);
    }

    #[test]
    fn expr_completes_as_may_fall_through() {
        assert_eq!(
            HirStmt::expr(HirExpr::Unit).completion(),
            Completion::MayFallThrough
        );
    }

    #[test]
    fn break_and_continue_completes_as_may_fall_through() {
        assert_eq!(
            HirStmt::Break { label: None }.completion(),
            Completion::MayFallThrough
        );
        assert_eq!(
            HirStmt::Continue { label: None }.completion(),
            Completion::MayFallThrough
        );
    }

    #[test]
    fn if_with_return_and_throw_completes_as_throws() {
        let s = HirStmt::If {
            cond: HirExpr::Bool(true),
            then: Box::new(HirStmt::ret(None)),
            otherwise: Some(Box::new(HirStmt::Throw {
                expr: HirExpr::Unit,
            })),
        };
        assert_eq!(s.completion(), Completion::Throws);
        assert!(s.is_terminal());
    }

    #[test]
    fn switch_without_default_is_not_terminal() {
        let cases = vec![HirSwitchCase::new(
            Some(HirExpr::Int(1)),
            vec![HirStmt::ret(None)],
        )];
        let s = HirStmt::Switch {
            disc: HirExpr::Int(0),
            cases,
        };
        assert!(!s.is_terminal());
    }

    #[test]
    fn try_with_terminal_finally_is_terminal_even_with_non_terminal_body() {
        let s = HirStmt::Try {
            body: Box::new(HirStmt::expr(HirExpr::Int(0))),
            catch: None,
            finally: Some(Box::new(HirStmt::ret(None))),
        };
        assert!(s.is_terminal());
    }

    #[test]
    fn try_with_terminal_finally_overrides_non_terminal_catch() {
        let s = HirStmt::Try {
            body: Box::new(HirStmt::ret(None)),
            catch: Some(HirCatchClause::new(
                None,
                Box::new(HirStmt::expr(HirExpr::Int(0))),
            )),
            finally: Some(Box::new(HirStmt::Throw {
                expr: HirExpr::Unit,
            })),
        };
        assert!(s.is_terminal());
    }

    #[test]
    fn try_with_non_terminal_finally_propagates_terminal_body() {
        let s = HirStmt::Try {
            body: Box::new(HirStmt::ret(None)),
            catch: None,
            finally: Some(Box::new(HirStmt::expr(HirExpr::Int(0)))),
        };
        assert!(s.is_terminal());
    }

    #[test]
    fn try_with_terminal_body_and_catch_and_non_terminal_finally_is_terminal() {
        let s = HirStmt::Try {
            body: Box::new(HirStmt::ret(None)),
            catch: Some(HirCatchClause::new(None, Box::new(HirStmt::ret(None)))),
            finally: Some(Box::new(HirStmt::expr(HirExpr::Int(0)))),
        };
        assert!(s.is_terminal());
    }

    #[test]
    fn try_with_non_terminal_body_and_non_terminal_finally_is_not_terminal() {
        let s = HirStmt::Try {
            body: Box::new(HirStmt::expr(HirExpr::Int(0))),
            catch: None,
            finally: Some(Box::new(HirStmt::expr(HirExpr::Int(0)))),
        };
        assert!(!s.is_terminal());
    }

    #[test]
    fn try_with_non_terminal_body_and_non_terminal_catch_is_not_terminal() {
        let s = HirStmt::Try {
            body: Box::new(HirStmt::expr(HirExpr::Int(0))),
            catch: Some(HirCatchClause::new(
                None,
                Box::new(HirStmt::expr(HirExpr::Int(0))),
            )),
            finally: Some(Box::new(HirStmt::expr(HirExpr::Int(0)))),
        };
        assert!(!s.is_terminal());
    }

    #[test]
    fn try_throw_with_non_terminal_finally_is_terminal() {
        let s = HirStmt::Try {
            body: Box::new(HirStmt::Throw {
                expr: HirExpr::Unit,
            }),
            catch: None,
            finally: Some(Box::new(HirStmt::expr(HirExpr::Int(0)))),
        };
        assert!(s.is_terminal());
    }

    #[test]
    fn switchcase_with_terminal_last_stmt_is_terminal() {
        let case = HirSwitchCase::new(
            Some(HirExpr::Int(1)),
            vec![HirStmt::expr(HirExpr::Int(0)), HirStmt::ret(None)],
        );
        assert!(case.is_terminal());
    }

    #[test]
    fn switchcase_with_non_terminal_last_stmt_is_not_terminal() {
        let case = HirSwitchCase::new(
            Some(HirExpr::Int(1)),
            vec![HirStmt::ret(None), HirStmt::expr(HirExpr::Int(0))],
        );
        assert!(!case.is_terminal());
    }

    #[test]
    fn switchcase_empty_body_is_not_terminal() {
        let case = HirSwitchCase::new(Some(HirExpr::Int(1)), vec![]);
        assert!(!case.is_terminal());
    }

    #[test]
    fn switch_case_fallthrough_to_terminal_default_is_terminal() {
        let cases = vec![
            HirSwitchCase::new(Some(HirExpr::Int(1)), vec![]),
            HirSwitchCase::new(None, vec![HirStmt::ret(None)]),
        ];
        assert!(cases[0].is_terminal_in_switch(&cases, 0));
        assert!(cases[1].is_terminal_in_switch(&cases, 1));
    }

    #[test]
    fn switch_case_fallthrough_to_break_default_is_not_terminal() {
        let cases = vec![
            HirSwitchCase::new(Some(HirExpr::Int(1)), vec![]),
            HirSwitchCase::new(None, vec![HirStmt::Break { label: None }]),
        ];
        assert!(!cases[0].is_terminal_in_switch(&cases, 0));
        assert!(!cases[1].is_terminal_in_switch(&cases, 1));
    }

    #[test]
    fn switch_case_breaking_inside_block_is_not_terminal() {
        let cases = vec![
            HirSwitchCase::new(
                Some(HirExpr::Int(1)),
                vec![HirStmt::block(vec![HirStmt::Break { label: None }])],
            ),
            HirSwitchCase::new(None, vec![HirStmt::ret(None)]),
        ];
        assert!(!cases[0].is_terminal_in_switch(&cases, 0));
    }

    #[test]
    fn switch_chain_fallthrough_through_non_terminal_to_return() {
        let cases = vec![
            HirSwitchCase::new(Some(HirExpr::Int(1)), vec![HirStmt::expr(HirExpr::Unit)]),
            HirSwitchCase::new(Some(HirExpr::Int(2)), vec![HirStmt::expr(HirExpr::Unit)]),
            HirSwitchCase::new(None, vec![HirStmt::ret(None)]),
        ];
        assert!(cases[0].is_terminal_in_switch(&cases, 0));
        assert!(cases[1].is_terminal_in_switch(&cases, 1));
        assert!(cases[2].is_terminal_in_switch(&cases, 2));
    }

    #[test]
    fn switch_case_last_non_terminal_with_no_following_case_is_not_terminal() {
        let cases = vec![HirSwitchCase::new(
            Some(HirExpr::Int(1)),
            vec![HirStmt::expr(HirExpr::Unit)],
        )];
        assert!(!cases[0].is_terminal_in_switch(&cases, 0));
    }

    #[test]
    fn switch_breaks_only_is_not_terminal() {
        let cases = vec![
            HirSwitchCase::new(Some(HirExpr::Int(1)), vec![HirStmt::Break { label: None }]),
            HirSwitchCase::new(None, vec![HirStmt::Break { label: None }]),
        ];
        let s = HirStmt::Switch {
            disc: HirExpr::Int(0),
            cases,
        };
        assert!(!s.is_terminal());
    }

    #[test]
    fn switch_fallthrough_to_return_is_terminal() {
        let cases = vec![
            HirSwitchCase::new(Some(HirExpr::Int(1)), vec![]),
            HirSwitchCase::new(None, vec![HirStmt::ret(None)]),
        ];
        let s = HirStmt::Switch {
            disc: HirExpr::Int(0),
            cases,
        };
        assert!(s.is_terminal());
    }

    #[test]
    fn dowhile_with_return_body_is_terminal() {
        let s = HirStmt::DoWhile {
            body: Box::new(HirStmt::ret(None)),
            cond: HirExpr::Bool(true),
        };
        assert!(s.is_terminal());
        assert_eq!(s.completion(), Completion::Returns);
    }

    #[test]
    fn dowhile_with_throw_body_is_terminal() {
        let s = HirStmt::DoWhile {
            body: Box::new(HirStmt::Throw {
                expr: HirExpr::Unit,
            }),
            cond: HirExpr::Bool(true),
        };
        assert!(s.is_terminal());
        assert_eq!(s.completion(), Completion::Throws);
    }

    #[test]
    fn dowhile_with_fallthrough_body_is_not_terminal() {
        let s = HirStmt::DoWhile {
            body: Box::new(HirStmt::expr(HirExpr::Int(0))),
            cond: HirExpr::Bool(false),
        };
        assert!(!s.is_terminal());
        assert_eq!(s.completion(), Completion::MayFallThrough);
    }

    #[test]
    fn dowhile_with_try_return_body_is_terminal_regardless_of_cond() {
        let s = HirStmt::DoWhile {
            body: Box::new(HirStmt::Try {
                body: Box::new(HirStmt::ret(None)),
                catch: None,
                finally: None,
            }),
            cond: HirExpr::Bool(false),
        };
        assert!(s.is_terminal());
        assert_eq!(s.completion(), Completion::Returns);
    }

    #[test]
    fn dowhile_with_if_return_body_is_terminal() {
        let s = HirStmt::DoWhile {
            body: Box::new(HirStmt::If {
                cond: HirExpr::Bool(false),
                then: Box::new(HirStmt::ret(None)),
                otherwise: Some(Box::new(HirStmt::ret(None))),
            }),
            cond: HirExpr::Bool(false),
        };
        assert!(s.is_terminal());
        assert_eq!(s.completion(), Completion::Returns);
    }
}
