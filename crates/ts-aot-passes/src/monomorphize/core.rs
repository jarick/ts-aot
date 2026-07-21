use std::collections::{HashMap, HashSet, VecDeque};

use ts_aot_core::{Atom, FunctionId, TypeId, TypeTable};
use ts_aot_ir_hir::{
    HirCallee, HirDecl, HirExpr, HirFunction, HirProgram, HirStmt, ObjectLiteralField,
};

use crate::PassContext;

use super::MonomorphizeStats;
use super::infer::{build_mapping, format_type_args, infer_type_args, type_args_resolved};
use super::substitute::{TypeSubstitutionResult, substitute_func};

type SpecializationKey = (FunctionId, Vec<TypeId>);

pub fn monomorphize(
    program: &mut HirProgram,
    types: &mut TypeTable,
    _ctx: &mut PassContext,
) -> MonomorphizeStats {
    let mut stats = MonomorphizeStats::default();
    let mut next_fid: u32 = 0;
    let mut generic_fn_ids: HashSet<FunctionId> = HashSet::new();
    let mut fn_index: HashMap<FunctionId, HirFunction> = HashMap::new();

    classify_decls(
        &program.declarations,
        &mut next_fid,
        &mut generic_fn_ids,
        &mut fn_index,
        &mut stats,
    );

    let mut worklist: VecDeque<SpecializationKey> = VecDeque::new();
    {
        let mut on_callee = |callee: &mut HirCallee, args: &[HirExpr]| {
            if let HirCallee::Function(fid) = callee
                && generic_fn_ids.contains(fid)
                && let Some(generic_fn) = fn_index.get(fid)
            {
                let type_args = infer_type_args(generic_fn, args, types);
                if type_args_resolved(&type_args, types) {
                    worklist.push_back((*fid, type_args));
                }
            }
        };
        visit_callees(&mut program.declarations, &mut on_callee);
    }

    let mut mono_for_specialization: HashMap<SpecializationKey, FunctionId> = HashMap::new();
    let mut new_decls: Vec<HirDecl> = Vec::new();
    let mut processed: HashSet<SpecializationKey> = HashSet::new();

    while let Some(key) = worklist.pop_front() {
        if !processed.insert(key.clone()) {
            continue;
        }
        let (generic_fid, type_args) = key.clone();
        let Some(generic_fn) = fn_index.get(&generic_fid).cloned() else {
            continue;
        };
        let mapping = build_mapping(&generic_fn, &type_args);
        let mono_fid = FunctionId::from_raw(next_fid);
        next_fid += 1;
        let mut mono_subst_result = TypeSubstitutionResult::default();
        let mut mono = substitute_func(&generic_fn, &mapping, types, &mut mono_subst_result);
        mono.name = Atom::from(format!(
            "{}_mono_{}",
            generic_fn.name.as_str(),
            format_type_args(&type_args)
        ));
        let mono_decl = HirDecl::Function(mono);

        let mut mono_for_scan = mono_decl.clone();
        let mut on_callee = |callee: &mut HirCallee, args: &[HirExpr]| {
            if let HirCallee::Function(fid) = callee
                && generic_fn_ids.contains(fid)
                && let Some(target_fn) = fn_index.get(fid)
            {
                let type_args = infer_type_args(target_fn, args, types);
                if type_args_resolved(&type_args, types) {
                    worklist.push_back((*fid, type_args));
                }
            }
        };
        visit_decl_callees(&mut mono_for_scan, &mut on_callee);

        new_decls.push(mono_decl);
        mono_for_specialization.insert(key, mono_fid);
        stats.monomorphized += 1;
    }

    program.declarations.extend(new_decls);

    {
        let mut on_callee = |callee: &mut HirCallee, args: &[HirExpr]| {
            if let HirCallee::Function(fid) = callee
                && generic_fn_ids.contains(fid)
                && let Some(generic_fn) = fn_index.get(fid)
            {
                let type_args = infer_type_args(generic_fn, args, types);
                let key: SpecializationKey = (*fid, type_args);
                if let Some(&mono_fid) = mono_for_specialization.get(&key) {
                    *callee = HirCallee::Function(mono_fid);
                    stats.calls_rewritten += 1;
                }
            }
        };
        visit_callees(&mut program.declarations, &mut on_callee);
    }

    stats
}

fn classify_decls(
    decls: &[HirDecl],
    next_fid: &mut u32,
    generic_fn_ids: &mut HashSet<FunctionId>,
    fn_index: &mut HashMap<FunctionId, HirFunction>,
    stats: &mut MonomorphizeStats,
) {
    for decl in decls {
        match decl {
            HirDecl::Function(f) => {
                let fid = FunctionId::from_raw(*next_fid);
                *next_fid += 1;
                if !f.type_params.is_empty() {
                    generic_fn_ids.insert(fid);
                    stats.generic_functions += 1;
                }
                fn_index.insert(fid, f.clone());
            }
            HirDecl::Class(c) => {
                for m in &c.methods {
                    if m.params.is_empty() {
                        continue;
                    }
                    let fid = FunctionId::from_raw(*next_fid);
                    *next_fid += 1;
                    if !m.type_params.is_empty() {
                        generic_fn_ids.insert(fid);
                        stats.generic_functions += 1;
                    }
                    fn_index.insert(fid, m.clone());
                }
            }
            HirDecl::Namespace { .. } => {}
            _ => {}
        }
    }
}

fn visit_callees(decls: &mut [HirDecl], on_callee: &mut dyn FnMut(&mut HirCallee, &[HirExpr])) {
    for decl in decls {
        visit_decl_callees(decl, on_callee);
    }
}

fn visit_decl_callees(decl: &mut HirDecl, on_callee: &mut dyn FnMut(&mut HirCallee, &[HirExpr])) {
    match decl {
        HirDecl::Function(f) => visit_body_callees(&mut f.body, on_callee),
        HirDecl::Class(c) => {
            for m in &mut c.methods {
                visit_body_callees(&mut m.body, on_callee);
            }
        }
        HirDecl::Global {
            init: Some(expr), ..
        } => visit_expr_callees(expr, on_callee),
        HirDecl::Namespace { members, .. } => visit_callees(members, on_callee),
        HirDecl::TypeAlias { .. }
        | HirDecl::Enum { .. }
        | HirDecl::Interface { .. }
        | HirDecl::Global { init: None, .. } => {}
    }
}

fn visit_body_callees(body: &mut [HirStmt], on_callee: &mut dyn FnMut(&mut HirCallee, &[HirExpr])) {
    for stmt in body {
        visit_stmt_callees(stmt, on_callee);
    }
}

fn visit_stmt_callees(stmt: &mut HirStmt, on_callee: &mut dyn FnMut(&mut HirCallee, &[HirExpr])) {
    match stmt {
        HirStmt::Block(stmts) => visit_body_callees(stmts, on_callee),
        HirStmt::Let {
            init: Some(expr), ..
        } => visit_expr_callees(expr, on_callee),
        HirStmt::Let { init: None, .. } => {}
        HirStmt::Expr { expr } => visit_expr_callees(expr, on_callee),
        HirStmt::If {
            cond,
            then,
            otherwise,
        } => {
            visit_expr_callees(cond, on_callee);
            visit_stmt_callees(then, on_callee);
            if let Some(e) = otherwise.as_mut() {
                visit_stmt_callees(e, on_callee);
            }
        }
        HirStmt::While { cond, body } | HirStmt::DoWhile { body, cond } => {
            visit_expr_callees(cond, on_callee);
            visit_stmt_callees(body, on_callee);
        }
        HirStmt::ForOf { iter, body, .. } | HirStmt::ForIn { iter, body, .. } => {
            visit_expr_callees(iter, on_callee);
            visit_stmt_callees(body, on_callee);
        }
        HirStmt::Switch { disc, cases } => {
            visit_expr_callees(disc, on_callee);
            for case in cases {
                if let Some(test) = case.test.as_mut() {
                    visit_expr_callees(test, on_callee);
                }
                visit_body_callees(&mut case.body, on_callee);
            }
        }
        HirStmt::Return { value: Some(expr) } => visit_expr_callees(expr, on_callee),
        HirStmt::Return { value: None } => {}
        HirStmt::Throw { expr } => visit_expr_callees(expr, on_callee),
        HirStmt::Try {
            body,
            catch,
            finally,
        } => {
            visit_stmt_callees(body, on_callee);
            if let Some(c) = catch.as_mut() {
                visit_stmt_callees(&mut c.body, on_callee);
            }
            if let Some(f) = finally.as_mut() {
                visit_stmt_callees(f, on_callee);
            }
        }
        HirStmt::Break { .. } | HirStmt::Continue { .. } => {}
        HirStmt::Decl(decl) => visit_decl_callees(decl, on_callee),
    }
}

fn visit_expr_callees(expr: &mut HirExpr, on_callee: &mut dyn FnMut(&mut HirCallee, &[HirExpr])) {
    match expr {
        HirExpr::Call { callee, args, .. } => {
            if let HirCallee::Indirect(inner) = callee {
                visit_expr_callees(inner, on_callee);
            }
            on_callee(callee, args);
            for arg in args {
                visit_expr_callees(arg, on_callee);
            }
        }
        HirExpr::Binary { lhs, rhs, .. } => {
            visit_expr_callees(lhs, on_callee);
            visit_expr_callees(rhs, on_callee);
        }
        HirExpr::Unary { expr, .. } => visit_expr_callees(expr, on_callee),
        HirExpr::Field { owner, .. } => visit_expr_callees(owner, on_callee),
        HirExpr::Index { owner, index, .. } => {
            visit_expr_callees(owner, on_callee);
            visit_expr_callees(index, on_callee);
        }
        HirExpr::StructLiteral { fields, .. } => {
            for (_, v) in fields {
                visit_expr_callees(v, on_callee);
            }
        }
        HirExpr::ArrayLiteral { elements, .. } => {
            for el in elements {
                visit_expr_callees(el, on_callee);
            }
        }
        HirExpr::ObjectLiteral { fields, .. } => {
            for f in fields {
                match f {
                    ObjectLiteralField::Property { value, .. } => {
                        visit_expr_callees(value, on_callee);
                    }
                    ObjectLiteralField::Spread(value) => {
                        visit_expr_callees(value, on_callee);
                    }
                }
            }
        }
        HirExpr::Ternary {
            cond,
            then_branch,
            else_branch,
            ..
        } => {
            visit_expr_callees(cond, on_callee);
            visit_expr_callees(then_branch, on_callee);
            visit_expr_callees(else_branch, on_callee);
        }
        HirExpr::Sequence { exprs, .. } => {
            for e in exprs {
                visit_expr_callees(e, on_callee);
            }
        }
        HirExpr::Closure { body, captures, .. } => {
            for cap in captures {
                visit_expr_callees(cap, on_callee);
            }
            visit_body_callees(body, on_callee);
        }
        HirExpr::Await { expr, .. }
        | HirExpr::Yield {
            expr: Some(expr), ..
        } => visit_expr_callees(expr, on_callee),
        HirExpr::Yield { expr: None, .. } => {}
        HirExpr::Template {
            tag, expressions, ..
        } => {
            if let Some(t) = tag.as_mut() {
                visit_expr_callees(t, on_callee);
            }
            for p in expressions {
                visit_expr_callees(p, on_callee);
            }
        }
        HirExpr::New { callee, args, .. } => {
            visit_expr_callees(callee, on_callee);
            for arg in args {
                visit_expr_callees(arg, on_callee);
            }
        }
        HirExpr::OptionalChain { base, .. } => visit_expr_callees(base, on_callee),
        HirExpr::TypeAssertion { expr, .. } => visit_expr_callees(expr, on_callee),
        HirExpr::Assignment { target, value, .. } => {
            visit_expr_callees(target, on_callee);
            visit_expr_callees(value, on_callee);
        }
        HirExpr::CompoundUpdate { target, rhs, .. } => {
            visit_expr_callees(target, on_callee);
            visit_expr_callees(rhs, on_callee);
        }
        HirExpr::Import { source, .. } => visit_expr_callees(source, on_callee),
        HirExpr::Unit
        | HirExpr::Bool(_)
        | HirExpr::Int(_)
        | HirExpr::Float(_)
        | HirExpr::String(_)
        | HirExpr::Null
        | HirExpr::Undefined
        | HirExpr::Local { .. }
        | HirExpr::Global { .. }
        | HirExpr::RegExp { .. }
        | HirExpr::BigInt { .. } => {}
    }
}
