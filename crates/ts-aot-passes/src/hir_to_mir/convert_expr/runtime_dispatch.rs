use ts_aot_ir_mir::RuntimeOp;

pub(in crate::hir_to_mir::convert_expr) enum RuntimeBuiltin {
    GetOrDefault,
    Concat,
    Hole,
    PromiseAll,
    PromiseRace,
    PromiseAllSettled,
    PromiseAny,
    PromiseResolveStatic,
    PromiseRejectStatic,
}

impl RuntimeBuiltin {
    pub(in crate::hir_to_mir::convert_expr) fn op(self) -> RuntimeOp {
        match self {
            Self::GetOrDefault => RuntimeOp::ArrayGetOrDefault,
            Self::Concat => RuntimeOp::ArrayConcat,
            Self::Hole => RuntimeOp::ArrayHole,
            Self::PromiseAll => RuntimeOp::PromiseAll,
            Self::PromiseRace => RuntimeOp::PromiseRace,
            Self::PromiseAllSettled => RuntimeOp::PromiseAllSettled,
            Self::PromiseAny => RuntimeOp::PromiseAny,
            Self::PromiseResolveStatic => RuntimeOp::PromiseResolveStatic,
            Self::PromiseRejectStatic => RuntimeOp::PromiseRejectStatic,
        }
    }
}

pub(in crate::hir_to_mir::convert_expr) fn lookup_builtin(name: &str) -> Option<RuntimeBuiltin> {
    match name {
        "__ts_aot_array_get_or_default" => Some(RuntimeBuiltin::GetOrDefault),
        "__ts_aot_array_concat" => Some(RuntimeBuiltin::Concat),
        "__ts_aot_array_hole" => Some(RuntimeBuiltin::Hole),
        "__ts_aot_promise_all" => Some(RuntimeBuiltin::PromiseAll),
        "__ts_aot_promise_race" => Some(RuntimeBuiltin::PromiseRace),
        "__ts_aot_promise_all_settled" => Some(RuntimeBuiltin::PromiseAllSettled),
        "__ts_aot_promise_any" => Some(RuntimeBuiltin::PromiseAny),
        "__ts_aot_promise_resolve_value" => Some(RuntimeBuiltin::PromiseResolveStatic),
        "__ts_aot_promise_reject_value" => Some(RuntimeBuiltin::PromiseRejectStatic),
        _ => None,
    }
}
