mod core;
pub mod infer;
mod substitute;
mod substitute_decl;
mod substitute_expr;
mod substitute_stmt;
mod substitute_ty;

#[cfg(test)]
mod infer_tests;

#[cfg(test)]
mod tests;

pub use core::monomorphize;
pub use infer::hir_expr_ty;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MonomorphizeStats {
    pub generic_functions: usize,
    pub monomorphized: usize,
    pub calls_rewritten: usize,
}
