use proc_macro2::TokenStream;

use ts_aot_ir_mir::MirProgram;

mod emitter;
mod error;
mod render;

pub use emitter::emit_program;
pub use error::BackendError;
pub use render::{RenderConfig, render_tokens};

pub fn compile_program(program: &MirProgram) -> Result<TokenStream, BackendError> {
    let cfg = RenderConfig::default();
    emit_program(program, &cfg)
}

pub fn compile_to_string(program: &MirProgram) -> Result<String, BackendError> {
    let cfg = RenderConfig::default();
    let tokens = emit_program(program, &cfg)?;
    Ok(render_tokens(&tokens, &cfg))
}

#[cfg(test)]
mod tests;
