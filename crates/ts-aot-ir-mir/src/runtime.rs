use crate::body::RuntimeOp;

#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RuntimeRequirements {
    pub needs_runtime: bool,
    pub needs_string: bool,
    pub needs_array: bool,
    pub needs_map: bool,
    pub needs_result: bool,
    pub needs_promise: bool,
    pub needs_scheduler: bool,
    pub needs_host_io: bool,
    pub needs_console: bool,
    pub needs_math: bool,
}

impl RuntimeRequirements {
    pub fn require(&mut self, op: RuntimeOp) {
        self.needs_runtime = true;
        match op {
            RuntimeOp::StringConcat | RuntimeOp::StringEquals | RuntimeOp::StringLen => {
                self.needs_string = true;
            }
            RuntimeOp::ArrayCreate
            | RuntimeOp::ArrayGet
            | RuntimeOp::ArraySet
            | RuntimeOp::ArrayLen => {
                self.needs_array = true;
            }
            RuntimeOp::MapGet | RuntimeOp::MapSet => {
                self.needs_map = true;
            }
            RuntimeOp::ResultOk | RuntimeOp::ResultErr | RuntimeOp::ResultUnwrapOk => {
                self.needs_result = true;
            }
            RuntimeOp::PromiseCreate | RuntimeOp::PromiseResolve => {
                self.needs_promise = true;
                self.needs_scheduler = true;
            }
            RuntimeOp::HostConsoleLog => {
                self.needs_host_io = true;
                self.needs_console = true;
            }
            RuntimeOp::MathSqrt => {
                self.needs_math = true;
            }
            RuntimeOp::OpIn | RuntimeOp::OpInstanceof => {
                self.needs_runtime = true;
            }
            RuntimeOp::OpObjectGet | RuntimeOp::OpObjectSet | RuntimeOp::OpObjectHas => {
                self.needs_runtime = true;
            }
            RuntimeOp::OpObjectDelete
            | RuntimeOp::OpObjectUnwrap
            | RuntimeOp::OpObjectNew
            | RuntimeOp::OpObjectProtoGet
            | RuntimeOp::OpObjectProtoSet
            | RuntimeOp::OpObjectSetPrototypeOf
            | RuntimeOp::OpObjectKeys
            | RuntimeOp::OpDynamicBinary => {
                self.needs_runtime = true;
            }
            RuntimeOp::TypeOf => unreachable!(
                "TypeOf is handled by MirExpr::TypeOf + emit_typeof, not MirStmt::Runtime"
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_has_no_requirements() {
        let r = RuntimeRequirements::default();
        assert!(!r.needs_runtime);
        assert!(!r.needs_string);
        assert!(!r.needs_array);
        assert!(!r.needs_map);
        assert!(!r.needs_result);
        assert!(!r.needs_promise);
        assert!(!r.needs_scheduler);
        assert!(!r.needs_host_io);
        assert!(!r.needs_console);
        assert!(!r.needs_math);
    }

    #[test]
    fn require_string_op_sets_string_and_runtime() {
        let mut r = RuntimeRequirements::default();
        r.require(RuntimeOp::StringConcat);
        assert!(r.needs_runtime);
        assert!(r.needs_string);
        assert!(!r.needs_array);
        assert!(!r.needs_math);
    }

    #[test]
    fn require_array_op_sets_array_and_runtime() {
        let mut r = RuntimeRequirements::default();
        r.require(RuntimeOp::ArrayCreate);
        assert!(r.needs_runtime);
        assert!(r.needs_array);
        assert!(!r.needs_string);
    }

    #[test]
    fn require_promise_op_sets_promise_and_scheduler() {
        let mut r = RuntimeRequirements::default();
        r.require(RuntimeOp::PromiseCreate);
        assert!(r.needs_runtime);
        assert!(r.needs_promise);
        assert!(r.needs_scheduler);
    }

    #[test]
    fn require_host_console_log_sets_host_io_and_console() {
        let mut r = RuntimeRequirements::default();
        r.require(RuntimeOp::HostConsoleLog);
        assert!(r.needs_runtime);
        assert!(r.needs_host_io);
        assert!(r.needs_console);
        assert!(!r.needs_math);
    }

    #[test]
    fn require_math_sqrt_sets_math_only() {
        let mut r = RuntimeRequirements::default();
        r.require(RuntimeOp::MathSqrt);
        assert!(r.needs_runtime);
        assert!(r.needs_math);
        assert!(!r.needs_string);
        assert!(!r.needs_console);
    }

    #[test]
    fn multiple_requires_accumulate() {
        let mut r = RuntimeRequirements::default();
        r.require(RuntimeOp::StringConcat);
        r.require(RuntimeOp::ArrayGet);
        r.require(RuntimeOp::MathSqrt);
        assert!(r.needs_runtime);
        assert!(r.needs_string);
        assert!(r.needs_array);
        assert!(r.needs_math);
    }
}
