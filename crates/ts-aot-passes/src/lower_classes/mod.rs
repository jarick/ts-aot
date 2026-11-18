use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use ts_aot_core::{Atom, FieldId, LocalId, Span, TypeId, TypeTable};
use ts_aot_ir_hir::{
    HirCallee, HirClass, HirDecl, HirExpr, HirField, HirFunction, HirProgram, HirStmt, Visitor,
    VisitorMut, walk_expr, walk_expr_mut,
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

    hoist_own_field_inits_into_constructors(program);

    merge_inherited_field_inits(program);

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

fn hoist_own_field_inits_into_constructors(program: &mut HirProgram) {
    let index = collect_class_index(&program.declarations);
    let mut hoistable: Vec<(TypeId, Vec<(Atom, HirExpr)>)> = Vec::new();
    for class in collect_all_classes(&program.declarations) {
        let Some(inits) = program.class_field_inits.get(&class.ty) else {
            continue;
        };
        if inits.is_empty() || !class_ctor_can_hoist(&class) {
            continue;
        }
        let every_init_relocatable = inits.iter().all(|(name, value)| {
            !is_capture_field(name)
                && is_relocatable(value)
                && class.fields.iter().any(|field| field.name == *name)
        });
        if !every_init_relocatable || !descendants_can_hoist(&class, &index, program, inits) {
            continue;
        }
        hoistable.push((class.ty, inits.clone()));
    }
    if hoistable.is_empty() {
        return;
    }
    let by_ty: HashMap<TypeId, Vec<(Atom, HirExpr)>> = hoistable.into_iter().collect();
    let mut hoisted: Vec<TypeId> = Vec::new();
    for decl in &mut program.declarations {
        apply_hoisted_inits(decl, &by_ty, &mut hoisted);
    }
    for ty in hoisted {
        program.class_field_inits.remove(&ty);
    }
}

fn apply_hoisted_inits(
    decl: &mut HirDecl,
    by_ty: &HashMap<TypeId, Vec<(Atom, HirExpr)>>,
    hoisted: &mut Vec<TypeId>,
) {
    match decl {
        HirDecl::Class(c) => {
            let Some(inits) = by_ty.get(&c.ty) else {
                return;
            };
            let Some(ctor_index) = c.methods.iter().position(|method| {
                method.name.as_str() == "constructor" && !method_is_static(method)
            }) else {
                return;
            };
            let Some(super_index) = super_ctor_stmt_index(&c.methods[ctor_index].body) else {
                return;
            };
            let mut assigns: Vec<HirStmt> = Vec::with_capacity(inits.len());
            for (name, value) in inits {
                let Some(field) = c.fields.iter().find(|field| field.name == *name) else {
                    return;
                };
                let span = value.span();
                assigns.push(HirStmt::Expr {
                    expr: HirExpr::Assignment {
                        target: Box::new(HirExpr::Field {
                            owner: Box::new(HirExpr::Local {
                                id: LocalId::from_raw(0),
                                ty: c.ty,
                                span,
                            }),
                            field: FieldId::from_raw(0),
                            field_name: name.clone(),
                            ty: field.ty,
                            span,
                        }),
                        value: Box::new(value.clone()),
                        ty: field.ty,
                        span,
                    },
                });
            }
            let at = super_index.saturating_add(1);
            c.methods[ctor_index].body.splice(at..at, assigns);
            hoisted.push(c.ty);
        }
        HirDecl::Namespace { members, .. } => {
            for member in members {
                apply_hoisted_inits(member, by_ty, hoisted);
            }
        }
        HirDecl::Function(_)
        | HirDecl::TypeAlias { .. }
        | HirDecl::Enum { .. }
        | HirDecl::Global { .. }
        | HirDecl::Interface { .. } => {}
    }
}

fn class_ctor_can_hoist(class: &HirClass) -> bool {
    if class.extends.is_none() {
        return false;
    }
    class.methods.iter().any(|method| {
        method.name.as_str() == "constructor"
            && !method_is_static(method)
            && super_ctor_stmt_index(&method.body).is_some()
    })
}

fn descendants_can_hoist(
    class: &HirClass,
    index: &HashMap<Atom, Rc<HirClass>>,
    program: &HirProgram,
    inits: &[(Atom, HirExpr)],
) -> bool {
    let hoisted_names: HashSet<Atom> = inits.iter().map(|(name, _)| name.clone()).collect();
    index.values().all(|other| {
        if other.name == class.name || !extends_chain_reaches(other, &class.name, index) {
            return true;
        }
        let has_non_static_ctor = other
            .methods
            .iter()
            .any(|method| method.name.as_str() == "constructor" && !method_is_static(method));
        let every_ctor_has_super = other.methods.iter().all(|method| {
            method.name.as_str() != "constructor"
                || method_is_static(method)
                || super_ctor_stmt_index(&method.body).is_some()
        });
        if !has_non_static_ctor || !every_ctor_has_super {
            return false;
        }
        let shadows_hoisted =
            program
                .class_field_inits
                .get(&other.ty)
                .is_some_and(|descendant_inits| {
                    descendant_inits
                        .iter()
                        .any(|(name, _)| hoisted_names.contains(name))
                });
        !shadows_hoisted
    })
}

fn extends_chain_reaches(
    class: &HirClass,
    target: &Atom,
    index: &HashMap<Atom, Rc<HirClass>>,
) -> bool {
    let mut visited: HashSet<Atom> = HashSet::new();
    visited.insert(class.name.clone());
    let mut current = class.extends.clone();
    while let Some(name) = current {
        if name == *target {
            return true;
        }
        if !visited.insert(name.clone()) {
            return false;
        }
        let Some(parent) = index.get(&name) else {
            return false;
        };
        current = parent.extends.clone();
    }
    false
}

fn super_ctor_stmt_index(body: &[HirStmt]) -> Option<usize> {
    body.iter().position(|stmt| match stmt {
        HirStmt::Expr {
            expr:
                HirExpr::Call {
                    callee: HirCallee::Indirect(callee),
                    ..
                },
        } => is_super_ctor_callee(callee),
        _ => false,
    })
}

fn is_super_ctor_callee(callee: &HirExpr) -> bool {
    matches!(
        callee,
        HirExpr::Field { field_name, .. }
            if field_name.as_str().starts_with("__super_")
                && field_name.as_str().ends_with("_constructor")
    )
}

fn is_capture_field(name: &Atom) -> bool {
    name.as_str().starts_with("__capture_")
}

struct RelocatableScan {
    relocatable: bool,
}

impl Visitor for RelocatableScan {
    fn visit_expr(&mut self, expr: &HirExpr) {
        match expr {
            HirExpr::Local { .. } | HirExpr::Closure { .. } => self.relocatable = false,
            HirExpr::Global { name, .. } if name.as_str() == "this" => self.relocatable = false,
            _ => walk_expr(self, expr),
        }
    }
}

fn is_relocatable(expr: &HirExpr) -> bool {
    let mut scan = RelocatableScan { relocatable: true };
    scan.visit_expr(expr);
    scan.relocatable
}

fn merge_inherited_field_inits(program: &mut HirProgram) {
    let index = collect_class_index(&program.declarations);
    let mut snapshot = program.class_field_inits.clone();
    let mut splice_by_ty: HashMap<TypeId, Vec<(Atom, HirExpr)>> = HashMap::new();
    let mut merged: Vec<(TypeId, Vec<(Atom, HirExpr)>)> = Vec::new();
    let mut cleared: Vec<TypeId> = Vec::new();
    for class in collect_all_classes(&program.declarations) {
        let mut visited: HashSet<Atom> = HashSet::new();
        let mut inits = collect_inherited_inits(&class, &index, &snapshot, &mut visited);
        let own_len = snapshot.get(&class.ty).map_or(0, Vec::len);
        if !inits.is_empty() && own_len > 0 && inits.len() >= own_len {
            let own_start = inits.len() - own_len;
            let mut kept: Vec<(Atom, HirExpr)> = Vec::new();
            let mut splice_targets: Vec<(Atom, HirExpr)> = Vec::new();
            for entry in inits.drain(own_start..) {
                if is_relocatable(&entry.1)
                    && !is_capture_field(&entry.0)
                    && class.fields.iter().any(|field| field.name == entry.0)
                {
                    splice_targets.push(entry);
                } else {
                    kept.push(entry);
                }
            }
            if !splice_targets.is_empty()
                && derived_ctor_super_splice_position(&class).is_some()
                && descendants_can_hoist(&class, &index, program, &splice_targets)
            {
                if let Some(entries) = snapshot.get_mut(&class.ty) {
                    entries.retain(|(name, _)| {
                        !splice_targets.iter().any(|(target, _)| target == name)
                    });
                }
                splice_by_ty.insert(class.ty, splice_targets);
                inits.extend(kept);
            } else {
                inits.extend(kept);
                inits.extend(splice_targets);
            }
        }
        if inits.is_empty() {
            if splice_by_ty.contains_key(&class.ty) {
                cleared.push(class.ty);
            }
        } else {
            merged.push((class.ty, inits));
        }
    }
    if !splice_by_ty.is_empty() {
        for decl in &mut program.declarations {
            splice_derived_own_inits(decl, &splice_by_ty);
        }
    }
    for ty in cleared {
        program.class_field_inits.remove(&ty);
    }
    for (ty, inits) in merged {
        program.class_field_inits.insert(ty, inits);
    }
}

fn derived_ctor_super_splice_position(class: &HirClass) -> Option<usize> {
    let method = class
        .methods
        .iter()
        .find(|method| method.name.as_str() == "constructor" && !method_is_static(method))?;
    if let Some(index) = super_ctor_stmt_index(&method.body) {
        return Some(index);
    }
    if class.extends.is_some()
        && matches!(
            method.body.first(),
            Some(HirStmt::Expr {
                expr: HirExpr::Sequence { .. },
                ..
            })
        )
    {
        return Some(0);
    }
    None
}

fn splice_derived_own_inits(decl: &mut HirDecl, by_ty: &HashMap<TypeId, Vec<(Atom, HirExpr)>>) {
    match decl {
        HirDecl::Class(c) => {
            let Some(inits) = by_ty.get(&c.ty) else {
                return;
            };
            let Some(ctor_index) = c.methods.iter().position(|method| {
                method.name.as_str() == "constructor" && !method_is_static(method)
            }) else {
                return;
            };
            let Some(super_index) = derived_ctor_super_splice_position(c) else {
                return;
            };
            let mut assigns: Vec<HirStmt> = Vec::with_capacity(inits.len());
            for (name, value) in inits {
                let Some(field) = c.fields.iter().find(|field| field.name == *name) else {
                    return;
                };
                let span = value.span();
                assigns.push(HirStmt::Expr {
                    expr: HirExpr::Assignment {
                        target: Box::new(HirExpr::Field {
                            owner: Box::new(HirExpr::Local {
                                id: LocalId::from_raw(0),
                                ty: c.ty,
                                span,
                            }),
                            field: FieldId::from_raw(0),
                            field_name: name.clone(),
                            ty: field.ty,
                            span,
                        }),
                        value: Box::new(value.clone()),
                        ty: field.ty,
                        span,
                    },
                });
            }
            let at = super_index.saturating_add(1);
            c.methods[ctor_index].body.splice(at..at, assigns);
        }
        HirDecl::Namespace { members, .. } => {
            for member in members {
                splice_derived_own_inits(member, by_ty);
            }
        }
        HirDecl::Function(_)
        | HirDecl::TypeAlias { .. }
        | HirDecl::Enum { .. }
        | HirDecl::Global { .. }
        | HirDecl::Interface { .. } => {}
    }
}

fn collect_inherited_inits(
    class: &HirClass,
    index: &HashMap<Atom, Rc<HirClass>>,
    snapshot: &HashMap<ts_aot_core::TypeId, Vec<(Atom, ts_aot_ir_hir::HirExpr)>>,
    visited: &mut HashSet<Atom>,
) -> Vec<(Atom, ts_aot_ir_hir::HirExpr)> {
    if !visited.insert(class.name.clone()) {
        return Vec::new();
    }
    let mut out: Vec<(Atom, ts_aot_ir_hir::HirExpr)> = class
        .extends
        .as_ref()
        .and_then(|parent| index.get(parent))
        .map_or_else(Vec::new, |parent| {
            collect_inherited_inits(parent, index, snapshot, visited)
        });
    if let Some(own) = snapshot.get(&class.ty) {
        for (name, expr) in own {
            out.push((name.clone(), expr.clone()));
        }
    }
    out
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

fn collect_all_classes(decls: &[HirDecl]) -> Vec<Rc<HirClass>> {
    let mut out = Vec::new();
    collect_all_into(decls, &mut out);
    out
}

fn collect_all_into(decls: &[HirDecl], out: &mut Vec<Rc<HirClass>>) {
    for decl in decls {
        match decl {
            HirDecl::Class(c) => out.push(Rc::new(c.clone())),
            HirDecl::Namespace { members, .. } => collect_all_into(members, out),
            HirDecl::Function(_)
            | HirDecl::TypeAlias { .. }
            | HirDecl::Enum { .. }
            | HirDecl::Global { .. }
            | HirDecl::Interface { .. } => {}
        }
    }
}

fn method_is_static(method: &HirFunction) -> bool {
    method
        .params
        .first()
        .is_none_or(|param| param.name.as_str() != "this")
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

    let own_method_names: HashSet<(Atom, bool)> = c
        .methods
        .iter()
        .map(|m| (m.name.clone(), method_is_static(m)))
        .collect();

    let mut kept_parent_methods: Vec<HirFunction> = Vec::new();
    let mut taken_names: HashSet<(Atom, bool)> = own_method_names.clone();
    let mut override_count: usize = 0;

    for parent in parents.iter().rev() {
        for pm in parent.methods.iter().rev() {
            let key = (pm.name.clone(), method_is_static(pm));
            if taken_names.contains(&key) {
                override_count += 1;
                continue;
            }
            taken_names.insert(key);
            kept_parent_methods.push(pm.clone());
        }
    }
    kept_parent_methods.reverse();
    for method in &mut kept_parent_methods {
        retype_this(method, c.ty);
    }

    let inherited_method_count = kept_parent_methods.len();

    let mut super_ref_sources = kept_parent_methods.clone();
    super_ref_sources.extend(c.methods.iter().cloned());
    let super_clones = build_super_method_clones(&super_ref_sources, c.ty, index);
    c.methods.extend(super_clones);

    let mut merged_fields: Vec<HirField> = Vec::new();
    let mut nearest: HashMap<Atom, HirField> = HashMap::new();
    for parent in parents.iter().rev() {
        for field in &parent.fields {
            nearest
                .entry(field.name.clone())
                .or_insert_with(|| field.clone());
        }
    }
    for parent in &parents {
        for field in &parent.fields {
            if c.fields.iter().any(|own| own.name == field.name) {
                continue;
            }
            if merged_fields.iter().any(|kept| kept.name == field.name) {
                continue;
            }
            match nearest.get(&field.name) {
                Some(declaration) => merged_fields.push(declaration.clone()),
                None => merged_fields.push(field.clone()),
            }
        }
    }
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
    index: &HashMap<Atom, Rc<HirClass>>,
) -> Vec<HirFunction> {
    let mut pending: Vec<Atom> = collect_super_method_refs(methods);
    let mut cloned: HashSet<Atom> = HashSet::new();
    let mut out: Vec<HirFunction> = Vec::new();
    while let Some(name) = pending.pop() {
        if !cloned.insert(name.clone()) {
            continue;
        }
        let Some((owner, method)) = parse_super_method_name(name.as_str()) else {
            continue;
        };
        let Some(owner_class) = index.get(&owner) else {
            continue;
        };
        let Some(original) = owner_class
            .methods
            .iter()
            .find(|m| m.name == method && !method_is_static(m))
            .or_else(|| owner_class.methods.iter().find(|m| m.name == method))
        else {
            continue;
        };
        let mut clone = original.clone();
        retype_this(&mut clone, class_ty);
        clone.name = name;
        pending.extend(collect_super_method_refs(std::slice::from_ref(&clone)));
        out.push(clone);
    }
    out
}

fn collect_super_method_refs(methods: &[HirFunction]) -> Vec<Atom> {
    struct SuperRefScan {
        found: Vec<Atom>,
    }
    impl Visitor for SuperRefScan {
        fn visit_expr(&mut self, expr: &HirExpr) {
            if let HirExpr::Field { field_name, .. } = expr
                && field_name.as_str().starts_with("__super_")
            {
                self.found.push(field_name.clone());
            }
            walk_expr(self, expr);
        }
    }
    let mut scan = SuperRefScan { found: Vec::new() };
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

fn retype_this(method: &mut HirFunction, class_ty: TypeId) {
    let is_instance = method
        .params
        .first()
        .is_some_and(|param| param.name.as_str() == "this");
    if !is_instance {
        return;
    }
    if let Some(param) = method.params.first_mut() {
        param.ty = class_ty;
    }
    let mut retyper = ThisRetyper { class_ty };
    for stmt in &mut method.body {
        retyper.visit_stmt_mut(stmt);
    }
}

struct ThisRetyper {
    class_ty: TypeId,
}

impl VisitorMut for ThisRetyper {
    fn visit_expr_mut(&mut self, expr: &mut HirExpr) {
        match expr {
            HirExpr::Local { id, ty, .. } if id.raw() == 0 => *ty = self.class_ty,
            HirExpr::Global { name, ty, .. } if name.as_str() == "this" => *ty = self.class_ty,
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
