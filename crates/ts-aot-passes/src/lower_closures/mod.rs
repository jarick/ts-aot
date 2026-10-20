use std::collections::{HashMap, HashSet};

use ts_aot_core::{Atom, LocalId, TypeTable};
use ts_aot_ir_hir::{HirDecl, HirProgram};

use crate::PassContext;

mod closure_lift;
mod rewrite;
mod walk;
mod walk_expr;
mod walk_stmt;

#[cfg(test)]
mod tests;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LowerClosuresStats {
    pub emitted_fns: usize,
    pub deferred_capturing: usize,
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Hash)]
pub struct LowerClosuresResult {
    pub stats: LowerClosuresStats,
    pub closure_names: Vec<Atom>,
}

struct ClosureLowerer<'a> {
    next_id: &'a mut u32,
    new_decls: &'a mut Vec<HirDecl>,
    generated: &'a mut Vec<Atom>,
    taken: &'a mut HashSet<Atom>,
    stats: &'a mut LowerClosuresStats,
    types: &'a mut TypeTable,
    ctx: &'a mut PassContext,
    closure_names: HashMap<LocalId, Atom>,
}

fn collect_taken_names(decls: &[HirDecl], taken: &mut HashSet<Atom>) {
    for decl in decls {
        match decl {
            HirDecl::Function(f) => {
                taken.insert(f.name.clone());
            }
            HirDecl::Class(c) => {
                taken.insert(c.name.clone());
                for m in &c.methods {
                    taken.insert(m.name.clone());
                }
            }
            HirDecl::Namespace { members, .. } => {
                collect_taken_names(members, taken);
            }
            HirDecl::Enum { name, .. }
            | HirDecl::Interface { name, .. }
            | HirDecl::TypeAlias { name, .. } => {
                taken.insert(name.clone());
            }
            HirDecl::Global { name, .. } => {
                taken.insert(name.clone());
            }
        }
    }
}

pub fn lower_closures(
    program: &mut HirProgram,
    types: &mut TypeTable,
    ctx: &mut PassContext,
) -> LowerClosuresResult {
    let mut stats = LowerClosuresStats::default();
    let mut generated: Vec<Atom> = Vec::new();
    let mut new_decls: Vec<HirDecl> = Vec::new();

    let mut taken: HashSet<Atom> = HashSet::new();
    collect_taken_names(&program.declarations, &mut taken);

    let mut next_closure_id: u32 = 0;
    while taken.contains(&Atom::from(format!("__ts_aot_closure_{next_closure_id}"))) {
        next_closure_id += 1;
    }

    {
        let mut lowerer = ClosureLowerer {
            next_id: &mut next_closure_id,
            new_decls: &mut new_decls,
            generated: &mut generated,
            taken: &mut taken,
            stats: &mut stats,
            types,
            ctx,
            closure_names: HashMap::new(),
        };
        for decl in &mut program.declarations {
            lowerer.walk_decl(decl);
        }
    }
    program.declarations.extend(new_decls);

    LowerClosuresResult {
        stats,
        closure_names: generated,
    }
}
