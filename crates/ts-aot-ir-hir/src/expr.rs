use ts_aot_core::{Atom, FieldId, FunctionId, LocalId, TypeId};

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
    Unit,
    Bool(bool),
    Int(i64),
    Float(u64),
    String(Atom),
    Null,
    Undefined,

    Local {
        id: LocalId,
        ty: TypeId,
    },
    Global {
        name: Atom,
        ty: TypeId,
    },
    Field {
        owner: Box<HirExpr>,
        field: FieldId,
        field_name: Atom,
        ty: TypeId,
    },
    Index {
        owner: Box<HirExpr>,
        index: Box<HirExpr>,
        ty: TypeId,
    },

    Call {
        callee: HirCallee,
        args: Vec<HirExpr>,
        ty: TypeId,
    },
    Binary {
        op: HirBinaryOp,
        lhs: Box<HirExpr>,
        rhs: Box<HirExpr>,
        ty: TypeId,
    },
    Unary {
        op: HirUnaryOp,
        expr: Box<HirExpr>,
        ty: TypeId,
    },

    StructLiteral {
        ty: TypeId,
        fields: Vec<(FieldId, HirExpr)>,
    },
    ObjectLiteral {
        fields: Vec<ObjectLiteralField>,
        ty: TypeId,
    },
    Ternary {
        cond: Box<HirExpr>,
        then_branch: Box<HirExpr>,
        else_branch: Box<HirExpr>,
        ty: TypeId,
    },
    ArrayLiteral {
        elements: Vec<HirExpr>,
        ty: TypeId,
    },
    Closure {
        id: LocalId,
        params: Vec<HirParam>,
        captures: Vec<HirExpr>,
        body: Vec<HirStmt>,
        ty: TypeId,
    },
    Await {
        expr: Box<HirExpr>,
        ty: TypeId,
    },
    Yield {
        expr: Option<Box<HirExpr>>,
        ty: TypeId,
    },
    Template {
        tag: Option<Box<HirExpr>>,
        expressions: Vec<HirExpr>,
        cooked_parts: Vec<Option<Atom>>,
        raw_parts: Vec<Option<Atom>>,
        ty: TypeId,
    },
    New {
        callee: Box<HirExpr>,
        args: Vec<HirExpr>,
        ty: TypeId,
    },
    OptionalChain {
        base: Box<HirExpr>,
        ty: TypeId,
    },
    TypeAssertion {
        expr: Box<HirExpr>,
        target: TypeId,
    },
    Assignment {
        target: Box<HirExpr>,
        value: Box<HirExpr>,
        ty: TypeId,
    },
    CompoundUpdate {
        target: Box<HirExpr>,
        op: HirBinaryOp,
        rhs: Box<HirExpr>,
        post: bool,
        ty: TypeId,
    },
    Sequence {
        exprs: Vec<HirExpr>,
        ty: TypeId,
    },
    RegExp {
        pattern: Atom,
        flags: Atom,
        ty: TypeId,
    },
    BigInt {
        value: Atom,
        ty: TypeId,
    },
    Import {
        source: Box<HirExpr>,
        ty: TypeId,
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
        let expr = HirExpr::Int(42);
        match expr {
            HirExpr::Int(v) => assert_eq!(v, 42),
            _ => panic!("expected Int"),
        }
        assert_eq!(int_ty.raw(), 0);
    }

    #[test]
    fn binary_expr_nests() {
        let int_ty = TypeId::from_raw(1);
        let a = HirExpr::Int(1);
        let b = HirExpr::Int(2);
        let sum = HirExpr::Binary {
            op: HirBinaryOp::Add,
            lhs: Box::new(a),
            rhs: Box::new(b),
            ty: int_ty,
        };
        match sum {
            HirExpr::Binary { op, .. } => assert_eq!(op, HirBinaryOp::Add),
            _ => panic!("expected Binary"),
        }
    }

    #[test]
    fn expr_supports_equality() {
        let a = HirExpr::Int(42);
        let b = HirExpr::Int(42);
        let c = HirExpr::Int(7);
        assert_eq!(a, b);
        assert_ne!(a, c);
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ObjectLiteralField {
    Property { name: Atom, value: HirExpr },
    Spread(HirExpr),
}
