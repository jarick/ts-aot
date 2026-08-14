use ts_aot_core::{Atom, FieldId, FunctionId, LocalId, Span, TypeId};

use crate::decl::HirParam;
use crate::stmt::HirStmt;

impl HirExpr {
    #[must_use]
    pub fn ty(&self) -> TypeId {
        match self {
            Self::Unit(_) | Self::Null(_) | Self::Undefined(_) => TypeId::from_raw(0),
            Self::Bool(_, _) | Self::Int(_, _) | Self::Float(_, _) | Self::String(_, _) => {
                TypeId::from_raw(0)
            }
            Self::Local { ty, .. }
            | Self::Global { ty, .. }
            | Self::Field { ty, .. }
            | Self::Index { ty, .. }
            | Self::Call { ty, .. }
            | Self::Binary { ty, .. }
            | Self::Unary { ty, .. }
            | Self::StructLiteral { ty, .. }
            | Self::ObjectLiteral { ty, .. }
            | Self::Ternary { ty, .. }
            | Self::ArrayLiteral { ty, .. }
            | Self::Closure { ty, .. }
            | Self::Await { ty, .. }
            | Self::Yield { ty, .. }
            | Self::Template { ty, .. }
            | Self::New { ty, .. }
            | Self::OptionalChain { ty, .. }
            | Self::TypeAssertion { target: ty, .. }
            | Self::Assignment { ty, .. }
            | Self::CompoundUpdate { ty, .. }
            | Self::Sequence { ty, .. }
            | Self::RegExp { ty, .. }
            | Self::BigInt { ty, .. }
            | Self::Import { ty, .. } => *ty,
        }
    }

    #[must_use]
    pub fn span(&self) -> Span {
        match self {
            Self::Unit(s)
            | Self::Bool(_, s)
            | Self::Int(_, s)
            | Self::Float(_, s)
            | Self::String(_, s)
            | Self::Null(s)
            | Self::Undefined(s)
            | Self::Local { span: s, .. }
            | Self::Global { span: s, .. }
            | Self::Field { span: s, .. }
            | Self::Index { span: s, .. }
            | Self::Call { span: s, .. }
            | Self::Binary { span: s, .. }
            | Self::Unary { span: s, .. }
            | Self::StructLiteral { span: s, .. }
            | Self::ObjectLiteral { span: s, .. }
            | Self::Ternary { span: s, .. }
            | Self::ArrayLiteral { span: s, .. }
            | Self::Closure { span: s, .. }
            | Self::Await { span: s, .. }
            | Self::Yield { span: s, .. }
            | Self::Template { span: s, .. }
            | Self::New { span: s, .. }
            | Self::OptionalChain { span: s, .. }
            | Self::TypeAssertion { span: s, .. }
            | Self::Assignment { span: s, .. }
            | Self::CompoundUpdate { span: s, .. }
            | Self::Sequence { span: s, .. }
            | Self::RegExp { span: s, .. }
            | Self::BigInt { span: s, .. }
            | Self::Import { span: s, .. } => *s,
        }
    }
}

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
        type_args: Vec<TypeId>,
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

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ObjectLiteralField {
    Property { name: Atom, value: HirExpr },
    Spread(HirExpr),
}

impl ObjectLiteralField {
    #[must_use]
    pub fn value(&self) -> &HirExpr {
        match self {
            Self::Property { value, .. } | Self::Spread(value) => value,
        }
    }

    #[must_use]
    pub fn value_mut(&mut self) -> &mut HirExpr {
        match self {
            Self::Property { value, .. } | Self::Spread(value) => value,
        }
    }
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

    #[test]
    fn span_returns_payload_span_for_simple_variants() {
        assert_eq!(HirExpr::Unit(Span::new(1, 2)).span(), Span::new(1, 2));
        assert_eq!(HirExpr::Bool(true, Span::new(3, 4)).span(), Span::new(3, 4));
        assert_eq!(HirExpr::Int(42, Span::new(5, 6)).span(), Span::new(5, 6));
        assert_eq!(
            HirExpr::String(Atom::new_inline("x"), Span::new(7, 8)).span(),
            Span::new(7, 8)
        );
        assert_eq!(HirExpr::Null(Span::new(9, 10)).span(), Span::new(9, 10));
        assert_eq!(
            HirExpr::Undefined(Span::new(11, 12)).span(),
            Span::new(11, 12)
        );
    }

    #[test]
    fn span_returns_struct_field_span_for_structured_variants() {
        let int_ty = TypeId::from_raw(0);
        let local = HirExpr::Local {
            id: LocalId::from_raw(0),
            ty: int_ty,
            span: Span::new(20, 21),
        };
        assert_eq!(local.span(), Span::new(20, 21));

        let call = HirExpr::Call {
            callee: HirCallee::Function(FunctionId::from_raw(0)),
            args: Vec::new(),
            type_args: Vec::new(),
            ty: int_ty,
            span: Span::new(30, 31),
        };
        assert_eq!(call.span(), Span::new(30, 31));

        let binary = HirExpr::Binary {
            op: HirBinaryOp::Add,
            lhs: Box::new(HirExpr::Int(1, Span::new(40, 41))),
            rhs: Box::new(HirExpr::Int(2, Span::new(42, 43))),
            ty: int_ty,
            span: Span::new(40, 43),
        };
        assert_eq!(binary.span(), Span::new(40, 43));

        let template = HirExpr::Template {
            tag: None,
            expressions: Vec::new(),
            cooked_parts: Vec::new(),
            raw_parts: Vec::new(),
            ty: int_ty,
            span: Span::new(50, 55),
        };
        assert_eq!(template.span(), Span::new(50, 55));
    }

    #[test]
    fn span_returns_struct_field_span_for_newer_variants() {
        let int_ty = TypeId::from_raw(0);
        let new_expr = HirExpr::New {
            callee: Box::new(HirExpr::Global {
                name: Atom::new_inline("X"),
                ty: int_ty,
                span: Span::new(60, 61),
            }),
            args: Vec::new(),
            ty: int_ty,
            span: Span::new(60, 70),
        };
        assert_eq!(new_expr.span(), Span::new(60, 70));

        let opt_chain = HirExpr::OptionalChain {
            base: Box::new(HirExpr::Local {
                id: LocalId::from_raw(0),
                ty: int_ty,
                span: Span::new(80, 81),
            }),
            ty: int_ty,
            span: Span::new(80, 90),
        };
        assert_eq!(opt_chain.span(), Span::new(80, 90));

        let regexp = HirExpr::RegExp {
            pattern: Atom::new_inline("foo"),
            flags: Atom::new_inline("g"),
            ty: int_ty,
            span: Span::new(100, 110),
        };
        assert_eq!(regexp.span(), Span::new(100, 110));

        let bigint = HirExpr::BigInt {
            value: Atom::new_inline("42"),
            ty: int_ty,
            span: Span::new(120, 130),
        };
        assert_eq!(bigint.span(), Span::new(120, 130));

        let import = HirExpr::Import {
            source: Box::new(HirExpr::String(
                Atom::new_inline("mod"),
                Span::new(140, 145),
            )),
            ty: int_ty,
            span: Span::new(140, 150),
        };
        assert_eq!(import.span(), Span::new(140, 150));
    }
}
