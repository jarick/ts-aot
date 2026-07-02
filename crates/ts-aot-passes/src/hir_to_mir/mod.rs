#![allow(clippy::match_same_arms)]
#![allow(dead_code)]

mod convert_expr;
mod convert_stmt;
mod converter;
mod ops;
mod program;

#[cfg(test)]
mod tests;

use std::ops::Deref;

use ts_aot_ir_hir::HirStmt;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct HirBlock(pub Vec<HirStmt>);

impl Deref for HirBlock {
    type Target = [HirStmt];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

pub use converter::ExprConverter;
pub use program::{convert_function, convert_program};

pub(crate) const PLACEHOLDER_FUNCTION: ts_aot_core::FunctionId =
    ts_aot_core::FunctionId::from_raw(u32::MAX);
