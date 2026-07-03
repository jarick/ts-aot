use ts_aot_core::TypeTable;
use ts_aot_ir_hir::{HirClass, HirDecl, HirFunction, HirStmt};

use super::substitute::{TypeParamMap, TypeSubstitutionResult};
use super::substitute_expr::substitute_expr;
use super::substitute_stmt::substitute_stmt;
use super::substitute_ty::{
    substitute_async_info, substitute_field, substitute_param, substitute_type, substitute_variant,
};

pub fn substitute_func(
    func: &HirFunction,
    mapping: &TypeParamMap,
    types: &mut TypeTable,
    result: &mut TypeSubstitutionResult,
) -> HirFunction {
    let params = func
        .params
        .iter()
        .map(|p| substitute_param(p, mapping, types, result))
        .collect();
    let ret = substitute_type(func.ret, mapping, types, result);
    let throws = func
        .throws
        .map(|t| substitute_type(t, mapping, types, result));
    let body = substitute_body(&func.body, mapping, types, result);
    let async_info = func
        .async_info
        .map(|a| substitute_async_info(a, mapping, types, result));

    HirFunction {
        name: func.name.clone(),
        params,
        ret,
        throws,
        body,
        is_async: func.is_async,
        is_generator: func.is_generator,
        is_exported: func.is_exported,
        type_params: Vec::new(),
        async_info,
    }
}

pub fn substitute_body(
    stmts: &[HirStmt],
    mapping: &TypeParamMap,
    types: &mut TypeTable,
    result: &mut TypeSubstitutionResult,
) -> Vec<HirStmt> {
    stmts
        .iter()
        .map(|s| substitute_stmt(s, mapping, types, result))
        .collect()
}

pub fn substitute_decl(
    decl: &HirDecl,
    mapping: &TypeParamMap,
    types: &mut TypeTable,
    result: &mut TypeSubstitutionResult,
) -> HirDecl {
    match decl {
        HirDecl::Function(f) => HirDecl::Function(substitute_func(f, mapping, types, result)),
        HirDecl::Class(c) => HirDecl::Class(substitute_class(c, mapping, types, result)),
        HirDecl::Namespace { name, members } => HirDecl::Namespace {
            name: name.clone(),
            members: members
                .iter()
                .map(|m| substitute_decl(m, mapping, types, result))
                .collect(),
        },
        HirDecl::TypeAlias { name, target } => HirDecl::TypeAlias {
            name: name.clone(),
            target: substitute_type(*target, mapping, types, result),
        },
        HirDecl::Enum { name, variants } => HirDecl::Enum {
            name: name.clone(),
            variants: variants
                .iter()
                .map(|v| substitute_variant(v, mapping, types, result))
                .collect(),
        },
        HirDecl::Interface { name } => HirDecl::Interface { name: name.clone() },
        HirDecl::Global { name, ty, init } => HirDecl::Global {
            name: name.clone(),
            ty: substitute_type(*ty, mapping, types, result),
            init: init
                .as_ref()
                .map(|e| substitute_expr(e, mapping, types, result)),
        },
    }
}

pub fn substitute_class(
    class: &HirClass,
    mapping: &TypeParamMap,
    types: &mut TypeTable,
    result: &mut TypeSubstitutionResult,
) -> HirClass {
    HirClass {
        name: class.name.clone(),
        ty: substitute_type(class.ty, mapping, types, result),
        fields: class
            .fields
            .iter()
            .map(|f| substitute_field(f, mapping, types, result))
            .collect(),
        methods: class
            .methods
            .iter()
            .map(|m| substitute_func(m, mapping, types, result))
            .collect(),
        extends: class.extends.clone(),
        type_params: Vec::new(),
    }
}
