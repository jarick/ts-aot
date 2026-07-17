use proc_macro2::{Literal, TokenStream};
use quote::quote;

use ts_aot_ir_mir::MirExpr;

use crate::error::BackendError;

pub(super) fn emit_const_expr(expr: &MirExpr) -> Result<TokenStream, BackendError> {
    match expr {
        MirExpr::Unit | MirExpr::Null { .. } => Ok(quote!(())),
        MirExpr::Bool(value) => Ok(quote!(#value)),
        MirExpr::Int { value, .. } => Ok(emit_whole_number_literal(*value)),
        MirExpr::Float { value, .. } => Ok(crate::emit_decl::body::emit_float(*value)),
        _ => Err(BackendError::NotImplemented),
    }
}

pub(super) fn emit_whole_number_literal(value: i128) -> TokenStream {
    if value < 0 {
        let magnitude = Literal::u128_unsuffixed(value.unsigned_abs());
        quote!(-#magnitude)
    } else {
        let value = u128::try_from(value).expect("non-negative literal must fit u128");
        let literal = Literal::u128_unsuffixed(value);
        quote!(#literal)
    }
}
