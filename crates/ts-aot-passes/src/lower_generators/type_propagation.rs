use std::collections::{HashMap, HashSet};

use ts_aot_core::{Atom, LocalId, Span, Type, TypeId, TypeTable};
use ts_aot_ir_hir::{
    HirCallee, HirDecl, HirExpr, HirFunction, HirProgram, HirStmt, VisitorMut, walk_expr_mut,
    walk_stmt_mut,
};

use crate::PassContext;
use crate::hir_to_mir::qualified_name;

use super::diagnostics::GENERATOR_DIAG_UNSUPPORTED_YIELD;

pub(super) fn propagate_generator_types(
    program: &mut HirProgram,
    types: &mut TypeTable,
    ctx: &mut PassContext,
    generator_fn_names: &HashSet<Atom>,
    non_generator_names: &HashSet<Atom>,
) {
    const MAX_PROPAGATION_PASSES: usize = 16;
    let name_to_gen_ty = collect_generator_name_types(program, generator_fn_names);

    let mut functions: Vec<(&mut HirFunction, Vec<String>)> = Vec::new();
    for decl in &mut program.declarations {
        collect_functions_with_path_mut(decl, &mut Vec::new(), &mut functions);
    }
    for (f, namespace_path) in functions {
        let mut generator_locals: HashMap<LocalId, TypeId> = HashMap::new();
        let mut reported_generator_value_uses: HashSet<(Atom, Span)> = HashSet::new();
        let mut converged = false;
        for _ in 0..MAX_PROPAGATION_PASSES {
            let before = generator_locals.clone();
            let before_reported = reported_generator_value_uses.clone();
            let mut walker = GenTypeWalker {
                generator_fn_names,
                non_generator_names,
                name_to_gen_ty: &name_to_gen_ty,
                namespace_path: &namespace_path,
                generator_locals: std::mem::take(&mut generator_locals),
                reported_generator_value_uses: std::mem::take(&mut reported_generator_value_uses),
                types: &mut *types,
                ctx: &mut *ctx,
            };
            walker.run(&mut f.body);
            generator_locals = walker.generator_locals;
            reported_generator_value_uses = walker.reported_generator_value_uses;
            if generator_locals == before && reported_generator_value_uses == before_reported {
                converged = true;
                break;
            }
        }
        if !converged {
            let qualified = qualified_name(&namespace_path, f.name.as_str());
            ctx.warning(
                "P0005",
                format!(
                    "generator type propagation did not converge after \
                     {MAX_PROPAGATION_PASSES} passes for `{}` \
                     (deep generator-copy chains may not dispatch `.next()` correctly)",
                    qualified.as_str()
                ),
                Span::new(0, 0),
            );
        }
    }
}

fn collect_functions_with_path_mut<'a>(
    decl: &'a mut HirDecl,
    path: &mut Vec<String>,
    out: &mut Vec<(&'a mut HirFunction, Vec<String>)>,
) {
    match decl {
        HirDecl::Function(f) => out.push((f, path.clone())),
        HirDecl::Class(c) => {
            for m in c.methods.iter_mut() {
                out.push((m, path.clone()));
            }
        }
        HirDecl::Namespace { name, members } => {
            path.push(name.as_str().to_owned());
            for m in members.iter_mut() {
                collect_functions_with_path_mut(m, path, out);
            }
            path.pop();
        }
        HirDecl::TypeAlias { .. }
        | HirDecl::Enum { .. }
        | HirDecl::Global { .. }
        | HirDecl::Interface { .. } => {}
    }
}

fn collect_generator_name_types(
    program: &HirProgram,
    generator_fn_names: &HashSet<Atom>,
) -> HashMap<Atom, TypeId> {
    let mut name_to_gen_ty: HashMap<Atom, TypeId> = HashMap::new();
    for decl in &program.declarations {
        collect_generator_name_types_in(decl, generator_fn_names, &[], &mut name_to_gen_ty);
    }
    name_to_gen_ty
}

fn collect_generator_name_types_in(
    decl: &HirDecl,
    generator_fn_names: &HashSet<Atom>,
    namespace_path: &[String],
    out: &mut HashMap<Atom, TypeId>,
) {
    match decl {
        HirDecl::Function(f) => {
            let qualified = qualified_name(namespace_path, f.name.as_str());
            if generator_fn_names.contains(&qualified) {
                out.insert(qualified, f.ret);
            }
        }
        HirDecl::Namespace { name, members } => {
            let mut child_path: Vec<String> = namespace_path.to_vec();
            child_path.push(name.as_str().to_owned());
            for m in members {
                collect_generator_name_types_in(m, generator_fn_names, &child_path, out);
            }
        }
        _ => {}
    }
}

struct GenTypeWalker<'a> {
    generator_fn_names: &'a HashSet<Atom>,
    non_generator_names: &'a HashSet<Atom>,
    name_to_gen_ty: &'a HashMap<Atom, TypeId>,
    namespace_path: &'a [String],
    generator_locals: HashMap<LocalId, TypeId>,
    reported_generator_value_uses: HashSet<(Atom, Span)>,
    types: &'a mut TypeTable,
    ctx: &'a mut PassContext,
}

impl<'a> GenTypeWalker<'a> {
    fn run(&mut self, stmts: &mut [HirStmt]) {
        for stmt in stmts {
            self.visit_stmt_mut(stmt);
        }
    }

    fn lookup_generator_name(&self, name: &Atom) -> Option<Atom> {
        let name_str = name.as_str();
        if name_str.contains("::") {
            if self.generator_fn_names.contains(name) {
                return Some(name.clone());
            }
            return None;
        }
        for depth in (1..=self.namespace_path.len()).rev() {
            let candidate = qualified_name(&self.namespace_path[..depth], name_str);
            if self.generator_fn_names.contains(&candidate) {
                return Some(candidate);
            }
            if self.non_generator_names.contains(&candidate) {
                return None;
            }
        }
        if self.generator_fn_names.contains(name) {
            Some(name.clone())
        } else {
            None
        }
    }
}

impl<'a> VisitorMut for GenTypeWalker<'a> {
    fn visit_stmt_mut(&mut self, stmt: &mut HirStmt) {
        if let HirStmt::Let { id, ty, init, .. } = stmt {
            if let Some(init_expr) = init {
                self.visit_expr_mut(init_expr);
                if is_generator_type_id(self.types, init_expr.ty()) {
                    let gen_ty = init_expr.ty();
                    *ty = gen_ty;
                    self.generator_locals.insert(*id, gen_ty);
                }
            }
            return;
        }
        walk_stmt_mut(self, stmt);
    }

    fn visit_expr_mut(&mut self, expr: &mut HirExpr) {
        match expr {
            HirExpr::Call {
                callee, args, ty, ..
            } => {
                if let HirCallee::Indirect(inner) = callee {
                    match inner.as_mut() {
                        HirExpr::Global { name, .. } => {
                            if let Some(lookup) = self.lookup_generator_name(name)
                                && let Some(gen_ty) = self.name_to_gen_ty.get(&lookup)
                            {
                                *ty = *gen_ty;
                            }
                        }
                        HirExpr::Field {
                            owner, field_name, ..
                        } => {
                            self.visit_expr_mut(owner);
                            if field_name.as_str() == "next"
                                && is_generator_type_id(self.types, owner.ty())
                            {
                                let inner_ty = match self.types.resolve(owner.ty()) {
                                    Some(Type::Generator { inner }) => *inner,
                                    _ => TypeId::from_raw(0),
                                };
                                *ty = self
                                    .types
                                    .intern(&Type::GeneratorResult { inner: inner_ty });
                            }
                        }
                        other => {
                            self.visit_expr_mut(other);
                        }
                    }
                }
                for arg in args {
                    self.visit_expr_mut(arg);
                }
            }
            HirExpr::Local { id, ty, .. } => {
                if let Some(gen_ty) = self.generator_locals.get(id) {
                    *ty = *gen_ty;
                }
            }
            HirExpr::Assignment { target, value, .. } => {
                self.visit_expr_mut(value);
                if let HirExpr::Local { id: target_id, .. } = target.as_ref()
                    && is_generator_type_id(self.types, value.ty())
                {
                    let gen_ty = value.ty();
                    self.generator_locals.insert(*target_id, gen_ty);
                }
                self.visit_expr_mut(target);
            }
            HirExpr::Global { name, span, .. } => {
                if self.lookup_generator_name(name).is_some()
                    && !self
                        .reported_generator_value_uses
                        .contains(&(name.clone(), *span))
                {
                    self.reported_generator_value_uses
                        .insert((name.clone(), *span));
                    self.ctx.error(
                        GENERATOR_DIAG_UNSUPPORTED_YIELD,
                        format!(
                            "using the generator function `{}` as a value is not supported \
                             (call it directly: `{0}()`)",
                            name.as_str()
                        ),
                        *span,
                    );
                }
            }
            HirExpr::Closure { captures, .. } => {
                for c in captures {
                    self.visit_expr_mut(c);
                }
            }
            _ => walk_expr_mut(self, expr),
        }
    }
}

fn is_generator_type_id(types: &TypeTable, ty: TypeId) -> bool {
    matches!(types.resolve(ty), Some(Type::Generator { .. }))
}
