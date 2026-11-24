use super::super::ctx::{BodyCtx, EmitCtx, EmitEnv};
use super::super::types::emit_type_id_with_ctx;
use super::expr::emit_expr;
use super::helpers::value_ty;
use super::place::expr_base_ty;
use crate::error::BackendError;
use proc_macro2::{Ident, TokenStream};
use quote::{format_ident, quote};
use ts_aot_core::{Type, TypeId, TypeTable};
use ts_aot_ir_mir::{BinaryOp, MirExpr};

pub(super) fn emit_same_value(
    types: &TypeTable,
    a_ty: &Type,
    b_ty: &Type,
    operands: (TokenStream, TokenStream),
    same_place: bool,
    identity_field: Option<&Ident>,
    depth: usize,
) -> Result<TokenStream, BackendError> {
    fn supported(types: &TypeTable, ty: &Type) -> bool {
        match ty {
            Type::Optional { inner, .. } => {
                types.resolve(*inner).is_some_and(|t| supported(types, t))
            }
            Type::Void
            | Type::Null
            | Type::Struct { .. }
            | Type::Named { .. }
            | Type::Array { .. }
            | Type::Bool
            | Type::I8
            | Type::I16
            | Type::I32
            | Type::I64
            | Type::U8
            | Type::U16
            | Type::U32
            | Type::U64
            | Type::F32
            | Type::F64
            | Type::String
            | Type::Symbol => true,
            _ => false,
        }
    }
    let (a, b) = operands;
    let unsupported = || BackendError::NotImplemented("Object.is: unsupported operand type");
    if !supported(types, a_ty) || !supported(types, b_ty) {
        return Err(unsupported());
    }
    let left = format_ident!("__ts_aot_left_{}", depth);
    let right = format_ident!("__ts_aot_right_{}", depth);
    match (a_ty, b_ty) {
        (Type::Optional { inner: x, .. }, Type::Optional { inner: y, .. }) => {
            let inner = emit_same_value(
                types,
                types.resolve(*x).ok_or_else(unsupported)?,
                types.resolve(*y).ok_or_else(unsupported)?,
                (quote!(#left), quote!(#right)),
                same_place,
                identity_field,
                depth + 1,
            )?;
            Ok(quote!(match (#a, #b) {
                (Some(#left), Some(#right)) => #inner,
                (None, None) => true,
                _ => false,
            }))
        }
        (Type::Optional { inner, nullish }, _) => {
            let comparison = emit_same_value(
                types,
                types.resolve(*inner).ok_or_else(unsupported)?,
                b_ty,
                (quote!(#left), b.clone()),
                same_place,
                identity_field,
                depth + 1,
            )?;
            let absent = match b_ty {
                Type::Null => nullish.null,
                Type::Void => nullish.undefined,
                _ => false,
            };
            Ok(quote!(match #a { Some(#left) => #comparison, None => #absent }))
        }
        (_, Type::Optional { .. }) => emit_same_value(
            types,
            b_ty,
            a_ty,
            (b, a),
            same_place,
            identity_field,
            depth + 1,
        ),
        (Type::Void, Type::Void) | (Type::Null, Type::Null) => Ok(quote!(true)),
        (Type::Struct { .. } | Type::Named { .. }, Type::Struct { .. } | Type::Named { .. }) => {
            Ok(if let Some(field) = identity_field {
                quote!(
                    ::std::rc::Rc::as_ptr(&(#a).#field) == ::std::rc::Rc::as_ptr(&(#b).#field)
                )
            } else {
                quote!(#same_place)
            })
        }
        (Type::Array { .. }, Type::Array { .. }) => Ok(quote!(#same_place)),
        _ if a_ty == b_ty => Ok(quote!(__ts_aot_object_is(#a, #b))),
        _ if is_number_type(a_ty) && is_number_type(b_ty) => {
            Ok(quote!(__ts_aot_object_is(&(*#a as f64), &(*#b as f64))))
        }
        _ => Ok(quote!(false)),
    }
}

fn is_number_type(ty: &Type) -> bool {
    matches!(
        ty,
        Type::I8
            | Type::I16
            | Type::I32
            | Type::I64
            | Type::U8
            | Type::U16
            | Type::U32
            | Type::U64
            | Type::F32
            | Type::F64
    )
}

pub(super) fn same_place(a: &MirExpr, b: &MirExpr) -> bool {
    match (a, b) {
        (MirExpr::Local(x), MirExpr::Local(y)) => x == y,
        (
            MirExpr::Field {
                base: ba,
                field: fa,
                ..
            },
            MirExpr::Field {
                base: bb,
                field: fb,
                ..
            },
        ) => fa == fb && same_place(ba, bb),
        (
            MirExpr::Index {
                base: ba,
                index: ia,
                ..
            },
            MirExpr::Index {
                base: bb,
                index: ib,
                ..
            },
        ) => same_place(ba, bb) && same_place(ia, ib),
        (MirExpr::Int { value: x, .. }, MirExpr::Int { value: y, .. }) => x == y,
        (MirExpr::Bool(x), MirExpr::Bool(y)) => x == y,
        (MirExpr::String { id: x, .. }, MirExpr::String { id: y, .. }) => x == y,
        _ => false,
    }
}

fn is_caught_comparable(emit_env: &EmitEnv, ty: TypeId) -> bool {
    emit_env
        .types
        .resolve(ty)
        .is_some_and(super::super::types::is_comparable_primitive)
}

pub(super) fn caught_operand_ty(
    expr: &MirExpr,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
) -> Option<TokenStream> {
    if matches!(expr, MirExpr::Bool(_)) {
        return Some(quote!(bool));
    }
    let ty = expr_base_ty(expr, body_ctx)?;
    if !is_caught_comparable(emit_env, ty) {
        return None;
    }
    Some(emit_type_id_with_ctx(ty, emit_env, ctx))
}

pub(super) fn emit_caught_literal_eq(
    caught: &MirExpr,
    other: &MirExpr,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
) -> Result<Option<TokenStream>, BackendError> {
    let variant = match other {
        MirExpr::Null { .. } => quote!(ts_aot_runtime::CaughtValue::Null),
        MirExpr::Unit => quote!(ts_aot_runtime::CaughtValue::Undefined),
        _ => return Ok(None),
    };
    let caught = emit_expr(caught, ctx, emit_env, body_ctx)?;
    Ok(Some(quote!(#caught == #variant)))
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum SourceNullish {
    Null,
    Undefined,
}

pub(super) fn source_nullish_kind(
    expr: &MirExpr,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
) -> Option<SourceNullish> {
    match value_ty(expr, ctx, emit_env, body_ctx).and_then(|ty| emit_env.types.resolve(ty)) {
        Some(Type::Null) => Some(SourceNullish::Null),
        Some(Type::Void) => Some(SourceNullish::Undefined),
        _ => None,
    }
}

pub(crate) fn is_nullish_literal(expr: &MirExpr) -> bool {
    matches!(expr, MirExpr::Null { .. } | MirExpr::Unit)
}

fn is_pure_operand(expr: &MirExpr) -> bool {
    match expr {
        MirExpr::Unit
        | MirExpr::Bool(_)
        | MirExpr::Int { .. }
        | MirExpr::Float { .. }
        | MirExpr::String { .. }
        | MirExpr::Null { .. }
        | MirExpr::Local(_)
        | MirExpr::Global(_) => true,
        MirExpr::Field { base, .. } => is_pure_operand(base),
        _ => false,
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum NullishKind {
    Null,
    Undefined,
}

fn nullish_kind(expr: &MirExpr) -> Option<NullishKind> {
    match expr {
        MirExpr::Null { .. } => Some(NullishKind::Null),
        MirExpr::Unit => Some(NullishKind::Undefined),
        _ => None,
    }
}

pub(super) fn emit_nullish_literal_equality(
    op: BinaryOp,
    left: &MirExpr,
    right: &MirExpr,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
) -> Result<Option<TokenStream>, BackendError> {
    let strict = matches!(op, BinaryOp::StrictEq | BinaryOp::StrictNe);
    let neg = matches!(op, BinaryOp::Ne | BinaryOp::StrictNe);
    let (left_kind, right_kind) = (nullish_kind(left), nullish_kind(right));
    if let (Some(left_kind), Some(right_kind)) = (left_kind, right_kind) {
        let equal = left_kind == right_kind || !strict;
        return Ok(Some(if equal == neg {
            quote!(false)
        } else {
            quote!(true)
        }));
    }
    let (operand, literal) = match (left_kind, right_kind) {
        (Some(kind), None) => (right, kind),
        (None, Some(kind)) => (left, kind),
        _ => return Ok(None),
    };
    let Some(ty) = value_ty(operand, ctx, emit_env, body_ctx) else {
        return Ok(None);
    };
    let resolved = emit_env.types.resolve(ty);
    if let Some(Type::Optional { nullish, .. }) = resolved {
        if strict && nullish.null && nullish.undefined {
            let op_name = match op {
                BinaryOp::StrictNe => "!==",
                _ => "===",
            };
            return Err(BackendError::UndecidableNullishComparison {
                code: "P0020",
                message: format!(
                    "`{op_name}` compares an optional value that allows both `null` and \
                     `undefined` with a nullish literal; the AOT target stores both as one \
                     `Option::None`, so the runtime value cannot decide the comparison \
                     (`null === undefined` is `false` in JavaScript while `None == None` is \
                     `true`). Narrow the operand to a single nullish kind (`T | null` or \
                     `T | undefined`), or use a presence test (`x == null` / `x != null`)"
                ),
            });
        }
        let includes_literal = !strict
            || match literal {
                NullishKind::Null => nullish.null,
                NullishKind::Undefined => nullish.undefined,
            };
        if includes_literal {
            let tokens = emit_expr(operand, ctx, emit_env, body_ctx)?;
            return Ok(Some(if neg {
                quote!((#tokens).is_some())
            } else {
                quote!((#tokens).is_none())
            }));
        }
        let folded = if neg { quote!(true) } else { quote!(false) };
        if is_pure_operand(operand) {
            return Ok(Some(folded));
        }
        let tokens = emit_expr(operand, ctx, emit_env, body_ctx)?;
        return Ok(Some(quote!({
            let _ = &(#tokens);
            #folded
        })));
    }
    if resolved.is_none() || matches!(resolved, Some(Type::Error | Type::GenericParam { .. })) {
        return Ok(None);
    }
    let equal = match (resolved, literal) {
        (Some(Type::Null), NullishKind::Null) | (Some(Type::Void), NullishKind::Undefined) => true,
        (Some(Type::Null), NullishKind::Undefined) | (Some(Type::Void), NullishKind::Null) => {
            !strict
        }
        _ => false,
    };
    let folded = if equal == neg {
        quote!(false)
    } else {
        quote!(true)
    };
    if is_pure_operand(operand) {
        return Ok(Some(folded));
    }
    let tokens = emit_expr(operand, ctx, emit_env, body_ctx)?;
    Ok(Some(quote!({
        let _ = &(#tokens);
        #folded
    })))
}

pub(super) fn is_rust_comparable_value(ty: &Type) -> bool {
    super::super::types::is_comparable_primitive(ty)
        || matches!(
            ty,
            Type::Symbol | Type::Date | Type::BigInt | Type::Null | Type::Void
        )
}

pub(super) fn index_of_element_is_comparable(ty: TypeId, emit_env: &EmitEnv) -> bool {
    match emit_env.types.resolve(ty) {
        Some(Type::Optional { inner, .. }) => index_of_element_is_comparable(*inner, emit_env),
        Some(resolved) => is_rust_comparable_value(resolved),
        None => false,
    }
}

#[cfg(test)]
mod same_value_tests {
    use super::emit_same_value;
    use ts_aot_core::{Type, TypeTable};

    #[test]
    fn object_is_checks_both_operand_types() {
        use quote::quote;
        let mut types = TypeTable::new();
        let number = types.intern(&Type::I64);
        let optional = Type::Optional {
            inner: number,
            nullish: ts_aot_core::NullishKinds::BOTH,
        };
        let emit = |a: &Type, b: &Type| {
            emit_same_value(&types, a, b, (quote!(a), quote!(b)), false, None, 0)
                .map(|tokens| tokens.to_string())
        };
        assert!(emit(&Type::I64, &Type::F64).unwrap().contains("as f64"));
        assert!(emit(&optional, &Type::I64).unwrap().contains("Some"));
        assert!(emit(&Type::I64, &optional).unwrap().contains("Some"));
        assert!(
            emit(&optional, &optional)
                .unwrap()
                .contains("(None , None) => true")
        );
        assert!(
            emit(&optional, &Type::Void)
                .unwrap()
                .contains("None => true")
        );
        assert_eq!(emit(&Type::Bool, &Type::I64).unwrap(), "false");
        let array = Type::Array { element: number };
        assert_eq!(emit(&array, &array).unwrap(), "false");
        for unsupported in [Type::Error, Type::BigInt, Type::Tuple { elements: vec![] }] {
            assert!(emit(&Type::I64, &unsupported).is_err());
            assert!(emit(&unsupported, &Type::I64).is_err());
        }
    }
}
