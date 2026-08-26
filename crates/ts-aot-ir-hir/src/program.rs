use ts_aot_core::{Atom, DiagnosticBag, ModuleId};

use crate::decl::{HirDecl, HirFunction};

#[must_use]
pub fn qualified_name(namespace_path: &[String], leaf: &str) -> Atom {
    if namespace_path.is_empty() {
        Atom::from(leaf.to_owned())
    } else {
        let mut s = String::with_capacity(
            namespace_path.iter().map(|p| p.len() + 2).sum::<usize>() + leaf.len(),
        );
        for (i, part) in namespace_path.iter().enumerate() {
            if i > 0 {
                s.push_str("::");
            }
            s.push_str(part);
        }
        s.push_str("::");
        s.push_str(leaf);
        Atom::from(s)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct HirImport {
    pub module: Atom,
    pub name: Atom,
    pub alias: Option<Atom>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct HirExport {
    pub name: Atom,
    pub alias: Option<Atom>,
}

#[derive(Debug, Clone)]
pub struct HirProgram {
    pub module: ModuleId,
    pub imports: Vec<HirImport>,
    pub exports: Vec<HirExport>,
    pub declarations: Vec<HirDecl>,
    pub diagnostics: DiagnosticBag,
}

impl HirProgram {
    #[must_use]
    pub fn new(module: ModuleId) -> Self {
        Self {
            module,
            imports: Vec::new(),
            exports: Vec::new(),
            declarations: Vec::new(),
            diagnostics: DiagnosticBag::new(),
        }
    }

    #[must_use]
    pub fn find_function_by_name(&self, name: &Atom) -> Option<&HirFunction> {
        for decl in &self.declarations {
            if let HirDecl::Function(f) = decl
                && f.name == *name
            {
                return Some(f);
            }
        }
        None
    }

    #[must_use]
    pub fn find_function_by_qualified_name(&self, qualified: &Atom) -> Option<&HirFunction> {
        find_function_in_decls(&self.declarations, qualified, &[])
    }

    pub fn push_decl(&mut self, decl: HirDecl) {
        self.declarations.push(decl);
    }

    #[must_use]
    pub fn decl_count(&self) -> usize {
        self.declarations.len()
    }
}

fn find_function_in_decls<'a>(
    decls: &'a [HirDecl],
    qualified: &Atom,
    path: &[String],
) -> Option<&'a HirFunction> {
    for decl in decls {
        match decl {
            HirDecl::Function(f) => {
                if path.is_empty() {
                    if f.name == *qualified {
                        return Some(f);
                    }
                } else if qualified_name(path, f.name.as_str()) == *qualified {
                    return Some(f);
                }
            }
            HirDecl::Namespace { name, members } => {
                let mut new_path = path.to_vec();
                new_path.push(name.as_str().to_owned());
                if let Some(f) = find_function_in_decls(members, qualified, &new_path) {
                    return Some(f);
                }
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decl::{HirClass, HirDecl, HirField, HirFunction};
    use ts_aot_core::{Atom, TypeId};

    #[test]
    fn empty_program_has_no_decls() {
        let prog = HirProgram::new(ModuleId::from_raw(0));
        assert_eq!(prog.decl_count(), 0);
        assert!(prog.diagnostics.is_empty());
    }

    #[test]
    fn push_decl_increments_count() {
        let mut prog = HirProgram::new(ModuleId::from_raw(7));
        prog.push_decl(HirDecl::Global {
            name: Atom::new_inline("1"),
            ty: TypeId::from_raw(2),
            init: None,
        });
        prog.push_decl(HirDecl::Class(HirClass {
            name: Atom::new_inline("3"),
            ty: TypeId::from_raw(5),
            fields: vec![HirField {
                name: Atom::new_inline("4"),
                ty: TypeId::from_raw(5),
            }],
            methods: vec![],
            extends: None,
            type_params: vec![],
        }));
        assert_eq!(prog.decl_count(), 2);
    }

    #[test]
    fn program_module_id_is_preserved() {
        let prog = HirProgram::new(ModuleId::from_raw(99));
        assert_eq!(prog.module.raw(), 99);
    }

    #[test]
    fn hir_function_minimal_construction() {
        let f = HirFunction {
            name: Atom::new_inline("1"),
            params: vec![],
            ret: TypeId::from_raw(2),
            throws: None,
            body: vec![],
            is_async: false,
            is_generator: false,
            is_exported: false,
            type_params: vec![],
            async_info: None,
        };
        assert!(!f.is_async);
        assert!(f.params.is_empty());
    }
}
