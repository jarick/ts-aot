use crate::expr::HirExpr;
use crate::stmt::HirStmt;
use ts_aot_core::{Atom, GenericParamId, TypeId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HirAsyncInfo {
    Promise {
        ok_ty: TypeId,
        err_ty: Option<TypeId>,
        promise_ty: TypeId,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct HirParam {
    pub name: Atom,
    pub ty: TypeId,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct HirField {
    pub name: Atom,
    pub ty: TypeId,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct HirEnumVariant {
    pub name: Atom,
    pub value: Option<HirExpr>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct HirFunction {
    pub name: Atom,
    pub params: Vec<HirParam>,
    pub ret: TypeId,
    pub throws: Option<TypeId>,
    pub body: Vec<HirStmt>,
    pub is_async: bool,
    pub is_generator: bool,
    pub is_exported: bool,
    pub type_params: Vec<GenericParamId>,
    pub async_info: Option<HirAsyncInfo>,
}

impl Default for HirFunction {
    fn default() -> Self {
        Self {
            name: Atom::from(""),
            params: Vec::new(),
            ret: TypeId::from_raw(0),
            throws: None,
            body: Vec::new(),
            is_async: false,
            is_generator: false,
            is_exported: false,
            type_params: Vec::new(),
            async_info: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct HirClass {
    pub name: Atom,
    pub ty: TypeId,
    pub fields: Vec<HirField>,
    pub methods: Vec<HirFunction>,
    pub extends: Option<Atom>,
    pub type_params: Vec<GenericParamId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum HirDecl {
    Function(HirFunction),
    Class(HirClass),
    TypeAlias {
        name: Atom,
        target: TypeId,
    },
    Enum {
        name: Atom,
        variants: Vec<HirEnumVariant>,
    },
    Global {
        name: Atom,
        ty: TypeId,
        init: Option<HirExpr>,
    },
    Interface {
        name: Atom,
    },
    Namespace {
        name: Atom,
        members: Vec<HirDecl>,
    },
}
