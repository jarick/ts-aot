use ts_aot_core::{Diagnostic, DiagnosticBag, FunctionId, LocalId, Span, TypeId};

#[derive(Debug, Default)]
pub struct PassContext {
    diagnostics: DiagnosticBag,
    next_function: u32,
    next_local: u32,
    next_type: u32,
}

impl PassContext {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn with_seeded_counters(next_function: u32, next_local: u32, next_type: u32) -> Self {
        Self {
            diagnostics: DiagnosticBag::new(),
            next_function,
            next_local,
            next_type,
        }
    }

    #[must_use]
    pub fn diagnostics(&self) -> &DiagnosticBag {
        &self.diagnostics
    }

    pub fn diagnostics_mut(&mut self) -> &mut DiagnosticBag {
        &mut self.diagnostics
    }

    #[must_use]
    pub fn has_errors(&self) -> bool {
        self.diagnostics.has_errors()
    }

    pub fn take_diagnostics(&mut self) -> DiagnosticBag {
        std::mem::take(&mut self.diagnostics)
    }

    pub fn push_diagnostic(&mut self, diag: Diagnostic) {
        self.diagnostics.push(diag);
    }

    pub fn error(
        &mut self,
        code: impl Into<ts_aot_core::DiagnosticCode>,
        message: impl Into<String>,
        span: Span,
    ) {
        self.diagnostics
            .push(Diagnostic::error(code, message, span));
    }

    pub fn warning(
        &mut self,
        code: impl Into<ts_aot_core::DiagnosticCode>,
        message: impl Into<String>,
        span: Span,
    ) {
        self.diagnostics
            .push(Diagnostic::warning(code, message, span));
    }

    pub fn next_function_id(&mut self) -> FunctionId {
        let id = FunctionId::from_raw(self.next_function);
        self.next_function += 1;
        id
    }

    pub fn next_local_id(&mut self) -> LocalId {
        let id = LocalId::from_raw(self.next_local);
        self.next_local += 1;
        id
    }

    pub fn next_type_id(&mut self) -> TypeId {
        let id = TypeId::from_raw(self.next_type);
        self.next_type += 1;
        id
    }

    #[must_use]
    pub fn function_counter(&self) -> u32 {
        self.next_function
    }

    #[must_use]
    pub fn local_counter(&self) -> u32 {
        self.next_local
    }

    #[must_use]
    pub fn type_counter(&self) -> u32 {
        self.next_type
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ts_aot_core::Severity;

    #[test]
    fn fresh_context_has_no_diagnostics() {
        let ctx = PassContext::new();
        assert!(!ctx.has_errors());
        assert_eq!(ctx.function_counter(), 0);
        assert_eq!(ctx.local_counter(), 0);
        assert_eq!(ctx.type_counter(), 0);
    }

    #[test]
    fn with_seeded_counters_initializes_them() {
        let ctx = PassContext::with_seeded_counters(5, 10, 15);
        assert_eq!(ctx.function_counter(), 5);
        assert_eq!(ctx.local_counter(), 10);
        assert_eq!(ctx.type_counter(), 15);
    }

    #[test]
    fn next_function_id_increments_counter() {
        let mut ctx = PassContext::new();
        let a = ctx.next_function_id();
        let b = ctx.next_function_id();
        assert_eq!(a.raw(), 0);
        assert_eq!(b.raw(), 1);
        assert_eq!(ctx.function_counter(), 2);
    }

    #[test]
    fn next_local_id_increments_counter() {
        let mut ctx = PassContext::new();
        let a = ctx.next_local_id();
        let b = ctx.next_local_id();
        let c = ctx.next_local_id();
        assert_eq!(a.raw(), 0);
        assert_eq!(b.raw(), 1);
        assert_eq!(c.raw(), 2);
        assert_eq!(ctx.local_counter(), 3);
    }

    #[test]
    fn next_type_id_increments_counter() {
        let mut ctx = PassContext::new();
        let a = ctx.next_type_id();
        let b = ctx.next_type_id();
        assert_eq!(a.raw(), 0);
        assert_eq!(b.raw(), 1);
        assert_eq!(ctx.type_counter(), 2);
    }

    #[test]
    fn id_counters_are_independent() {
        let mut ctx = PassContext::new();
        ctx.next_function_id();
        ctx.next_local_id();
        ctx.next_type_id();
        assert_eq!(ctx.function_counter(), 1);
        assert_eq!(ctx.local_counter(), 1);
        assert_eq!(ctx.type_counter(), 1);
    }

    #[test]
    fn error_pushes_diagnostic_and_marks_has_errors() {
        let mut ctx = PassContext::new();
        ctx.error("E0001", "boom", Span::new(0, 4));
        assert!(ctx.has_errors());
        assert_eq!(ctx.diagnostics().len(), 1);
    }

    #[test]
    fn warning_pushes_diagnostic_without_errors() {
        let mut ctx = PassContext::new();
        ctx.warning("W0001", "watch out", Span::new(0, 4));
        assert!(!ctx.has_errors());
        assert_eq!(ctx.diagnostics().len(), 1);
    }

    #[test]
    fn take_diagnostics_resets_bag() {
        let mut ctx = PassContext::new();
        ctx.error("E0001", "first", Span::new(0, 1));
        ctx.error("E0002", "second", Span::new(2, 3));
        assert_eq!(ctx.diagnostics().len(), 2);

        let taken = ctx.take_diagnostics();
        assert_eq!(taken.len(), 2);
        assert!(!ctx.has_errors());
        assert_eq!(ctx.diagnostics().len(), 0);
    }

    #[test]
    fn push_diagnostic_preserves_severity() {
        let mut ctx = PassContext::new();
        ctx.push_diagnostic(Diagnostic::note("N0001", "fyi", Span::new(0, 1)));
        assert!(!ctx.has_errors());
        let diag = ctx.diagnostics().iter().next().unwrap();
        assert_eq!(diag.severity, Severity::Note);
    }

    #[test]
    fn diagnostics_mut_allows_extend() {
        let mut ctx = PassContext::new();
        ctx.error("E0001", "first", Span::new(0, 1));
        let incoming = vec![Diagnostic::error("E0002", "second", Span::new(2, 3))];
        ctx.diagnostics_mut().extend(incoming);
        assert_eq!(ctx.diagnostics().len(), 2);
    }
}
