use ts_aot_ir_mir::RuntimeOp;

pub(in crate::hir_to_mir::convert_expr) enum RuntimeBuiltin {
    ArrayGetOrDefault,
}

impl RuntimeBuiltin {
    pub(in crate::hir_to_mir::convert_expr) fn op(self) -> RuntimeOp {
        match self {
            Self::ArrayGetOrDefault => RuntimeOp::ArrayGetOrDefault,
        }
    }
}

pub(in crate::hir_to_mir::convert_expr) fn lookup_builtin(name: &str) -> Option<RuntimeBuiltin> {
    match name {
        "__ts_aot_array_get_or_default" => Some(RuntimeBuiltin::ArrayGetOrDefault),
        _ => None,
    }
}
