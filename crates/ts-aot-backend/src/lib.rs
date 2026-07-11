use proc_macro2::TokenStream;

use ts_aot_core::TypeTable;
use ts_aot_ir_mir::MirProgram;

mod emit_decl;
mod error;
mod render;

pub use emit_decl::emit_decls;
pub use error::BackendError;
pub use render::{RenderConfig, render_tokens};

pub fn compile_program(program: &MirProgram) -> Result<TokenStream, BackendError> {
    let types = TypeTable::new();
    let cfg = RenderConfig::default();
    compile_with_types(program, &types, &cfg)
}

pub fn compile_with_types(
    program: &MirProgram,
    types: &TypeTable,
    _cfg: &RenderConfig,
) -> Result<TokenStream, BackendError> {
    emit_decls(program, types)
}

pub fn compile_to_string(program: &MirProgram) -> Result<String, BackendError> {
    let cfg = RenderConfig::default();
    let types = TypeTable::new();
    let tokens = compile_with_types(program, &types, &cfg)?;
    Ok(render_tokens(&tokens, &cfg))
}

pub fn compile_to_string_with_types(
    program: &MirProgram,
    types: &TypeTable,
) -> Result<String, BackendError> {
    let cfg = RenderConfig::default();
    let tokens = compile_with_types(program, types, &cfg)?;
    Ok(render_tokens(&tokens, &cfg))
}

#[cfg(test)]
mod tests;
