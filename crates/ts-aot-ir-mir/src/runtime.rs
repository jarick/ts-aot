use std::collections::HashSet;

use crate::body::RuntimeOp;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RuntimeFeature {
    String,
    Array,
    Map,
    Result,
    Promise,
    Scheduler,
    HostIo,
    Console,
    Math,
}

impl RuntimeFeature {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::String => "string",
            Self::Array => "array",
            Self::Map => "map",
            Self::Result => "result",
            Self::Promise => "promise",
            Self::Scheduler => "scheduler",
            Self::HostIo => "host_io",
            Self::Console => "console",
            Self::Math => "math",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RuntimeRequirements {
    needs_runtime: bool,
    features: HashSet<RuntimeFeature>,
}

impl RuntimeRequirements {
    #[must_use]
    pub fn needs_runtime(&self) -> bool {
        self.needs_runtime
    }

    #[must_use]
    pub fn needs(&self, feature: RuntimeFeature) -> bool {
        self.features.contains(&feature)
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        !self.needs_runtime && self.features.is_empty()
    }

    pub fn features(&self) -> impl Iterator<Item = RuntimeFeature> + '_ {
        self.features.iter().copied()
    }

    pub fn require(&mut self, op: RuntimeOp) {
        self.needs_runtime = true;
        for feature in features_for(op) {
            self.features.insert(*feature);
        }
    }

    pub fn merge(&mut self, other: &RuntimeRequirements) {
        self.needs_runtime |= other.needs_runtime;
        self.features.extend(other.features());
    }
}

fn features_for(op: RuntimeOp) -> &'static [RuntimeFeature] {
    use RuntimeFeature::{
        Array, Console, HostIo, Map, Math, Promise, Result as ResultFeat, Scheduler,
        String as StringFeat,
    };
    match op {
        RuntimeOp::StringConcat | RuntimeOp::StringEquals | RuntimeOp::StringLen => &[StringFeat],
        RuntimeOp::ArrayCreate
        | RuntimeOp::ArrayGet
        | RuntimeOp::ArraySet
        | RuntimeOp::ArrayLen => &[Array],
        RuntimeOp::MapGet | RuntimeOp::MapSet => &[Map],
        RuntimeOp::ResultOk | RuntimeOp::ResultErr | RuntimeOp::ResultUnwrapOk => &[ResultFeat],
        RuntimeOp::PromiseCreate | RuntimeOp::PromiseResolve => &[Promise, Scheduler],
        RuntimeOp::HostConsoleLog => &[HostIo, Console],
        RuntimeOp::MathSqrt => &[Math],
        RuntimeOp::OpIn
        | RuntimeOp::OpInstanceof
        | RuntimeOp::OpObjectGet
        | RuntimeOp::OpObjectSet
        | RuntimeOp::OpObjectHas
        | RuntimeOp::OpObjectDelete
        | RuntimeOp::OpObjectUnwrap
        | RuntimeOp::OpObjectNew
        | RuntimeOp::OpObjectProtoGet
        | RuntimeOp::OpObjectProtoSet
        | RuntimeOp::OpObjectSetPrototypeOf
        | RuntimeOp::OpObjectKeys
        | RuntimeOp::OpDynamicBinary
        | RuntimeOp::DynVecNew
        | RuntimeOp::DynVecAppend => &[],
        RuntimeOp::TypeOf => {
            unreachable!("TypeOf is handled by MirExpr::TypeOf + emit_typeof, not MirStmt::Runtime")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_has_no_requirements() {
        let r = RuntimeRequirements::default();
        assert!(!r.needs_runtime());
        assert!(!r.needs(RuntimeFeature::String));
        assert!(!r.needs(RuntimeFeature::Array));
        assert!(!r.needs(RuntimeFeature::Map));
        assert!(!r.needs(RuntimeFeature::Result));
        assert!(!r.needs(RuntimeFeature::Promise));
        assert!(!r.needs(RuntimeFeature::Scheduler));
        assert!(!r.needs(RuntimeFeature::HostIo));
        assert!(!r.needs(RuntimeFeature::Console));
        assert!(!r.needs(RuntimeFeature::Math));
        assert!(r.is_empty());
    }

    #[test]
    fn require_string_op_sets_string_and_runtime() {
        let mut r = RuntimeRequirements::default();
        r.require(RuntimeOp::StringConcat);
        assert!(r.needs_runtime());
        assert!(r.needs(RuntimeFeature::String));
        assert!(!r.needs(RuntimeFeature::Array));
        assert!(!r.needs(RuntimeFeature::Math));
    }

    #[test]
    fn require_array_op_sets_array_and_runtime() {
        let mut r = RuntimeRequirements::default();
        r.require(RuntimeOp::ArrayCreate);
        assert!(r.needs_runtime());
        assert!(r.needs(RuntimeFeature::Array));
        assert!(!r.needs(RuntimeFeature::String));
    }

    #[test]
    fn require_promise_op_sets_promise_and_scheduler() {
        let mut r = RuntimeRequirements::default();
        r.require(RuntimeOp::PromiseCreate);
        assert!(r.needs_runtime());
        assert!(r.needs(RuntimeFeature::Promise));
        assert!(r.needs(RuntimeFeature::Scheduler));
    }

    #[test]
    fn require_host_console_log_sets_host_io_and_console() {
        let mut r = RuntimeRequirements::default();
        r.require(RuntimeOp::HostConsoleLog);
        assert!(r.needs_runtime());
        assert!(r.needs(RuntimeFeature::HostIo));
        assert!(r.needs(RuntimeFeature::Console));
        assert!(!r.needs(RuntimeFeature::Math));
    }

    #[test]
    fn require_math_sqrt_sets_math_only() {
        let mut r = RuntimeRequirements::default();
        r.require(RuntimeOp::MathSqrt);
        assert!(r.needs_runtime());
        assert!(r.needs(RuntimeFeature::Math));
        assert!(!r.needs(RuntimeFeature::String));
        assert!(!r.needs(RuntimeFeature::Console));
    }

    #[test]
    fn multiple_requires_accumulate() {
        let mut r = RuntimeRequirements::default();
        r.require(RuntimeOp::StringConcat);
        r.require(RuntimeOp::ArrayGet);
        r.require(RuntimeOp::MathSqrt);
        assert!(r.needs_runtime());
        assert!(r.needs(RuntimeFeature::String));
        assert!(r.needs(RuntimeFeature::Array));
        assert!(r.needs(RuntimeFeature::Math));
    }

    #[test]
    fn merge_combines_two_requirements() {
        let mut a = RuntimeRequirements::default();
        a.require(RuntimeOp::StringConcat);
        let mut b = RuntimeRequirements::default();
        b.require(RuntimeOp::ArrayCreate);
        a.merge(&b);
        assert!(a.needs(RuntimeFeature::String));
        assert!(a.needs(RuntimeFeature::Array));
    }
}
