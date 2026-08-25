use ts_aot_core::{Type, TypeId, TypeTable};
use ts_aot_ir_hir::{HirAsyncInfo, HirEnumVariant, HirField, HirParam};

use super::substitute::{TypeParamMap, TypeSubstitutionResult};
use super::substitute_expr::substitute_expr;

pub fn substitute_param(
    param: &HirParam,
    mapping: &TypeParamMap,
    types: &mut TypeTable,
    result: &mut TypeSubstitutionResult,
) -> HirParam {
    HirParam {
        name: param.name.clone(),
        ty: substitute_type(param.ty, mapping, types, result),
    }
}

pub fn substitute_field(
    field: &HirField,
    mapping: &TypeParamMap,
    types: &mut TypeTable,
    result: &mut TypeSubstitutionResult,
) -> HirField {
    HirField {
        name: field.name.clone(),
        ty: substitute_type(field.ty, mapping, types, result),
    }
}

pub fn substitute_variant(
    variant: &HirEnumVariant,
    mapping: &TypeParamMap,
    types: &mut TypeTable,
    result: &mut TypeSubstitutionResult,
) -> HirEnumVariant {
    HirEnumVariant {
        name: variant.name.clone(),
        value: variant
            .value
            .as_ref()
            .map(|e| substitute_expr(e, mapping, types, result)),
    }
}

pub fn substitute_async_info(
    info: HirAsyncInfo,
    mapping: &TypeParamMap,
    types: &mut TypeTable,
    result: &mut TypeSubstitutionResult,
) -> HirAsyncInfo {
    match info {
        HirAsyncInfo::Promise {
            ok_ty,
            err_ty,
            promise_ty,
        } => HirAsyncInfo::Promise {
            ok_ty: substitute_type(ok_ty, mapping, types, result),
            err_ty: err_ty.map(|t| substitute_type(t, mapping, types, result)),
            promise_ty: substitute_type(promise_ty, mapping, types, result),
        },
    }
}

pub fn substitute_type(
    ty: TypeId,
    mapping: &TypeParamMap,
    types: &mut TypeTable,
    result: &mut TypeSubstitutionResult,
) -> TypeId {
    let Some(resolved) = types.resolve(ty).cloned() else {
        result.unchanged += 1;
        return ty;
    };
    match resolved {
        Type::GenericParam { id } => match mapping.get(&id) {
            Some(&mapped) => {
                result.mapped += 1;
                mapped
            }
            None => {
                result.unchanged += 1;
                ty
            }
        },
        Type::Optional { inner } => {
            let new_inner = substitute_type(inner, mapping, types, result);
            intern_substituted(ty, types, result, Type::Optional { inner: new_inner })
        }
        Type::Array { element } => {
            let new_element = substitute_type(element, mapping, types, result);
            intern_substituted(
                ty,
                types,
                result,
                Type::Array {
                    element: new_element,
                },
            )
        }
        Type::Fn { params, ret, err } => {
            let new_params: Vec<TypeId> = params
                .iter()
                .map(|p| substitute_type(*p, mapping, types, result))
                .collect();
            let new_ret = substitute_type(ret, mapping, types, result);
            let new_err = err.map(|e| substitute_type(e, mapping, types, result));
            intern_substituted(
                ty,
                types,
                result,
                Type::Fn {
                    params: new_params,
                    ret: new_ret,
                    err: new_err,
                },
            )
        }
        Type::Promise { ok, err } => {
            let new_ok = substitute_type(ok, mapping, types, result);
            let new_err = err.map(|e| substitute_type(e, mapping, types, result));
            intern_substituted(
                ty,
                types,
                result,
                Type::Promise {
                    ok: new_ok,
                    err: new_err,
                },
            )
        }
        Type::Result { ok, err } => {
            let new_ok = substitute_type(ok, mapping, types, result);
            let new_err = substitute_type(err, mapping, types, result);
            intern_substituted(
                ty,
                types,
                result,
                Type::Result {
                    ok: new_ok,
                    err: new_err,
                },
            )
        }
        Type::Generator { inner } => {
            let new_inner = substitute_type(inner, mapping, types, result);
            intern_substituted(ty, types, result, Type::Generator { inner: new_inner })
        }
        Type::GeneratorResult { inner } => {
            let new_inner = substitute_type(inner, mapping, types, result);
            intern_substituted(
                ty,
                types,
                result,
                Type::GeneratorResult { inner: new_inner },
            )
        }
        _ => {
            result.unchanged += 1;
            ty
        }
    }
}

fn intern_substituted(
    ty: TypeId,
    types: &mut TypeTable,
    result: &mut TypeSubstitutionResult,
    new_ty: Type,
) -> TypeId {
    let new_id = types.intern(&new_ty);
    if new_id == ty {
        result.unchanged += 1;
    } else {
        result.mapped += 1;
    }
    new_id
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use ts_aot_core::GenericParamId;

    fn empty_mapping() -> TypeParamMap {
        HashMap::new()
    }

    #[test]
    fn substitute_type_walks_into_generator_inner() {
        let mut types = TypeTable::new();
        let i64_ty = types.intern(&Type::I64);
        let gen_param = types.intern(&Type::GenericParam {
            id: GenericParamId::from_raw(0),
        });
        let gen_ty = types.intern(&Type::Generator { inner: gen_param });
        let mut mapping = empty_mapping();
        mapping.insert(GenericParamId::from_raw(0), i64_ty);
        let mut result = TypeSubstitutionResult::default();
        let new_id = substitute_type(gen_ty, &mapping, &mut types, &mut result);
        let new_ty = types.resolve(new_id).expect("substituted type resolves");
        match new_ty {
            Type::Generator { inner } => assert_eq!(
                *inner, i64_ty,
                "Generator<GenericParam> must substitute to Generator<i64>"
            ),
            other => panic!("expected Generator, got {other:?}"),
        }
    }

    #[test]
    fn substitute_type_walks_into_generator_result_inner() {
        let mut types = TypeTable::new();
        let i64_ty = types.intern(&Type::I64);
        let gen_param = types.intern(&Type::GenericParam {
            id: GenericParamId::from_raw(0),
        });
        let gr_ty = types.intern(&Type::GeneratorResult { inner: gen_param });
        let mut mapping = empty_mapping();
        mapping.insert(GenericParamId::from_raw(0), i64_ty);
        let mut result = TypeSubstitutionResult::default();
        let new_id = substitute_type(gr_ty, &mapping, &mut types, &mut result);
        let new_ty = types.resolve(new_id).expect("substituted type resolves");
        match new_ty {
            Type::GeneratorResult { inner } => assert_eq!(
                *inner, i64_ty,
                "GeneratorResult<GenericParam> must substitute to GeneratorResult<i64>"
            ),
            other => panic!("expected GeneratorResult, got {other:?}"),
        }
    }

    #[test]
    fn substitute_type_unchanged_generator_without_generic_param() {
        let mut types = TypeTable::new();
        let i64_ty = types.intern(&Type::I64);
        let gen_ty = types.intern(&Type::Generator { inner: i64_ty });
        let mapping = empty_mapping();
        let mut result = TypeSubstitutionResult::default();
        let new_id = substitute_type(gen_ty, &mapping, &mut types, &mut result);
        assert_eq!(
            new_id, gen_ty,
            "Generator<i64> with no GenericParam must stay unchanged"
        );
    }
}
