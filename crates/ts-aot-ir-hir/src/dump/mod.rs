mod decl;
mod expr;

use ts_aot_core::Atom;

use crate::program::{HirExport, HirImport, HirProgram};

pub(crate) struct Dumper {
    indent: usize,
    pub(crate) buf: String,
}

impl Dumper {
    pub(crate) fn new() -> Self {
        Self {
            indent: 0,
            buf: String::new(),
        }
    }

    pub(crate) fn write(&mut self, s: &str) {
        self.buf.push_str(s);
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

impl HirProgram {
    pub fn dump_text(&self) -> String {
        let mut d = Dumper::new();
        d.line(&format!("HirProgram(module={}) {{", self.module.raw()));
        d.push();
        d.line("imports: [");
        d.push();
        for imp in &self.imports {
            dump_import(imp, &mut d);
        }
        if self.imports.is_empty() {
            d.line("");
        }
        d.pop();
        d.line("]");
        d.line("exports: [");
        d.push();
        for exp in &self.exports {
            dump_export(exp, &mut d);
        }
        if self.exports.is_empty() {
            d.line("");
        }
        d.pop();
        d.line("]");
        d.line("declarations: [");
        d.push();
        for decl in &self.declarations {
            decl::dump_decl(decl, &mut d);
        }
        if self.declarations.is_empty() {
            d.line("");
        }
        d.pop();
        d.line("]");
        d.pop();
        d.line("}");
        d.buf
    }
}

fn dump_import(imp: &HirImport, d: &mut Dumper) {
    let mut line = format!(
        "import {{ {} }} from {:?}",
        dump_sym(&imp.name, d),
        imp.module
    );
    if let Some(alias) = &imp.alias {
        line.push_str(&format!(" as {}", dump_sym(alias, d)));
    }
    d.line(&line);
}

fn dump_export(exp: &HirExport, d: &mut Dumper) {
    let mut line = format!("export {{ {} }}", dump_sym(&exp.name, d));
    if let Some(alias) = &exp.alias {
        line.push_str(&format!(" as {}", dump_sym(alias, d)));
    }
    d.line(&line);
}

pub(crate) fn dump_sym(id: &Atom, _d: &Dumper) -> String {
    id.as_str().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decl::{HirClass, HirDecl, HirField, HirFunction};
    use crate::program::HirProgram;
    use ts_aot_core::{Atom, ModuleId, TypeId};

    fn empty_func(name: &str) -> HirFunction {
        HirFunction {
            name: Atom::new_inline(name),
            params: Vec::new(),
            ret: TypeId::from_raw(0),
            throws: None,
            body: Vec::new(),
            is_async: false,
            is_generator: false,
            is_exported: false,
            type_params: Vec::new(),
            async_info: None,
        }
    }

    #[test]
    fn dump_empty_program() {
        let prog = HirProgram::new(ModuleId::from_raw(0));
        let text = prog.dump_text();
        assert!(text.contains("HirProgram(module=0)"));
        assert!(text.contains("imports: ["));
        assert!(text.contains("exports: ["));
        assert!(text.contains("declarations: ["));
    }

    #[test]
    fn dump_program_with_module_id() {
        let prog = HirProgram::new(ModuleId::from_raw(42));
        let text = prog.dump_text();
        assert!(text.contains("HirProgram(module=42)"));
    }

    #[test]
    fn dump_program_with_import() {
        let mut prog = HirProgram::new(ModuleId::from_raw(0));
        prog.imports.push(HirImport {
            module: Atom::new_inline("./foo"),
            name: Atom::new_inline("bar"),
            alias: None,
        });
        let text = prog.dump_text();
        assert!(text.contains("import"));
        assert!(text.contains("bar"));
        assert!(text.contains("./foo"));
    }

    #[test]
    fn dump_program_with_import_with_alias() {
        let mut prog = HirProgram::new(ModuleId::from_raw(0));
        prog.imports.push(HirImport {
            module: Atom::new_inline("./m"),
            name: Atom::new_inline("orig"),
            alias: Some(Atom::new_inline("renamed")),
        });
        let text = prog.dump_text();
        assert!(text.contains("as renamed"));
    }

    #[test]
    fn dump_program_with_export() {
        let mut prog = HirProgram::new(ModuleId::from_raw(0));
        prog.exports.push(HirExport {
            name: Atom::new_inline("hello"),
            alias: None,
        });
        let text = prog.dump_text();
        assert!(text.contains("export"));
        assert!(text.contains("hello"));
    }

    #[test]
    fn dump_program_with_export_with_alias() {
        let mut prog = HirProgram::new(ModuleId::from_raw(0));
        prog.exports.push(HirExport {
            name: Atom::new_inline("inner"),
            alias: Some(Atom::new_inline("outer")),
        });
        let text = prog.dump_text();
        assert!(text.contains("as outer"));
    }

    #[test]
    fn dump_program_with_function_decl() {
        let mut prog = HirProgram::new(ModuleId::from_raw(0));
        prog.push_decl(HirDecl::Function(empty_func("alpha")));
        let text = prog.dump_text();
        assert!(text.contains("fn alpha"));
    }

    #[test]
    fn dump_program_with_class_decl() {
        let mut prog = HirProgram::new(ModuleId::from_raw(0));
        prog.push_decl(HirDecl::Class(HirClass {
            name: Atom::new_inline("Beta"),
            ty: TypeId::from_raw(0),
            fields: vec![],
            methods: vec![],
            extends: None,
            type_params: vec![],
        }));
        let text = prog.dump_text();
        assert!(text.contains("class Beta"));
    }

    #[test]
    fn dump_program_with_global_decl() {
        let mut prog = HirProgram::new(ModuleId::from_raw(0));
        prog.push_decl(HirDecl::Global {
            name: Atom::new_inline("g1"),
            ty: TypeId::from_raw(0),
            init: None,
        });
        let text = prog.dump_text();
        assert!(text.contains("global g1"));
    }

    #[test]
    fn dump_program_with_type_alias() {
        let mut prog = HirProgram::new(ModuleId::from_raw(0));
        prog.push_decl(HirDecl::TypeAlias {
            name: Atom::new_inline("MyT"),
            target: TypeId::from_raw(7),
        });
        let text = prog.dump_text();
        assert!(text.contains("typealias MyT = 7"));
    }

    #[test]
    fn dump_program_with_interface() {
        let mut prog = HirProgram::new(ModuleId::from_raw(0));
        prog.push_decl(HirDecl::Interface {
            name: Atom::new_inline("IFace"),
        });
        let text = prog.dump_text();
        assert!(text.contains("interface IFace"));
    }

    #[test]
    fn dump_program_with_enum() {
        use crate::decl::HirEnumVariant;
        use crate::expr::HirExpr;
        let mut prog = HirProgram::new(ModuleId::from_raw(0));
        prog.push_decl(HirDecl::Enum {
            name: Atom::new_inline("Color"),
            variants: vec![
                HirEnumVariant {
                    name: Atom::new_inline("Red"),
                    value: None,
                },
                HirEnumVariant {
                    name: Atom::new_inline("Green"),
                    value: Some(HirExpr::Int(0)),
                },
            ],
        });
        let text = prog.dump_text();
        assert!(text.contains("enum Color"));
        assert!(text.contains("Red"));
        assert!(text.contains("Green"));
    }

    #[test]
    fn dump_program_with_namespace_recurses_into_members() {
        let mut prog = HirProgram::new(ModuleId::from_raw(0));
        prog.push_decl(HirDecl::Namespace {
            name: Atom::new_inline("NS"),
            members: vec![HirDecl::Interface {
                name: Atom::new_inline("Nested"),
            }],
        });
        let text = prog.dump_text();
        assert!(text.contains("namespace NS"));
        assert!(text.contains("Nested"));
    }

    #[test]
    fn dump_program_with_class_fields_and_extends() {
        let mut prog = HirProgram::new(ModuleId::from_raw(0));
        prog.push_decl(HirDecl::Class(HirClass {
            name: Atom::new_inline("Child"),
            ty: TypeId::from_raw(0),
            fields: vec![HirField {
                name: Atom::new_inline("x"),
                ty: TypeId::from_raw(3),
            }],
            methods: vec![empty_func("m")],
            extends: Some(Atom::new_inline("Parent")),
            type_params: vec![],
        }));
        let text = prog.dump_text();
        assert!(text.contains("class Child extends Parent"));
        assert!(text.contains("x: 3"));
        assert!(text.contains("method m"));
    }

    #[test]
    fn dump_sym_returns_atom_string() {
        let atom = Atom::from("helloworld");
        let d = Dumper::new();
        assert_eq!(dump_sym(&atom, &d), "helloworld");
    }

    #[test]
    fn dump_indent_push_and_pop() {
        let mut d = Dumper::new();
        d.line("a");
        d.push();
        d.line("b");
        d.push();
        d.line("c");
        d.pop();
        d.line("d");
        d.pop();
        d.line("e");
        let expected = "a\n  b\n    c\n  d\ne\n";
        assert_eq!(d.buf, expected);
    }

    #[test]
    fn dump_write_no_indent() {
        let mut d = Dumper::new();
        d.push();
        d.write("noindent");
        assert_eq!(d.buf, "noindent");
    }

    #[test]
    fn dump_showcase_full_program() {
        use crate::decl::HirDecl;
        use crate::decl::HirField;
        use crate::decl::HirFunction;
        use crate::decl::HirParam;
        use crate::expr::HirExpr;
        use crate::stmt::HirStmt;
        let mut f = HirFunction {
            name: Atom::new_inline("render"),
            params: vec![HirParam {
                name: Atom::new_inline("name"),
                ty: TypeId::from_raw(1),
            }],
            ret: TypeId::from_raw(2),
            throws: None,
            body: vec![HirStmt::ret(Some(HirExpr::Binary {
                op: crate::expr::HirBinaryOp::Add,
                lhs: Box::new(HirExpr::String(Atom::new_inline("Hello, "))),
                rhs: Box::new(HirExpr::Local {
                    id: ts_aot_core::LocalId::from_raw(0),
                    ty: TypeId::from_raw(1),
                }),
                ty: TypeId::from_raw(2),
            }))],
            is_async: false,
            is_generator: false,
            is_exported: true,
            type_params: Vec::new(),
            async_info: None,
        };
        f.name = Atom::new_inline("render");
        let class = HirDecl::Class(HirClass {
            name: Atom::new_inline("Page"),
            ty: TypeId::from_raw(3),
            fields: vec![HirField {
                name: Atom::new_inline("title"),
                ty: TypeId::from_raw(1),
            }],
            methods: Vec::new(),
            extends: None,
            type_params: Vec::new(),
        });
        let mut prog = HirProgram::new(ModuleId::from_raw(0));
        prog.imports.push(HirImport {
            module: Atom::new_inline("./util"),
            name: Atom::new_inline("escape"),
            alias: None,
        });
        prog.exports.push(HirExport {
            name: Atom::new_inline("render"),
            alias: None,
        });
        prog.push_decl(HirDecl::Function(f));
        prog.push_decl(class);
        let text = prog.dump_text();
        assert!(text.contains("HirProgram(module=0)"));
        assert!(text.contains("import { escape } from \"./util\""));
        assert!(text.contains("export { render }"));
        assert!(text.contains("fn render"));
        assert!(text.contains("exported"));
        assert!(text.contains("Hello,"));
        assert!(text.contains("class Page"));
        assert!(text.contains("field title: 1"));
    }
}
