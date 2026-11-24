use super::super::ctx::{BodyCtx, EmitCtx, EmitEnv};
use super::super::runtime_op::runtime_op_ident;
use super::expr::emit_exprs;
use crate::error::BackendError;
use proc_macro2::TokenStream;
use quote::quote;
use ts_aot_core::TypeId;
use ts_aot_ir_mir::{MirExpr, RuntimeOp};

pub(super) mod array_index_of;
pub(super) mod arrays;
pub(super) mod collections;
pub(super) mod misc;
pub(super) mod numbers;
pub(super) mod strings;

pub(super) fn emit_runtime_call(
    op: RuntimeOp,
    args: &[MirExpr],
    ty: TypeId,
    target_ty: Option<TypeId>,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
) -> Result<TokenStream, BackendError> {
    ctx.mark_runtime_used();
    match op {
        RuntimeOp::TypedArrayNew
        | RuntimeOp::ArrayGetOrDefault
        | RuntimeOp::ArrayFromMapped
        | RuntimeOp::ArrayBufferSlice
        | RuntimeOp::ArrayCreate
        | RuntimeOp::ArrayConcat
        | RuntimeOp::ArrayHole
        | RuntimeOp::ArrayPush
        | RuntimeOp::ArraySet
        | RuntimeOp::ArrayLen
        | RuntimeOp::ArraySlice => {
            self::arrays::emit_arrays_runtime_call(op, args, ty, target_ty, ctx, emit_env, body_ctx)
        }
        RuntimeOp::ArrayIndexOf => self::array_index_of::emit_array_index_of_runtime_call(
            op, args, ty, target_ty, ctx, emit_env, body_ctx,
        ),
        RuntimeOp::ObjectKeys
        | RuntimeOp::ArrayOwnKeys
        | RuntimeOp::MapSet
        | RuntimeOp::MapGet
        | RuntimeOp::WeakMapGet
        | RuntimeOp::WeakMapSet
        | RuntimeOp::WeakMapHas
        | RuntimeOp::WeakMapDelete
        | RuntimeOp::WeakMapClear => self::collections::emit_collections_runtime_call(
            op, args, ty, target_ty, ctx, emit_env, body_ctx,
        ),
        RuntimeOp::JsonParseString
        | RuntimeOp::JsonStringifyString
        | RuntimeOp::StringConcat
        | RuntimeOp::StringEquals
        | RuntimeOp::StringLen
        | RuntimeOp::StringIndexOf
        | RuntimeOp::StringCharAt
        | RuntimeOp::ArrayFromString
        | RuntimeOp::HostConsoleLog
        | RuntimeOp::SymbolFor
        | RuntimeOp::JsonParse
        | RuntimeOp::JsonStringify
        | RuntimeOp::SymbolNew => self::strings::emit_strings_runtime_call(
            op, args, ty, target_ty, ctx, emit_env, body_ctx,
        ),
        RuntimeOp::MathMax
        | RuntimeOp::MathMin
        | RuntimeOp::BigIntNeg
        | RuntimeOp::BigIntNot
        | RuntimeOp::BigIntToNumber
        | RuntimeOp::BigIntToString
        | RuntimeOp::NumberToString
        | RuntimeOp::NumberToFixed
        | RuntimeOp::NumberToExponential
        | RuntimeOp::NumberToPrecision
        | RuntimeOp::BigIntNew
        | RuntimeOp::BigIntAdd
        | RuntimeOp::BigIntSub
        | RuntimeOp::BigIntMul
        | RuntimeOp::BigIntDiv
        | RuntimeOp::BigIntRem
        | RuntimeOp::BigIntAnd
        | RuntimeOp::BigIntOr
        | RuntimeOp::BigIntXor
        | RuntimeOp::BigIntShl
        | RuntimeOp::BigIntShr
        | RuntimeOp::BigIntLt
        | RuntimeOp::BigIntLe
        | RuntimeOp::BigIntGt
        | RuntimeOp::BigIntGe
        | RuntimeOp::BigIntEq
        | RuntimeOp::BigIntNeq => self::numbers::emit_numbers_runtime_call(
            op, args, ty, target_ty, ctx, emit_env, body_ctx,
        ),
        RuntimeOp::ArrayIsArray
        | RuntimeOp::ArrayIsArrayFalse
        | RuntimeOp::OpIn
        | RuntimeOp::ArrayFrom
        | RuntimeOp::ObjectIs
        | RuntimeOp::DateParse
        | RuntimeOp::GeneratorNext
        | RuntimeOp::OpInstanceof
        | RuntimeOp::TypeOf
        | RuntimeOp::PromiseAll
        | RuntimeOp::PromiseAllSettled
        | RuntimeOp::PromiseRace
        | RuntimeOp::PromiseAny
        | RuntimeOp::PromiseResolveStatic
        | RuntimeOp::PromiseRejectStatic
        | RuntimeOp::PromiseThenInstance
        | RuntimeOp::PromiseCatchInstance
        | RuntimeOp::PromiseFinallyInstance => {
            self::misc::emit_misc_runtime_call(op, args, ty, target_ty, ctx, emit_env, body_ctx)
        }
        _ => self::misc::emit_misc_runtime_call(op, args, ty, target_ty, ctx, emit_env, body_ctx),
    }
}

fn runtime_op_takes_ctx(op: RuntimeOp) -> bool {
    matches!(
        op,
        RuntimeOp::StringConcat
            | RuntimeOp::StringCharAt
            | RuntimeOp::StringFromCharCode
            | RuntimeOp::StringFromCodePoint
            | RuntimeOp::JsonParseString
            | RuntimeOp::JsonStringifyString
            | RuntimeOp::JsonStringify
            | RuntimeOp::ArrayFromString
            | RuntimeOp::ArrayCreateWithLen
            | RuntimeOp::ArrayFromLengthMapped
            | RuntimeOp::ObjectKeys
            | RuntimeOp::DateToIsoString
            | RuntimeOp::SymbolKeyFor
    )
}

fn emit_default_runtime_call(
    op: RuntimeOp,
    args: &[MirExpr],
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
) -> Result<TokenStream, BackendError> {
    let name = runtime_op_ident(op)?;
    let ctx_arg = if runtime_op_takes_ctx(op) {
        quote!(__ctx,)
    } else {
        quote!()
    };
    let args = emit_exprs(args, ctx, emit_env, body_ctx)?;
    Ok(quote!(#name(#ctx_arg #(#args),*)))
}
