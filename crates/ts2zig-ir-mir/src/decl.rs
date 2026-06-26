use ts2zig_core::{FieldId, FunctionId, LocalId, StructId, SymbolId, TypeId, Visibility};

use crate::body::MirBody;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MirParam {
    pub id: LocalId,
    pub name: SymbolId,
    pub ty: TypeId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MirFieldDecl {
    pub id: FieldId,
    pub name: SymbolId,
    pub ty: TypeId,
    pub mutable: bool,
    pub visibility: Visibility,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FunctionKind {
    Plain,
    Method {
        owner: StructId,
        self_param: LocalId,
    },
    Closure,
    Constructor {
        owner: StructId,
    },
    RuntimeShim,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct FunctionEffects {
    pub can_throw: bool,
    pub is_async: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MirFunctionDecl {
    pub id: FunctionId,
    pub name: SymbolId,
    pub export_name: Option<String>,
    pub params: Vec<MirParam>,
    pub ret: TypeId,
    pub throws: Option<TypeId>,
    pub body: MirBody,
    pub kind: FunctionKind,
    pub effects: FunctionEffects,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MirStructDecl {
    pub id: StructId,
    pub name: SymbolId,
    pub fields: Vec<MirFieldDecl>,
    pub methods: Vec<MirFunctionDecl>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MirGlobalDecl {
    pub name: SymbolId,
    pub ty: TypeId,
    pub mutable: bool,
    pub visibility: Visibility,
    pub export_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum MirDecl {
    Function(MirFunctionDecl),
    Struct(MirStructDecl),
    Global(MirGlobalDecl),
}

impl From<MirFunctionDecl> for MirDecl {
    fn from(f: MirFunctionDecl) -> Self {
        Self::Function(f)
    }
}

impl From<MirStructDecl> for MirDecl {
    fn from(s: MirStructDecl) -> Self {
        Self::Struct(s)
    }
}

impl From<MirGlobalDecl> for MirDecl {
    fn from(g: MirGlobalDecl) -> Self {
        Self::Global(g)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ts2zig_core::{StructId, SymbolId, TypeId};

    #[test]
    fn param_roundtrip() {
        let p = MirParam {
            id: LocalId::from_raw(3),
            name: SymbolId::from_raw(7),
            ty: TypeId::from_raw(0),
        };
        assert_eq!(p.id, LocalId::from_raw(3));
        assert_eq!(p.name, SymbolId::from_raw(7));
    }

    #[test]
    fn field_visibility_roundtrip() {
        let f = MirFieldDecl {
            id: FieldId::from_raw(0),
            name: SymbolId::from_raw(1),
            ty: TypeId::from_raw(0),
            mutable: false,
            visibility: Visibility::Private,
        };
        assert_eq!(f.visibility, Visibility::Private);
        assert!(!f.mutable);
    }

    #[test]
    fn function_kind_variants_are_distinct() {
        let plain = FunctionKind::Plain;
        let method = FunctionKind::Method {
            owner: StructId::from_raw(0),
            self_param: LocalId::from_raw(1),
        };
        let ctor = FunctionKind::Constructor {
            owner: StructId::from_raw(0),
        };
        let closure = FunctionKind::Closure;
        let shim = FunctionKind::RuntimeShim;
        assert_ne!(plain, method);
        assert_ne!(method, ctor);
        assert_ne!(closure, plain);
        assert_ne!(shim, plain);
    }

    #[test]
    fn function_effects_default_is_empty() {
        let e = FunctionEffects::default();
        assert!(!e.can_throw);
        assert!(!e.is_async);
    }

    #[test]
    fn function_decl_carries_metadata() {
        let f = MirFunctionDecl {
            id: FunctionId::from_raw(0),
            name: SymbolId::from_raw(1),
            export_name: Some("render".to_owned()),
            params: vec![MirParam {
                id: LocalId::from_raw(0),
                name: SymbolId::from_raw(10),
                ty: TypeId::from_raw(0),
            }],
            ret: TypeId::from_raw(0),
            throws: None,
            body: MirBody::default(),
            kind: FunctionKind::Plain,
            effects: FunctionEffects {
                can_throw: true,
                is_async: false,
            },
        };
        assert_eq!(f.export_name.as_deref(), Some("render"));
        assert_eq!(f.params.len(), 1);
        assert!(f.effects.can_throw);
        assert!(f.body.block.is_empty());
        assert!(f.body.locals.is_empty());
    }

    #[test]
    fn function_decl_body_carries_locals_and_block() {
        let f = MirFunctionDecl {
            id: FunctionId::from_raw(0),
            name: SymbolId::from_raw(1),
            export_name: None,
            params: Vec::new(),
            ret: TypeId::from_raw(0),
            throws: None,
            body: MirBody {
                locals: vec![crate::body::MirLocalDecl {
                    id: LocalId::from_raw(0),
                    name: SymbolId::from_raw(1),
                    ty: TypeId::from_raw(2),
                    mutable: false,
                }],
                block: crate::body::MirBlock::with(crate::body::MirStmt::Return(None)),
            },
            kind: FunctionKind::Plain,
            effects: FunctionEffects::default(),
        };
        assert_eq!(f.body.locals.len(), 1);
        assert_eq!(f.body.block.len(), 1);
    }

    #[test]
    fn struct_decl_carries_fields_and_methods() {
        let s = MirStructDecl {
            id: StructId::from_raw(0),
            name: SymbolId::from_raw(1),
            fields: vec![MirFieldDecl {
                id: FieldId::from_raw(0),
                name: SymbolId::from_raw(10),
                ty: TypeId::from_raw(0),
                mutable: true,
                visibility: Visibility::Public,
            }],
            methods: Vec::new(),
        };
        assert_eq!(s.fields.len(), 1);
        assert!(s.methods.is_empty());
        assert_eq!(s.fields[0].visibility, Visibility::Public);
    }

    #[test]
    fn global_decl_carries_visibility_and_mutation() {
        let g = MirGlobalDecl {
            name: SymbolId::from_raw(1),
            ty: TypeId::from_raw(0),
            mutable: false,
            visibility: Visibility::Public,
            export_name: None,
        };
        assert!(!g.mutable);
        assert_eq!(g.visibility, Visibility::Public);
    }

    #[test]
    fn decl_from_impls_work_for_all_variants() {
        let f = MirFunctionDecl {
            id: FunctionId::from_raw(0),
            name: SymbolId::from_raw(1),
            export_name: None,
            params: Vec::new(),
            ret: TypeId::from_raw(0),
            throws: None,
            body: MirBody::default(),
            kind: FunctionKind::Plain,
            effects: FunctionEffects::default(),
        };
        let s = MirStructDecl {
            id: StructId::from_raw(0),
            name: SymbolId::from_raw(1),
            fields: Vec::new(),
            methods: Vec::new(),
        };
        let g = MirGlobalDecl {
            name: SymbolId::from_raw(1),
            ty: TypeId::from_raw(0),
            mutable: false,
            visibility: Visibility::Private,
            export_name: None,
        };
        let _: MirDecl = f.into();
        let _: MirDecl = s.into();
        let _: MirDecl = g.into();
    }
}
