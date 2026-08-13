use ts_aot_ir_mir::RuntimeOp;

pub(in crate::hir_to_mir::convert_expr) enum RuntimeBuiltin {
    GetOrDefault,
    Concat,
    Hole,
}

impl RuntimeBuiltin {
    pub(in crate::hir_to_mir::convert_expr) fn op(self) -> RuntimeOp {
        match self {
            Self::GetOrDefault => RuntimeOp::ArrayGetOrDefault,
            Self::Concat => RuntimeOp::ArrayConcat,
            Self::Hole => RuntimeOp::ArrayHole,
        }
    }
}

pub(in crate::hir_to_mir::convert_expr) fn lookup_builtin(name: &str) -> Option<RuntimeBuiltin> {
    match name {
        "__ts_aot_array_get_or_default" => Some(RuntimeBuiltin::GetOrDefault),
        "__ts_aot_array_concat" => Some(RuntimeBuiltin::Concat),
        "__ts_aot_array_hole" => Some(RuntimeBuiltin::Hole),
        _ => None,
    }
}
