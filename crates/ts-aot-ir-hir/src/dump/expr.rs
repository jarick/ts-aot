use super::Dumper;

use crate::expr::{HirBinaryOp, HirCallee, HirExpr, HirUnaryOp, ObjectLiteralField};
use crate::stmt::{HirCatchClause, HirStmt, HirSwitchCase};

pub(crate) fn dump_body(stmts: &[HirStmt], d: &mut Dumper) {
    for stmt in stmts {
        dump_stmt(stmt, d);
    }
}

pub(crate) fn dump_stmt(stmt: &HirStmt, d: &mut Dumper) {
    match stmt {
        HirStmt::Block(inner) => {
            d.line("{");
            d.push();
            for s in inner {
                dump_stmt(s, d);
            }
            d.pop();
            d.line("}");
        }
        HirStmt::Let { id, name, ty, init } => {
            d.write(&format!(
                "let {} (id={}): {}",
                name.as_str(),
                id.raw(),
                ty.raw()
            ));
            if let Some(init) = init {
                d.write(" = ");
                dump_expr_inline(init, d);
            }
            d.write("\n");
        }
        HirStmt::Expr { expr } => {
            d.write("expr ");
            dump_expr_inline(expr, d);
            d.write("\n");
        }
        HirStmt::If {
            cond,
            then,
            otherwise,
        } => {
            d.write("if (");
            dump_expr_inline(cond, d);
            d.write(") {\n");
            d.push();
            dump_stmt(then, d);
            d.pop();
            d.line("}");
            if let Some(otherwise) = otherwise {
                d.line("else {");
                d.push();
                dump_stmt(otherwise, d);
                d.pop();
                d.line("}");
            }
        }
        HirStmt::While { cond, body } => {
            d.write("while (");
            dump_expr_inline(cond, d);
            d.write(") {\n");
            d.push();
            dump_stmt(body, d);
            d.pop();
            d.line("}");
        }
        HirStmt::DoWhile { body, cond } => {
            d.line("do {");
            d.push();
            dump_stmt(body, d);
            d.pop();
            d.write("} while (");
            dump_expr_inline(cond, d);
            d.write(")\n");
        }
        HirStmt::ForOf {
            binding,
            iter,
            body,
        } => {
            d.write(&format!("for_of (id={}) in ", binding.raw()));
            dump_expr_inline(iter, d);
            d.write(" {\n");
            d.push();
            dump_stmt(body, d);
            d.pop();
            d.line("}");
        }
        HirStmt::ForIn {
            binding,
            iter,
            body,
        } => {
            d.write(&format!("for_in (id={}) in ", binding.raw()));
            dump_expr_inline(iter, d);
            d.write(" {\n");
            d.push();
            dump_stmt(body, d);
            d.pop();
            d.line("}");
        }
        HirStmt::Switch { disc, cases } => {
            d.write("switch (");
            dump_expr_inline(disc, d);
            d.write(") {\n");
            d.push();
            for c in cases {
                dump_switch_case(c, d);
            }
            d.pop();
            d.line("}");
        }
        HirStmt::Return { value: None } => d.line("return"),
        HirStmt::Return { value: Some(v) } => {
            d.write("return ");
            dump_expr_inline(v, d);
            d.write("\n");
        }
        HirStmt::Throw { expr } => {
            d.write("throw ");
            dump_expr_inline(expr, d);
            d.write("\n");
        }
        HirStmt::Break { label: None } => d.line("break"),
        HirStmt::Break { label: Some(l) } => d.line(&format!("break {}", l.as_str())),
        HirStmt::Continue { label: None } => d.line("continue"),
        HirStmt::Continue { label: Some(l) } => d.line(&format!("continue {}", l.as_str())),
        HirStmt::Try {
            body,
            catch,
            finally,
        } => {
            d.line("try {");
            d.push();
            dump_stmt(body, d);
            d.pop();
            d.line("}");
            if let Some(c) = catch {
                dump_catch(c, d);
            }
            if let Some(finally) = finally {
                d.line("finally {");
                d.push();
                dump_stmt(finally, d);
                d.pop();
                d.line("}");
            }
        }
        HirStmt::Decl(decl) => {
            d.write("decl ");
            super::decl::dump_decl(decl, d);
        }
    }
}

fn dump_switch_case(case: &HirSwitchCase, d: &mut Dumper) {
    match &case.test {
        Some(t) => {
            d.write("case ");
            dump_expr_inline(t, d);
            d.write(":\n");
        }
        None => d.line("default:"),
    }
    d.push();
    for s in &case.body {
        dump_stmt(s, d);
    }
    d.pop();
}

fn dump_catch(catch: &HirCatchClause, d: &mut Dumper) {
    match &catch.binding {
        Some((id, name)) => d.write(&format!(
            "catch (id={}, name={}) {{\n",
            id.raw(),
            name.as_str()
        )),
        None => d.write("catch {\n"),
    }
    d.push();
    dump_stmt(&catch.body, d);
    d.pop();
    d.line("}");
}

pub(crate) fn dump_expr_inline(expr: &HirExpr, d: &mut Dumper) {
    match expr {
        HirExpr::Unit => d.write("()"),
        HirExpr::Bool(v) => d.write(if *v { "true" } else { "false" }),
        HirExpr::Int(v) => d.write(&format!("{v}")),
        HirExpr::Float(bits) => d.write(&format!("float({bits})")),
        HirExpr::String(atom) => d.write(&format!("string({:?})", atom.as_str())),
        HirExpr::Null => d.write("null"),
        HirExpr::Undefined => d.write("undefined"),
        HirExpr::Local { id, ty } => d.write(&format!("local({}):{}", id.raw(), ty.raw())),
        HirExpr::Global { name, ty } => d.write(&format!("global({}):{}", name.as_str(), ty.raw())),
        HirExpr::Field {
            owner,
            field,
            field_name,
            ty,
        } => {
            dump_expr_inline(owner, d);
            d.write(&format!(
                ".{} (id={:?}):{}",
                field_name.as_str(),
                field.raw(),
                ty.raw()
            ));
        }
        HirExpr::Index { owner, index, ty } => {
            dump_expr_inline(owner, d);
            d.write("[");
            dump_expr_inline(index, d);
            d.write(&format!("]:{}", ty.raw()));
        }
        HirExpr::Call { callee, args, ty } => {
            dump_callee(callee, d);
            d.write("(");
            for (i, arg) in args.iter().enumerate() {
                if i > 0 {
                    d.write(", ");
                }
                dump_expr_inline(arg, d);
            }
            d.write(&format!("):{}", ty.raw()));
        }
        HirExpr::Binary { op, lhs, rhs, ty } => {
            d.write("(");
            dump_expr_inline(lhs, d);
            d.write(&format!(" {} ", fmt_bin_op(*op)));
            dump_expr_inline(rhs, d);
            d.write(&format!("):{}", ty.raw()));
        }
        HirExpr::Unary { op, expr, ty } => {
            d.write(&format!("{}(", fmt_un_op(*op)));
            dump_expr_inline(expr, d);
            d.write(&format!("):{}", ty.raw()));
        }
        HirExpr::StructLiteral { ty, fields } => {
            d.write("struct{");
            for (i, (fid, val)) in fields.iter().enumerate() {
                if i > 0 {
                    d.write(", ");
                }
                d.write(&format!("{}:", fid.raw()));
                dump_expr_inline(val, d);
            }
            d.write(&format!("}}:{}", ty.raw()));
        }
        HirExpr::ObjectLiteral { fields, ty } => {
            d.write("{");
            for (i, field) in fields.iter().enumerate() {
                if i > 0 {
                    d.write(", ");
                }
                match field {
                    ObjectLiteralField::Property { name, value } => {
                        d.write(&format!("{}:", name.as_str()));
                        dump_expr_inline(value, d);
                    }
                    ObjectLiteralField::Spread(value) => {
                        d.write("...");
                        dump_expr_inline(value, d);
                    }
                }
            }
            d.write(&format!("}}:{{{}}}", ty.raw()));
        }
        HirExpr::Ternary {
            cond,
            then_branch,
            else_branch,
            ty,
        } => {
            d.write("ternary(");
            dump_expr_inline(cond, d);
            d.write(" ? ");
            dump_expr_inline(then_branch, d);
            d.write(" : ");
            dump_expr_inline(else_branch, d);
            d.write(&format!("):{}", ty.raw()));
        }
        HirExpr::ArrayLiteral { elements, ty } => {
            d.write("[");
            for (i, e) in elements.iter().enumerate() {
                if i > 0 {
                    d.write(", ");
                }
                dump_expr_inline(e, d);
            }
            d.write(&format!("]:{}", ty.raw()));
        }
        HirExpr::Closure {
            id,
            params,
            captures,
            body,
            ty,
        } => {
            d.write(&format!("closure(id={})[", id.raw()));
            for (i, p) in params.iter().enumerate() {
                if i > 0 {
                    d.write(", ");
                }
                d.write(&format!("{}:{}", p.name.as_str(), p.ty.raw()));
            }
            d.write("]captures=[");
            for (i, c) in captures.iter().enumerate() {
                if i > 0 {
                    d.write(", ");
                }
                dump_expr_inline(c, d);
            }
            d.write("]body=");
            dump_inline_body(body, d);
            d.write(&format!(":{}", ty.raw()));
        }
        HirExpr::Await { expr, ty } => {
            d.write("await(");
            dump_expr_inline(expr, d);
            d.write(&format!("):{}", ty.raw()));
        }
        HirExpr::Yield { expr, ty } => {
            d.write("yield(");
            if let Some(e) = expr {
                dump_expr_inline(e, d);
            } else {
                d.write("()");
            }
            d.write(&format!("):{}", ty.raw()));
        }
        HirExpr::Template {
            tag,
            expressions,
            cooked_parts,
            raw_parts,
            ty,
        } => {
            match tag {
                Some(t) => {
                    d.write("template(tag=");
                    dump_expr_inline(t, d);
                    d.write(")[");
                }
                None => d.write("template["),
            }
            for (i, e) in expressions.iter().enumerate() {
                if i > 0 {
                    d.write(", ");
                }
                if let Some(c) = cooked_parts.get(i) {
                    match c {
                        Some(s) => d.write(&format!("cooked({:?})", s.as_str())),
                        None => d.write("cooked(None)"),
                    }
                }
                if let Some(r) = raw_parts.get(i) {
                    match r {
                        Some(s) => d.write(&format!(",raw({:?})", s.as_str())),
                        None => d.write(",raw(None)"),
                    }
                }
                d.write(",");
                dump_expr_inline(e, d);
            }
            let last_idx = expressions.len();
            if let Some(c) = cooked_parts.get(last_idx) {
                d.write(",");
                match c {
                    Some(s) => d.write(&format!("cooked({:?})", s.as_str())),
                    None => d.write("cooked(None)"),
                }
            }
            if let Some(r) = raw_parts.get(last_idx) {
                match r {
                    Some(s) => d.write(&format!(",raw({:?})", s.as_str())),
                    None => d.write(",raw(None)"),
                }
            }
            d.write(&format!("]:{}", ty.raw()));
        }
        HirExpr::New { callee, args, ty } => {
            d.write("new ");
            dump_expr_inline(callee, d);
            d.write("(");
            for (i, arg) in args.iter().enumerate() {
                if i > 0 {
                    d.write(", ");
                }
                dump_expr_inline(arg, d);
            }
            d.write(&format!("):{}", ty.raw()));
        }
        HirExpr::OptionalChain { base, ty } => {
            d.write("opt(");
            dump_expr_inline(base, d);
            d.write(&format!("):{}", ty.raw()));
        }
        HirExpr::TypeAssertion { expr, target } => {
            d.write("assert<");
            d.write(&format!("{}>(", target.raw()));
            dump_expr_inline(expr, d);
            d.write(")");
        }
        HirExpr::Assignment { target, value, ty } => {
            d.write("assign(");
            dump_expr_inline(target, d);
            d.write(" = ");
            dump_expr_inline(value, d);
            d.write(&format!("):{}", ty.raw()));
        }
        HirExpr::CompoundUpdate {
            target,
            op,
            rhs,
            post,
            ty,
        } => {
            d.write("compound_update(");
            dump_expr_inline(target, d);
            d.write(&format!(" {} ", fmt_bin_op(*op)));
            dump_expr_inline(rhs, d);
            d.write(&format!(", post={}):{}", post, ty.raw()));
        }
        HirExpr::Sequence { exprs, ty } => {
            d.write("seq(");
            for (i, e) in exprs.iter().enumerate() {
                if i > 0 {
                    d.write(", ");
                }
                dump_expr_inline(e, d);
            }
            d.write(&format!("):{}", ty.raw()));
        }
        HirExpr::RegExp { pattern, flags, ty } => d.write(&format!(
            "regexp({:?}, {:?}):{}",
            pattern.as_str(),
            flags.as_str(),
            ty.raw()
        )),
        HirExpr::BigInt { value, ty } => {
            d.write(&format!("bigint({:?}):{}", value.as_str(), ty.raw()))
        }
    }
}

fn dump_callee(callee: &HirCallee, d: &mut Dumper) {
    match callee {
        HirCallee::Function(fid) => d.write(&format!("call_fn({})", fid.raw())),
        HirCallee::Indirect(expr) => dump_expr_inline(expr, d),
        HirCallee::Closure(id) => d.write(&format!("closure({})", id.raw())),
        HirCallee::Runtime { name, ty } => {
            d.write(&format!("runtime({:?}):{}", name.as_str(), ty.raw()));
        }
    }
}

fn dump_inline_body(stmts: &[HirStmt], d: &mut Dumper) {
    d.write("{");
    d.push();
    for s in stmts {
        dump_stmt(s, d);
    }
    d.pop();
    d.write("}");
}

fn fmt_bin_op(op: HirBinaryOp) -> &'static str {
    match op {
        HirBinaryOp::Add => "+",
        HirBinaryOp::Sub => "-",
        HirBinaryOp::Mul => "*",
        HirBinaryOp::Div => "/",
        HirBinaryOp::Mod => "%",
        HirBinaryOp::Eq => "==",
        HirBinaryOp::Ne => "!=",
        HirBinaryOp::Lt => "<",
        HirBinaryOp::Le => "<=",
        HirBinaryOp::Gt => ">",
        HirBinaryOp::Ge => ">=",
        HirBinaryOp::And => "&&",
        HirBinaryOp::Or => "||",
        HirBinaryOp::BitAnd => "&",
        HirBinaryOp::BitOr => "|",
        HirBinaryOp::BitXor => "^",
        HirBinaryOp::Shl => "<<",
        HirBinaryOp::Shr => ">>",
        HirBinaryOp::Usr => ">>>",
        HirBinaryOp::In => "in",
        HirBinaryOp::InstanceOf => "instanceof",
    }
}

fn fmt_un_op(op: HirUnaryOp) -> &'static str {
    match op {
        HirUnaryOp::Neg => "-",
        HirUnaryOp::Not => "!",
        HirUnaryOp::BitNot => "~",
        HirUnaryOp::TypeOf => "typeof",
        HirUnaryOp::Void => "void",
        HirUnaryOp::Delete => "delete",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decl::{HirDecl, HirFunction};
    use crate::program::HirProgram;
    use ts_aot_core::{Atom, FieldId, FunctionId, LocalId, ModuleId, TypeId};

    fn wrap(stmts: Vec<HirStmt>) -> HirStmt {
        HirStmt::Block(stmts)
    }

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

    fn dump(stmts: Vec<HirStmt>) -> String {
        let mut d = Dumper::new();
        dump_body(&stmts, &mut d);
        d.buf
    }

    #[test]
    fn dump_block_stmt() {
        let stmts = wrap(vec![HirStmt::ret(None)]);
        let text = dump(vec![stmts]);
        assert!(text.contains("{"));
        assert!(text.contains("return"));
        assert!(text.contains("}"));
    }

    #[test]
    fn dump_let_stmt_without_init() {
        let s = HirStmt::let_(
            LocalId::from_raw(0),
            Atom::new_inline("x"),
            TypeId::from_raw(3),
            None,
        );
        let text = dump(vec![s]);
        assert_eq!(text, "let x (id=0): 3\n");
    }

    #[test]
    fn dump_let_stmt_with_init() {
        let s = HirStmt::let_(
            LocalId::from_raw(2),
            Atom::new_inline("y"),
            TypeId::from_raw(3),
            Some(HirExpr::Int(42)),
        );
        let text = dump(vec![s]);
        assert!(text.contains("let y (id=2): 3 = 42"));
    }

    #[test]
    fn dump_expr_stmt() {
        let text = dump(vec![HirStmt::expr(HirExpr::Int(7))]);
        assert!(text.contains("expr 7"));
    }

    #[test]
    fn dump_if_stmt_with_else() {
        let s = HirStmt::If {
            cond: HirExpr::Bool(true),
            then: Box::new(HirStmt::ret(None)),
            otherwise: Some(Box::new(HirStmt::ret(Some(HirExpr::Int(0))))),
        };
        let text = dump(vec![s]);
        assert!(text.contains("if (true)"));
        assert!(text.contains("else"));
        assert!(text.contains("return 0"));
    }

    #[test]
    fn dump_if_stmt_without_else() {
        let s = HirStmt::If {
            cond: HirExpr::Bool(false),
            then: Box::new(HirStmt::ret(None)),
            otherwise: None,
        };
        let text = dump(vec![s]);
        assert!(!text.contains("else"));
    }

    #[test]
    fn dump_while_stmt() {
        let s = HirStmt::While {
            cond: HirExpr::Bool(true),
            body: Box::new(HirStmt::expr(HirExpr::Unit)),
        };
        let text = dump(vec![s]);
        assert!(text.contains("while (true)"));
    }

    #[test]
    fn dump_dowhile_stmt() {
        let s = HirStmt::DoWhile {
            body: Box::new(HirStmt::ret(None)),
            cond: HirExpr::Bool(false),
        };
        let text = dump(vec![s]);
        assert!(text.contains("do {"));
        assert!(text.contains("} while (false)"));
    }

    #[test]
    fn dump_for_of_stmt() {
        let s = HirStmt::ForOf {
            binding: LocalId::from_raw(1),
            iter: HirExpr::Unit,
            body: Box::new(HirStmt::expr(HirExpr::Unit)),
        };
        let text = dump(vec![s]);
        assert!(text.contains("for_of (id=1) in"));
    }

    #[test]
    fn dump_for_in_stmt() {
        let s = HirStmt::ForIn {
            binding: LocalId::from_raw(2),
            iter: HirExpr::Unit,
            body: Box::new(HirStmt::expr(HirExpr::Unit)),
        };
        let text = dump(vec![s]);
        assert!(text.contains("for_in (id=2) in"));
    }

    #[test]
    fn dump_switch_stmt() {
        let s = HirStmt::Switch {
            disc: HirExpr::Int(0),
            cases: vec![HirSwitchCase::new(
                Some(HirExpr::Int(1)),
                vec![HirStmt::ret(None)],
            )],
        };
        let text = dump(vec![s]);
        assert!(text.contains("switch (0)"));
        assert!(text.contains("case 1:"));
    }

    #[test]
    fn dump_return_without_value() {
        let text = dump(vec![HirStmt::ret(None)]);
        assert!(text.contains("return\n") || text.ends_with("return\n"));
    }

    #[test]
    fn dump_return_with_value() {
        let text = dump(vec![HirStmt::ret(Some(HirExpr::Int(5)))]);
        assert!(text.contains("return 5"));
    }

    #[test]
    fn dump_throw_stmt() {
        let text = dump(vec![HirStmt::Throw {
            expr: HirExpr::Int(8),
        }]);
        assert!(text.contains("throw 8"));
    }

    #[test]
    fn dump_break_stmt_with_and_without_label() {
        assert!(
            dump(vec![HirStmt::Break { label: None }])
                .trim_end()
                .ends_with("break")
        );
        assert!(
            dump(vec![HirStmt::Break {
                label: Some(Atom::new_inline("L"))
            }])
            .contains("break L")
        );
    }

    #[test]
    fn dump_continue_stmt_with_and_without_label() {
        assert!(
            dump(vec![HirStmt::Continue { label: None }])
                .trim_end()
                .ends_with("continue")
        );
        assert!(
            dump(vec![HirStmt::Continue {
                label: Some(Atom::new_inline("L"))
            }])
            .contains("continue L")
        );
    }

    #[test]
    fn dump_try_with_catch_and_finally() {
        let s = HirStmt::Try {
            body: Box::new(HirStmt::ret(None)),
            catch: Some(HirCatchClause::new(
                Some((LocalId::from_raw(0), Atom::new_inline("e"))),
                Box::new(HirStmt::ret(None)),
            )),
            finally: Some(Box::new(HirStmt::ret(None))),
        };
        let text = dump(vec![s]);
        assert!(text.contains("try {"));
        assert!(text.contains("catch (id=0, name=e)"));
        assert!(text.contains("finally {"));
    }

    #[test]
    fn dump_try_without_catch_binding() {
        let s = HirStmt::Try {
            body: Box::new(HirStmt::ret(None)),
            catch: Some(HirCatchClause::new(None, Box::new(HirStmt::ret(None)))),
            finally: None,
        };
        let text = dump(vec![s]);
        assert!(text.contains("catch {"));
    }

    #[test]
    fn dump_stmt_decl_emits_decl_prefix() {
        let s = HirStmt::Decl(HirDecl::Function(empty_func("nested")));
        let text = dump(vec![s]);
        assert!(text.contains("decl"));
        assert!(text.contains("fn nested"));
    }

    #[test]
    fn dump_unit_expr() {
        let mut d = Dumper::new();
        dump_expr_inline(&HirExpr::Unit, &mut d);
        assert_eq!(d.buf, "()");
    }

    #[test]
    fn dump_bool_expr() {
        let mut d = Dumper::new();
        dump_expr_inline(&HirExpr::Bool(true), &mut d);
        assert_eq!(d.buf, "true");
        let mut d = Dumper::new();
        dump_expr_inline(&HirExpr::Bool(false), &mut d);
        assert_eq!(d.buf, "false");
    }

    #[test]
    fn dump_int_expr() {
        let mut d = Dumper::new();
        dump_expr_inline(&HirExpr::Int(123), &mut d);
        assert_eq!(d.buf, "123");
    }

    #[test]
    fn dump_float_expr() {
        let mut d = Dumper::new();
        dump_expr_inline(&HirExpr::Float(7), &mut d);
        assert!(d.buf.contains("float(7)"));
    }

    #[test]
    fn dump_string_expr() {
        let mut d = Dumper::new();
        dump_expr_inline(&HirExpr::String(Atom::new_inline("hello")), &mut d);
        assert!(d.buf.contains("string(\"hello\")"));
    }

    #[test]
    fn dump_null_and_undefined_expr() {
        let mut d = Dumper::new();
        dump_expr_inline(&HirExpr::Null, &mut d);
        assert_eq!(d.buf, "null");
        let mut d = Dumper::new();
        dump_expr_inline(&HirExpr::Undefined, &mut d);
        assert_eq!(d.buf, "undefined");
    }

    #[test]
    fn dump_local_expr() {
        let mut d = Dumper::new();
        dump_expr_inline(
            &HirExpr::Local {
                id: LocalId::from_raw(4),
                ty: TypeId::from_raw(7),
            },
            &mut d,
        );
        assert_eq!(d.buf, "local(4):7");
    }

    #[test]
    fn dump_global_expr() {
        let mut d = Dumper::new();
        dump_expr_inline(
            &HirExpr::Global {
                name: Atom::new_inline("MyG"),
                ty: TypeId::from_raw(2),
            },
            &mut d,
        );
        assert_eq!(d.buf, "global(MyG):2");
    }

    #[test]
    fn dump_field_expr() {
        let mut d = Dumper::new();
        dump_expr_inline(
            &HirExpr::Field {
                owner: Box::new(HirExpr::Local {
                    id: LocalId::from_raw(0),
                    ty: TypeId::from_raw(1),
                }),
                field: FieldId::from_raw(2),
                field_name: Atom::new_inline("name"),
                ty: TypeId::from_raw(3),
            },
            &mut d,
        );
        assert!(d.buf.contains(".name"));
        assert!(d.buf.contains(":3"));
    }

    #[test]
    fn dump_index_expr() {
        let mut d = Dumper::new();
        dump_expr_inline(
            &HirExpr::Index {
                owner: Box::new(HirExpr::Local {
                    id: LocalId::from_raw(0),
                    ty: TypeId::from_raw(1),
                }),
                index: Box::new(HirExpr::Int(2)),
                ty: TypeId::from_raw(3),
            },
            &mut d,
        );
        assert!(d.buf.contains("[2]"));
        assert!(d.buf.contains(":3"));
    }

    #[test]
    fn dump_call_expr_with_function_callee() {
        let mut d = Dumper::new();
        dump_expr_inline(
            &HirExpr::Call {
                callee: HirCallee::Function(FunctionId::from_raw(5)),
                args: vec![HirExpr::Int(1), HirExpr::Bool(true)],
                ty: TypeId::from_raw(8),
            },
            &mut d,
        );
        assert!(d.buf.contains("call_fn(5)(1, true):8"));
    }

    #[test]
    fn dump_call_expr_with_indirect_callee() {
        let mut d = Dumper::new();
        dump_expr_inline(
            &HirExpr::Call {
                callee: HirCallee::Indirect(Box::new(HirExpr::Global {
                    name: Atom::new_inline("f"),
                    ty: TypeId::from_raw(0),
                })),
                args: vec![],
                ty: TypeId::from_raw(0),
            },
            &mut d,
        );
        assert!(d.buf.contains("global(f)"));
    }

    #[test]
    fn dump_call_expr_with_closure_callee() {
        let mut d = Dumper::new();
        dump_expr_inline(
            &HirExpr::Call {
                callee: HirCallee::Closure(LocalId::from_raw(3)),
                args: vec![],
                ty: TypeId::from_raw(0),
            },
            &mut d,
        );
        assert!(d.buf.contains("closure(3)"));
    }

    #[test]
    fn dump_call_expr_with_runtime_callee() {
        let mut d = Dumper::new();
        dump_expr_inline(
            &HirExpr::Call {
                callee: HirCallee::Runtime {
                    name: Atom::new_inline("console_log"),
                    ty: TypeId::from_raw(0),
                },
                args: vec![],
                ty: TypeId::from_raw(0),
            },
            &mut d,
        );
        assert!(d.buf.contains("runtime(\"console_log\")"));
    }

    #[test]
    fn dump_binary_expr() {
        let mut d = Dumper::new();
        dump_expr_inline(
            &HirExpr::Binary {
                op: HirBinaryOp::Add,
                lhs: Box::new(HirExpr::Int(1)),
                rhs: Box::new(HirExpr::Int(2)),
                ty: TypeId::from_raw(0),
            },
            &mut d,
        );
        assert_eq!(d.buf, "(1 + 2):0");
    }

    #[test]
    fn dump_unary_expr() {
        let mut d = Dumper::new();
        dump_expr_inline(
            &HirExpr::Unary {
                op: HirUnaryOp::Neg,
                expr: Box::new(HirExpr::Int(5)),
                ty: TypeId::from_raw(0),
            },
            &mut d,
        );
        assert_eq!(d.buf, "-(5):0");
    }

    #[test]
    fn dump_struct_literal_expr() {
        let mut d = Dumper::new();
        dump_expr_inline(
            &HirExpr::StructLiteral {
                ty: TypeId::from_raw(0),
                fields: vec![(FieldId::from_raw(0), HirExpr::Int(7))],
            },
            &mut d,
        );
        assert!(d.buf.contains("struct{0:7}:0"));
    }

    #[test]
    fn dump_array_literal_expr() {
        let mut d = Dumper::new();
        dump_expr_inline(
            &HirExpr::ArrayLiteral {
                elements: vec![HirExpr::Int(1), HirExpr::Int(2)],
                ty: TypeId::from_raw(0),
            },
            &mut d,
        );
        assert!(d.buf.contains("[1, 2]:0"));
    }

    #[test]
    fn dump_closure_expr() {
        let mut d = Dumper::new();
        dump_expr_inline(
            &HirExpr::Closure {
                id: LocalId::from_raw(0),
                params: vec![crate::decl::HirParam {
                    name: Atom::new_inline("a"),
                    ty: TypeId::from_raw(1),
                }],
                captures: vec![HirExpr::Int(2)],
                body: vec![HirStmt::ret(Some(HirExpr::Int(3)))],
                ty: TypeId::from_raw(4),
            },
            &mut d,
        );
        assert!(d.buf.contains("closure(id=0)"));
        assert!(d.buf.contains("a:1"));
        assert!(d.buf.contains("return 3"));
        assert!(d.buf.contains(":4"));
    }

    #[test]
    fn dump_await_expr() {
        let mut d = Dumper::new();
        dump_expr_inline(
            &HirExpr::Await {
                expr: Box::new(HirExpr::Int(1)),
                ty: TypeId::from_raw(0),
            },
            &mut d,
        );
        assert_eq!(d.buf, "await(1):0");
    }

    #[test]
    fn dump_yield_expr_with_and_without_value() {
        let mut d = Dumper::new();
        dump_expr_inline(
            &HirExpr::Yield {
                expr: Some(Box::new(HirExpr::Int(2))),
                ty: TypeId::from_raw(0),
            },
            &mut d,
        );
        assert!(d.buf.contains("yield(2):0"));

        let mut d = Dumper::new();
        dump_expr_inline(
            &HirExpr::Yield {
                expr: None,
                ty: TypeId::from_raw(0),
            },
            &mut d,
        );
        assert!(d.buf.contains("yield(()):0"));
    }

    #[test]
    fn dump_template_expr_without_tag() {
        let mut d = Dumper::new();
        dump_expr_inline(
            &HirExpr::Template {
                tag: None,
                expressions: vec![HirExpr::Int(1)],
                cooked_parts: vec![None, None],
                raw_parts: vec![None, None],
                ty: TypeId::from_raw(0),
            },
            &mut d,
        );
        assert!(d.buf.contains("template["));
        assert!(d.buf.contains("1"));
    }

    #[test]
    fn dump_template_expr_with_tag() {
        let mut d = Dumper::new();
        dump_expr_inline(
            &HirExpr::Template {
                tag: Some(Box::new(HirExpr::Global {
                    name: Atom::new_inline("tag"),
                    ty: TypeId::from_raw(0),
                })),
                expressions: vec![HirExpr::Int(1)],
                cooked_parts: vec![None, None],
                raw_parts: vec![None, None],
                ty: TypeId::from_raw(0),
            },
            &mut d,
        );
        assert!(d.buf.contains("template(tag="));
    }

    #[test]
    fn dump_new_expr() {
        let mut d = Dumper::new();
        dump_expr_inline(
            &HirExpr::New {
                callee: Box::new(HirExpr::Global {
                    name: Atom::new_inline("Ctor"),
                    ty: TypeId::from_raw(0),
                }),
                args: vec![HirExpr::Int(1)],
                ty: TypeId::from_raw(0),
            },
            &mut d,
        );
        assert!(d.buf.contains("new global(Ctor):0"));
    }

    #[test]
    fn dump_optional_chain_expr() {
        let mut d = Dumper::new();
        dump_expr_inline(
            &HirExpr::OptionalChain {
                base: Box::new(HirExpr::Int(1)),
                ty: TypeId::from_raw(0),
            },
            &mut d,
        );
        assert!(d.buf.contains("opt("));
        assert!(d.buf.contains(":0"));
    }

    #[test]
    fn dump_type_assertion_expr() {
        let mut d = Dumper::new();
        dump_expr_inline(
            &HirExpr::TypeAssertion {
                expr: Box::new(HirExpr::Int(3)),
                target: TypeId::from_raw(5),
            },
            &mut d,
        );
        assert!(d.buf.contains("assert<5>(3)"));
    }

    #[test]
    fn dump_assignment_expr() {
        let mut d = Dumper::new();
        dump_expr_inline(
            &HirExpr::Assignment {
                target: Box::new(HirExpr::Int(0)),
                value: Box::new(HirExpr::Int(7)),
                ty: TypeId::from_raw(3),
            },
            &mut d,
        );
        assert!(d.buf.contains("assign("));
        assert!(d.buf.contains("= 7"));
        assert!(d.buf.contains(":3"));
    }

    #[test]
    fn dump_compound_update_pre() {
        let mut d = Dumper::new();
        dump_expr_inline(
            &HirExpr::CompoundUpdate {
                target: Box::new(HirExpr::Int(0)),
                op: HirBinaryOp::Add,
                rhs: Box::new(HirExpr::Int(1)),
                post: false,
                ty: TypeId::from_raw(0),
            },
            &mut d,
        );
        assert!(d.buf.contains("compound_update("));
        assert!(d.buf.contains(" + "));
        assert!(d.buf.contains("post=false"));
    }

    #[test]
    fn dump_compound_update_post() {
        let mut d = Dumper::new();
        dump_expr_inline(
            &HirExpr::CompoundUpdate {
                target: Box::new(HirExpr::Int(0)),
                op: HirBinaryOp::Sub,
                rhs: Box::new(HirExpr::Int(1)),
                post: true,
                ty: TypeId::from_raw(0),
            },
            &mut d,
        );
        assert!(d.buf.contains("post=true"));
    }

    #[test]
    fn fmt_bin_op_variants() {
        let map: &[(&str, HirBinaryOp)] = &[
            ("+", HirBinaryOp::Add),
            ("-", HirBinaryOp::Sub),
            ("*", HirBinaryOp::Mul),
            ("/", HirBinaryOp::Div),
            ("%", HirBinaryOp::Mod),
            ("==", HirBinaryOp::Eq),
            ("!=", HirBinaryOp::Ne),
            ("<", HirBinaryOp::Lt),
            ("<=", HirBinaryOp::Le),
            (">", HirBinaryOp::Gt),
            (">=", HirBinaryOp::Ge),
            ("&&", HirBinaryOp::And),
            ("||", HirBinaryOp::Or),
            ("&", HirBinaryOp::BitAnd),
            ("|", HirBinaryOp::BitOr),
            ("^", HirBinaryOp::BitXor),
            ("<<", HirBinaryOp::Shl),
            (">>", HirBinaryOp::Shr),
            (">>>", HirBinaryOp::Usr),
            ("in", HirBinaryOp::In),
            ("instanceof", HirBinaryOp::InstanceOf),
        ];
        for (sym, op) in map {
            assert_eq!(fmt_bin_op(*op), *sym);
        }
    }

    #[test]
    fn fmt_un_op_variants() {
        assert_eq!(fmt_un_op(HirUnaryOp::Neg), "-");
        assert_eq!(fmt_un_op(HirUnaryOp::Not), "!");
        assert_eq!(fmt_un_op(HirUnaryOp::BitNot), "~");
        assert_eq!(fmt_un_op(HirUnaryOp::TypeOf), "typeof");
        assert_eq!(fmt_un_op(HirUnaryOp::Void), "void");
        assert_eq!(fmt_un_op(HirUnaryOp::Delete), "delete");
    }

    #[test]
    fn dump_full_program_with_function_body() {
        let mut f = empty_func("compute");
        f.body = vec![HirStmt::ret(Some(HirExpr::Binary {
            op: HirBinaryOp::Add,
            lhs: Box::new(HirExpr::Int(1)),
            rhs: Box::new(HirExpr::Int(2)),
            ty: TypeId::from_raw(0),
        }))];
        let mut prog = HirProgram::new(ModuleId::from_raw(0));
        prog.push_decl(HirDecl::Function(f));
        let text = prog.dump_text();
        assert!(text.contains("fn compute"));
        assert!(text.contains("return (1 + 2):0"));
    }

    #[test]
    fn dump_nested_block() {
        let inner = HirStmt::Block(vec![HirStmt::ret(Some(HirExpr::Int(1)))]);
        let outer = HirStmt::Block(vec![inner]);
        let text = dump(vec![outer]);
        assert!(text.contains("{"));
        assert!(text.contains("}"));
        assert!(text.contains("return 1"));
    }

    #[test]
    fn dump_switch_default_case() {
        let s = HirStmt::Switch {
            disc: HirExpr::Int(0),
            cases: vec![
                HirSwitchCase::new(Some(HirExpr::Int(1)), vec![HirStmt::ret(None)]),
                HirSwitchCase::new(None, vec![HirStmt::ret(None)]),
            ],
        };
        let text = dump(vec![s]);
        assert!(text.contains("default:"));
    }
}
