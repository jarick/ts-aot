use ts_aot_core::Span;
use ts_aot_ir_hir::{HirBinaryOp, HirUnaryOp};
use ts_aot_ir_mir::{BinaryOp, UnaryOp};

use crate::PassContext;

pub fn convert_binop(op: HirBinaryOp, ctx: &mut PassContext) -> BinaryOp {
    use BinaryOp as M;
    match op {
        HirBinaryOp::Add => M::Add,
        HirBinaryOp::Sub => M::Sub,
        HirBinaryOp::Mul => M::Mul,
        HirBinaryOp::Div => M::Div,
        HirBinaryOp::Mod => M::Mod,
        HirBinaryOp::Eq => M::Eq,
        HirBinaryOp::Ne => M::Ne,
        HirBinaryOp::Lt => M::Lt,
        HirBinaryOp::Le => M::Le,
        HirBinaryOp::Gt => M::Gt,
        HirBinaryOp::Ge => M::Ge,
        HirBinaryOp::And => M::And,
        HirBinaryOp::Or => M::Or,
        HirBinaryOp::BitAnd => M::BitAnd,
        HirBinaryOp::BitOr => M::BitOr,
        HirBinaryOp::BitXor => M::BitXor,
        HirBinaryOp::Shl => M::Shl,
        HirBinaryOp::Shr => M::Shr,
        HirBinaryOp::Usr | HirBinaryOp::In | HirBinaryOp::InstanceOf => {
            ctx.error(
                "P0005",
                format!("binary operator {:?} is not yet supported in HIR→MIR", op),
                Span::new(0, 0),
            );
            M::Eq
        }
    }
}

pub fn convert_unaryop(op: HirUnaryOp, ctx: &mut PassContext) -> UnaryOp {
    match op {
        HirUnaryOp::Neg => UnaryOp::Neg,
        HirUnaryOp::BitNot => UnaryOp::BitNot,
        HirUnaryOp::TypeOf | HirUnaryOp::Void | HirUnaryOp::Delete => {
            ctx.error(
                "P0005",
                format!("unary operator {:?} is not yet supported in HIR→MIR", op),
                Span::new(0, 0),
            );
            UnaryOp::Not
        }
        HirUnaryOp::Not => UnaryOp::Not,
    }
}
