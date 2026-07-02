mod decl;
mod expr;

use std::fmt;

use ts_aot_core::Atom;

use crate::program::{MirExport, MirImport, MirProgram};
use crate::runtime::RuntimeRequirements;

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

impl MirProgram {
    pub fn dump_text(&self) -> String {
        let mut d = Dumper::new();
        dump_program(self, &mut d);
        d.buf
    }
}

fn dump_program(prog: &MirProgram, d: &mut Dumper) {
    d.line(&format!("MirProgram(module={}) {{", prog.module.raw()));
    d.push();
    d.line("imports: [");
    d.push();
    for imp in &prog.imports {
        d.write("  ");
        dump_import(imp, d);
    }
    if prog.imports.is_empty() {
        d.line("");
    }
    d.pop();
    d.line("]");
    d.line("exports: [");
    d.push();
    for exp in &prog.exports {
        d.write("  ");
        dump_export(exp, d);
    }
    if prog.exports.is_empty() {
        d.line("");
    }
    d.pop();
    d.line("]");
    d.line("declarations: [");
    d.push();
    for decl in &prog.declarations {
        decl::dump_decl(decl, d);
    }
    d.pop();
    d.line("]");
    d.pop();
    d.line("}");
}

fn dump_import(imp: &MirImport, d: &mut Dumper) {
    d.write(&format!(
        "import {} from {:?}",
        dump_sym(&imp.symbol, d),
        imp.module,
    ));
    if let Some(alias) = &imp.alias {
        d.write(&format!(" as {}", dump_sym(alias, d)));
    }
    d.write("\n");
}

fn dump_export(exp: &MirExport, d: &mut Dumper) {
    d.write(&format!("export {}", dump_sym(&exp.symbol, d)));
    if let Some(alias) = &exp.alias {
        d.write(&format!(" as {}", dump_sym(alias, d)));
    }
    d.write("\n");
}

pub(crate) fn dump_sym(id: &Atom, _d: &Dumper) -> String {
    id.as_str().to_owned()
}

impl fmt::Display for RuntimeRequirements {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut parts = Vec::new();
        if self.needs_runtime {
            parts.push("runtime");
        }
        if self.needs_string {
            parts.push("string");
        }
        if self.needs_array {
            parts.push("array");
        }
        if self.needs_map {
            parts.push("map");
        }
        if self.needs_result {
            parts.push("result");
        }
        if self.needs_promise {
            parts.push("promise");
        }
        if self.needs_scheduler {
            parts.push("scheduler");
        }
        if self.needs_host_io {
            parts.push("host_io");
        }
        if self.needs_console {
            parts.push("console");
        }
        if self.needs_math {
            parts.push("math");
        }
        if parts.is_empty() {
            write!(f, "RuntimeRequirements(none)")
        } else {
            write!(f, "RuntimeRequirements({})", parts.join(", "))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::body::{
        BinaryOp, MirBlock, MirBody, MirExpr, MirLocalDecl, MirPlace, MirPlaceBase, MirStmt,
        RuntimeOp, UnaryOp,
    };
    use crate::decl::{FunctionEffects, FunctionKind, MirDecl, MirFunctionDecl, MirStructDecl};
    use ts_aot_core::{Atom, FieldId, FunctionId, LocalId, ModuleId, StructId, TypeId};

    fn make_import(module: &str, symbol: u32) -> MirImport {
        MirImport {
            module: module.to_owned(),
            symbol: Atom::from(format!("sym{}", symbol)),
            alias: None,
        }
    }

    fn make_export(symbol: u32) -> MirExport {
        MirExport {
            symbol: Atom::from(format!("sym{}", symbol)),
            alias: None,
        }
    }

    fn empty_func(id: u32) -> MirFunctionDecl {
        MirFunctionDecl {
            id: FunctionId::from_raw(id),
            name: Atom::from(format!("fn{}", id)),
            export_name: None,
            params: Vec::new(),
            ret: TypeId::from_raw(0),
            throws: None,
            body: MirBody::default(),
            kind: FunctionKind::Plain,
            effects: FunctionEffects::default(),
        }
    }

    fn empty_struct(id: u32) -> MirStructDecl {
        MirStructDecl {
            id: StructId::from_raw(id),
            name: Atom::from(format!("s{}", id)),
            fields: Vec::new(),
            methods: Vec::new(),
        }
    }

    fn wrap_body(stmts: Vec<MirStmt>) -> MirBody {
        MirBody {
            locals: vec![],
            block: MirBlock { stmts },
        }
    }

    fn wrap_prog(body: MirBody) -> MirProgram {
        let mut f = empty_func(0);
        f.body = body;
        let mut prog = MirProgram::new(ModuleId::from_raw(0));
        prog.push_decl(MirDecl::Function(f));
        prog
    }

    #[test]
    fn dump_empty_program() {
        let prog = MirProgram::new(ModuleId::from_raw(0));
        let text = prog.dump_text();
        assert!(text.contains("MirProgram(module=0)"));
        assert!(text.contains("imports: ["));
        assert!(text.contains("declarations: ["));
    }

    #[test]
    fn dump_program_with_import_export() {
        let mut prog = MirProgram::new(ModuleId::from_raw(1));
        prog.push_import(make_import("./foo", 1));
        prog.push_export(make_export(2));
        let text = prog.dump_text();
        assert!(text.contains("import"));
        assert!(text.contains("export"));
        assert!(text.contains("./foo"));
    }

    #[test]
    fn dump_program_with_function() {
        let mut prog = MirProgram::new(ModuleId::from_raw(0));
        prog.push_decl(MirDecl::Function(empty_func(0)));
        let text = prog.dump_text();
        assert!(text.contains("fn"));
        assert!(text.contains("#0"));
    }

    #[test]
    fn dump_program_with_struct() {
        let mut prog = MirProgram::new(ModuleId::from_raw(0));
        prog.push_decl(MirDecl::Struct(empty_struct(0)));
        let text = prog.dump_text();
        assert!(text.contains("struct"));
    }

    #[test]
    fn dump_let_stmt() {
        let body = MirBody {
            locals: vec![MirLocalDecl {
                id: LocalId::from_raw(0),
                name: Atom::new_inline("2"),
                ty: TypeId::from_raw(1),
                mutable: false,
            }],
            block: MirBlock {
                stmts: vec![MirStmt::Let {
                    local: LocalId::from_raw(0),
                    ty: TypeId::from_raw(1),
                    init: Some(MirExpr::Int {
                        value: 42,
                        ty: TypeId::from_raw(1),
                    }),
                    mutable: false,
                }],
            },
        };
        let text = wrap_prog(body).dump_text();
        assert!(text.contains("let"));
        assert!(text.contains("42"));
    }

    #[test]
    fn dump_runtime_requirements_none() {
        let r = RuntimeRequirements::default();
        assert!(format!("{r}").contains("none"));
    }

    #[test]
    fn dump_if_stmt() {
        let stmt = MirStmt::If {
            cond: MirExpr::Bool(true),
            then_block: MirBlock::with(MirStmt::Return(Some(MirExpr::Unit))),
            else_block: None,
        };
        let text = wrap_prog(wrap_body(vec![stmt])).dump_text();
        assert!(text.contains("if"));
        assert!(text.contains("true"));
        assert!(text.contains("return"));
    }

    #[test]
    fn dump_while_stmt() {
        let stmt = MirStmt::While {
            cond: MirExpr::Bool(true),
            body: MirBlock::with(MirStmt::Break),
        };
        let text = wrap_prog(wrap_body(vec![stmt])).dump_text();
        assert!(text.contains("while"));
        assert!(text.contains("break"));
    }

    #[test]
    fn dump_for_of_stmt() {
        let stmt = MirStmt::ForOf {
            item: LocalId::from_raw(0),
            iterable: MirExpr::Unit,
            body: MirBlock::with(MirStmt::Continue),
        };
        let text = wrap_prog(wrap_body(vec![stmt])).dump_text();
        assert!(text.contains("for"));
        assert!(text.contains("continue"));
    }

    #[test]
    fn dump_runtime_stmt() {
        let stmt = MirStmt::Runtime {
            op: RuntimeOp::StringConcat,
            args: vec![MirExpr::Unit, MirExpr::Unit],
            dest: None,
            ty: TypeId::from_raw(0),
        };
        let text = wrap_prog(wrap_body(vec![stmt])).dump_text();
        assert!(text.contains("string_concat"));
        assert!(text.contains("runtime"));
    }

    #[test]
    fn dump_return_result_err() {
        let stmt = MirStmt::ReturnResultErr {
            error: MirExpr::Unit,
            err_ty: TypeId::from_raw(1),
        };
        let text = wrap_prog(wrap_body(vec![stmt])).dump_text();
        assert!(text.contains("return_result_err"));
    }

    #[test]
    fn dump_throw_stmt() {
        let stmt = MirStmt::Throw {
            error: MirExpr::Unit,
            error_ty: TypeId::from_raw(1),
        };
        let text = wrap_prog(wrap_body(vec![stmt])).dump_text();
        assert!(text.contains("throw"));
    }

    #[test]
    fn dump_runtime_requirements_with_ops() {
        let mut r = RuntimeRequirements::default();
        r.require(RuntimeOp::StringConcat);
        r.require(RuntimeOp::ArrayCreate);
        let text = format!("{r}");
        assert!(text.contains("string"));
        assert!(text.contains("array"));
        assert!(text.contains("runtime"));
    }

    #[test]
    fn dump_unary_neg() {
        let stmt = MirStmt::Return(Some(MirExpr::Unary {
            op: UnaryOp::Neg,
            expr: Box::new(MirExpr::Int {
                value: 5,
                ty: TypeId::from_raw(0),
            }),
            ty: TypeId::from_raw(7),
        }));
        let text = wrap_prog(wrap_body(vec![stmt])).dump_text();
        assert!(text.contains("return -(5(:0)):7"));
    }

    #[test]
    fn dump_unary_not() {
        let stmt = MirStmt::Return(Some(MirExpr::Unary {
            op: UnaryOp::Not,
            expr: Box::new(MirExpr::Bool(false)),
            ty: TypeId::from_raw(3),
        }));
        let text = wrap_prog(wrap_body(vec![stmt])).dump_text();
        assert!(text.contains("return !(false):3"));
    }

    #[test]
    fn dump_unary_bitnot() {
        let stmt = MirStmt::Return(Some(MirExpr::Unary {
            op: UnaryOp::BitNot,
            expr: Box::new(MirExpr::Int {
                value: 1,
                ty: TypeId::from_raw(0),
            }),
            ty: TypeId::from_raw(2),
        }));
        let text = wrap_prog(wrap_body(vec![stmt])).dump_text();
        assert!(text.contains("return ~(1(:0)):2"));
    }

    #[test]
    fn dump_binary_add() {
        let stmt = MirStmt::Return(Some(MirExpr::Binary {
            op: BinaryOp::Add,
            left: Box::new(MirExpr::Int {
                value: 1,
                ty: TypeId::from_raw(0),
            }),
            right: Box::new(MirExpr::Int {
                value: 2,
                ty: TypeId::from_raw(0),
            }),
            ty: TypeId::from_raw(9),
        }));
        let text = wrap_prog(wrap_body(vec![stmt])).dump_text();
        assert!(text.contains("return (1(:0) + 2(:0)):9"));
    }

    #[test]
    fn dump_expr_field() {
        let stmt = MirStmt::Return(Some(MirExpr::Field {
            base: Box::new(MirExpr::Local(LocalId::from_raw(0))),
            field: FieldId::from_raw(2),
            ty: TypeId::from_raw(7),
        }));
        let text = wrap_prog(wrap_body(vec![stmt])).dump_text();
        assert!(text.contains("return local(0).2:7"));
    }

    #[test]
    fn dump_expr_index() {
        let stmt = MirStmt::Return(Some(MirExpr::Index {
            base: Box::new(MirExpr::Local(LocalId::from_raw(0))),
            index: Box::new(MirExpr::Int {
                value: 3,
                ty: TypeId::from_raw(0),
            }),
            ty: TypeId::from_raw(4),
        }));
        let text = wrap_prog(wrap_body(vec![stmt])).dump_text();
        assert!(text.contains("return local(0)[3(:0)]:4"));
    }

    #[test]
    fn dump_expr_call() {
        let stmt = MirStmt::Return(Some(MirExpr::Call {
            callee: FunctionId::from_raw(5),
            args: vec![
                MirExpr::Int {
                    value: 1,
                    ty: TypeId::from_raw(0),
                },
                MirExpr::Bool(true),
            ],
            ty: TypeId::from_raw(8),
        }));
        let text = wrap_prog(wrap_body(vec![stmt])).dump_text();
        assert!(text.contains("return call fn(5)(1(:0), true):8"));
    }

    #[test]
    fn dump_expr_struct_literal() {
        let stmt = MirStmt::Return(Some(MirExpr::StructLiteral {
            struct_id: StructId::from_raw(3),
            fields: vec![(
                FieldId::from_raw(0),
                MirExpr::Int {
                    value: 7,
                    ty: TypeId::from_raw(0),
                },
            )],
            ty: TypeId::from_raw(6),
        }));
        let text = wrap_prog(wrap_body(vec![stmt])).dump_text();
        assert!(text.contains("return struct(3){0:7(:0)}:6"));
    }

    #[test]
    fn dump_expr_result_ok() {
        let stmt = MirStmt::Return(Some(MirExpr::ResultOk {
            value: Box::new(MirExpr::Int {
                value: 1,
                ty: TypeId::from_raw(0),
            }),
            ty: TypeId::from_raw(5),
        }));
        let text = wrap_prog(wrap_body(vec![stmt])).dump_text();
        assert!(text.contains("return ok(1(:0)):5"));
    }

    #[test]
    fn dump_expr_result_err() {
        let stmt = MirStmt::Return(Some(MirExpr::ResultErr {
            error: Box::new(MirExpr::Unit),
            ty: TypeId::from_raw(5),
        }));
        let text = wrap_prog(wrap_body(vec![stmt])).dump_text();
        assert!(text.contains("return err(()):5"));
    }

    #[test]
    fn dump_expr_string() {
        let stmt = MirStmt::Return(Some(MirExpr::String {
            id: ts_aot_core::Atom::new_inline("hello"),
            ty: TypeId::from_raw(2),
        }));
        let text = wrap_prog(wrap_body(vec![stmt])).dump_text();
        assert!(text.contains("return string(\"hello\")(:2)"));
    }

    #[test]
    fn dump_expr_null() {
        let stmt = MirStmt::Return(Some(MirExpr::Null {
            ty: TypeId::from_raw(2),
        }));
        let text = wrap_prog(wrap_body(vec![stmt])).dump_text();
        assert!(text.contains("return null(:2)"));
    }

    #[test]
    fn dump_expr_global() {
        let stmt = MirStmt::Return(Some(MirExpr::Global(Atom::new_inline("myglobal"))));
        let text = wrap_prog(wrap_body(vec![stmt])).dump_text();
        assert!(text.contains("return myglobal"));
    }

    #[test]
    fn dump_expr_float() {
        let stmt = MirStmt::Return(Some(MirExpr::Float {
            value: 1.5,
            ty: TypeId::from_raw(2),
        }));
        let text = wrap_prog(wrap_body(vec![stmt])).dump_text();
        assert!(text.contains("1.5(:2)"));
    }

    #[test]
    fn dump_expr_int() {
        let stmt = MirStmt::Return(Some(MirExpr::Int {
            value: 42,
            ty: TypeId::from_raw(2),
        }));
        let text = wrap_prog(wrap_body(vec![stmt])).dump_text();
        assert!(text.contains("42(:2)"));
    }

    #[test]
    fn dump_expr_bool() {
        let stmt = MirStmt::Return(Some(MirExpr::Bool(false)));
        let text = wrap_prog(wrap_body(vec![stmt])).dump_text();
        assert!(text.contains("return false"));
    }

    #[test]
    fn dump_expr_unit() {
        let stmt = MirStmt::Return(Some(MirExpr::Unit));
        let text = wrap_prog(wrap_body(vec![stmt])).dump_text();
        assert!(text.contains("return ()"));
    }

    #[test]
    fn dump_expr_local() {
        let stmt = MirStmt::Return(Some(MirExpr::Local(LocalId::from_raw(3))));
        let text = wrap_prog(wrap_body(vec![stmt])).dump_text();
        assert!(text.contains("return local(3)"));
    }

    #[test]
    fn dump_stmt_assign_to_local() {
        let stmt = MirStmt::Assign {
            target: MirPlace::Local {
                id: LocalId::from_raw(0),
            },
            value: MirExpr::Int {
                value: 9,
                ty: TypeId::from_raw(0),
            },
        };
        let text = wrap_prog(wrap_body(vec![stmt])).dump_text();
        assert!(text.contains("assign local(0) = 9(:0)"));
    }

    #[test]
    fn dump_stmt_assign_to_field() {
        let stmt = MirStmt::Assign {
            target: MirPlace::Field {
                base: Box::new(MirPlaceBase::Local(LocalId::from_raw(0))),
                field: FieldId::from_raw(1),
                ty: TypeId::from_raw(7),
            },
            value: MirExpr::Int {
                value: 9,
                ty: TypeId::from_raw(0),
            },
        };
        let text = wrap_prog(wrap_body(vec![stmt])).dump_text();
        assert!(text.contains("assign field(local(0).1:7) = 9(:0)"));
    }

    #[test]
    fn dump_stmt_assign_to_index() {
        let stmt = MirStmt::Assign {
            target: MirPlace::Index {
                base: Box::new(MirExpr::Local(LocalId::from_raw(0))),
                index: Box::new(MirExpr::Int {
                    value: 1,
                    ty: TypeId::from_raw(0),
                }),
                ty: TypeId::from_raw(7),
            },
            value: MirExpr::Int {
                value: 4,
                ty: TypeId::from_raw(0),
            },
        };
        let text = wrap_prog(wrap_body(vec![stmt])).dump_text();
        assert!(text.contains("assign index(local(0)[1(:0)]:7) = 4(:0)"));
    }

    #[test]
    fn dump_stmt_return_unit() {
        let text = wrap_prog(wrap_body(vec![MirStmt::Return(None)])).dump_text();
        assert!(text.contains("return\n"));
    }

    #[test]
    fn dump_stmt_expr_stmt() {
        let stmt = MirStmt::Expr(MirExpr::Int {
            value: 11,
            ty: TypeId::from_raw(0),
        });
        let text = wrap_prog(wrap_body(vec![stmt])).dump_text();
        assert!(text.contains("expr 11(:0)"));
    }

    #[test]
    fn dump_program_with_global() {
        let mut prog = MirProgram::new(ModuleId::from_raw(0));
        prog.push_decl(MirDecl::Global(crate::decl::MirGlobalDecl {
            name: Atom::new_inline("myglobal"),
            ty: TypeId::from_raw(0),
            mutable: true,
            visibility: ts_aot_core::Visibility::Public,
            export_name: None,
            init: None,
        }));
        let text = prog.dump_text();
        assert!(text.contains("global"));
        assert!(text.contains("myglobal"));
        assert!(text.contains("mut: true"));
    }
}
