use proc_macro2::TokenStream;
use quote::quote;

use ts_aot_ir_mir::MirProgram;

use crate::error::BackendError;
use crate::render::RenderConfig;

pub fn emit_program(
    program: &MirProgram,
    _cfg: &RenderConfig,
) -> Result<TokenStream, BackendError> {
    if program.decl_count() == 0 {
        Ok(quote! {})
    } else {
        Err(BackendError::NotImplemented)
    }
}
