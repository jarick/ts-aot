use super::Dumper;
use super::dump_sym;

use crate::body::{
    BinaryOp, ConstValue, MirBody, MirExpr, MirPlace, MirPlaceBase, MirStmt, RuntimeOp, UnaryOp,
};

pub(crate) fn dump_body(body: &MirBody, d: &mut Dumper) {
    d.push();
    d.line("locals: [");
    d.push();
    for local in &body.locals {
        d.line(&format!(
            "{}: {}{} = {}",
            dump_sym(&local.name, d),
            if local.mutable { "var " } else { "" },
            local.ty.raw(),
            local.id.raw(),
        ));
    }
    if body.locals.is_empty() {
        d.line("");
    }
    d.pop();
    d.line("]");
    d.line("block: {");
    d.push();
    if body.block.is_empty() {
        d.line("");
    }
    for stmt in &body.block.stmts {
        dump_stmt(stmt, d);
    }
    d.pop();
    d.line("}");
    d.pop();
}

pub(crate) fn dump_stmt(stmt: &MirStmt, d: &mut Dumper) {
    match stmt {
        MirStmt::Let {
            local,
            ty,
            init,
            mutable,
        } => {
            d.write(&format!(
                "let {}{}: {}",
                if *mutable { "mut " } else { "" },
                local.raw(),
                ty.raw()
            ));
            if let Some(init) = init {
                d.write(" = ");
                dump_expr_inline(init, d);
            }
            d.write("\n");
        }
        MirStmt::Assign { target, value } => {
            d.write("assign ");
            dump_place(target, d);
            d.write(" = ");
            dump_expr_inline(value, d);
            d.write("\n");
        }
        MirStmt::Expr(e) => {
            d.write("expr ");
            dump_expr_inline(e, d);
            d.write("\n");
        }
        MirStmt::Return(None) => d.line("return"),
        MirStmt::Return(Some(e)) => {
            d.write("return ");
            dump_expr_inline(e, d);
            d.write("\n");
        }
        MirStmt::ReturnResultErr { error, err_ty } => {
            d.write("return_result_err(");
            dump_expr_inline(error, d);
            d.write(&format!(", {})", err_ty.raw()));
            d.write("\n");
        }
        MirStmt::Throw { error, error_ty } => {
            d.write("throw(");
            dump_expr_inline(error, d);
            d.write(&format!(", {})", error_ty.raw()));
            d.write("\n");
        }
        MirStmt::If {
            cond,
            then_block,
            else_block,
        } => {
            d.write("if (");
            dump_expr_inline(cond, d);
            d.write(") {\n");
            d.push();
            for s in &then_block.stmts {
                dump_stmt(s, d);
            }
            d.pop();
            d.line("}");
            if let Some(else_block) = else_block {
                d.line("else {");
                d.push();
                for s in &else_block.stmts {
                    dump_stmt(s, d);
                }
                d.pop();
                d.line("}");
            }
        }
        MirStmt::While { cond, body } => {
            d.write("while (");
            dump_expr_inline(cond, d);
            d.write(") {\n");
            d.push();
            for s in &body.stmts {
                dump_stmt(s, d);
            }
            d.pop();
            d.line("}");
        }
        MirStmt::DoWhile { body, cond } => {
            d.line("do {");
            d.push();
            for s in &body.stmts {
                dump_stmt(s, d);
            }
            d.pop();
            d.write("} while (");
            dump_expr_inline(cond, d);
            d.write(")\n");
        }
        MirStmt::ForOf {
            item,
            iterable,
            body,
        } => {
            d.write(&format!("for {} of ", item.raw()));
            dump_expr_inline(iterable, d);
            d.write(" {\n");
            d.push();
            for s in &body.stmts {
                dump_stmt(s, d);
            }
            d.pop();
            d.line("}");
        }
        MirStmt::ForIn { key, object, body } => {
            d.write(&format!("for {} in ", key.raw()));
            dump_expr_inline(object, d);
            d.write(" {\n");
            d.push();
            for s in &body.stmts {
                dump_stmt(s, d);
            }
            d.pop();
            d.line("}");
        }
        MirStmt::Break => d.line("break"),
        MirStmt::Continue => d.line("continue"),
        MirStmt::Runtime { op, args, dest, ty } => {
            d.write(&format!("runtime {}(", fmt_op(*op)));
            for (i, arg) in args.iter().enumerate() {
                if i > 0 {
                    d.write(", ");
                }
                dump_expr_inline(arg, d);
            }
            d.write(&format!(") -> {}", ty.raw()));
            if let Some(d2) = dest {
                d.write(&format!(" dest=local({})", d2.raw()));
            }
            d.write("\n");
        }
        MirStmt::Switch {
            disc,
            cases,
            default,
        } => {
            d.write("switch (");
            dump_expr_inline(disc, d);
            d.write(") {\n");
            d.push();
            for case in cases {
                d.write("case ");
                match &case.value {
                    ConstValue::Int(v) => d.write(&format!("{}:", v)),
                    ConstValue::String(s) => d.write(&format!("{:?}:", s.as_str())),
                }
                d.write("\n");
                d.push();
                for s in &case.body.stmts {
                    dump_stmt(s, d);
                }
                d.pop();
            }
            if let Some(def) = default {
                d.line("default:");
                d.push();
                for s in &def.stmts {
                    dump_stmt(s, d);
                }
                d.pop();
            }
            d.pop();
            d.line("}");
        }
        MirStmt::Try {
            body,
            catch_param,
            catch,
            finally,
        } => {
            d.line("try {");
            d.push();
            for s in &body.stmts {
                dump_stmt(s, d);
            }
            d.pop();
            match (catch, catch_param) {
                (Some(catch_block), Some(param)) => {
                    d.line(&format!("}} catch (local({})) {{", param.raw()));
                    d.push();
                    for s in &catch_block.stmts {
                        dump_stmt(s, d);
                    }
                    d.pop();
                }
                (Some(catch_block), None) => {
                    d.line("} catch {");
                    d.push();
                    for s in &catch_block.stmts {
                        dump_stmt(s, d);
                    }
                    d.pop();
                }
                (None, _) => {}
            }
            if let Some(fin) = finally {
                d.line("} finally {");
                d.push();
                for s in &fin.stmts {
                    dump_stmt(s, d);
                }
                d.pop();
            }
            d.line("}");
        }
    }
}

pub(crate) fn dump_place(place: &MirPlace, d: &mut Dumper) {
    match place {
        MirPlace::Local { id } => d.write(&format!("local({})", id.raw())),
        MirPlace::Field { base, field, ty } => {
            d.write("field(");
            dump_place_base(base, d);
            d.write(&format!(".{}:{})", field.raw(), ty.raw()));
        }
        MirPlace::Index { base, index, ty } => {
            d.write("index(");
            dump_expr_inline(base, d);
            d.write("[");
            dump_expr_inline(index, d);
            d.write(&format!("]:{})", ty.raw()));
        }
    }
}

fn dump_place_base(base: &MirPlaceBase, d: &mut Dumper) {
    match base {
        MirPlaceBase::Local(id) => d.write(&format!("local({})", id.raw())),
        MirPlaceBase::Field { base, field, .. } => {
            dump_place_base(base, d);
            d.write(&format!(".{}", field.raw()));
        }
        MirPlaceBase::Index { base, index, ty } => {
            d.write("index(");
            dump_expr_inline(base, d);
            d.write("[");
            dump_expr_inline(index, d);
            d.write(&format!("]:{})", ty.raw()));
        }
        MirPlaceBase::Chain { base, ty } => {
            d.write("chain(");
            dump_expr_inline(base, d);
            d.write(&format!("):{}", ty.raw()));
        }
    }
}

pub(crate) fn dump_expr_inline(expr: &MirExpr, d: &mut Dumper) {
    match expr {
        MirExpr::Unit => d.write("()"),
        MirExpr::Bool(v) => d.write(if *v { "true" } else { "false" }),
        MirExpr::Int { value, ty } => d.write(&format!("{}(:{})", value, ty.raw())),
        MirExpr::Float { value, ty } => d.write(&format!("{}(:{})", value, ty.raw())),
        MirExpr::String { id, ty } => d.write(&format!("string({:?})(:{})", id, ty.raw())),
        MirExpr::Null { ty } => d.write(&format!("null(:{})", ty.raw())),
        MirExpr::Local(id) => d.write(&format!("local({})", id.raw())),
        MirExpr::Global(sym) => d.write(&dump_sym(sym, d)),
        MirExpr::Field { base, field, ty } => {
            dump_expr_inline(base, d);
            d.write(&format!(".{}:{}", field.raw(), ty.raw()));
        }
        MirExpr::Index { base, index, ty } => {
            dump_expr_inline(base, d);
            d.write("[");
            dump_expr_inline(index, d);
            d.write(&format!("]:{}", ty.raw()));
        }
        MirExpr::Call { callee, args, ty } => {
            d.write(&format!("call fn({})(", callee.raw()));
            for (i, arg) in args.iter().enumerate() {
                if i > 0 {
                    d.write(", ");
                }
                dump_expr_inline(arg, d);
            }
            d.write(&format!("):{}", ty.raw()));
        }
        MirExpr::IndirectCall { callee, args, ty } => {
            d.write("indirect_call(");
            dump_expr_inline(callee, d);
            d.write(")(");
            for (i, arg) in args.iter().enumerate() {
                if i > 0 {
                    d.write(", ");
                }
                dump_expr_inline(arg, d);
            }
            d.write(&format!("):{}", ty.raw()));
        }
        MirExpr::StructLiteral {
            struct_id,
            fields,
            ty,
        } => {
            d.write(&format!("struct({}){{", struct_id.raw()));
            for (i, (fid, val)) in fields.iter().enumerate() {
                if i > 0 {
                    d.write(", ");
                }
                d.write(&format!("{}:", fid.raw()));
                dump_expr_inline(val, d);
            }
            d.write(&format!("}}:{}", ty.raw()));
        }
        MirExpr::ResultOk { value, ty } => {
            d.write("ok(");
            dump_expr_inline(value, d);
            d.write(&format!("):{}", ty.raw()));
        }
        MirExpr::ResultErr { error, ty } => {
            d.write("err(");
            dump_expr_inline(error, d);
            d.write(&format!("):{}", ty.raw()));
        }
        MirExpr::Binary {
            op,
            left,
            right,
            ty,
        } => {
            d.write("(");
            dump_expr_inline(left, d);
            d.write(&format!(" {} ", fmt_bin_op(*op)));
            dump_expr_inline(right, d);
            d.write(&format!("):{}", ty.raw()));
        }
        MirExpr::Unary { op, expr, ty } => {
            d.write(fmt_unary(*op));
            d.write("(");
            dump_expr_inline(expr, d);
            d.write(&format!("):{}", ty.raw()));
        }
        MirExpr::Await { expr, ty } => {
            d.write("await(");
            dump_expr_inline(expr, d);
            d.write(&format!("):{}", ty.raw()));
        }
        MirExpr::Yield { expr, ty } => {
            d.write("yield(");
            if let Some(e) = expr {
                dump_expr_inline(e, d);
            } else {
                d.write("()");
            }
            d.write(&format!("):{}", ty.raw()));
        }
        MirExpr::OptionalChain { base, ty } => {
            d.write("opt(");
            dump_expr_inline(base, d);
            d.write(&format!("):{}", ty.raw()));
        }
        MirExpr::TypeOf { expr, ty } => {
            d.write("typeof(");
            dump_expr_inline(expr, d);
            d.write(&format!("):{}", ty.raw()));
        }
        MirExpr::DynamicFrom { value, ty } => {
            d.write("dynfrom(");
            dump_expr_inline(value, d);
            d.write(&format!("):{}", ty.raw()));
        }
    }
}

fn fmt_bin_op(op: BinaryOp) -> &'static str {
    match op {
        BinaryOp::Add => "+",
        BinaryOp::Sub => "-",
        BinaryOp::Mul => "*",
        BinaryOp::Div => "/",
        BinaryOp::Mod => "%",
        BinaryOp::Eq => "==",
        BinaryOp::Ne => "!=",
        BinaryOp::Lt => "<",
        BinaryOp::Le => "<=",
        BinaryOp::Gt => ">",
        BinaryOp::Ge => ">=",
        BinaryOp::And => "&&",
        BinaryOp::Or => "||",
        BinaryOp::BitAnd => "&",
        BinaryOp::BitOr => "|",
        BinaryOp::BitXor => "^",
        BinaryOp::Shl => "<<",
        BinaryOp::Shr => ">>",
    }
}

fn fmt_op(op: RuntimeOp) -> &'static str {
    match op {
        RuntimeOp::StringConcat => "string_concat",
        RuntimeOp::StringEquals => "string_equals",
        RuntimeOp::StringLen => "string_len",
        RuntimeOp::ArrayCreate => "array_create",
        RuntimeOp::ArrayGet => "array_get",
        RuntimeOp::ArraySet => "array_set",
        RuntimeOp::ArrayLen => "array_len",
        RuntimeOp::MapGet => "map_get",
        RuntimeOp::MapSet => "map_set",
        RuntimeOp::ResultOk => "result_ok",
        RuntimeOp::ResultErr => "result_err",
        RuntimeOp::ResultUnwrapOk => "result_unwrap_ok",
        RuntimeOp::PromiseCreate => "promise_create",
        RuntimeOp::PromiseResolve => "promise_resolve",
        RuntimeOp::TypeOf => {
            unreachable!("TypeOf is not a MirStmt::Runtime; it's a MirExpr::TypeOf")
        }
        RuntimeOp::OpIn => "op_in",
        RuntimeOp::OpInstanceof => "op_instanceof",
        RuntimeOp::OpObjectGet => "object_get",
        RuntimeOp::OpObjectSet => "object_set",
        RuntimeOp::OpObjectHas => "object_has",
        RuntimeOp::OpObjectDelete => "object_delete",
        RuntimeOp::OpObjectUnwrap => "object_unwrap",
        RuntimeOp::OpObjectNew => "object_new",
        RuntimeOp::OpObjectProtoGet => "object_proto_get",
        RuntimeOp::OpObjectProtoSet => "object_proto_set",
        RuntimeOp::OpObjectSetPrototypeOf => "object_set_prototype_of",
        RuntimeOp::OpObjectKeys => "object_keys",
        RuntimeOp::OpDynamicBinary => "dynamic_binary",
        RuntimeOp::HostConsoleLog => "host_console_log",
        RuntimeOp::MathSqrt => "math_sqrt",
    }
}

fn fmt_unary(op: UnaryOp) -> &'static str {
    match op {
        UnaryOp::Neg => "-",
        UnaryOp::Not => "!",
        UnaryOp::BitNot => "~",
    }
}
