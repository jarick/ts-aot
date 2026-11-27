use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use ts_aot_core::{Atom, LocalId, Span, TypeId, TypeTable};
use ts_aot_ir_hir::{
    HirCallee, HirClass, HirDecl, HirExpr, HirField, HirFunction, HirProgram, HirStmt, Visitor,
    VisitorMut, walk_expr, walk_expr_mut, walk_stmt_mut,
};

use crate::PassContext;

#[cfg(test)]
mod tests;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LowerClassesStats {
    pub flattened_classes: usize,
    pub inherited_fields: usize,
    pub inherited_methods: usize,
    pub overridden_methods: usize,
    pub cycles_detected: usize,
    pub missing_parents: usize,
}

enum CollectResult {
    Ok(Vec<Rc<HirClass>>),
    Missing,
    Cycle,
}

pub fn lower_classes(
    program: &mut HirProgram,
    _types: &mut TypeTable,
    ctx: &mut PassContext,
) -> LowerClassesStats {
    let mut stats = LowerClassesStats::default();

    let class_index = collect_class_index(&program.declarations);

    for decl in &mut program.declarations {
        walk_decl(decl, &class_index, &mut stats, ctx);
    }

    stats
}

fn walk_decl(
    decl: &mut HirDecl,
    index: &HashMap<Atom, Rc<HirClass>>,
    stats: &mut LowerClassesStats,
    ctx: &mut PassContext,
) {
    match decl {
        HirDecl::Class(c) => flatten_class(c, index, stats, ctx),
        HirDecl::Namespace { members, .. } => {
            for m in members {
                walk_decl(m, index, stats, ctx);
            }
        }
        HirDecl::Function(_)
        | HirDecl::TypeAlias { .. }
        | HirDecl::Enum { .. }
        | HirDecl::Global { .. }
        | HirDecl::Interface { .. } => {}
    }
}

fn collect_class_index(decls: &[HirDecl]) -> HashMap<Atom, Rc<HirClass>> {
    let mut index = HashMap::new();
    collect_into(decls, &mut index);
    index
}

fn collect_into(decls: &[HirDecl], index: &mut HashMap<Atom, Rc<HirClass>>) {
    for decl in decls {
        match decl {
            HirDecl::Class(c) => {
                index.insert(c.name.clone(), Rc::new(c.clone()));
            }
            HirDecl::Namespace { members, .. } => collect_into(members, index),
            HirDecl::Function(_)
            | HirDecl::TypeAlias { .. }
            | HirDecl::Enum { .. }
            | HirDecl::Global { .. }
            | HirDecl::Interface { .. } => {}
        }
    }
}

fn flatten_class(
    c: &mut HirClass,
    index: &HashMap<Atom, Rc<HirClass>>,
    stats: &mut LowerClassesStats,
    ctx: &mut PassContext,
) {
    let extends = match c.extends.clone() {
        Some(e) => e,
        None => return,
    };

    let mut parents = match collect_parents(&extends, c.name.clone(), index, ctx) {
        CollectResult::Ok(p) => p,
        CollectResult::Missing => {
            stats.missing_parents += 1;
            c.extends = None;
            return;
        }
        CollectResult::Cycle => {
            stats.cycles_detected += 1;
            c.extends = None;
            return;
        }
    };

    parents.reverse();

    let mut parent_fields: Vec<HirField> = Vec::new();
    for parent in &parents {
        parent_fields.extend(parent.fields.iter().cloned());
    }

    let inherited_field_count = parent_fields.len();

    let own_method_names: HashSet<Atom> = c.methods.iter().map(|m| m.name.clone()).collect();

    let mut kept_parent_methods: Vec<HirFunction> = Vec::new();
    let mut taken_names: HashSet<Atom> = own_method_names.clone();
    let mut override_count: usize = 0;

    for parent in parents.iter().rev() {
        for pm in parent.methods.iter().rev() {
            if taken_names.contains(&pm.name) {
                override_count += 1;
                continue;
            }
            taken_names.insert(pm.name.clone());
            kept_parent_methods.push(pm.clone());
        }
    }
    kept_parent_methods.reverse();
    let inherited_receiver_tys: Vec<TypeId> = parents.iter().map(|parent| parent.ty).collect();
    for method in &mut kept_parent_methods {
        let rejected_writes = retype_this(method, c.ty, &inherited_receiver_tys);
        report_rejected_receiver_writes(ctx, &c.name, &method.name, rejected_writes);
    }

    let inherited_method_count = kept_parent_methods.len();

    let mut super_ref_sources = kept_parent_methods.clone();
    super_ref_sources.extend(c.methods.iter().cloned());
    let class_name = c.name.clone();
    let (mut super_clones, rewrites) =
        build_super_method_clones(&super_ref_sources, c.ty, &class_name, &parents, index, ctx);
    if !rewrites.is_empty() {
        for method in kept_parent_methods.iter_mut().chain(c.methods.iter_mut()) {
            apply_super_alias_rewrites(method, &rewrites);
        }
        for clone in &mut super_clones {
            apply_super_alias_rewrites(clone, &rewrites);
        }
    }
    c.methods.extend(super_clones);

    let mut merged_fields = parent_fields;
    merged_fields.extend(c.fields.iter().cloned());

    let mut merged_methods = kept_parent_methods;
    merged_methods.extend(c.methods.iter().cloned());

    c.fields = merged_fields;
    c.methods = merged_methods;
    c.extends = None;

    stats.flattened_classes += 1;
    stats.inherited_fields += inherited_field_count;
    stats.inherited_methods += inherited_method_count;
    stats.overridden_methods += override_count;
}

fn build_super_method_clones(
    methods: &[HirFunction],
    class_ty: TypeId,
    class_name: &Atom,
    ancestors: &[Rc<HirClass>],
    index: &HashMap<Atom, Rc<HirClass>>,
    ctx: &mut PassContext,
) -> (Vec<HirFunction>, SuperAliasRewrites) {
    let declared_members = declared_member_names(class_name, ancestors, index);
    let declared_alias_rets = declared_alias_rets(&declared_members, ancestors, index);
    let duplicate_members = duplicate_member_names(class_name, ancestors, index);
    let mut pending: Vec<Atom> = collect_super_method_refs(
        methods,
        &declared_members,
        &declared_alias_rets,
        &duplicate_members,
    );
    let ancestor_receiver_tys: Vec<TypeId> = ancestors.iter().map(|ancestor| ancestor.ty).collect();
    let mut cloned: HashSet<Atom> = HashSet::new();
    let mut taken_names: HashSet<Atom> = declared_members.clone();
    let mut rewrites = SuperAliasRewrites::default();
    let mut out: Vec<HirFunction> = Vec::new();
    while let Some(name) = pending.pop() {
        if !cloned.insert(name.clone()) {
            continue;
        }
        let Some((owner, method)) = parse_super_method_name(name.as_str()) else {
            continue;
        };
        let Some(owner_class) = index.get(&owner) else {
            ctx.error(
                "P0023",
                format!(
                    "the super alias `{}` names the unknown class `{}`; a super method must be \
                     an instance method of a declared ancestor",
                    name.as_str(),
                    owner.as_str()
                ),
                Span::new(0, 0),
            );
            continue;
        };
        let resolved = owner_class
            .methods
            .iter()
            .find(|m| m.name == method && !method_is_static(m));
        let Some(original) = resolved else {
            let message = if owner_class.methods.iter().any(|m| m.name == method) {
                format!(
                    "the super alias `{}` names the static method `{}.{}`, which has no instance \
                     receiver; a super method must be an instance method with a `this` parameter",
                    name.as_str(),
                    owner.as_str(),
                    method.as_str()
                )
            } else {
                format!(
                    "the super alias `{}` names no instance method `{}.{}`; a super method must \
                     be an instance method of a declared ancestor",
                    name.as_str(),
                    owner.as_str(),
                    method.as_str()
                )
            };
            ctx.error("P0023", message, Span::new(0, 0));
            continue;
        };
        if !ancestors.iter().any(|ancestor| ancestor.name == owner) {
            ctx.error(
                "P0023",
                format!(
                    "the super alias `{}` names the instance method `{}.{}`, but `{}` is not a \
                     declared ancestor of `{}`; a super method must be an instance method of a \
                     declared ancestor",
                    name.as_str(),
                    owner.as_str(),
                    method.as_str(),
                    owner.as_str(),
                    class_name.as_str()
                ),
                Span::new(0, 0),
            );
            continue;
        }
        let mut clone = original.clone();
        let declared_ret = clone.ret;
        let rejected_writes = retype_this(&mut clone, class_ty, &ancestor_receiver_tys);
        report_rejected_receiver_writes(ctx, class_name, &clone.name, rejected_writes);
        let mut clone_name = name.clone();
        if declared_members.contains(&name) {
            clone_name = fresh_super_alias_name(&name, &taken_names);
            rewrites.renames.insert(name.clone(), clone_name.clone());
            rewrites.alias_rets.insert(name.clone(), declared_ret);
        }
        taken_names.insert(clone_name.clone());
        if clone.ret != declared_ret {
            rewrites
                .rets
                .insert(name.clone(), (declared_ret, clone.ret));
            rewrites.alias_rets.insert(name.clone(), declared_ret);
        }
        clone.name = clone_name;
        pending.extend(collect_super_method_refs(
            std::slice::from_ref(&clone),
            &declared_members,
            &declared_alias_rets,
            &duplicate_members,
        ));
        out.push(clone);
    }
    (out, rewrites)
}

fn fresh_super_alias_name(base: &Atom, taken: &HashSet<Atom>) -> Atom {
    let mut suffix: u32 = 2;
    loop {
        let candidate = Atom::from(format!("{}_{}", base.as_str(), suffix));
        if !taken.contains(&candidate) {
            return candidate;
        }
        let Some(next) = suffix.checked_add(1) else {
            return candidate;
        };
        suffix = next;
    }
}

#[derive(Default)]
struct SuperAliasRewrites {
    renames: HashMap<Atom, Atom>,
    rets: HashMap<Atom, (TypeId, TypeId)>,
    alias_rets: HashMap<Atom, TypeId>,
}

impl SuperAliasRewrites {
    fn is_empty(&self) -> bool {
        self.renames.is_empty() && self.rets.is_empty()
    }
}

fn apply_super_alias_rewrites(method: &mut HirFunction, rewrites: &SuperAliasRewrites) {
    let mut rewriter = SuperAliasRewriter { rewrites };
    for stmt in &mut method.body {
        rewriter.visit_stmt_mut(stmt);
    }
}

struct SuperAliasRewriter<'a> {
    rewrites: &'a SuperAliasRewrites,
}

impl VisitorMut for SuperAliasRewriter<'_> {
    fn visit_stmt_mut(&mut self, stmt: &mut HirStmt) {
        if let HirStmt::Let {
            ty,
            init:
                Some(HirExpr::Call {
                    callee,
                    ty: call_ty,
                    ..
                }),
            ..
        } = stmt
            && let HirCallee::Indirect(inner) = callee
            && let HirExpr::Field {
                field_name,
                ty: field_ty,
                ..
            } = inner.as_mut()
            && *call_ty == *field_ty
            && self.rewrites.alias_rets.get(field_name) == Some(field_ty)
            && let Some((old_ret, new_ret)) = self.rewrites.rets.get(field_name).copied()
            && *ty == old_ret
        {
            *ty = new_ret;
        }
        walk_stmt_mut(self, stmt);
    }

    fn visit_expr_mut(&mut self, expr: &mut HirExpr) {
        if let HirExpr::Call {
            callee: HirCallee::Indirect(inner),
            ty: call_ty,
            ..
        } = expr
            && let HirExpr::Field {
                field_name,
                ty: field_ty,
                ..
            } = inner.as_mut()
            && *call_ty == *field_ty
            && self.rewrites.alias_rets.get(field_name) == Some(field_ty)
        {
            if let Some((_, new_ret)) = self.rewrites.rets.get(field_name).copied() {
                *field_ty = new_ret;
                *call_ty = new_ret;
            }
            if let Some(new_name) = self.rewrites.renames.get(field_name) {
                *field_name = new_name.clone();
            }
        }
        walk_expr_mut(self, expr);
    }
}

fn declared_member_names(
    class_name: &Atom,
    ancestors: &[Rc<HirClass>],
    index: &HashMap<Atom, Rc<HirClass>>,
) -> HashSet<Atom> {
    let mut names = HashSet::new();
    for class in index.get(class_name).into_iter().chain(ancestors.iter()) {
        names.extend(class.fields.iter().map(|field| field.name.clone()));
        names.extend(class.methods.iter().map(|method| method.name.clone()));
    }
    names
}

fn declared_alias_rets(
    declared_members: &HashSet<Atom>,
    ancestors: &[Rc<HirClass>],
    index: &HashMap<Atom, Rc<HirClass>>,
) -> HashMap<Atom, TypeId> {
    let mut rets = HashMap::new();
    for name in declared_members {
        let Some((owner, method)) = parse_super_method_name(name.as_str()) else {
            continue;
        };
        if !ancestors.iter().any(|ancestor| ancestor.name == owner) {
            continue;
        }
        let Some(owner_class) = index.get(&owner) else {
            continue;
        };
        let Some(found) = owner_class
            .methods
            .iter()
            .find(|m| m.name == method && !method_is_static(m))
        else {
            continue;
        };
        rets.insert(name.clone(), found.ret);
    }
    rets
}

fn is_synthetic_super_ref(
    name: &Atom,
    field_ty: TypeId,
    declared_members: &HashSet<Atom>,
    alias_rets: &HashMap<Atom, TypeId>,
    duplicate_members: &HashSet<Atom>,
) -> bool {
    parse_super_method_name(name.as_str()).is_some()
        && !duplicate_members.contains(name)
        && (!declared_members.contains(name)
            || alias_rets.get(name).is_some_and(|ret| *ret == field_ty))
}

fn duplicate_member_names(
    class_name: &Atom,
    ancestors: &[Rc<HirClass>],
    index: &HashMap<Atom, Rc<HirClass>>,
) -> HashSet<Atom> {
    let mut fields = HashSet::new();
    let mut methods = HashSet::new();
    for class in index.get(class_name).into_iter().chain(ancestors.iter()) {
        fields.extend(class.fields.iter().map(|field| field.name.clone()));
        methods.extend(class.methods.iter().map(|method| method.name.clone()));
    }
    fields.intersection(&methods).cloned().collect()
}

fn collect_super_method_refs(
    methods: &[HirFunction],
    declared_members: &HashSet<Atom>,
    alias_rets: &HashMap<Atom, TypeId>,
    duplicate_members: &HashSet<Atom>,
) -> Vec<Atom> {
    struct SuperRefScan<'a> {
        declared_members: &'a HashSet<Atom>,
        alias_rets: &'a HashMap<Atom, TypeId>,
        duplicate_members: &'a HashSet<Atom>,
        found: Vec<Atom>,
    }
    impl Visitor for SuperRefScan<'_> {
        fn visit_expr(&mut self, expr: &HirExpr) {
            match expr {
                HirExpr::Call {
                    callee: HirCallee::Indirect(inner),
                    ty: call_ty,
                    ..
                } => {
                    if let HirExpr::Field {
                        field_name,
                        ty: field_ty,
                        ..
                    } = inner.as_ref()
                        && *call_ty == *field_ty
                        && is_synthetic_super_ref(
                            field_name,
                            *field_ty,
                            self.declared_members,
                            self.alias_rets,
                            self.duplicate_members,
                        )
                    {
                        self.found.push(field_name.clone());
                    }
                }
                HirExpr::Field { field_name, .. }
                    if !self.declared_members.contains(field_name)
                        && parse_super_method_name(field_name.as_str()).is_some() =>
                {
                    self.found.push(field_name.clone());
                }
                _ => {}
            }
            walk_expr(self, expr);
        }
    }
    let mut scan = SuperRefScan {
        declared_members,
        alias_rets,
        duplicate_members,
        found: Vec::new(),
    };
    for method in methods {
        for stmt in &method.body {
            scan.visit_stmt(stmt);
        }
    }
    scan.found
}

fn parse_super_method_name(name: &str) -> Option<(Atom, Atom)> {
    let rest = name.strip_prefix("__super_")?;
    let (len_text, after_len) = rest.split_once('_')?;
    let owner_len: usize = len_text.parse().ok()?;
    if owner_len == 0 {
        return None;
    }
    let owner_end = after_len
        .char_indices()
        .nth(owner_len)
        .map_or(after_len.len(), |(index, _)| index);
    let owner = after_len.get(..owner_end)?;
    let method = after_len.get(owner_end..)?.strip_prefix('_')?;
    if method.is_empty() {
        return None;
    }
    Some((Atom::from(owner), Atom::from(method)))
}

fn method_is_static(method: &HirFunction) -> bool {
    method
        .params
        .first()
        .is_none_or(|param| param.name.as_str() != "this")
}

fn retype_this(method: &mut HirFunction, class_ty: TypeId, receiver_tys: &[TypeId]) -> Vec<Span> {
    let Some(this_id) = method
        .params
        .first()
        .filter(|param| param.name.as_str() == "this")
        .map(|param| param.id)
    else {
        return Vec::new();
    };
    let declared_ret = method.ret;
    if let Some(param) = method.params.first_mut() {
        param.ty = class_ty;
    }
    let mut retyper = ThisRetyper {
        class_ty,
        this_id,
        receiver_locals: HashSet::new(),
        rejected_writes: Vec::new(),
        closure_depth: 0,
        saw_value_return: false,
        foreign_return: false,
    };
    for stmt in &mut method.body {
        retyper.visit_stmt_mut(stmt);
    }
    if receiver_tys.contains(&declared_ret) && retyper.returns_only_the_receiver() {
        method.ret = class_ty;
    }
    retyper.rejected_writes
}

fn report_rejected_receiver_writes(
    ctx: &mut PassContext,
    class_name: &Atom,
    method_name: &Atom,
    rejected_writes: Vec<Span>,
) {
    for span in rejected_writes {
        ctx.error(
            "P0024",
            format!(
                "the inherited method `{}.{}` writes a local bound to the receiver with a value \
                 that is not the receiver; the generated Rust represents a subclass and its parent \
                 as distinct structs, so one binding cannot hold both; keep the local bound to the \
                 receiver",
                class_name.as_str(),
                method_name.as_str()
            ),
            span,
        );
    }
}

struct ThisRetyper {
    class_ty: TypeId,
    this_id: LocalId,
    receiver_locals: HashSet<LocalId>,
    rejected_writes: Vec<Span>,
    closure_depth: usize,
    saw_value_return: bool,
    foreign_return: bool,
}

impl ThisRetyper {
    fn is_receiver(&self, expr: &HirExpr) -> bool {
        match expr {
            HirExpr::Local { id, .. } => *id == self.this_id || self.receiver_locals.contains(id),
            HirExpr::Global { name, .. } => name.as_str() == "this",
            HirExpr::TypeAssertion { expr, .. } => self.is_receiver(expr),
            HirExpr::Ternary {
                then_branch,
                else_branch,
                ..
            } => self.is_receiver(then_branch) && self.is_receiver(else_branch),
            _ => false,
        }
    }

    fn returns_only_the_receiver(&self) -> bool {
        self.saw_value_return && !self.foreign_return
    }
}

impl VisitorMut for ThisRetyper {
    fn visit_stmt_mut(&mut self, stmt: &mut HirStmt) {
        if let HirStmt::Let { id, ty, init, .. } = stmt {
            let bound_to_receiver = init.as_ref().is_some_and(|init| self.is_receiver(init));
            if bound_to_receiver {
                *ty = self.class_ty;
                self.receiver_locals.insert(*id);
            }
        }
        walk_stmt_mut(self, stmt);
        if self.closure_depth == 0 {
            match stmt {
                HirStmt::Return { value: Some(value) } => {
                    self.saw_value_return = true;
                    if !self.is_receiver(value) {
                        self.foreign_return = true;
                    }
                }
                HirStmt::Return { value: None } => self.foreign_return = true,
                _ => {}
            }
        }
    }

    fn visit_expr_mut(&mut self, expr: &mut HirExpr) {
        match expr {
            HirExpr::Local { id, ty, .. }
                if *id == self.this_id || self.receiver_locals.contains(id) =>
            {
                *ty = self.class_ty;
            }
            HirExpr::Global { name, ty, .. } if name.as_str() == "this" => *ty = self.class_ty,
            HirExpr::Assignment {
                target,
                value,
                span,
                ..
            } => {
                if let HirExpr::Local { id, .. } = target.as_ref()
                    && self.receiver_locals.contains(id)
                    && !self.is_receiver(value)
                {
                    self.rejected_writes.push(*span);
                }
            }
            HirExpr::CompoundUpdate {
                target, rhs, span, ..
            } => {
                if let HirExpr::Local { id, .. } = target.as_ref()
                    && self.receiver_locals.contains(id)
                    && !self.is_receiver(rhs)
                {
                    self.rejected_writes.push(*span);
                }
            }
            HirExpr::Closure { captures, body, .. } => {
                self.closure_depth += 1;
                for capture in captures.iter_mut() {
                    self.visit_expr_mut(capture);
                }
                self.visit_block_mut(body);
                self.closure_depth -= 1;
                return;
            }
            _ => {}
        }
        walk_expr_mut(self, expr);
    }
}

fn collect_parents(
    extends: &Atom,
    self_name: Atom,
    index: &HashMap<Atom, Rc<HirClass>>,
    ctx: &mut PassContext,
) -> CollectResult {
    let mut chain: Vec<Rc<HirClass>> = Vec::new();
    let mut current = extends.clone();
    let mut visited: HashSet<Atom> = HashSet::new();
    visited.insert(self_name);

    loop {
        if !visited.insert(current.clone()) {
            ctx.error(
                "P0008",
                format!("class extends cycle through {}", current.as_str()),
                Span::new(0, 0),
            );
            return CollectResult::Cycle;
        }
        let parent = match index.get(&current) {
            Some(p) => Rc::clone(p),
            None => {
                ctx.warning(
                    "P0009",
                    format!(
                        "class extends unknown class {}; the class is lowered without its parent",
                        current.as_str()
                    ),
                    Span::new(0, 0),
                );
                return CollectResult::Missing;
            }
        };
        let next_extends = parent.extends.clone();
        chain.push(parent);
        match next_extends {
            Some(ne) => current = ne,
            None => return CollectResult::Ok(chain),
        }
    }
}
