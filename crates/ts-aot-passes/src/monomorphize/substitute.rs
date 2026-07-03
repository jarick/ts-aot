use std::collections::HashMap;

use ts_aot_core::{GenericParamId, TypeId};

pub type TypeParamMap = HashMap<GenericParamId, TypeId>;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TypeSubstitutionResult {
    pub mapped: usize,
    pub unchanged: usize,
}

pub use super::substitute_decl::substitute_func;
