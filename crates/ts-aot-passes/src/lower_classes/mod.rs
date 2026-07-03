use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use ts_aot_core::{Atom, Span, TypeTable};
use ts_aot_ir_hir::{HirClass, HirDecl, HirField, HirFunction, HirProgram};

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

    let inherited_method_count = kept_parent_methods.len();

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
                ctx.error(
                    "P0009",
                    format!("class extends unknown class {}", current.as_str()),
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
