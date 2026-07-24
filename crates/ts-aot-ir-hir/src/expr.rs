use ts_aot_core::{Atom, FieldId, FunctionId, LocalId, Span, TypeId};

use crate::decl::HirParam;
use crate::stmt::HirStmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HirBinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
    Usr,
    In,
    InstanceOf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HirUnaryOp {
    Neg,
    Not,
    BitNot,
    TypeOf,
    Void,
    Delete,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum HirCallee {
    Function(FunctionId),
    Indirect(Box<HirExpr>),
    Closure(LocalId),
    Runtime { name: Atom, ty: TypeId },
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum HirExpr {
    Unit(Span),
    Bool(bool, Span),
    Int(i64, Span),
    Float(u64, Span),
    String(Atom, Span),
    Null(Span),
    Undefined(Span),

    Local {
        id: LocalId,
        ty: TypeId,
        span: Span,
    },
    Global {
        name: Atom,
        ty: TypeId,
        span: Span,
    },
    Field {
        owner: Box<HirExpr>,
        field: FieldId,
        field_name: Atom,
        ty: TypeId,
        span: Span,
    },
    Index {
        owner: Box<HirExpr>,
        index: Box<HirExpr>,
        ty: TypeId,
        span: Span,
    },

    Call {
        callee: HirCallee,
        args: Vec<HirExpr>,
        ty: TypeId,
        span: Span,
    },
    Binary {
        op: HirBinaryOp,
        lhs: Box<HirExpr>,
        rhs: Box<HirExpr>,
        ty: TypeId,
        span: Span,
    },
    Unary {
        op: HirUnaryOp,
        expr: Box<HirExpr>,
        ty: TypeId,
        span: Span,
    },

    StructLiteral {
        ty: TypeId,
        fields: Vec<(FieldId, HirExpr)>,
        span: Span,
    },
    ObjectLiteral {
        fields: Vec<ObjectLiteralField>,
        ty: TypeId,
        span: Span,
    },
    Ternary {
        cond: Box<HirExpr>,
        then_branch: Box<HirExpr>,
        else_branch: Box<HirExpr>,
        ty: TypeId,
        span: Span,
    },
    ArrayLiteral {
        elements: Vec<HirExpr>,
        ty: TypeId,
        span: Span,
    },
    Closure {
        id: LocalId,
        params: Vec<HirParam>,
        captures: Vec<HirExpr>,
        body: Vec<HirStmt>,
        ty: TypeId,
        span: Span,
    },
    Await {
        expr: Box<HirExpr>,
        ty: TypeId,
        span: Span,
    },
    Yield {
        expr: Option<Box<HirExpr>>,
        ty: TypeId,
        span: Span,
    },
    Template {
        tag: Option<Box<HirExpr>>,
        expressions: Vec<HirExpr>,
        cooked_parts: Vec<Option<Atom>>,
        raw_parts: Vec<Option<Atom>>,
        ty: TypeId,
        span: Span,
    },
    New {
        callee: Box<HirExpr>,
        args: Vec<HirExpr>,
        ty: TypeId,
        span: Span,
    },
    OptionalChain {
        base: Box<HirExpr>,
        ty: TypeId,
        span: Span,
    },
    TypeAssertion {
        expr: Box<HirExpr>,
        target: TypeId,
        span: Span,
    },
    Assignment {
        target: Box<HirExpr>,
        value: Box<HirExpr>,
        ty: TypeId,
        span: Span,
    },
    CompoundUpdate {
        target: Box<HirExpr>,
        op: HirBinaryOp,
        rhs: Box<HirExpr>,
        post: bool,
        ty: TypeId,
        span: Span,
    },
    Sequence {
        exprs: Vec<HirExpr>,
        ty: TypeId,
        span: Span,
    },
    RegExp {
        pattern: Atom,
        flags: Atom,
        ty: TypeId,
        span: Span,
    },
    BigInt {
        value: Atom,
        ty: TypeId,
        span: Span,
    },
    Import {
        source: Box<HirExpr>,
        ty: TypeId,
        span: Span,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binary_op_variants_are_distinct() {
        assert_ne!(HirBinaryOp::Add, HirBinaryOp::Sub);
        assert_ne!(HirBinaryOp::Eq, HirBinaryOp::Ne);
        assert_ne!(HirBinaryOp::BitAnd, HirBinaryOp::BitOr);
        assert_ne!(HirBinaryOp::Shl, HirBinaryOp::Shr);
    }

    #[test]
    fn unary_op_variants_are_distinct() {
        assert_ne!(HirUnaryOp::Neg, HirUnaryOp::Not);
        assert_ne!(HirUnaryOp::BitNot, HirUnaryOp::TypeOf);
    }

    #[test]
    fn expr_construction_does_not_panic() {
        let int_ty = TypeId::from_raw(0);
        let span = Span::new(0, 0);
        let expr = HirExpr::Int(42, span);
        match expr {
            HirExpr::Int(v, _) => assert_eq!(v, 42),
            _ => panic!("expected Int"),
        }
        assert_eq!(int_ty.raw(), 0);
    }

    #[test]
    fn binary_expr_nests() {
        let int_ty = TypeId::from_raw(1);
        let span = Span::new(0, 0);
        let a = HirExpr::Int(1, span);
        let b = HirExpr::Int(2, span);
        let sum = HirExpr::Binary {
            op: HirBinaryOp::Add,
            lhs: Box::new(a),
            rhs: Box::new(b),
            ty: int_ty,
            span,
        };
        match sum {
            HirExpr::Binary { op, .. } => assert_eq!(op, HirBinaryOp::Add),
            _ => panic!("expected Binary"),
        }
    }

    #[test]
    fn expr_supports_equality() {
        let span = Span::new(0, 0);
        let a = HirExpr::Int(42, span);
        let b = HirExpr::Int(42, span);
        let c = HirExpr::Int(7, span);
        assert_eq!(a, b);
        assert_ne!(a, c);
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ObjectLiteralField {
    Property { name: Atom, value: HirExpr },
    Spread(HirExpr),
}
