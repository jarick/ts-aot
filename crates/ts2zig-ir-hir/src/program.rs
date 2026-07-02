use ts2zig_core::{Atom, DiagnosticBag, ModuleId};

use crate::decl::HirDecl;

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

    pub fn push_decl(&mut self, decl: HirDecl) {
        self.declarations.push(decl);
    }

    #[must_use]
    pub fn decl_count(&self) -> usize {
        self.declarations.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decl::{HirClass, HirDecl, HirField, HirFunction};
    use ts2zig_core::{Atom, TypeId};

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
