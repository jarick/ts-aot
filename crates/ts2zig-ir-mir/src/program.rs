use ts2zig_core::{Atom, ModuleId};

use crate::decl::{MirDecl, MirFunctionDecl, MirGlobalDecl, MirStructDecl};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MirImport {
    pub module: String,
    pub symbol: Atom,
    pub alias: Option<Atom>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MirExport {
    pub symbol: Atom,
    pub alias: Option<Atom>,
}

#[derive(Debug, Clone)]
pub struct MirProgram {
    pub module: ModuleId,
    pub imports: Vec<MirImport>,
    pub exports: Vec<MirExport>,
    pub declarations: Vec<MirDecl>,
}

impl MirProgram {
    #[must_use]
    pub fn new(module: ModuleId) -> Self {
        Self {
            module,
            imports: Vec::new(),
            exports: Vec::new(),
            declarations: Vec::new(),
        }
    }

    pub fn push_decl(&mut self, decl: MirDecl) {
        self.declarations.push(decl);
    }

    pub fn push_import(&mut self, import: MirImport) {
        self.imports.push(import);
    }

    pub fn push_export(&mut self, export: MirExport) {
        self.exports.push(export);
    }

    #[must_use]
    pub fn decl_count(&self) -> usize {
        self.declarations.len()
    }

    pub fn functions(&self) -> impl Iterator<Item = &MirFunctionDecl> {
        self.declarations.iter().filter_map(|d| match d {
            MirDecl::Function(f) => Some(f),
            _ => None,
        })
    }

    pub fn structs(&self) -> impl Iterator<Item = &MirStructDecl> {
        self.declarations.iter().filter_map(|d| match d {
            MirDecl::Struct(s) => Some(s),
            _ => None,
        })
    }

    pub fn globals(&self) -> impl Iterator<Item = &MirGlobalDecl> {
        self.declarations.iter().filter_map(|d| match d {
            MirDecl::Global(g) => Some(g),
            _ => None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decl::{
        FunctionEffects, FunctionKind, MirDecl, MirFieldDecl, MirFunctionDecl, MirStructDecl,
    };
    use ts2zig_core::{Atom, FieldId, FunctionId, LocalId, StructId, TypeId, Visibility};

    fn empty_function(id: u32) -> MirFunctionDecl {
        MirFunctionDecl {
            id: FunctionId::from_raw(id),
            name: Atom::from(format!("fn{}", id)),
            export_name: None,
            params: Vec::new(),
            ret: TypeId::from_raw(0),
            throws: None,
            body: crate::body::MirBody::default(),
            kind: FunctionKind::Plain,
            effects: FunctionEffects::default(),
        }
    }

    fn empty_struct(id: u32) -> MirStructDecl {
        MirStructDecl {
            id: StructId::from_raw(id),
            name: Atom::from(format!("s{}", id)),
            fields: Vec::new(),
            methods: Vec::new(),
        }
    }

    #[test]
    fn empty_program_has_no_decls() {
        let prog = MirProgram::new(ModuleId::from_raw(0));
        assert_eq!(prog.decl_count(), 0);
        assert!(prog.functions().next().is_none());
        assert!(prog.structs().next().is_none());
        assert!(prog.globals().next().is_none());
    }

    #[test]
    fn push_decl_increments_count() {
        let mut prog = MirProgram::new(ModuleId::from_raw(7));
        prog.push_decl(MirDecl::Function(empty_function(0)));
        prog.push_decl(MirDecl::Struct(empty_struct(1)));
        assert_eq!(prog.decl_count(), 2);
        assert_eq!(prog.functions().count(), 1);
        assert_eq!(prog.structs().count(), 1);
    }

    #[test]
    fn module_id_is_preserved() {
        let prog = MirProgram::new(ModuleId::from_raw(99));
        assert_eq!(prog.module, ModuleId::from_raw(99));
    }

    #[test]
    fn import_carries_module_symbol_alias() {
        let import = MirImport {
            module: "./other".to_owned(),
            symbol: Atom::new_inline("1"),
            alias: Some(Atom::new_inline("2")),
        };
        assert_eq!(import.module, "./other");
        assert_eq!(import.symbol, Atom::new_inline("1"));
        assert_eq!(import.alias, Some(Atom::new_inline("2")));
    }

    #[test]
    fn import_without_alias_is_none() {
        let import = MirImport {
            module: "./a".to_owned(),
            symbol: Atom::new_inline("3"),
            alias: None,
        };
        assert!(import.alias.is_none());
    }

    #[test]
    fn export_with_and_without_alias() {
        let plain = MirExport {
            symbol: Atom::new_inline("1"),
            alias: None,
        };
        let renamed = MirExport {
            symbol: Atom::new_inline("2"),
            alias: Some(Atom::new_inline("3")),
        };
        assert!(plain.alias.is_none());
        assert_eq!(renamed.alias, Some(Atom::new_inline("3")));
    }

    #[test]
    fn program_iterators_filter_by_decl_kind() {
        let mut prog = MirProgram::new(ModuleId::from_raw(0));
        prog.push_decl(MirDecl::Function(empty_function(0)));
        prog.push_decl(MirDecl::Struct(MirStructDecl {
            id: StructId::from_raw(2),
            name: Atom::new_inline("3"),
            fields: vec![MirFieldDecl {
                id: FieldId::from_raw(0),
                name: Atom::new_inline("10"),
                ty: TypeId::from_raw(0),
                mutable: false,
                visibility: Visibility::Public,
            }],
            methods: Vec::new(),
        }));
        prog.push_decl(MirDecl::Function(empty_function(4)));
        assert_eq!(prog.functions().count(), 2);
        assert_eq!(prog.structs().count(), 1);
        assert_eq!(prog.globals().count(), 0);
    }

    #[test]
    fn function_local_id_roundtrip() {
        let f = MirFunctionDecl {
            id: FunctionId::from_raw(42),
            name: Atom::new_inline("7"),
            export_name: Some("render".to_owned()),
            params: vec![],
            ret: TypeId::from_raw(0),
            throws: None,
            body: crate::body::MirBody::default(),
            kind: FunctionKind::Plain,
            effects: FunctionEffects::default(),
        };
        assert_eq!(f.id, FunctionId::from_raw(42));
        assert_eq!(f.export_name.as_deref(), Some("render"));
        assert!(matches!(f.kind, FunctionKind::Plain));
    }

    #[test]
    fn _local_id_type_is_in_scope() {
        let _id = LocalId::from_raw(0);
    }
}
