use oxc_ast::ast::ForStatementLeft;
use oxc_span::GetSpan;
use oxc_syntax::operator::{AssignmentOperator, BinaryOperator, LogicalOperator, UnaryOperator};
use ts_aot_core::{Atom, Span};
use ts_aot_ir_hir::{HirBinaryOp, HirExpr, HirUnaryOp};

const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_992.0;

pub(crate) fn label_atom(label: Option<&str>) -> Option<Atom> {
    label.map(Atom::from)
}

pub(crate) fn left_span(left: &ForStatementLeft<'_>) -> oxc_span::Span {
    match left {
        ForStatementLeft::VariableDeclaration(v) => v.span,
        other => other.span(),
    }
}

fn f64_integer_to_i64(value: f64) -> i64 {
    debug_assert!(value.is_finite());
    debug_assert_eq!(value.fract().to_bits(), 0.0_f64.to_bits());
    debug_assert!(value.abs() < MAX_SAFE_INTEGER);
    let bits = value.to_bits();
    let sign = bits >> 63;
    let biased_exp = (bits >> 52) & 0x7FF;
    let mantissa = bits & 0xF_FFFF_FFFF_FFFF;
    let mag_u64 = if biased_exp == 0 {
        0u64
    } else {
        let effective = mantissa | (1u64 << 52);
        let shift: u32 =
            u32::try_from(1075u64.saturating_sub(biased_exp)).expect("shift amount fits in u32");
        effective >> shift
    };
    let magnitude_i64 = i64::from_le_bytes(mag_u64.to_le_bytes());
    if sign == 1 {
        -magnitude_i64
    } else {
        magnitude_i64
    }
}

pub(crate) fn number_to_hir(value: f64, span: Span) -> HirExpr {
    if value.is_finite() && value.fract() == 0.0 && value.abs() < MAX_SAFE_INTEGER {
        HirExpr::Int(f64_integer_to_i64(value), span)
    } else {
        HirExpr::Float(value.to_bits(), span)
    }
}

pub(crate) fn map_binary_op(op: BinaryOperator) -> Option<HirBinaryOp> {
    Some(match op {
        BinaryOperator::Addition => HirBinaryOp::Add,
        BinaryOperator::Subtraction => HirBinaryOp::Sub,
        BinaryOperator::Multiplication => HirBinaryOp::Mul,
        BinaryOperator::Division => HirBinaryOp::Div,
        BinaryOperator::Remainder => HirBinaryOp::Mod,
        BinaryOperator::Equality | BinaryOperator::StrictEquality => HirBinaryOp::Eq,
        BinaryOperator::Inequality | BinaryOperator::StrictInequality => HirBinaryOp::Ne,
        BinaryOperator::LessThan => HirBinaryOp::Lt,
        BinaryOperator::LessEqualThan => HirBinaryOp::Le,
        BinaryOperator::GreaterThan => HirBinaryOp::Gt,
        BinaryOperator::GreaterEqualThan => HirBinaryOp::Ge,
        BinaryOperator::BitwiseOR => HirBinaryOp::BitOr,
        BinaryOperator::BitwiseXOR => HirBinaryOp::BitXor,
        BinaryOperator::BitwiseAnd => HirBinaryOp::BitAnd,
        BinaryOperator::ShiftLeft => HirBinaryOp::Shl,
        BinaryOperator::ShiftRight => HirBinaryOp::Shr,
        BinaryOperator::ShiftRightZeroFill => HirBinaryOp::Usr,
        BinaryOperator::In => HirBinaryOp::In,
        BinaryOperator::Instanceof => HirBinaryOp::InstanceOf,
        BinaryOperator::Exponential => return None,
    })
}

pub(crate) fn map_logical_op(op: LogicalOperator) -> HirBinaryOp {
    match op {
        LogicalOperator::And => HirBinaryOp::And,
        LogicalOperator::Or | LogicalOperator::Coalesce => HirBinaryOp::Or,
    }
}

pub(crate) fn map_unary_op(op: UnaryOperator) -> Option<HirUnaryOp> {
    match op {
        UnaryOperator::UnaryNegation => Some(HirUnaryOp::Neg),
        UnaryOperator::LogicalNot => Some(HirUnaryOp::Not),
        UnaryOperator::BitwiseNot => Some(HirUnaryOp::BitNot),
        UnaryOperator::Typeof => Some(HirUnaryOp::TypeOf),
        UnaryOperator::Void => Some(HirUnaryOp::Void),
        UnaryOperator::Delete => Some(HirUnaryOp::Delete),
        UnaryOperator::UnaryPlus => None,
    }
}

pub(crate) enum CompoundOp {
    Assign,
    Binary(HirBinaryOp),
    Unsupported,
}

pub(crate) fn compound_op(op: AssignmentOperator) -> CompoundOp {
    match op {
        AssignmentOperator::Assign => CompoundOp::Assign,
        AssignmentOperator::Addition => CompoundOp::Binary(HirBinaryOp::Add),
        AssignmentOperator::Subtraction => CompoundOp::Binary(HirBinaryOp::Sub),
        AssignmentOperator::Multiplication => CompoundOp::Binary(HirBinaryOp::Mul),
        AssignmentOperator::Division => CompoundOp::Binary(HirBinaryOp::Div),
        AssignmentOperator::Remainder => CompoundOp::Binary(HirBinaryOp::Mod),
        AssignmentOperator::ShiftLeft => CompoundOp::Binary(HirBinaryOp::Shl),
        AssignmentOperator::ShiftRight => CompoundOp::Binary(HirBinaryOp::Shr),
        AssignmentOperator::ShiftRightZeroFill => CompoundOp::Binary(HirBinaryOp::Usr),
        AssignmentOperator::BitwiseOR => CompoundOp::Binary(HirBinaryOp::BitOr),
        AssignmentOperator::BitwiseXOR => CompoundOp::Binary(HirBinaryOp::BitXor),
        AssignmentOperator::BitwiseAnd => CompoundOp::Binary(HirBinaryOp::BitAnd),
        AssignmentOperator::LogicalAnd => CompoundOp::Binary(HirBinaryOp::And),
        AssignmentOperator::LogicalOr | AssignmentOperator::LogicalNullish => {
            CompoundOp::Binary(HirBinaryOp::Or)
        }
        AssignmentOperator::Exponential => CompoundOp::Unsupported,
    }
}
