use crate::expr::HirExpr;
use crate::stmt::HirStmt;
use ts2zig_core::{GenericParamId, StringId, SymbolId, TypeId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HirAsyncInfo {
    Promise {
        ok_ty: TypeId,
        err_ty: Option<TypeId>,
        promise_ty: TypeId,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HirParam {
    pub name: StringId,
    pub ty: TypeId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HirField {
    pub name: StringId,
    pub ty: TypeId,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct HirEnumVariant {
    pub name: StringId,
    pub value: Option<HirExpr>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct HirFunction {
    pub name: SymbolId,
    pub params: Vec<HirParam>,
    pub ret: TypeId,
    pub body: Vec<HirStmt>,
    pub is_async: bool,
    pub is_generator: bool,
    pub is_exported: bool,
    pub type_params: Vec<GenericParamId>,
    pub async_info: Option<HirAsyncInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct HirClass {
    pub name: SymbolId,
    pub ty: TypeId,
    pub fields: Vec<HirField>,
    pub methods: Vec<HirFunction>,
    pub extends: Option<SymbolId>,
    pub type_params: Vec<GenericParamId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum HirDecl {
    Function(HirFunction),
    Class(HirClass),
    TypeAlias {
        name: SymbolId,
        target: TypeId,
    },
    Enum {
        name: SymbolId,
        variants: Vec<HirEnumVariant>,
    },
    Global {
        name: SymbolId,
        ty: TypeId,
        init: Option<HirExpr>,
    },
    Interface {
        name: SymbolId,
    },
    Namespace {
        name: SymbolId,
        members: Vec<HirDecl>,
    },
}
