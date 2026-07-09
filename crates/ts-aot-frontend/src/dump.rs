use ts_aot_core::{Diagnostic, Severity};

use crate::FrontendOutput;

pub(crate) struct Dumper {
    indent: usize,
    buf: String,
}

impl Dumper {
    pub(crate) fn new() -> Self {
        Self {
            indent: 0,
            buf: String::new(),
        }
    }

    pub(crate) fn indent_write(&mut self, s: &str) {
        for _ in 0..self.indent {
            self.buf.push_str("  ");
        }
        self.buf.push_str(s);
    }

    pub(crate) fn line(&mut self, s: &str) {
        self.indent_write(s);
        self.buf.push('\n');
    }

    pub(crate) fn push(&mut self) {
        self.indent += 1;
    }

    pub(crate) fn pop(&mut self) {
        self.indent -= 1;
    }
}

impl FrontendOutput {
    #[must_use]
    pub fn dump_text(&self) -> String {
        let mut d = Dumper::new();
        d.line("FrontendOutput {");
        d.push();
        d.line(&format!("source: \"module={}\"", self.program.module.raw()));
        dump_diagnostics(&self.diagnostics, &mut d);
        d.line("program:");
        d.push();
        for line in self.program.dump_text().lines() {
            d.line(line);
        }
        d.pop();
        d.pop();
        d.line("}");
        d.buf
    }
}

fn dump_diagnostics(bag: &ts_aot_core::DiagnosticBag, d: &mut Dumper) {
    d.line(&format!("diagnostics: {} [", bag.len()));
    d.push();
    for diag in bag {
        dump_diagnostic(diag, d);
    }
    if bag.is_empty() {
        d.line("");
    }
    d.pop();
    d.line("]");
}

fn dump_diagnostic(diag: &Diagnostic, d: &mut Dumper) {
    let sev = match diag.severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
        Severity::Note => "note",
    };
    d.line(&format!(
        "{} [{}] {} ({}..{})",
        sev,
        diag.code.as_str(),
        diag.message,
        diag.span.start,
        diag.span.end,
    ));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::FrontendPass;

    fn dump(src: &str) -> String {
        FrontendPass::new().run("test.ts", src).dump_text()
    }

    #[test]
    fn dump_clean_program_starts_with_frontendoutput_header() {
        let text = dump("function noop(): void {}");
        assert!(text.starts_with("FrontendOutput {\n"));
    }

    #[test]
    fn dump_clean_program_has_no_diagnostic_lines() {
        let text = dump("function noop(): void {}");
        assert!(text.contains("diagnostics: 0 ["));
        for line in text.lines() {
            assert!(
                !line.contains("error [")
                    && !line.contains("warning [")
                    && !line.contains("note ["),
                "clean program should not emit diagnostic lines, got: {line}"
            );
        }
    }

    #[test]
    fn dump_includes_program_dump_block() {
        let text = dump("function noop(): void {}");
        assert!(text.contains("program:"));
        assert!(text.contains("HirProgram(module=0)"));
        assert!(text.contains("fn noop"));
    }

    #[test]
    fn dump_includes_source_label() {
        let text = dump("function noop(): void {}");
        assert!(text.contains("source: \"module=0\""));
    }

    #[test]
    fn dump_emits_parse_error_diagnostic() {
        let out = FrontendPass::new().run("bad.ts", "const x: number = ;");
        assert!(out.diagnostics.has_errors(), "expected parse errors");
        let text = out.dump_text();
        assert!(text.contains("diagnostics:"));
        assert!(text.contains("error [E0200]") || text.contains("error [E0100]"));
        assert!(text.contains("(0.."));
    }

    #[test]
    fn dump_emits_severity_word_per_diagnostic() {
        let text = dump("const x: number = ;");
        assert!(text.contains("error ["));
    }

    #[test]
    fn dump_nested_program_block_is_indented() {
        let text = dump("function noop(): void {}");
        let program_line = text
            .lines()
            .find(|l| l.contains("HirProgram(module=0)"))
            .expect("program line");
        assert!(
            program_line.starts_with("    "),
            "expected HirProgram line to be indented inside FrontendOutput, got: {program_line:?}"
        );
    }

    #[test]
    fn dump_indent_push_and_pop_works() {
        let mut d = Dumper::new();
        d.line("a");
        d.push();
        d.line("b");
        d.pop();
        d.line("c");
        let expected = "a\n  b\nc\n";
        assert_eq!(d.buf, expected);
    }

    #[test]
    fn dump_program_block_preserves_declaration_dump() {
        let text = dump("function greet(name: string): string { return name; }");
        assert!(text.contains("fn greet(name:"));
        assert!(text.contains("imports: ["));
        assert!(text.contains("exports: ["));
        assert!(text.contains("declarations: ["));
    }
}
