use std::collections::{HashMap, HashSet, VecDeque};

use ts_aot_core::{Atom, FunctionId, TypeId, TypeTable};
use ts_aot_ir_hir::{
    HirCallee, HirDecl, HirExpr, HirFunction, HirProgram, HirStmt, ObjectLiteralField,
};

use crate::PassContext;
use crate::hir_to_mir::ExprConverter;

use super::MonomorphizeStats;
use super::infer::{build_mapping, format_type_args, infer_type_args, type_args_resolved};
use super::substitute::{TypeSubstitutionResult, substitute_func};

type SpecializationKey = (FunctionId, Vec<TypeId>);
#[derive(Default)]
struct MethodIndex {
    generic: HashMap<(TypeId, Atom, bool), FunctionId>,
    class_names: HashSet<Atom>,
}

impl MethodIndex {
    fn insert(&mut self, key: (TypeId, Atom, bool), fid: FunctionId) {
        self.generic.insert(key, fid);
    }

    fn get(&self, key: &(TypeId, Atom, bool)) -> Option<&FunctionId> {
        self.generic.get(key)
    }
}

type OnCallee<'a> = &'a mut dyn FnMut(&mut HirCallee, &[HirExpr], &[TypeId]);

pub fn monomorphize(
    program: &mut HirProgram,
    types: &mut TypeTable,
    ctx: &mut PassContext,
) -> MonomorphizeStats {
    let mut stats = MonomorphizeStats::default();
    let mut next_fid: u32 = 0;
    let mut generic_fn_ids: HashSet<FunctionId> = HashSet::new();
    let mut fn_index: HashMap<FunctionId, HirFunction> = HashMap::new();

    let mut name_to_generic: HashMap<Atom, FunctionId> = HashMap::new();
    let mut method_to_generic: MethodIndex = MethodIndex::default();
    let mut method_owner: HashMap<FunctionId, TypeId> = HashMap::new();
    classify_decls(
        &program.declarations,
        &mut next_fid,
        &mut generic_fn_ids,
        &mut fn_index,
        &mut name_to_generic,
        &mut method_to_generic,
        &mut method_owner,
        &mut stats,
    );

    let mut worklist: VecDeque<SpecializationKey> = VecDeque::new();
    let mut inference = ExprConverter::new();
    if !generic_fn_ids.is_empty() {
        inference.set_program(std::sync::Arc::new(program.clone()));
    }
    {
        let mut on_callee =
            |callee: &mut HirCallee, args: &[HirExpr], explicit_type_args: &[TypeId]| {
                push_specialization(
                    callee,
                    args,
                    explicit_type_args,
                    &generic_fn_ids,
                    &name_to_generic,
                    &method_to_generic,
                    &fn_index,
                    &inference,
                    types,
                    &mut worklist,
                );
            };
        visit_callees(&mut program.declarations, &mut on_callee);
        for inits in program.class_field_inits.values_mut() {
            for (_, init) in inits.iter_mut() {
                visit_expr_callees(init, &mut on_callee);
            }
        }
    }

    let mut mono_for_specialization: HashMap<SpecializationKey, FunctionId> = HashMap::new();
    let mut specialization_names: HashMap<FunctionId, Atom> = HashMap::new();
    let mut new_decls: Vec<HirDecl> = Vec::new();
    let mut method_specializations: Vec<(TypeId, HirFunction)> = Vec::new();
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
        specialization_names.insert(mono_fid, mono.name.clone());
        let mono_decl = HirDecl::Function(mono.clone());

        let mut mono_for_scan = mono_decl.clone();
        let mut on_callee =
            |callee: &mut HirCallee, args: &[HirExpr], explicit_type_args: &[TypeId]| {
                push_specialization(
                    callee,
                    args,
                    explicit_type_args,
                    &generic_fn_ids,
                    &name_to_generic,
                    &method_to_generic,
                    &fn_index,
                    &inference,
                    types,
                    &mut worklist,
                );
            };
        visit_decl_callees(&mut mono_for_scan, &mut on_callee);

        if let Some(owner_ty) = method_owner.get(&generic_fid).copied() {
            method_specializations.push((owner_ty, mono));
        } else {
            new_decls.push(mono_decl);
        }
        mono_for_specialization.insert(key, mono_fid);
        stats.monomorphized += 1;
    }

    program.declarations.extend(new_decls);
    for (owner_ty, method) in method_specializations {
        let name = method.name.clone();
        let class = program.declarations.iter_mut().find_map(|decl| match decl {
            HirDecl::Class(c) if c.ty == owner_ty => Some(c),
            _ => None,
        });
        match class {
            Some(class) => class.methods.push(method),
            None => ctx.error(
                "E0300",
                format!(
                    "the class of method `{}` is not emitted (it was dropped as unemittable), \
                     so its specialization has nowhere to go",
                    name.as_str()
                ),
                ts_aot_core::Span::new(0, 0),
            ),
        }
    }

    {
        let mut on_callee = |callee: &mut HirCallee,
                             args: &[HirExpr],
                             explicit_type_args: &[TypeId]| {
            let Some(fid) = generic_callee(
                callee,
                &generic_fn_ids,
                &name_to_generic,
                &method_to_generic,
                &inference,
                types,
            ) else {
                return;
            };
            let Some(generic_fn) = fn_index.get(&fid) else {
                return;
            };
            let type_args = infer_for_callee(
                callee,
                &method_to_generic,
                generic_fn,
                args,
                explicit_type_args,
                &inference,
                types,
            );
            let key: SpecializationKey = (fid, type_args);
            if let Some(&mono_fid) = mono_for_specialization.get(&key) {
                let renamed = match (&mut *callee, specialization_names.get(&mono_fid)) {
                    (HirCallee::Indirect(inner), Some(mono_name)) => match inner.as_mut() {
                        HirExpr::Field { field_name, .. } if method_owner.contains_key(&fid) => {
                            *field_name = mono_name.clone();
                            true
                        }
                        _ => false,
                    },
                    _ => false,
                };
                if !renamed {
                    *callee = HirCallee::Function(mono_fid);
                }
                stats.calls_rewritten += 1;
            }
        };
        visit_callees(&mut program.declarations, &mut on_callee);
        for inits in program.class_field_inits.values_mut() {
            for (_, init) in inits.iter_mut() {
                visit_expr_callees(init, &mut on_callee);
            }
        }
    }

    stats
}

fn receiver_class_ty(owner: &HirExpr, converter: &ExprConverter, types: &mut TypeTable) -> TypeId {
    let ty = owner.ty();
    let ty = if matches!(types.resolve(ty), Some(ts_aot_core::Type::Error) | None) {
        converter.expr_type(owner, types).unwrap_or(ty)
    } else {
        ty
    };
    match types.resolve(ty) {
        Some(ts_aot_core::Type::Optional { inner, .. }) => *inner,
        _ => ty,
    }
}

fn receiver_is_static(owner: &HirExpr, class_names: &HashSet<Atom>) -> bool {
    match owner {
        HirExpr::Global { name, .. } => class_names.contains(name),
        _ => false,
    }
}

fn method_generic(
    index: &MethodIndex,
    owner: &HirExpr,
    field_name: &Atom,
    converter: &ExprConverter,
    types: &mut TypeTable,
) -> Option<FunctionId> {
    let ty = receiver_class_ty(owner, converter, types);
    let preferred = receiver_is_static(owner, &index.class_names);
    index.get(&(ty, field_name.clone(), preferred)).copied()
}

fn generic_callee(
    callee: &HirCallee,
    generic_fn_ids: &HashSet<FunctionId>,
    name_to_generic: &HashMap<Atom, FunctionId>,
    method_to_generic: &MethodIndex,
    converter: &ExprConverter,
    types: &mut TypeTable,
) -> Option<FunctionId> {
    match callee {
        HirCallee::Function(fid) if generic_fn_ids.contains(fid) => Some(*fid),
        HirCallee::Function(_) => None,
        HirCallee::Indirect(inner) => match inner.as_ref() {
            HirExpr::Global { name, .. } => name_to_generic.get(name).copied(),
            HirExpr::Field {
                owner, field_name, ..
            } => method_generic(method_to_generic, owner, field_name, converter, types),
            _ => None,
        },
        HirCallee::Closure(_) | HirCallee::Runtime { .. } => None,
    }
}

fn infer_for_callee(
    callee: &HirCallee,
    method_to_generic: &MethodIndex,
    generic_fn: &HirFunction,
    args: &[HirExpr],
    explicit_type_args: &[TypeId],
    converter: &ExprConverter,
    types: &mut TypeTable,
) -> Vec<TypeId> {
    if !explicit_type_args.is_empty() && explicit_type_args.len() == generic_fn.type_params.len() {
        return explicit_type_args.to_vec();
    }
    if let HirCallee::Indirect(inner) = callee
        && let HirExpr::Field {
            owner, field_name, ..
        } = inner.as_ref()
        && method_generic(method_to_generic, owner, field_name, converter, types).is_some()
        && generic_fn
            .params
            .first()
            .is_some_and(|param| param.name.as_str() == "this")
    {
        let mut all = Vec::with_capacity(args.len() + 1);
        all.push(owner.as_ref().clone());
        all.extend(args.iter().cloned());
        return infer_type_args(generic_fn, &all, explicit_type_args, types, converter);
    }
    infer_type_args(generic_fn, args, explicit_type_args, types, converter)
}

fn push_specialization(
    callee: &HirCallee,
    args: &[HirExpr],
    explicit_type_args: &[TypeId],
    generic_fn_ids: &HashSet<FunctionId>,
    name_to_generic: &HashMap<Atom, FunctionId>,
    method_to_generic: &MethodIndex,
    fn_index: &HashMap<FunctionId, HirFunction>,
    converter: &ExprConverter,
    types: &mut TypeTable,
    worklist: &mut VecDeque<SpecializationKey>,
) {
    let Some(fid) = generic_callee(
        callee,
        generic_fn_ids,
        name_to_generic,
        method_to_generic,
        converter,
        types,
    ) else {
        return;
    };
    let Some(generic_fn) = fn_index.get(&fid) else {
        return;
    };
    let type_args = infer_for_callee(
        callee,
        method_to_generic,
        generic_fn,
        args,
        explicit_type_args,
        converter,
        types,
    );
    if type_args_resolved(&type_args, types) {
        worklist.push_back((fid, type_args));
    }
}

fn classify_decls(
    decls: &[HirDecl],
    next_fid: &mut u32,
    generic_fn_ids: &mut HashSet<FunctionId>,
    fn_index: &mut HashMap<FunctionId, HirFunction>,
    top_level_names: &mut HashMap<Atom, FunctionId>,
    method_to_generic: &mut MethodIndex,
    method_owner: &mut HashMap<FunctionId, TypeId>,
    stats: &mut MonomorphizeStats,
) {
    for decl in decls {
        match decl {
            HirDecl::Function(f) => {
                let fid = FunctionId::from_raw(*next_fid);
                *next_fid += 1;
                if !f.type_params.is_empty() {
                    generic_fn_ids.insert(fid);
                    top_level_names.insert(f.name.clone(), fid);
                    stats.generic_functions += 1;
                }
                fn_index.insert(fid, f.clone());
            }
            HirDecl::Class(c) => {
                method_to_generic.class_names.insert(c.name.clone());
                for m in &c.methods {
                    if m.params.is_empty() {
                        continue;
                    }
                    let fid = FunctionId::from_raw(*next_fid);
                    *next_fid += 1;
                    let is_static = m.params.first().is_none_or(|p| p.name.as_str() != "this");
                    let shadowed_by_field = !is_static && c.fields.iter().any(|f| f.name == m.name);
                    if !m.type_params.is_empty() && !shadowed_by_field {
                        generic_fn_ids.insert(fid);
                        method_to_generic.insert((c.ty, m.name.clone(), is_static), fid);
                        method_owner.insert(fid, c.ty);
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
fn visit_callees(decls: &mut [HirDecl], on_callee: OnCallee<'_>) {
    for decl in decls {
        visit_decl_callees(decl, on_callee);
    }
}

fn visit_decl_callees(decl: &mut HirDecl, on_callee: OnCallee<'_>) {
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

fn visit_body_callees(body: &mut [HirStmt], on_callee: OnCallee<'_>) {
    for stmt in body {
        visit_stmt_callees(stmt, on_callee);
    }
}

fn visit_stmt_callees(stmt: &mut HirStmt, on_callee: OnCallee<'_>) {
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
        HirStmt::ForOf { iter, body, .. }
        | HirStmt::ForAwaitOf { iter, body, .. }
        | HirStmt::ForIn { iter, body, .. } => {
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

fn visit_expr_callees(expr: &mut HirExpr, on_callee: OnCallee<'_>) {
    match expr {
        HirExpr::DefaultConstruct { .. } => {}
        HirExpr::Call {
            callee,
            args,
            type_args,
            ..
        } => {
            if let HirCallee::Indirect(inner) = callee {
                visit_expr_callees(inner, on_callee);
            }
            on_callee(callee, args, type_args);
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
        HirExpr::Unit(_)
        | HirExpr::Bool(_, _)
        | HirExpr::Int(_, _)
        | HirExpr::Float(_, _)
        | HirExpr::String(_, _)
        | HirExpr::Null(_)
        | HirExpr::Undefined(_)
        | HirExpr::Local { .. }
        | HirExpr::Global { .. }
        | HirExpr::RegExp { .. }
        | HirExpr::BigInt { .. } => {}
    }
}
