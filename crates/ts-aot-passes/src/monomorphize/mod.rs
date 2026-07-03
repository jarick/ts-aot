mod core;
mod substitute;
mod substitute_decl;
mod substitute_expr;
mod substitute_stmt;
mod substitute_ty;

#[cfg(test)]
mod tests;

pub use core::monomorphize;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MonomorphizeStats {
    pub generic_functions: usize,
    pub monomorphized: usize,
    pub calls_rewritten: usize,
}
