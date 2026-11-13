use std::collections::HashMap;

use ts_aot_core::{Atom, NullishKinds, Span, StructId, Type, TypeId, TypeTable};
use ts_aot_ir_hir::{HirBinaryOp, HirExpr, HirUnaryOp};
use ts_aot_ir_mir::{BinaryOp, MirBlock, MirExpr, MirPlace, MirStmt, RuntimeOp};

use crate::PassContext;
use crate::hir_to_mir::convert_expr::call::is_numeric_type;
use crate::hir_to_mir::convert_expr::util::{
    has_potential_side_effects, hir_expr_type_id, mir_expr_ty,
};
use crate::hir_to_mir::converter::ExprConverter;
use crate::hir_to_mir::nullish::{LOGICAL_DIAG, NULLISH_NARROWING_DIAG, kinds_of_hir, kinds_of_ty};
use crate::hir_to_mir::ops::{convert_binop, convert_unaryop};

fn logical_op_name(op: HirBinaryOp) -> &'static str {
    match op {
        HirBinaryOp::And => "&&",
        HirBinaryOp::Coalesce => "??",
        _ => "||",
    }
}

fn known_type(ty: Option<TypeId>, types: &TypeTable) -> Option<TypeId> {
    ty.filter(|t| !matches!(types.resolve(*t), None | Some(Type::Error)))
}

impl ExprConverter {
    /// The statically known type of a converted expression, for the lowerings
    /// that must know it (`??`, `&&`, `||`, optional chains, `x!`).
    ///
    /// `mir_expr_ty` returns a *placeholder* for the node kinds that carry no
    /// type of their own (`Local`, `Global`, `Bool`, `Unit`), and the
    /// placeholder is `TypeId::from_raw(0)` - a perfectly valid id - so reading
    /// it as a type silently yields an unrelated type. Those kinds are resolved
    /// through the converter's tables instead, and a genuinely unknown type
    /// stays `None` so the caller can report it rather than mis-lower.
    pub(super) fn mir_expr_static_ty(&self, expr: &MirExpr, types: &TypeTable) -> Option<TypeId> {
        let ty = match expr {
            MirExpr::Local(id) => self
                .temp_locals
                .iter()
                .rev()
                .find(|decl| decl.id == *id)
                .map(|decl| decl.ty),
            MirExpr::Global(name) => self.global_decl_type(name),
            MirExpr::Unit | MirExpr::Bool(_) => None,
            other => Some(mir_expr_ty(other)),
        };
        known_type(ty, types)
    }

    /// The statically known type of a HIR operand that carries none of its own.
    ///
    /// A module-level reference is typed by its declaration, a call by the
    /// return type of the callee it names, and a member read by the field of the
    /// owner's class when HIR leaves it `Error`. Calls and object-literal fields
    /// have no type on their own node, so a caller that only looked at
    /// `hir_expr_type_id` used to see `None` for them.
    fn hir_static_ty(&self, expr: &HirExpr, types: &mut TypeTable) -> Option<TypeId> {
        match expr {
            HirExpr::Global { name, .. } => known_type(self.global_decl_type(name), types),
            HirExpr::Call { .. } => known_type(self.expr_type(expr, types), types),
            HirExpr::Field {
                owner,
                field_name,
                ty,
                ..
            } => known_type(Some(*ty), types).or_else(|| {
                let owner_ty = self.operand_static_ty(owner, types)?;
                self.program
                    .class_fields
                    .get(&owner_ty)?
                    .iter()
                    .find(|field| field.name == *field_name)
                    .and_then(|field| known_type(Some(field.ty), types))
            }),
            _ => None,
        }
    }

    /// The statically known type of an operand, whichever HIR kind carries it:
    /// the node's own `ty`, the declaration behind a global/call/member read, or
    /// the kind of a literal (which never carries a type id of its own).
    pub(super) fn operand_static_ty(
        &self,
        expr: &HirExpr,
        types: &mut TypeTable,
    ) -> Option<TypeId> {
        known_type(hir_expr_type_id(expr), types)
            .or_else(|| self.hir_static_ty(expr, types))
            .or_else(|| self.expr_type(expr, types))
    }
}

/// A user-facing name for a type, so diagnostics do not leak the internal
/// `Debug` form (`Optional { inner: 3, nullish: ... }`, `F64`, `Some(..)`).
fn type_name(types: &TypeTable, ty: TypeId) -> String {
    fn is_integer(ty: &Type) -> bool {
        matches!(
            ty,
            Type::I8
                | Type::I16
                | Type::I32
                | Type::I64
                | Type::U8
                | Type::U16
                | Type::U32
                | Type::U64
        )
    }
    match types.resolve(ty) {
        Some(t) if is_integer(t) || matches!(t, Type::F32 | Type::F64) => "number".to_owned(),
        Some(Type::String) => "string".to_owned(),
        Some(Type::Bool) => "boolean".to_owned(),
        Some(Type::BigInt) => "bigint".to_owned(),
        Some(Type::Null) => "null".to_owned(),
        Some(Type::Void) => "undefined".to_owned(),
        Some(Type::Optional { inner, nullish }) => {
            let kinds = match (nullish.null, nullish.undefined) {
                (true, true) => "null | undefined",
                (false, true) => "undefined",
                _ => "null",
            };
            format!("{} | {kinds}", type_name(types, *inner))
        }
        Some(Type::Union { variants }) => variants
            .iter()
            .map(|variant| type_name(types, *variant))
            .collect::<Vec<_>>()
            .join(" | "),
        Some(Type::Error) | None => "<unknown>".to_owned(),
        Some(other) => format!("{other:?}"),
    }
}

/// The name of a HIR operand's type for a diagnostic, including the literal
/// kinds that carry no type id of their own.
fn operand_type_name(expr: &HirExpr, ty: Option<TypeId>, types: &TypeTable) -> String {
    if let Some(ty) = ty {
        return type_name(types, ty);
    }
    match expr {
        HirExpr::Int(..) | HirExpr::Float(..) => "number".to_owned(),
        HirExpr::String(..) => "string".to_owned(),
        HirExpr::Bool(..) => "boolean".to_owned(),
        HirExpr::Null(_) => "null".to_owned(),
        HirExpr::Undefined(_) | HirExpr::Unit(_) => "undefined".to_owned(),
        _ => "<unknown>".to_owned(),
    }
}

fn is_relational_op(op: HirBinaryOp) -> bool {
    matches!(
        op,
        HirBinaryOp::Lt | HirBinaryOp::Le | HirBinaryOp::Gt | HirBinaryOp::Ge
    )
}

fn comparison_op_name(op: HirBinaryOp) -> &'static str {
    match op {
        HirBinaryOp::Eq => "==",
        HirBinaryOp::Ne => "!=",
        HirBinaryOp::StrictEq => "===",
        HirBinaryOp::StrictNe => "!==",
        HirBinaryOp::Lt => "<",
        HirBinaryOp::Le => "<=",
        HirBinaryOp::Gt => ">",
        HirBinaryOp::Ge => ">=",
        _ => "?",
    }
}

/// The `T` of a `T | null` / `T | undefined` operand, if it is an optional.
fn optional_inner(ty: Option<TypeId>, types: &TypeTable) -> Option<TypeId> {
    match types.resolve(ty?) {
        Some(Type::Optional { inner, .. }) => known_type(Some(*inner), types),
        _ => None,
    }
}

/// Whether two types can meet in one Rust expression: the AOT target emits the
/// same type for both, or numeric values (which the backend coerces).
fn types_compatible(a: TypeId, b: TypeId, types: &TypeTable) -> bool {
    match (types.resolve(a), types.resolve(b)) {
        (None | Some(Type::Error), _) | (_, None | Some(Type::Error)) => true,
        (Some(x), Some(y)) => x == y || (is_numeric_type(a, types) && is_numeric_type(b, types)),
    }
}

/// The empty [`NullishKinds`]: a value that can never be nullish.
const NO_KINDS: NullishKinds = NullishKinds {
    null: false,
    undefined: false,
};

/// The nullish kinds a branch that may yield either of two operands can
/// produce: `a && b` is `a` or `b`, so either one's nullish case can reach the
/// result.
fn union_kinds(left: NullishKinds, right: NullishKinds) -> NullishKinds {
    NullishKinds {
        null: left.null || right.null,
        undefined: left.undefined || right.undefined,
    }
}

/// The numeric type that can hold both operands when they are spelled
/// differently: a mixed `int`/`float` pair meets in the float type, so a
/// fractional operand keeps its fraction (`a ?? 1.5` must not become `1`).
/// Returns `None` for every other pair, so non-numeric operands - and two
/// integer spellings, which the backend already coerces between - keep the type
/// they had.
fn common_numeric_ty(a: TypeId, b: TypeId, types: &mut TypeTable) -> Option<TypeId> {
    if a == b || !is_numeric_type(a, types) || !is_numeric_type(b, types) {
        return None;
    }
    let (a_resolved, b_resolved) = (types.resolve(a).cloned(), types.resolve(b).cloned());
    let is_float = |ty: &Option<Type>| matches!(ty, Some(Type::F32 | Type::F64));
    if !is_float(&a_resolved) && !is_float(&b_resolved) {
        return None;
    }
    // `f64` holds every `f32` value (and every integer), so it wins over `f32`.
    let float = if matches!(a_resolved, Some(Type::F64)) || matches!(b_resolved, Some(Type::F64)) {
        Type::F64
    } else {
        Type::F32
    };
    Some(types.intern(&float))
}

/// The non-nullish part of an operand that may be optional, so a numeric
/// fallback can be compared with the type the destination would hold.
fn optional_value_ty(ty: TypeId, types: &TypeTable) -> TypeId {
    match types.resolve(ty) {
        Some(Type::Optional { inner, .. }) => *inner,
        _ => ty,
    }
}

fn is_nullable_type(ty: Option<&Type>, types: &TypeTable) -> bool {
    match ty {
        Some(Type::Null | Type::Void | Type::Optional { .. }) => true,
        Some(Type::Union { variants }) => variants
            .iter()
            .any(|variant| is_nullable_type(types.resolve(*variant), types)),
        _ => false,
    }
}

/// Whether the expression is a literal, i.e. a node whose type is only implied
/// by the other operand.
fn is_literal(expr: &HirExpr) -> bool {
    matches!(
        expr,
        HirExpr::Int(..) | HirExpr::Float(..) | HirExpr::String(..) | HirExpr::Bool(..)
    )
}

/// The static type of an operand that is *not* a literal: a literal has no type
/// of its own and is typed by the other operand instead (`literal_fits`).
fn literal_free_ty(expr: &HirExpr, ty: Option<TypeId>) -> Option<TypeId> {
    if is_literal(expr) { None } else { ty }
}

/// Literals carry no type of their own in HIR: they take the other operand's
/// type when it can represent them.
fn literal_fits(expr: &HirExpr, ty: TypeId, types: &TypeTable) -> bool {
    match expr {
        // A literal is *lowered against* this type, not stored in it directly: the
        // destination is the common numeric type of both operands (`right || 1.5`
        // over an `int` is an `f64`), so any numeric type accepts a numeric
        // literal here. A literal that a concrete integer temporary cannot hold is
        // rejected by `int_literal_fits` before the destination is chosen.
        HirExpr::Int(..) | HirExpr::Float(..) => is_numeric_type(ty, types),
        HirExpr::String(..) => matches!(types.resolve(ty), Some(Type::String)),
        HirExpr::Bool(..) => matches!(types.resolve(ty), Some(Type::Bool)),
        _ => false,
    }
}

/// Whether `value` is representable in the integer type `target` without
/// wrapping, which is what makes it a valid Rust literal of that type.
fn int_fits(value: i64, target: &Type) -> bool {
    match target {
        Type::I8 => i8::try_from(value).is_ok(),
        Type::I16 => i16::try_from(value).is_ok(),
        Type::I32 => i32::try_from(value).is_ok(),
        Type::I64 => true,
        Type::U8 => u8::try_from(value).is_ok(),
        Type::U16 => u16::try_from(value).is_ok(),
        Type::U32 => u32::try_from(value).is_ok(),
        Type::U64 => u64::try_from(value).is_ok(),
        _ => false,
    }
}

/// Whether an *integer* literal operand is representable in the integer type the
/// operator is lowered against: `x || 300` over a `u8` yields `300` on one
/// branch, and a `u8` temporary holding it is an out-of-range Rust literal (the
/// generated crate is rejected, and truncating it would answer differently from
/// JavaScript).
///
/// A non-literal operand, a float literal and a non-integer type are left to the
/// numeric widening the lowering already does, so `x || 1.5` over an `int` still
/// meets in `f64`. `None` is the operand of a lowered compound assignment, which
/// is never a literal.
fn int_literal_fits(expr: Option<&HirExpr>, ty: TypeId, types: &TypeTable) -> bool {
    let Some(HirExpr::Int(value, _)) = expr else {
        return true;
    };
    match types.resolve(ty) {
        Some(
            target @ (Type::I8
            | Type::I16
            | Type::I32
            | Type::I64
            | Type::U8
            | Type::U16
            | Type::U32
            | Type::U64),
        ) => int_fits(*value, target),
        _ => true,
    }
}

fn is_nullish_variant(ty: TypeId, types: &TypeTable) -> bool {
    matches!(
        types.resolve(ty),
        Some(Type::Null | Type::Void | Type::Optional { .. })
    )
}

fn is_bigint_type(types: &TypeTable, ty: TypeId) -> bool {
    matches!(types.resolve(ty), Some(Type::BigInt))
}

fn expr_yields_bigint(expr: &HirExpr, types: &TypeTable) -> bool {
    if is_bigint_type(types, expr.ty()) {
        return true;
    }
    match expr {
        HirExpr::Binary { lhs, rhs, .. } => {
            expr_yields_bigint(lhs, types) && expr_yields_bigint(rhs, types)
        }
        HirExpr::Unary { expr, .. } => expr_yields_bigint(expr, types),
        HirExpr::Call { args, .. } => args.iter().any(|a| expr_yields_bigint(a, types)),
        _ => false,
    }
}

fn bigint_op_for_binary(op: HirBinaryOp) -> Option<RuntimeOp> {
    match op {
        HirBinaryOp::Add => Some(RuntimeOp::BigIntAdd),
        HirBinaryOp::Sub => Some(RuntimeOp::BigIntSub),
        HirBinaryOp::Mul => Some(RuntimeOp::BigIntMul),
        HirBinaryOp::Div => Some(RuntimeOp::BigIntDiv),
        HirBinaryOp::Mod => Some(RuntimeOp::BigIntRem),
        HirBinaryOp::Eq => Some(RuntimeOp::BigIntEq),
        HirBinaryOp::Ne => Some(RuntimeOp::BigIntNeq),
        HirBinaryOp::StrictEq => Some(RuntimeOp::BigIntEq),
        HirBinaryOp::StrictNe => Some(RuntimeOp::BigIntNeq),
        HirBinaryOp::Lt => Some(RuntimeOp::BigIntLt),
        HirBinaryOp::Le => Some(RuntimeOp::BigIntLe),
        HirBinaryOp::Gt => Some(RuntimeOp::BigIntGt),
        HirBinaryOp::Ge => Some(RuntimeOp::BigIntGe),
        HirBinaryOp::BitAnd => Some(RuntimeOp::BigIntAnd),
        HirBinaryOp::BitOr => Some(RuntimeOp::BigIntOr),
        HirBinaryOp::BitXor => Some(RuntimeOp::BigIntXor),
        HirBinaryOp::Shl => Some(RuntimeOp::BigIntShl),
        HirBinaryOp::Shr => Some(RuntimeOp::BigIntShr),
        _ => None,
    }
}

fn is_comparison_op(op: HirBinaryOp) -> bool {
    matches!(
        op,
        HirBinaryOp::Eq
            | HirBinaryOp::Ne
            | HirBinaryOp::StrictEq
            | HirBinaryOp::StrictNe
            | HirBinaryOp::Lt
            | HirBinaryOp::Le
            | HirBinaryOp::Gt
            | HirBinaryOp::Ge
    )
}

fn is_equality_op(op: HirBinaryOp) -> bool {
    matches!(
        op,
        HirBinaryOp::Eq | HirBinaryOp::Ne | HirBinaryOp::StrictEq | HirBinaryOp::StrictNe
    )
}

fn is_nullish_hir_literal(expr: &HirExpr) -> bool {
    nullish_literal_kinds(expr).is_some()
}

/// The nullish kinds of an always-nullish HIR literal (`null`, `undefined`,
/// `void`), or `None` when the expression is not one.
///
/// `null` and `undefined` share the `Option::None` representation, so the kinds
/// are what keeps `x || null` (`null`-only) apart from `x || undefined`
/// (`undefined`-only) once the operator's result is optional.
fn nullish_literal_kinds(expr: &HirExpr) -> Option<NullishKinds> {
    match expr {
        HirExpr::Null(_) => Some(NullishKinds::NULL_ONLY),
        HirExpr::Undefined(_) | HirExpr::Unit(_) => Some(NullishKinds::UNDEFINED_ONLY),
        _ => None,
    }
}

fn op_name_for_bigint(op: HirBinaryOp) -> &'static str {
    match op {
        HirBinaryOp::Add => "+",
        HirBinaryOp::Sub => "-",
        HirBinaryOp::Mul => "*",
        HirBinaryOp::Div => "/",
        HirBinaryOp::Mod => "%",
        HirBinaryOp::Eq => "==",
        HirBinaryOp::Ne => "!=",
        HirBinaryOp::StrictEq => "===",
        HirBinaryOp::StrictNe => "!==",
        HirBinaryOp::Lt => "<",
        HirBinaryOp::Le => "<=",
        HirBinaryOp::Gt => ">",
        HirBinaryOp::Ge => ">=",
        HirBinaryOp::BitAnd => "&",
        HirBinaryOp::BitOr => "|",
        HirBinaryOp::BitXor => "^",
        HirBinaryOp::Shl => "<<",
        HirBinaryOp::Shr => ">>",
        _ => "?",
    }
}

fn hir_expr_may_reassign(expr: &HirExpr) -> bool {
    match expr {
        HirExpr::DefaultConstruct { .. } => false,
        HirExpr::Assignment { .. } | HirExpr::CompoundUpdate { .. } => true,
        HirExpr::Field { owner, .. }
        | HirExpr::Unary { expr: owner, .. }
        | HirExpr::Await { expr: owner, .. }
        | HirExpr::TypeAssertion { expr: owner, .. }
        | HirExpr::OptionalChain { base: owner, .. }
        | HirExpr::Yield {
            expr: Some(owner), ..
        }
        | HirExpr::Import { source: owner, .. } => hir_expr_may_reassign(owner),
        HirExpr::Index { owner, index, .. } => {
            hir_expr_may_reassign(owner) || hir_expr_may_reassign(index)
        }
        HirExpr::Binary { lhs, rhs, .. } => {
            hir_expr_may_reassign(lhs) || hir_expr_may_reassign(rhs)
        }
        HirExpr::Call { args, .. } => args.iter().any(hir_expr_may_reassign),
        HirExpr::New { callee, args, .. } => {
            hir_expr_may_reassign(callee) || args.iter().any(hir_expr_may_reassign)
        }
        HirExpr::StructLiteral { fields, .. } => {
            fields.iter().any(|(_, v)| hir_expr_may_reassign(v))
        }
        HirExpr::ObjectLiteral { fields, .. } => {
            fields.iter().any(|f| hir_expr_may_reassign(f.value()))
        }
        HirExpr::Ternary {
            cond,
            then_branch,
            else_branch,
            ..
        } => {
            hir_expr_may_reassign(cond)
                || hir_expr_may_reassign(then_branch)
                || hir_expr_may_reassign(else_branch)
        }
        HirExpr::ArrayLiteral { elements, .. }
        | HirExpr::Sequence {
            exprs: elements, ..
        } => elements.iter().any(hir_expr_may_reassign),
        HirExpr::Template { expressions, .. } => expressions.iter().any(hir_expr_may_reassign),
        HirExpr::Unit(_)
        | HirExpr::Bool(_, _)
        | HirExpr::Int(_, _)
        | HirExpr::Float(_, _)
        | HirExpr::String(_, _)
        | HirExpr::Null(_)
        | HirExpr::Undefined(_)
        | HirExpr::Local { .. }
        | HirExpr::Global { .. }
        | HirExpr::Closure { .. }
        | HirExpr::Yield { expr: None, .. }
        | HirExpr::RegExp { .. }
        | HirExpr::BigInt { .. } => false,
    }
}

fn hir_expr_contains_call(expr: &HirExpr) -> bool {
    match expr {
        HirExpr::DefaultConstruct { .. } => true,
        HirExpr::Call { .. }
        | HirExpr::New { .. }
        | HirExpr::Await { .. }
        | HirExpr::Yield { expr: Some(_), .. }
        | HirExpr::Template { tag: Some(_), .. } => true,
        HirExpr::Field { owner, .. }
        | HirExpr::Unary { expr: owner, .. }
        | HirExpr::TypeAssertion { expr: owner, .. }
        | HirExpr::OptionalChain { base: owner, .. }
        | HirExpr::Import { source: owner, .. } => hir_expr_contains_call(owner),
        HirExpr::Index { owner, index, .. } => {
            hir_expr_contains_call(owner) || hir_expr_contains_call(index)
        }
        HirExpr::Binary { lhs, rhs, .. } => {
            hir_expr_contains_call(lhs) || hir_expr_contains_call(rhs)
        }
        HirExpr::Assignment { target, value, .. } => {
            hir_expr_contains_call(target) || hir_expr_contains_call(value)
        }
        HirExpr::CompoundUpdate { target, rhs, .. } => {
            hir_expr_contains_call(target) || hir_expr_contains_call(rhs)
        }
        HirExpr::StructLiteral { fields, .. } => {
            fields.iter().any(|(_, v)| hir_expr_contains_call(v))
        }
        HirExpr::ObjectLiteral { fields, .. } => {
            fields.iter().any(|f| hir_expr_contains_call(f.value()))
        }
        HirExpr::Ternary {
            cond,
            then_branch,
            else_branch,
            ..
        } => {
            hir_expr_contains_call(cond)
                || hir_expr_contains_call(then_branch)
                || hir_expr_contains_call(else_branch)
        }
        HirExpr::ArrayLiteral { elements, .. }
        | HirExpr::Sequence {
            exprs: elements, ..
        } => elements.iter().any(hir_expr_contains_call),
        HirExpr::Template { expressions, .. } => expressions.iter().any(hir_expr_contains_call),
        HirExpr::Unit(_)
        | HirExpr::Bool(_, _)
        | HirExpr::Int(_, _)
        | HirExpr::Float(_, _)
        | HirExpr::String(_, _)
        | HirExpr::Null(_)
        | HirExpr::Undefined(_)
        | HirExpr::Local { .. }
        | HirExpr::Global { .. }
        | HirExpr::Closure { .. }
        | HirExpr::Yield { expr: None, .. }
        | HirExpr::RegExp { .. }
        | HirExpr::BigInt { .. } => false,
    }
}

fn hir_is_place(expr: &HirExpr) -> bool {
    matches!(
        expr,
        HirExpr::Local { .. }
            | HirExpr::Global { .. }
            | HirExpr::Field { .. }
            | HirExpr::Index { .. }
    )
}

fn is_string_operand(expr: &HirExpr, types: &TypeTable) -> bool {
    match expr {
        HirExpr::String(..) | HirExpr::Template { .. } => true,
        HirExpr::Binary {
            op: HirBinaryOp::Add,
            lhs,
            rhs,
            ..
        } => is_string_operand(lhs, types) || is_string_operand(rhs, types),
        HirExpr::Ternary {
            then_branch,
            else_branch,
            ..
        } => is_string_operand(then_branch, types) && is_string_operand(else_branch, types),
        _ => hir_expr_type_id(expr)
            .and_then(|t| types.resolve(t))
            .is_some_and(|t| matches!(t, Type::String)),
    }
}

impl ExprConverter {
    pub(super) fn convert_binary(
        &mut self,
        op: HirBinaryOp,
        lhs: &HirExpr,
        rhs: &HirExpr,
        ty: TypeId,
        out: &mut Vec<MirStmt>,
        shared_struct_ids: &mut HashMap<TypeId, StructId>,
        shared_next_struct: &mut u32,
        types: &mut TypeTable,
        ctx: &mut PassContext,
    ) -> MirExpr {
        if is_equality_op(op)
            && let Some(operand) = match (is_nullish_hir_literal(lhs), is_nullish_hir_literal(rhs))
            {
                (true, false) => Some(rhs),
                (false, true) => Some(lhs),
                _ => None,
            }
        {
            if let Some(operand_ty) = hir_expr_type_id(operand)
                && matches!(types.resolve(operand_ty), Some(Type::GenericParam { .. }))
            {
                ctx.error(
                    "P0005",
                    "comparing a generic type parameter with `null`/`undefined` cannot be decided \
                     statically in this AOT target; instantiate the type parameter with a concrete \
                     type (and call the function) so the comparison can be monomorphized",
                    operand.span(),
                );
            }
            if let Some(op_name) = match op {
                HirBinaryOp::StrictEq => Some("==="),
                HirBinaryOp::StrictNe => Some("!=="),
                _ => None,
            } {
                // A call operand carries no type of its own in HIR (`Type::Error`),
                // so resolve its declared return type first: otherwise
                // `f() === null` would be accepted merely because the type is
                // unknown, even when `f` returns a value that also includes
                // `undefined`.
                let resolved_ty = match operand {
                    HirExpr::Call { .. } => self.expr_type(operand, types),
                    _ => None,
                };
                crate::hir_to_mir::nullish::check_nullish_comparison_ty(
                    operand,
                    resolved_ty,
                    types,
                    ctx,
                    op_name,
                );
            }
        }
        self.check_comparison_operands(op, lhs, rhs, types, ctx);
        if op == HirBinaryOp::Coalesce {
            return self.convert_coalesce(
                lhs,
                rhs,
                ty,
                out,
                shared_struct_ids,
                shared_next_struct,
                types,
                ctx,
            );
        }
        if op == HirBinaryOp::Add
            && (is_string_operand(lhs, types) || is_string_operand(rhs, types))
        {
            let string_ty = types.intern(&Type::String);
            let lhs_mir = match lhs {
                HirExpr::Null(_) => MirExpr::String {
                    id: Atom::from("null"),
                    ty: string_ty,
                },
                HirExpr::Undefined(_) => MirExpr::String {
                    id: Atom::from("undefined"),
                    ty: string_ty,
                },
                _ => self.convert_expr(lhs, out, shared_struct_ids, shared_next_struct, types, ctx),
            };
            let lhs_is_inline = hir_is_place(lhs) || hir_expr_contains_call(lhs);
            let rhs_may_change_lhs =
                hir_expr_may_reassign(rhs) || (lhs_is_inline && hir_expr_contains_call(rhs));
            let lhs_snapshot_ty = if rhs_may_change_lhs {
                hir_expr_type_id(lhs)
                    .filter(|t| !matches!(types.resolve(*t), None | Some(Type::Error)))
                    .or_else(|| is_string_operand(lhs, types).then_some(string_ty))
            } else {
                None
            };
            let lhs_operand = if let Some(lhs_temp_ty) = lhs_snapshot_ty {
                let lhs_temp = self.fresh_local();
                self.push_temp_local(lhs_temp, lhs_temp_ty);
                out.push(MirStmt::Let {
                    local: lhs_temp,
                    ty: lhs_temp_ty,
                    init: Some(lhs_mir),
                    mutable: false,
                });
                MirExpr::Local(lhs_temp)
            } else {
                lhs_mir
            };
            let rhs_mir = match rhs {
                HirExpr::Null(_) => MirExpr::String {
                    id: Atom::from("null"),
                    ty: string_ty,
                },
                HirExpr::Undefined(_) => MirExpr::String {
                    id: Atom::from("undefined"),
                    ty: string_ty,
                },
                _ => self.convert_expr(rhs, out, shared_struct_ids, shared_next_struct, types, ctx),
            };
            let dest = self.fresh_local();
            self.push_temp_local(dest, string_ty);
            out.push(MirStmt::Runtime {
                op: RuntimeOp::StringConcat,
                args: vec![lhs_operand, rhs_mir],
                dest: Some(dest),
                ty: string_ty,
                target_ty: None,
            });
            return MirExpr::Local(dest);
        }
        if let Some(bigint_op) = bigint_op_for_binary(op)
            && expr_yields_bigint(lhs, types)
            && expr_yields_bigint(rhs, types)
        {
            return self.convert_bigint_binary(
                bigint_op,
                op,
                lhs,
                rhs,
                ty,
                out,
                shared_struct_ids,
                shared_next_struct,
                types,
                ctx,
            );
        } else if bigint_op_for_binary(op).is_some()
            && (expr_yields_bigint(lhs, types) || expr_yields_bigint(rhs, types))
        {
            ctx.warning(
                "P0005",
                format!(
                    "mixed BigInt and non-BigInt operands in `{}`; the operation is lowered \
                     using the left operand as-is; rhs was {:?}, lhs was {:?}",
                    op_name_for_bigint(op),
                    types.resolve(rhs.ty()),
                    types.resolve(lhs.ty()),
                ),
                lhs.span(),
            );
            let lhs_mir =
                self.convert_expr(lhs, out, shared_struct_ids, shared_next_struct, types, ctx);
            let lhs_temp_ty = hir_expr_type_id(lhs).unwrap_or(ty);
            let lhs_temp = self.fresh_local();
            self.push_temp_local(lhs_temp, lhs_temp_ty);
            out.push(MirStmt::Let {
                local: lhs_temp,
                ty: lhs_temp_ty,
                init: Some(lhs_mir),
                mutable: false,
            });
            let rhs_mir =
                self.convert_expr(rhs, out, shared_struct_ids, shared_next_struct, types, ctx);
            if has_potential_side_effects(&rhs_mir) {
                out.push(MirStmt::Expr(rhs_mir));
            }
            return MirExpr::Local(lhs_temp);
        }
        match op {
            HirBinaryOp::In => {
                let lhs_mir =
                    self.convert_expr(lhs, out, shared_struct_ids, shared_next_struct, types, ctx);
                let rhs_mir =
                    self.convert_expr(rhs, out, shared_struct_ids, shared_next_struct, types, ctx);
                let bool_ty = types.intern(&Type::Bool);
                let dest = self.fresh_local();
                self.push_temp_local(dest, bool_ty);
                out.push(MirStmt::Runtime {
                    op: RuntimeOp::OpIn,
                    args: vec![lhs_mir, rhs_mir],
                    dest: Some(dest),
                    ty: bool_ty,
                    target_ty: None,
                });
                MirExpr::Local(dest)
            }
            HirBinaryOp::InstanceOf => {
                let value_mir =
                    self.convert_expr(lhs, out, shared_struct_ids, shared_next_struct, types, ctx);
                let target_mir =
                    self.convert_expr(rhs, out, shared_struct_ids, shared_next_struct, types, ctx);
                let target_type_id: u32 = match rhs {
                    HirExpr::Global { ty, .. } => {
                        shared_struct_ids.get(ty).map(|sid| sid.raw()).unwrap_or(0)
                    }
                    _ => {
                        ctx.warning(
                            "P0005",
                            "instanceof rhs must be a class reference (HirExpr::Global); \
                             dynamic constructor expressions like getConstructor() are not \
                             yet supported (PR 1.6: identity of non-Global rhs cannot be \
                             resolved at convert time). rhs is still evaluated and its side \
                             effects preserved; runtime returns false.",
                            Span::new(0, 0),
                        );
                        0
                    }
                };
                let dest = self.fresh_local();
                self.push_temp_local(dest, ty);
                out.push(MirStmt::Runtime {
                    op: RuntimeOp::OpInstanceof,
                    args: vec![
                        value_mir,
                        target_mir,
                        MirExpr::Int {
                            value: target_type_id as i128,
                            ty: TypeId::from_raw(0),
                        },
                    ],
                    dest: Some(dest),
                    ty,
                    target_ty: None,
                });
                MirExpr::Local(dest)
            }
            _ => {
                if matches!(op, HirBinaryOp::And | HirBinaryOp::Or)
                    && let Some(logical) = self.convert_operand_logical(
                        op,
                        lhs,
                        rhs,
                        ty,
                        out,
                        shared_struct_ids,
                        shared_next_struct,
                        types,
                        ctx,
                    )
                {
                    return logical;
                }
                MirExpr::Binary {
                    op: convert_binop(op, ctx),
                    left: Box::new(self.convert_expr(
                        lhs,
                        out,
                        shared_struct_ids,
                        shared_next_struct,
                        types,
                        ctx,
                    )),
                    right: Box::new(self.convert_expr(
                        rhs,
                        out,
                        shared_struct_ids,
                        shared_next_struct,
                        types,
                        ctx,
                    )),
                    ty,
                }
            }
        }
    }

    /// Rejects comparisons the generated Rust cannot express.
    ///
    /// `===`/`!==`/`==`/`!=` against `null`/`undefined` are handled by the
    /// nullish machinery and folded by the backend, so they are left alone. What
    /// is left is a comparison where one side is an optional (`T | null`) and
    /// the other is a non-optional type the AOT target cannot compare against
    /// `Option<T>`: `g === "0"`, `t === 1`, `b === 1`, and the relational
    /// operators, which have no optional-aware lowering at all (`g < 5` used to
    /// emit `Option<f64> < f64`).
    fn check_comparison_operands(
        &self,
        op: HirBinaryOp,
        lhs: &HirExpr,
        rhs: &HirExpr,
        types: &mut TypeTable,
        ctx: &mut PassContext,
    ) {
        if !is_comparison_op(op) || is_nullish_hir_literal(lhs) || is_nullish_hir_literal(rhs) {
            return;
        }
        let lhs_ty = self.operand_static_ty(lhs, types);
        let rhs_ty = self.operand_static_ty(rhs, types);
        let lhs_inner = optional_inner(lhs_ty, types);
        let rhs_inner = optional_inner(rhs_ty, types);
        let op_name = comparison_op_name(op);
        let left = operand_type_name(lhs, lhs_ty, types);
        let right = operand_type_name(rhs, rhs_ty, types);
        if is_relational_op(op) {
            if lhs_inner.is_some() || rhs_inner.is_some() {
                ctx.error(
                    NULLISH_NARROWING_DIAG,
                    format!(
                        "`{op_name}` cannot compare an optional value: the left operand is \
                         `{left}` and the right one is `{right}`. The AOT target represents an \
                         optional value as `Option<T>` and has no ordering operator for it; handle \
                         the nullish case first (for example with `x!` after a check, or by \
                         narrowing the operand's type)"
                    ),
                    lhs.span(),
                );
            }
            return;
        }
        let incompatible = if let (Some(a), Some(b)) = (lhs_inner, rhs_inner) {
            !types_compatible(a, b, types)
        } else if let Some(a) = lhs_inner {
            rhs_ty.is_some_and(|b| !types_compatible(a, b, types))
        } else if let Some(b) = rhs_inner {
            lhs_ty.is_some_and(|a| !types_compatible(a, b, types))
        } else {
            false
        };
        if incompatible {
            ctx.error(
                LOGICAL_DIAG,
                format!(
                    "`{op_name}` cannot compare `{left}` with `{right}`: the AOT target lowers an \
                     optional value as `Option<T>`, so a comparison against an unrelated \
                     non-optional type has no Rust representation. Narrow the optional operand \
                     first (for example with `x!` after a check), or compare it against a value of \
                     its own type"
                ),
                lhs.span(),
            );
        }
    }

    /// The optional type that holds `inner` plus the nullish kinds `nullish`.
    fn optional_type(&self, inner: TypeId, nullish: NullishKinds, types: &mut TypeTable) -> TypeId {
        types.intern(&Type::Optional { inner, nullish })
    }

    fn report_operand_mismatch(
        &self,
        op: HirBinaryOp,
        lhs_ty: TypeId,
        rhs_ty: TypeId,
        span: Span,
        types: &TypeTable,
        ctx: &mut PassContext,
    ) {
        ctx.error(
            LOGICAL_DIAG,
            format!(
                "`{}` needs both operands to have the same type (it yields one of them); the left \
                 operand is `{}` and the right one is `{}`",
                logical_op_name(op),
                type_name(types, lhs_ty),
                type_name(types, rhs_ty),
            ),
            span,
        );
    }

    fn report_unknown_operand(
        &self,
        op: HirBinaryOp,
        side: &str,
        name: &str,
        span: Span,
        ctx: &mut PassContext,
    ) {
        ctx.error(
            LOGICAL_DIAG,
            format!(
                "the type of the {side} operand of `{}` is not known statically (`{name}`), so the \
                 AOT target cannot lower the operator (it yields one of its operands, which Rust's \
                 own operators cannot express); annotate the value (or the function that produced \
                 it)",
                logical_op_name(op)
            ),
            span,
        );
    }

    fn report_untruthy_operand(
        &self,
        op: HirBinaryOp,
        side: &str,
        ty: TypeId,
        span: Span,
        types: &TypeTable,
        ctx: &mut PassContext,
    ) {
        ctx.error(
            LOGICAL_DIAG,
            format!(
                "the truthiness of the {side} operand of `{}` cannot be decided: its type is `{}`, \
                 which the AOT target has no Rust representation for (a union may hold variants \
                 with different truthiness). Narrow it to `T | null` (or `T | undefined`), which \
                 lowers to `Option<T>`, or split the expression",
                logical_op_name(op),
                type_name(types, ty),
            ),
            span,
        );
    }

    /// Whether `expected` is nothing more than the frontend's own inference of
    /// the expression being converted, rather than an independent requirement.
    ///
    /// The frontend types `x ?? <nullish literal>` as the non-nullish part of
    /// `x`, because the literal carries no type of its own, and an unannotated
    /// binding simply takes that inferred type (`const r = a ?? null` is typed
    /// `number` for `a: number | undefined` even though the value may be
    /// `null`). The operator's lowering stays optional, so comparing it with
    /// the very type that same inference produced rejects valid TypeScript.
    ///
    /// A *declared* return type is a requirement of its own even when it
    /// happens to coincide with the inference (`function f(): number { return a
    /// ?? null; }` really cannot lower to a plain `f64`), so a return position
    /// keeps being reported.
    fn expectation_is_the_expression_inference(
        &self,
        expected: Option<TypeId>,
        expr_ty: TypeId,
    ) -> bool {
        expected == Some(expr_ty) && self.current_return_ty != expected
    }

    /// Reports a result that may be nullish where the surrounding context (a
    /// declared return type, an annotation or a parameter) requires a value.
    /// `expected` is the expectation *of this operator's result*, captured before
    /// its operands were converted (they are allowed to be optional).
    fn report_optional_result(
        &self,
        op: HirBinaryOp,
        dest_ty: TypeId,
        expected: Option<TypeId>,
        span: Span,
        types: &TypeTable,
        ctx: &mut PassContext,
    ) {
        let Some(Type::Optional { inner, .. }) = types.resolve(dest_ty) else {
            return;
        };
        let inner = *inner;
        let Some(expected) = expected else {
            return;
        };
        let Some(expected_resolved) = types.resolve(expected) else {
            return;
        };
        if matches!(expected_resolved, Type::Error) {
            return;
        }
        if is_nullable_type(Some(expected_resolved), types) {
            // Both sides are optional, but their *kinds* still have to agree: the
            // result of `g() && 5` over `number | undefined` is never `null`, and
            // a `number | null` target stores it as the same `Option::None`, so a
            // `=== null` test on it would answer `true` for a JavaScript
            // `undefined`. `check_nullish_write_kinds` reports exactly that (and
            // stays permissive for a target whose kinds are unknown).
            crate::hir_to_mir::nullish::check_nullish_write_kinds(
                kinds_of_ty(dest_ty, types),
                expected,
                types,
                span,
                ctx,
                &format!("the target type of this `{}` result", logical_op_name(op)),
            );
            return;
        }
        if !types_compatible(inner, expected, types) {
            return;
        }
        ctx.error(
            NULLISH_NARROWING_DIAG,
            format!(
                "the result of `{}` is `{}` (the operator yields one of its operands, and one of \
                 them may be nullish), but a non-optional `{}` is required here. Handle the \
                 nullish case first (for example with `x!` after a check, or by widening the \
                 target type)",
                logical_op_name(op),
                type_name(types, dest_ty),
                type_name(types, expected),
            ),
            span,
        );
    }

    /// Converts an expression whose type is decided by its *own* position, not
    /// by the type the enclosing expression must produce.
    ///
    /// The operand of `??`, `||`, `&&` or the condition of a conditional is
    /// allowed to be optional even when the operator's result is not: in
    /// `return (x || y) ?? 0` and `c ? (x && y) : z` the outer expectation must
    /// not leak into the nested operator, or it would report a phantom `P0018`.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn convert_without_expected(
        &mut self,
        expr: &HirExpr,
        out: &mut Vec<MirStmt>,
        shared_struct_ids: &mut HashMap<TypeId, StructId>,
        shared_next_struct: &mut u32,
        types: &mut TypeTable,
        ctx: &mut PassContext,
    ) -> MirExpr {
        let saved = self.expected_ty;
        self.expected_ty = None;
        let mir = self.convert_expr(expr, out, shared_struct_ids, shared_next_struct, types, ctx);
        self.expected_ty = saved;
        mir
    }

    /// `left ?? right`.
    ///
    /// The AOT target represents `T | null` as `Option<T>`, so a nullable
    /// left-hand side is *unwrapped* and the fallback is evaluated only when
    /// the value is nullish. A left-hand side that can never be nullish makes
    /// the whole expression equal to it: JavaScript never evaluates the
    /// fallback in that case.
    ///
    /// The result materialises in a temporary assigned from an `if`, mirroring
    /// `convert_ternary`, so the statements produced by converting the fallback
    /// stay inside its branch and are not evaluated unconditionally.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn convert_coalesce(
        &mut self,
        lhs: &HirExpr,
        rhs: &HirExpr,
        ty: TypeId,
        out: &mut Vec<MirStmt>,
        shared_struct_ids: &mut HashMap<TypeId, StructId>,
        shared_next_struct: &mut u32,
        types: &mut TypeTable,
        ctx: &mut PassContext,
    ) -> MirExpr {
        match lhs {
            // `null ?? x` and `undefined ?? x` are always the fallback.
            HirExpr::Null(_) | HirExpr::Undefined(_) | HirExpr::Unit(_) => {
                return self.convert_expr(
                    rhs,
                    out,
                    shared_struct_ids,
                    shared_next_struct,
                    types,
                    ctx,
                );
            }
            // A literal cannot be nullish, so the fallback is dead code.
            HirExpr::Int(..) | HirExpr::Float(..) | HirExpr::String(..) | HirExpr::Bool(..) => {
                return self.convert_expr(
                    lhs,
                    out,
                    shared_struct_ids,
                    shared_next_struct,
                    types,
                    ctx,
                );
            }
            _ => {}
        }
        let lhs_hir_ty = known_type(hir_expr_type_id(lhs), types);
        // The left-hand side is evaluated once and unconditionally (JavaScript
        // evaluates it whatever the fallback does). It is *allowed* to be
        // optional, so the enclosing expectation must not leak into it:
        // `(x || y) ?? 0` returns a plain `number` even though the `??` operand
        // may be nullish.
        let left_mir = self.convert_without_expected(
            lhs,
            out,
            shared_struct_ids,
            shared_next_struct,
            types,
            ctx,
        );
        // Calls and member reads are untyped in HIR, but the converted MIR
        // expression knows its type (`c?.m()`, `obj.field`), and a plain call to
        // a declared function is typed by that declaration (`side() ?? 7` with
        // `side(): null` is the fallback, not an "unknown type" error).
        let lhs_ty = lhs_hir_ty
            .or_else(|| self.mir_expr_static_ty(&left_mir, types))
            .or_else(|| self.hir_static_ty(lhs, types));
        self.convert_coalesce_operand(
            left_mir,
            lhs_ty,
            lhs.span(),
            rhs,
            ty,
            out,
            shared_struct_ids,
            shared_next_struct,
            types,
            ctx,
            None,
        )
    }

    /// `left ?? right` where the left operand is already lowered.
    ///
    /// `x ??= v` loads `x` into a temporary before the operator is lowered, so
    /// the left operand has no HIR node to convert: the loaded value is handed
    /// over as a [`MirExpr`] together with its static type and the span its
    /// diagnostics use. Resolving a synthetic HIR `Local` through the `LocalId`
    /// map instead would allocate a fresh, never declared local for it.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn convert_coalesce_operand(
        &mut self,
        left_mir: MirExpr,
        lhs_ty: Option<TypeId>,
        lhs_span: Span,
        rhs: &HirExpr,
        ty: TypeId,
        out: &mut Vec<MirStmt>,
        shared_struct_ids: &mut HashMap<TypeId, StructId>,
        shared_next_struct: &mut u32,
        types: &mut TypeTable,
        ctx: &mut PassContext,
        setter: Option<(ts_aot_core::FunctionId, Vec<MirExpr>, TypeId)>,
    ) -> MirExpr {
        // The enclosing expectation of the *result*, read after the left operand
        // was converted (that conversion restores it).
        let expected_ty = self.expected_ty;
        let inner = match lhs_ty.and_then(|t| types.resolve(t)).cloned() {
            Some(Type::Optional { inner, .. })
                if !matches!(types.resolve(inner), None | Some(Type::Error)) =>
            {
                inner
            }
            // Always nullish: the value is `undefined`/`void`, never a value, so
            // the expression is the fallback. The left operand is still
            // evaluated first (JavaScript evaluates it whatever the fallback
            // does), and a call is converted to an inline expression - emit it,
            // or its effects would be lost.
            Some(Type::Null) | Some(Type::Void) => {
                if has_potential_side_effects(&left_mir) {
                    out.push(MirStmt::Expr(left_mir));
                }
                let right_mir =
                    self.convert_expr(rhs, out, shared_struct_ids, shared_next_struct, types, ctx);
                return self.logical_setter_value(right_mir, setter, out, types, rhs, ctx);
            }
            Some(Type::Error) | None | Some(Type::GenericParam { .. }) => {
                ctx.error(
                    LOGICAL_DIAG,
                    "the type of the left-hand side of `??` is not known statically, so the AOT \
                     target cannot decide whether the fallback must be evaluated; annotate the \
                     value (or the function that produced it)",
                    lhs_span,
                );
                return left_mir;
            }
            Some(Type::Union { variants })
                if variants.iter().any(|v| is_nullish_variant(*v, types)) =>
            {
                ctx.error(
                    LOGICAL_DIAG,
                    "the left-hand side of `??` may be nullish but its type is a union the AOT \
                     target cannot represent as an optional value; narrow it to `T | null` (or \
                     `T | undefined`) so the fallback can be lowered",
                    lhs_span,
                );
                return left_mir;
            }
            // Never nullish: `left ?? right` short-circuits to `left`.
            _ => return left_mir,
        };
        // The fallback's representation is decided before the destination: a
        // call or a member read carries no type in HIR, so the *converted*
        // fallback (or the callee's declaration) supplies the type the
        // destination has to accept. Converting it first also keeps its
        // statements inside the nullish branch.
        let mut else_stmts: Vec<MirStmt> = Vec::new();
        let right_mir = self.convert_expr(
            rhs,
            &mut else_stmts,
            shared_struct_ids,
            shared_next_struct,
            types,
            ctx,
        );
        let right_mir =
            self.logical_setter_value(right_mir, setter.clone(), &mut else_stmts, types, rhs, ctx);
        // A literal has no type of its own in HIR (it is typed by the slot it
        // fills), while a call or a member read is typed by its converted value
        // and, failing that, by the callee's declaration. Both are recovered
        // before the destination is chosen, so the destination can accept what
        // the fallback branch actually produces.
        let rhs_ty = known_type(hir_expr_type_id(rhs), types).or_else(|| {
            if is_literal(rhs) {
                None
            } else {
                self.mir_expr_static_ty(&right_mir, types)
            }
        });
        let rhs_ty = match rhs_ty {
            Some(ty) => Some(ty),
            // A literal keeps no type of its own: it is typed by the destination
            // (`literal_fits`), and only its *converted* value supplies the
            // numeric type the common numeric type needs (`1.5` is `f64`).
            None if is_literal(rhs) => None,
            None => self.hir_static_ty(rhs, types),
        };
        let rhs_resolved = rhs_ty.and_then(|t| types.resolve(t)).cloned();
        // `a ?? null` is `a`: a nullish fallback cannot add a value, because the
        // AOT target represents it as the same `Option::None` its left operand's
        // own nullish case produces. `null`/`undefined` literals carry no type
        // id of their own, so they are classified explicitly.
        let nullish_fallback =
            is_nullish_hir_literal(rhs) || matches!(rhs_resolved, Some(Type::Null | Type::Void));
        let rhs_kinds = if is_nullish_hir_literal(rhs) {
            kinds_of_hir(rhs, types)
        } else {
            rhs_ty.map_or(NO_KINDS, |ty| kinds_of_ty(ty, types))
        };
        // A nullable fallback keeps the optional result: `a ?? b` is `a` when it
        // is not nullish, otherwise `b` (which may be nullish itself), so the
        // result's nullish kinds are the fallback's ones.
        let rhs_value_ty = match rhs_ty {
            Some(ty) => Some(optional_value_ty(ty, types)),
            None if is_literal(rhs) => self
                .mir_expr_static_ty(&right_mir, types)
                .map(|ty| optional_value_ty(ty, types)),
            None => None,
        };
        let nullable_fallback = nullish_fallback
            || (matches!(rhs_resolved, Some(Type::Optional { .. }))
                && rhs_value_ty.is_some_and(|rhs_value| {
                    rhs_value == inner
                        || (is_numeric_type(inner, types) && is_numeric_type(rhs_value, types))
                }));
        // Compatible mixed `int`/`float` operands meet in the common numeric
        // type, so the fractional fallback keeps its fraction.
        let value_ty = rhs_value_ty
            .and_then(|rhs_value| common_numeric_ty(inner, rhs_value, types))
            .unwrap_or(inner);
        // A nullish fallback is `Option::None`. When the left operand's
        // non-nullish part already carries that nullish kind - a nested optional
        // such as the ABI type `Option<Option<T>>` of a defaulted `x: T | null`
        // parameter - the result is that part itself; otherwise the nullish
        // fallback needs its own optional layer to be representable.
        let nullish_fits_inner = nullish_fallback && (rhs_kinds.null || rhs_kinds.undefined) && {
            let inner_kinds = kinds_of_ty(inner, types);
            (!rhs_kinds.null || inner_kinds.null) && (!rhs_kinds.undefined || inner_kinds.undefined)
        };
        let compatible = if nullable_fallback {
            // A nullish fallback adds no value: `a ?? null` is `a`.
            true
        } else {
            match rhs_ty {
                // A literal carries no type of its own, so it is the destination
                // that types it and it has to fit: a string or a bool cannot fill
                // a numeric `inner`, and an integer literal has to be
                // representable in it (`a ?? 300` over a `u8 | null` would emit
                // an out-of-range Rust literal).
                None if is_literal(rhs) => {
                    // The literal fills the *value* level of the destination: a
                    // nested optional (the ABI of a defaulted nullable parameter,
                    // whose non-nullish part is itself `T | null`) takes it in its
                    // inner type.
                    let target = optional_value_ty(inner, types);
                    literal_fits(rhs, target, types) && int_literal_fits(Some(rhs), target, types)
                }
                // An unknown *non-literal* (a call or a member read the passes
                // cannot type from HIR) keeps the long-standing permissive path.
                None => true,
                Some(ty) => {
                    // An unresolvable id counts as unknown, not as a mismatch: the
                    // old check accepted `rhs_resolved == None` for the same reason.
                    rhs_resolved.is_none()
                        || matches!(types.resolve(ty), Some(Type::Error))
                        || rhs_resolved.as_ref() == types.resolve(inner)
                        || (is_numeric_type(inner, types) && is_numeric_type(ty, types))
                }
            }
        };
        if !compatible {
            ctx.error(
                LOGICAL_DIAG,
                format!(
                    "the fallback of `??` must have the same type as the non-nullish part of its \
                     left-hand side; the left-hand side is `{}`, the fallback is `{}`",
                    type_name(types, inner),
                    rhs_ty
                        .map(|t| type_name(types, t))
                        .unwrap_or_else(|| "<unknown>".to_owned()),
                ),
                rhs.span(),
            );
        }
        let optional_ty = lhs_ty.unwrap_or(inner);
        let (left_place, mut left_stmts) = self.bind_to_temp(left_mir, optional_ty);
        out.append(&mut left_stmts);
        let bool_ty = types.intern(&Type::Bool);
        let cond = MirExpr::IsSome {
            expr: Box::new(left_place.clone()),
            ty: bool_ty,
        };
        let (dest_ty, then_value) = if nullish_fits_inner {
            // The result is the value inside the left operand; the nullish
            // fallback is the `None` its own type already holds.
            (
                value_ty,
                MirExpr::UnwrapOptional {
                    expr: Box::new(left_place),
                    ty: inner,
                },
            )
        } else if nullable_fallback {
            // The result stays optional. The left operand's value is re-wrapped
            // when the common numeric type differs from its own, so the
            // destination can hold it.
            let then_value = if value_ty == inner {
                left_place
            } else {
                MirExpr::UnwrapOptional {
                    expr: Box::new(left_place),
                    ty: inner,
                }
            };
            (self.optional_type(value_ty, rhs_kinds, types), then_value)
        } else {
            (
                value_ty,
                MirExpr::UnwrapOptional {
                    expr: Box::new(left_place),
                    ty: inner,
                },
            )
        };
        let else_value = if nullish_fallback {
            // A nullish fallback still has to run when the left operand is
            // nullish, but it contributes `Option::None`.
            if has_potential_side_effects(&right_mir) {
                else_stmts.push(MirStmt::Expr(right_mir));
            }
            MirExpr::Unit
        } else {
            right_mir
        };
        // A nullish fallback keeps the result optional, so a non-optional
        // position must be reported here: the converted value is a lowering
        // temp, which the `return` check cannot see. A binding that merely
        // adopts the frontend's own inference of this expression is not such a
        // position (see `expectation_is_the_expression_inference`).
        if !(is_nullish_hir_literal(rhs)
            && self.expectation_is_the_expression_inference(expected_ty, ty))
        {
            self.report_optional_result(
                HirBinaryOp::Coalesce,
                dest_ty,
                expected_ty,
                lhs_span,
                types,
                ctx,
            );
        }
        self.finish_conditional_value(
            cond,
            then_value,
            Vec::new(),
            else_value,
            else_stmts,
            dest_ty,
            out,
        )
    }

    /// `left && right` / `left || right` over operands that are not `bool`.
    ///
    /// JavaScript yields one of the *operands* (not a `bool`), so Rust's `||`
    /// and `&&` cannot be used. The `bool` case is exact with Rust's own
    /// operators, but only while lowering the right operand produces no
    /// statements, so it is decided by [`Self::convert_operand_logical_lowered`],
    /// which can see them.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn convert_operand_logical(
        &mut self,
        op: HirBinaryOp,
        lhs: &HirExpr,
        rhs: &HirExpr,
        _ty: TypeId,
        out: &mut Vec<MirStmt>,
        shared_struct_ids: &mut HashMap<TypeId, StructId>,
        shared_next_struct: &mut u32,
        types: &mut TypeTable,
        ctx: &mut PassContext,
    ) -> Option<MirExpr> {
        let lhs_hir_ty = self.operand_static_ty(lhs, types);
        // `null`/`undefined` are always falsy, so the operator is decided by the
        // left operand alone: `null || x` is `x` and `null && x` is the nullish
        // literal itself, with the right operand not evaluated at all. This must
        // be handled before `bind_to_temp`, which would declare a plain-value
        // temporary of the *other* operand's type (`let tmp: f64 = ();`).
        if nullish_literal_kinds(lhs).is_some() {
            if op == HirBinaryOp::And {
                // The result *is* the nullish literal, so a surrounding context
                // that requires a value has no lowering for it.
                if let Some(expected) = self.expected_ty {
                    crate::hir_to_mir::nullish::check_nullish_write(
                        lhs,
                        expected,
                        types,
                        lhs.span(),
                        ctx,
                        "the result of this operator",
                        false,
                    );
                }
                return Some(self.convert_expr(
                    lhs,
                    out,
                    shared_struct_ids,
                    shared_next_struct,
                    types,
                    ctx,
                ));
            }
            return Some(self.convert_expr(
                rhs,
                out,
                shared_struct_ids,
                shared_next_struct,
                types,
                ctx,
            ));
        }
        // The operands are allowed to be optional whatever the surrounding
        // context expects from the *result* (see `convert_without_expected`).
        // The left operand is converted before the right one, whatever branch
        // lowers them.
        let expected_ty = self.expected_ty;
        let left_mir = self.convert_without_expected(
            lhs,
            out,
            shared_struct_ids,
            shared_next_struct,
            types,
            ctx,
        );
        self.convert_operand_logical_lowered(
            op,
            left_mir,
            Some(lhs),
            lhs_hir_ty,
            lhs.span(),
            rhs,
            expected_ty,
            out,
            shared_struct_ids,
            shared_next_struct,
            types,
            ctx,
            None,
        )
    }

    /// The type both operands of `&&`/`||` are lowered against.
    ///
    /// `lhs` is `None` when the left operand has no HIR node of its own (an
    /// already-loaded compound-assignment target): such an operand is never a
    /// literal, so the arms that test the left operand's literal-ness do not
    /// apply to it.
    #[allow(clippy::too_many_arguments)]
    fn select_logical_operand_ty(
        &mut self,
        op: HirBinaryOp,
        lhs: Option<&HirExpr>,
        lhs_hir_ty: Option<TypeId>,
        lhs_span: Span,
        rhs: &HirExpr,
        rhs_hir_ty: Option<TypeId>,
        types: &mut TypeTable,
        ctx: &mut PassContext,
    ) -> Option<TypeId> {
        match (lhs_hir_ty, rhs_hir_ty) {
            // A known common type is the type both branches are lowered against, so
            // it has to *hold* an integer literal operand: `x || 300` over a `u8`
            // yields `300` on one branch, and a `u8` temporary holding it is an
            // out-of-range Rust literal.
            (Some(a), Some(_))
                if !int_literal_fits(lhs, a, types) || !int_literal_fits(Some(rhs), a, types) =>
            {
                ctx.error(
                    LOGICAL_DIAG,
                    format!(
                        "the literal operand of `{}` does not fit `{}`; the operator yields one of \
                         its operands, so both branches have to be representable in the same Rust \
                         type",
                        logical_op_name(op),
                        type_name(types, a)
                    ),
                    lhs_span,
                );
                None
            }
            (Some(a), Some(_)) => Some(a),
            // A literal has no type of its own: it takes the other operand's type
            // when that type can represent it.
            (Some(a), None) if literal_fits(rhs, a, types) => Some(a),
            // A call or an object-literal field carries no type in HIR at all: it
            // is lowered against the known side (the backend coerces the value).
            (Some(a), None) if !is_literal(rhs) => Some(a),
            (None, Some(b)) if lhs.is_some_and(|lhs| literal_fits(lhs, b, types)) => Some(b),
            (None, Some(b)) if !lhs.is_some_and(is_literal) => Some(b),
            _ => {
                let message = if lhs_hir_ty.is_none() && rhs_hir_ty.is_none() {
                    format!(
                        "the operand types of `{}` are not known statically, so the AOT target \
                         cannot lower the operator (it yields one of its operands, which Rust's own \
                         operators cannot express); annotate the operands",
                        logical_op_name(op)
                    )
                } else {
                    format!(
                        "`{}` needs both operands to have the same statically known type (it \
                         yields one of them, so the result has no single Rust type); annotate the \
                         operands or split the expression",
                        logical_op_name(op)
                    )
                };
                ctx.error(LOGICAL_DIAG, message, lhs_span);
                None
            }
        }
    }

    /// `left && right` / `left || right` where the left operand is already
    /// lowered.
    ///
    /// `x &&= v` / `x ||= v` load `x` into a temporary before the operator is
    /// lowered, so the operand has no HIR node to convert: the loaded value is
    /// handed over as a [`MirExpr`] together with its static type and the span
    /// its diagnostics use. Resolving a synthetic HIR `Local` through the
    /// `LocalId` map instead would allocate a fresh, never declared local for it.
    ///
    /// Returns `None` for `bool` operands, where the plain `MirExpr::Binary` is
    /// exact.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn convert_operand_logical_lowered(
        &mut self,
        op: HirBinaryOp,
        left_mir: MirExpr,
        lhs: Option<&HirExpr>,
        lhs_hir_ty: Option<TypeId>,
        lhs_span: Span,
        rhs: &HirExpr,
        expected_ty: Option<TypeId>,
        out: &mut Vec<MirStmt>,
        shared_struct_ids: &mut HashMap<TypeId, StructId>,
        shared_next_struct: &mut u32,
        types: &mut TypeTable,
        ctx: &mut PassContext,
        setter: Option<(ts_aot_core::FunctionId, Vec<MirExpr>, TypeId)>,
    ) -> Option<MirExpr> {
        let rhs_hir_ty = self.operand_static_ty(rhs, types);
        let lhs_bool = lhs_hir_ty.is_some_and(|t| matches!(types.resolve(t), Some(Type::Bool)))
            || lhs.is_some_and(|lhs| matches!(lhs, HirExpr::Bool(..)));
        let rhs_bool = rhs_hir_ty.is_some_and(|t| matches!(types.resolve(t), Some(Type::Bool)))
            || matches!(rhs, HirExpr::Bool(..));
        if lhs_bool && rhs_bool {
            // `bool` is the one case Rust's own `||`/`&&` express exactly, and the
            // right operand stays lazy inside the emitted `a && b`. That is only
            // exact while lowering the right operand produces no *statements*:
            // those are appended to the enclosing block (the caller passes `out`
            // straight through) and would run before the operator. When there are
            // any, the short circuit is built by hand so they stay in the branch
            // the operator guards - the way the `??` lowering keeps its fallback
            // inside the nullish branch.
            let bool_ty = types.intern(&Type::Bool);
            let mut rhs_stmts: Vec<MirStmt> = Vec::new();
            let right_mir = self.convert_without_expected(
                rhs,
                &mut rhs_stmts,
                shared_struct_ids,
                shared_next_struct,
                types,
                ctx,
            );
            let right_mir = self.logical_setter_value(
                right_mir,
                setter.clone(),
                &mut rhs_stmts,
                types,
                rhs,
                ctx,
            );
            if rhs_stmts.is_empty() {
                return Some(MirExpr::Binary {
                    op: convert_binop(op, ctx),
                    left: Box::new(left_mir),
                    right: Box::new(right_mir),
                    ty: bool_ty,
                });
            }
            let (then_value, then_stmts, else_value, else_stmts) = match op {
                // `a && b` is `b` when `a` is truthy, and the falsy `a` otherwise.
                HirBinaryOp::And => (right_mir, rhs_stmts, MirExpr::Bool(false), Vec::new()),
                // `a || b` is the truthy `a`, and `b` otherwise.
                _ => (MirExpr::Bool(true), Vec::new(), right_mir, rhs_stmts),
            };
            return Some(self.finish_conditional_value(
                left_mir, then_value, then_stmts, else_value, else_stmts, bool_ty, out,
            ));
        }
        // An optional left operand is compared by the truthiness of the value
        // *inside* it: `null`/`undefined` are falsy, so `a || b` yields the inner
        // value and `a && b` yields `a` itself. Rust has no such notion for
        // `Option<T>`, so this case is lowered by hand (including the result type,
        // which depends on whether the right operand can be nullish).
        if let Some(lhs_ty) = lhs_hir_ty
            && let Some(Type::Optional { inner, .. }) = types.resolve(lhs_ty)
        {
            let inner = *inner;
            return Some(self.convert_optional_operand_logical(
                op,
                left_mir,
                lhs_span,
                rhs,
                lhs_ty,
                inner,
                out,
                shared_struct_ids,
                shared_next_struct,
                types,
                ctx,
                setter.clone(),
            ));
        }
        let Some(lhs_ty) = self
            .select_logical_operand_ty(op, lhs, lhs_hir_ty, lhs_span, rhs, rhs_hir_ty, types, ctx)
        else {
            // The diagnostic was reported; the operator yields one of its
            // operands, so the left operand's value is what is left to return.
            return Some(left_mir);
        };
        // The left operand may only be typed through the right one (a call, an
        // object-literal field): an optional left still belongs to the dedicated
        // lowering, which compares the value inside it.
        if let Some(Type::Optional { inner, .. }) = types.resolve(lhs_ty) {
            let inner = *inner;
            return Some(self.convert_optional_operand_logical(
                op,
                left_mir,
                lhs_span,
                rhs,
                lhs_ty,
                inner,
                out,
                shared_struct_ids,
                shared_next_struct,
                types,
                ctx,
                setter.clone(),
            ));
        }
        let (left_place, mut left_stmts) = self.bind_to_temp(left_mir, lhs_ty);
        out.append(&mut left_stmts);
        let Some(truthy) = self.truthy_condition(&left_place, lhs_ty, out, types) else {
            self.report_untruthy_operand(op, "left", lhs_ty, lhs_span, types, ctx);
            return Some(left_place);
        };
        // A nullish left operand is always falsy and an object-like one always
        // truthy (a `bool` literal is constant too), so the operator statically
        // yields one operand alone: `undefined || x` is `x`, `undefined && x` is
        // the `undefined`. The *other* operand's type must not constrain the result
        // - no Rust type holds both a `()` and a `number` - and only the operand
        // the result actually is may be evaluated (`undefined && f()` must not call
        // `f`). The left operand is already evaluated above, for its effects.
        if let MirExpr::Bool(lhs_truthy) = &truthy {
            let yields_right = match op {
                HirBinaryOp::Or => !*lhs_truthy,
                _ => *lhs_truthy,
            };
            if !yields_right {
                // The result *is* the left operand, and a nullish one has to fit the
                // destination's nullish kinds: `undefined` stored in a `number | null`
                // slot would answer `x === null` with `true` (P0019).
                if let Some(expected) = self.expected_ty {
                    crate::hir_to_mir::nullish::check_nullish_write_kinds(
                        kinds_of_ty(lhs_ty, types),
                        expected,
                        types,
                        lhs_span,
                        ctx,
                        "the result of this operator",
                    );
                }
                return Some(left_place);
            }
            let right_mir = self.convert_without_expected(
                rhs,
                out,
                shared_struct_ids,
                shared_next_struct,
                types,
                ctx,
            );
            let right_mir =
                self.logical_setter_value(right_mir, setter.clone(), out, types, rhs, ctx);
            return Some(right_mir);
        }
        let mut rhs_stmts: Vec<MirStmt> = Vec::new();
        let right_mir = self.convert_without_expected(
            rhs,
            &mut rhs_stmts,
            shared_struct_ids,
            shared_next_struct,
            types,
            ctx,
        );
        let right_mir =
            self.logical_setter_value(right_mir, setter.clone(), &mut rhs_stmts, types, rhs, ctx);
        // A `null`/`undefined` right operand is always nullish: `a || null` is a
        // valid `a | null`, and the literal has to be stored as `Option::None`
        // rather than coerced into a plain-value temporary (`let tmp: f64 = ();`
        // cannot compile). The literal's own kinds are what the destination keeps.
        let dest_ty = if let Some(rhs_kinds) = nullish_literal_kinds(rhs) {
            // `&&` also yields the left operand when it is falsy, so the left
            // operand's kinds belong to the result as well.
            let nullish = match op {
                HirBinaryOp::And => union_kinds(kinds_of_ty(lhs_ty, types), rhs_kinds),
                _ => rhs_kinds,
            };
            self.optional_type(lhs_ty, nullish, types)
        } else {
            // A call or a member read carries no type in HIR, so the converted
            // value is what tells whether the right operand can be nullish:
            // `a || c.m()` must not be typed `Option<T>` when `m` returns a plain
            // `T`.
            let rhs_static = literal_free_ty(rhs, rhs_hir_ty)
                .or_else(|| self.mir_expr_static_ty(&right_mir, types));
            match rhs_static.map(|ty| (ty, types.resolve(ty).cloned())) {
                // `a && b` / `a || b` yields `b` when it is nullish after all, so
                // the result stays optional even though the left operand is not.
                Some((rhs_ty, Some(Type::Optional { inner, nullish }))) => {
                    if !types_compatible(lhs_ty, inner, types) {
                        self.report_operand_mismatch(op, lhs_ty, rhs_ty, lhs_span, types, ctx);
                        return Some(left_place);
                    }
                    // `&&` also yields the left operand, which may be nullish;
                    // `||` yields the left value only when it is truthy (the left
                    // operand is not optional here), so the result can hold
                    // exactly the right operand's nullish kinds.
                    let nullish = match op {
                        HirBinaryOp::And => union_kinds(kinds_of_ty(lhs_ty, types), nullish),
                        _ => nullish,
                    };
                    // Compatible mixed `int`/`float` operands meet in the common
                    // numeric type, so a fractional right operand keeps its
                    // fraction.
                    let value_ty = common_numeric_ty(lhs_ty, inner, types).unwrap_or(lhs_ty);
                    self.optional_type(value_ty, nullish, types)
                }
                Some((rhs_ty, _)) => {
                    if !types_compatible(lhs_ty, rhs_ty, types) {
                        self.report_operand_mismatch(op, lhs_ty, rhs_ty, lhs_span, types, ctx);
                        return Some(left_place);
                    }
                    common_numeric_ty(lhs_ty, rhs_ty, types).unwrap_or(lhs_ty)
                }
                None => lhs_ty,
            }
        };
        self.report_optional_result(op, dest_ty, expected_ty, lhs_span, types, ctx);
        Some(if op == HirBinaryOp::And {
            // `a && b` is `b` when `a` is truthy, otherwise `a` itself.
            self.finish_conditional_value(
                truthy,
                right_mir,
                rhs_stmts,
                left_place,
                Vec::new(),
                dest_ty,
                out,
            )
        } else {
            // `a || b` is `a` when it is truthy, otherwise `b`.
            self.finish_conditional_value(
                truthy,
                left_place,
                Vec::new(),
                right_mir,
                rhs_stmts,
                dest_ty,
                out,
            )
        })
    }

    /// Lowers `a || b` / `a && b` when `a` is optional.
    ///
    /// JavaScript compares the value inside the optional by truthiness and
    /// yields one of the operands, so neither Rust's `||`/`&&` nor a plain
    /// `Option` comparison expresses it. The inner value is only read when the
    /// optional is present, because `Option::unwrap` panics otherwise.
    ///
    /// The left operand is already lowered (its conversion must precede the
    /// right operand's), and `lhs_span` stands in for its HIR node in
    /// diagnostics.
    ///
    /// The result type is decided after the right operand is converted, from its
    /// *converted* type: a call or a member read carries no type in HIR, and
    /// typing it as `lhs_ty` used to emit `Option<T>` where `T` was required
    /// (`x || call()` inside a `-> f64` function).
    #[allow(clippy::too_many_arguments)]
    fn convert_optional_operand_logical(
        &mut self,
        op: HirBinaryOp,
        left_mir: MirExpr,
        lhs_span: Span,
        rhs: &HirExpr,
        lhs_ty: TypeId,
        inner: TypeId,
        out: &mut Vec<MirStmt>,
        shared_struct_ids: &mut HashMap<TypeId, StructId>,
        shared_next_struct: &mut u32,
        types: &mut TypeTable,
        ctx: &mut PassContext,
        setter: Option<(ts_aot_core::FunctionId, Vec<MirExpr>, TypeId)>,
    ) -> MirExpr {
        let bool_ty = types.intern(&Type::Bool);
        let expected_ty = self.expected_ty;
        let (left_place, mut left_stmts) = self.bind_to_temp(left_mir, lhs_ty);
        out.append(&mut left_stmts);
        let mut rhs_stmts: Vec<MirStmt> = Vec::new();
        let right_mir = self.convert_without_expected(
            rhs,
            &mut rhs_stmts,
            shared_struct_ids,
            shared_next_struct,
            types,
            ctx,
        );
        let right_mir =
            self.logical_setter_value(right_mir, setter.clone(), &mut rhs_stmts, types, rhs, ctx);
        let rhs_hir_ty = self.operand_static_ty(rhs, types);
        let rhs_static =
            literal_free_ty(rhs, rhs_hir_ty).or_else(|| self.mir_expr_static_ty(&right_mir, types));
        // `a || b` is non-nullish only when `b` cannot be nullish.
        let rhs_fits_inner = match rhs_static {
            Some(rhs_ty) => types_compatible(inner, rhs_ty, types),
            None => literal_fits(rhs, inner, types),
        };
        let rhs_fits_lhs = rhs_static.is_some_and(|rhs_ty| {
            matches!(
                types.resolve(rhs_ty),
                Some(Type::Optional { inner: rhs_inner, .. })
                    if types_compatible(*rhs_inner, inner, types)
            )
        });
        if !rhs_fits_inner && !rhs_fits_lhs {
            let rhs_name = operand_type_name(rhs, rhs_static.or(rhs_hir_ty), types);
            if rhs_static.is_none() && !is_literal(rhs) {
                self.report_unknown_operand(op, "right", &rhs_name, rhs.span(), ctx);
            } else {
                self.report_operand_mismatch(
                    op,
                    lhs_ty,
                    rhs_static.unwrap_or(lhs_ty),
                    rhs.span(),
                    types,
                    ctx,
                );
            }
            return left_place;
        }
        // The result's nullish kinds come from the branches that can actually
        // reach it: `&&` also yields the left operand (which may be nullish),
        // while `||` yields the left value only when it is truthy - never
        // nullish, as the left operand here is not optional - so only the right
        // operand's kinds survive.
        let rhs_value_ty = rhs_static.map(|ty| optional_value_ty(ty, types));
        let rhs_kinds = rhs_static.map_or(NO_KINDS, |ty| kinds_of_ty(ty, types));
        let nullish = match op {
            HirBinaryOp::And => union_kinds(kinds_of_ty(lhs_ty, types), rhs_kinds),
            _ => rhs_kinds,
        };
        let value_ty = match (op, rhs_fits_inner) {
            // `&&` yields the right operand when the left one is truthy and the
            // left operand itself otherwise, so the destination has to hold both
            // the left operand's inner type and the right operand: compatible
            // mixed `int`/`float` operands meet in the common numeric type, as in
            // the non-optional lowering and in `||`/`??` (`a && 1.5` over
            // `a: i64 | null` is `1.5`, not the truncated `1`).
            (HirBinaryOp::And, _) => rhs_value_ty
                .and_then(|rhs_value| common_numeric_ty(inner, rhs_value, types))
                .unwrap_or(inner),
            // `||` over a non-nullish right operand: both branches carry a plain
            // value, so compatible mixed `int`/`float` operands meet in the
            // common numeric type and a fractional right operand keeps its
            // fraction.
            (_, true) => rhs_value_ty
                .and_then(|rhs_value| common_numeric_ty(inner, rhs_value, types))
                .unwrap_or(inner),
            // `||` over a nullable right operand: the "falsy" branch yields that
            // operand, the "truthy" one the left value, so the destination still
            // has to hold both - compatible mixed `int`/`float` operands meet in
            // the common numeric type here too (`x: f64 || y: i64 | null` keeps
            // the fraction of a truthy `x`).
            (_, false) => rhs_value_ty
                .and_then(|rhs_value| common_numeric_ty(inner, rhs_value, types))
                .unwrap_or(inner),
        };
        let dest_is_optional = nullish.null || nullish.undefined;
        let dest_ty = if dest_is_optional {
            self.optional_type(value_ty, nullish, types)
        } else {
            value_ty
        };
        self.report_optional_result(op, dest_ty, expected_ty, lhs_span, types, ctx);
        let dest = self.fresh_local();
        self.push_temp_local(dest, dest_ty);
        out.push(MirStmt::Let {
            local: dest,
            ty: dest_ty,
            init: None,
            mutable: true,
        });
        // The value inside the optional, read only in the "present" arm.
        let inner_local = self.fresh_local();
        self.push_temp_local(inner_local, inner);
        let inner_mir = MirExpr::Local(inner_local);
        let mut inner_cond_stmts: Vec<MirStmt> = Vec::new();
        let Some(inner_truthy) =
            self.truthy_condition(&inner_mir, inner, &mut inner_cond_stmts, types)
        else {
            self.report_untruthy_operand(op, "left", inner, lhs_span, types, ctx);
            return left_place;
        };
        // The destination holds the common numeric type of the two operands, so
        // its inner type can differ from the left operand's own: the left
        // operand is then re-wrapped from the inner value instead of being
        // assigned as the `Option<inner>` it is stored as (the backend coerces
        // a scalar, not an optional's inner).
        let left_inner_widened = value_ty != inner;
        let (then_stmts, else_stmts) = if op == HirBinaryOp::And {
            let mut then_stmts = rhs_stmts;
            then_stmts.push(MirStmt::Assign {
                target: MirPlace::Local { id: dest },
                value: right_mir.clone(),
            });
            // The "present but falsy" branch yields the left operand's value,
            // which the enclosing `IsSome` guard has already proved present.
            let value = if left_inner_widened {
                MirExpr::UnwrapOptional {
                    expr: Box::new(left_place.clone()),
                    ty: inner,
                }
            } else {
                left_place.clone()
            };
            let else_stmts = vec![MirStmt::Assign {
                target: MirPlace::Local { id: dest },
                value,
            }];
            (then_stmts, else_stmts)
        } else {
            let then_stmts = vec![MirStmt::Assign {
                target: MirPlace::Local { id: dest },
                value: inner_mir.clone(),
            }];
            let mut else_stmts = rhs_stmts;
            else_stmts.push(MirStmt::Assign {
                target: MirPlace::Local { id: dest },
                value: right_mir.clone(),
            });
            (then_stmts, else_stmts)
        };
        // The branch that runs when the left operand is absent. `&&` yields that
        // operand, so it repeats the "present but falsy" assignment while the
        // types match; once the destination's inner type is widened, the left
        // operand's own `Option<inner>` cannot be assigned into it, so the
        // destination's `None` is assigned instead (`Unit` lowers to the target's
        // nullish representation).
        let absent_stmts = if op == HirBinaryOp::And && left_inner_widened {
            vec![MirStmt::Assign {
                target: MirPlace::Local { id: dest },
                value: MirExpr::Unit,
            }]
        } else {
            else_stmts.clone()
        };
        let else_block = MirBlock { stmts: else_stmts };
        let absent_block = MirBlock {
            stmts: absent_stmts,
        };
        let mut present_stmts = vec![MirStmt::Let {
            local: inner_local,
            ty: inner,
            init: Some(MirExpr::UnwrapOptional {
                expr: Box::new(left_place.clone()),
                ty: inner,
            }),
            mutable: false,
        }];
        present_stmts.extend(inner_cond_stmts);
        present_stmts.push(MirStmt::If {
            cond: inner_truthy,
            then_block: MirBlock { stmts: then_stmts },
            else_block: Some(else_block.clone()),
        });
        out.push(MirStmt::If {
            cond: MirExpr::IsSome {
                expr: Box::new(left_place),
                ty: bool_ty,
            },
            then_block: MirBlock {
                stmts: present_stmts,
            },
            else_block: Some(absent_block),
        });
        MirExpr::Local(dest)
    }
    /// Materialize the RHS once and assign it while still in its short-circuit
    /// branch. The old getter value never reaches the setter.
    fn logical_setter_value(
        &mut self,
        value: MirExpr,
        setter: Option<(ts_aot_core::FunctionId, Vec<MirExpr>, TypeId)>,
        out: &mut Vec<MirStmt>,
        types: &mut TypeTable,
        rhs: &HirExpr,
        ctx: &mut PassContext,
    ) -> MirExpr {
        let Some((callee, mut args, ty)) = setter else {
            return value;
        };
        let source_ty = self.mir_expr_static_ty(&value, types).unwrap_or(ty);
        if !super::control::is_assignment_compatible(rhs, source_ty, ty, types, rhs.span(), ctx) {
            return MirExpr::Unit;
        }
        let kinds = if is_nullish_hir_literal(rhs) {
            kinds_of_hir(rhs, types)
        } else {
            kinds_of_ty(source_ty, types)
        };
        crate::hir_to_mir::nullish::check_nullish_write_kinds(
            kinds,
            ty,
            types,
            rhs.span(),
            ctx,
            "the accessor assignment value",
        );
        let (value, mut stmts) = self.bind_to_temp(value, ty);
        out.append(&mut stmts);
        args.push(value.clone());
        out.push(MirStmt::Expr(MirExpr::Call {
            callee,
            args,
            ty: types.intern(&Type::Void),
        }));
        value
    }

    /// JavaScript truthiness of `value` as a MIR condition.
    ///
    /// Returns `None` when the truthiness cannot be expressed: a union may hold
    /// variants with different truthiness (a number `0` is falsy, a string is
    /// not), and the AOT target has no representation for one anyway. Assuming
    /// `true` there would silently change what `a || b` yields.
    fn truthy_condition(
        &mut self,
        value: &MirExpr,
        ty: TypeId,
        out: &mut Vec<MirStmt>,
        types: &mut TypeTable,
    ) -> Option<MirExpr> {
        let bool_ty = types.intern(&Type::Bool);
        Some(match types.resolve(ty) {
            Some(Type::Bool) => value.clone(),
            Some(Type::String) => {
                let len_ty = types.intern(&Type::F64);
                let len = self.fresh_local();
                self.push_temp_local(len, len_ty);
                out.push(MirStmt::Runtime {
                    op: RuntimeOp::StringLen,
                    args: vec![value.clone()],
                    dest: Some(len),
                    ty: len_ty,
                    target_ty: None,
                });
                MirExpr::Binary {
                    op: BinaryOp::Ne,
                    left: Box::new(MirExpr::Local(len)),
                    right: Box::new(MirExpr::Int {
                        value: 0,
                        ty: len_ty,
                    }),
                    ty: bool_ty,
                }
            }
            Some(Type::F32 | Type::F64) => {
                // `x != 0 && x == x`: `NaN` is falsy in JavaScript and is the
                // only value that is not equal to itself.
                MirExpr::Binary {
                    op: BinaryOp::And,
                    left: Box::new(MirExpr::Binary {
                        op: BinaryOp::Ne,
                        left: Box::new(value.clone()),
                        right: Box::new(MirExpr::Int { value: 0, ty }),
                        ty: bool_ty,
                    }),
                    right: Box::new(MirExpr::Binary {
                        op: BinaryOp::Eq,
                        left: Box::new(value.clone()),
                        right: Box::new(value.clone()),
                        ty: bool_ty,
                    }),
                    ty: bool_ty,
                }
            }
            Some(
                Type::I8
                | Type::I16
                | Type::I32
                | Type::I64
                | Type::U8
                | Type::U16
                | Type::U32
                | Type::U64,
            ) => MirExpr::Binary {
                op: BinaryOp::Ne,
                left: Box::new(value.clone()),
                right: Box::new(MirExpr::Int { value: 0, ty }),
                ty: bool_ty,
            },
            // `0n` is falsy in JavaScript and every other BigInt is truthy: the
            // always-truthy object rule below must not swallow it, or `0n || 1n`
            // would keep `0n`.
            Some(Type::BigInt) => {
                let bigint_ty = types.intern(&Type::BigInt);
                let dest = self.fresh_local();
                self.push_temp_local(dest, bool_ty);
                out.push(MirStmt::Runtime {
                    op: RuntimeOp::BigIntNeq,
                    args: vec![
                        value.clone(),
                        MirExpr::BigInt {
                            value: "0".to_owned(),
                            ty: bigint_ty,
                        },
                    ],
                    dest: Some(dest),
                    ty: bool_ty,
                    target_ty: None,
                });
                MirExpr::Local(dest)
            }
            // A union has no single Rust shape, and its variants need not agree
            // on truthiness: report rather than answer `true`.
            Some(Type::Union { .. }) => return None,
            // `null` and `undefined` are always falsy, including through a call
            // that produces them: `f() || 5` over a `void` (or `null`-returning)
            // `f` is `5`, not the `undefined` the left operand holds.
            Some(Type::Null | Type::Void) => MirExpr::Bool(false),
            // Objects, arrays, functions, dates ... are always truthy.
            _ => MirExpr::Bool(true),
        })
    }

    /// Assigns the value of a conditional expression through a temporary, the
    /// way `convert_ternary` does, so both branches are evaluated lazily.
    #[allow(clippy::too_many_arguments)]
    fn finish_conditional_value(
        &mut self,
        cond: MirExpr,
        then_value: MirExpr,
        then_stmts: Vec<MirStmt>,
        else_value: MirExpr,
        else_stmts: Vec<MirStmt>,
        dest_ty: TypeId,
        out: &mut Vec<MirStmt>,
    ) -> MirExpr {
        let dest = self.fresh_local();
        self.push_temp_local(dest, dest_ty);
        out.push(MirStmt::Let {
            local: dest,
            ty: dest_ty,
            init: None,
            mutable: true,
        });
        let mut then_stmts = then_stmts;
        then_stmts.push(MirStmt::Assign {
            target: MirPlace::Local { id: dest },
            value: then_value,
        });
        let mut else_stmts = else_stmts;
        else_stmts.push(MirStmt::Assign {
            target: MirPlace::Local { id: dest },
            value: else_value,
        });
        out.push(MirStmt::If {
            cond,
            then_block: MirBlock { stmts: then_stmts },
            else_block: Some(MirBlock { stmts: else_stmts }),
        });
        MirExpr::Local(dest)
    }

    /// Binds `value` to a fresh temporary so it can be tested and moved without
    /// being evaluated twice.
    fn bind_to_temp(&mut self, value: MirExpr, ty: TypeId) -> (MirExpr, Vec<MirStmt>) {
        let temp = self.fresh_local();
        self.push_temp_local(temp, ty);
        (
            MirExpr::Local(temp),
            vec![MirStmt::Let {
                local: temp,
                ty,
                init: Some(value),
                mutable: false,
            }],
        )
    }
    fn convert_bigint_binary(
        &mut self,
        bigint_op: RuntimeOp,
        op: HirBinaryOp,
        lhs: &HirExpr,
        rhs: &HirExpr,
        _ty: TypeId,
        out: &mut Vec<MirStmt>,
        shared_struct_ids: &mut HashMap<TypeId, StructId>,
        shared_next_struct: &mut u32,
        types: &mut TypeTable,
        ctx: &mut PassContext,
    ) -> MirExpr {
        let lhs_mir =
            self.convert_expr(lhs, out, shared_struct_ids, shared_next_struct, types, ctx);
        let rhs_mir =
            self.convert_expr(rhs, out, shared_struct_ids, shared_next_struct, types, ctx);
        let result_ty = if is_comparison_op(op) {
            types.intern(&Type::Bool)
        } else {
            types.intern(&Type::BigInt)
        };
        let dest = self.fresh_local();
        self.push_temp_local(dest, result_ty);
        out.push(MirStmt::Runtime {
            op: bigint_op,
            args: vec![lhs_mir, rhs_mir],
            dest: Some(dest),
            ty: result_ty,
            target_ty: None,
        });
        MirExpr::Local(dest)
    }

    pub(super) fn convert_unary(
        &mut self,
        op: HirUnaryOp,
        expr: &HirExpr,
        ty: TypeId,
        out: &mut Vec<MirStmt>,
        shared_struct_ids: &mut HashMap<TypeId, StructId>,
        shared_next_struct: &mut u32,
        types: &mut TypeTable,
        ctx: &mut PassContext,
    ) -> MirExpr {
        if expr_yields_bigint(expr, types) {
            match op {
                HirUnaryOp::Neg => {
                    return self.convert_bigint_unary(
                        RuntimeOp::BigIntNeg,
                        expr,
                        ty,
                        out,
                        shared_struct_ids,
                        shared_next_struct,
                        types,
                        ctx,
                    );
                }
                HirUnaryOp::BitNot => {
                    return self.convert_bigint_unary(
                        RuntimeOp::BigIntNot,
                        expr,
                        ty,
                        out,
                        shared_struct_ids,
                        shared_next_struct,
                        types,
                        ctx,
                    );
                }
                _ => {}
            }
        }
        match op {
            HirUnaryOp::TypeOf => {
                let inner =
                    self.convert_expr(expr, out, shared_struct_ids, shared_next_struct, types, ctx);
                let string_ty = types.intern(&ts_aot_core::Type::String);
                MirExpr::TypeOf {
                    expr: Box::new(inner),
                    ty: string_ty,
                }
            }
            HirUnaryOp::Void => {
                let inner =
                    self.convert_expr(expr, out, shared_struct_ids, shared_next_struct, types, ctx);
                if has_potential_side_effects(&inner) {
                    out.push(MirStmt::Expr(inner));
                }
                MirExpr::Unit
            }
            HirUnaryOp::Delete => {
                let inner =
                    self.convert_expr(expr, out, shared_struct_ids, shared_next_struct, types, ctx);
                if has_potential_side_effects(&inner) {
                    out.push(MirStmt::Expr(inner));
                }
                MirExpr::Bool(true)
            }
            _ => MirExpr::Unary {
                op: convert_unaryop(op, ctx),
                expr: Box::new(self.convert_expr(
                    expr,
                    out,
                    shared_struct_ids,
                    shared_next_struct,
                    types,
                    ctx,
                )),
                ty,
            },
        }
    }

    fn convert_bigint_unary(
        &mut self,
        bigint_op: RuntimeOp,
        expr: &HirExpr,
        ty: TypeId,
        out: &mut Vec<MirStmt>,
        shared_struct_ids: &mut HashMap<TypeId, StructId>,
        shared_next_struct: &mut u32,
        types: &mut TypeTable,
        ctx: &mut PassContext,
    ) -> MirExpr {
        let inner_mir =
            self.convert_expr(expr, out, shared_struct_ids, shared_next_struct, types, ctx);
        let dest = self.fresh_local();
        self.push_temp_local(dest, ty);
        out.push(MirStmt::Runtime {
            op: bigint_op,
            args: vec![inner_mir],
            dest: Some(dest),
            ty,
            target_ty: None,
        });
        MirExpr::Local(dest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// JavaScript treats `null` and `undefined` as falsy, whether the value comes
    /// from a literal or from a call that produces one (`f(): void`): the
    /// condition is `false`, not the always-truthy fallback - which used to keep a
    /// nullish value alive in `x || y` and to answer `true` for `if (f())`.
    ///
    /// The condition is built by a direct call so both types are covered at once;
    /// the source shape that reaches this arm is a logical expression whose
    /// operands are *both* nullish (`const w: void = u || v`, see
    /// `a_nullish_operand_is_falsy_in_a_logical_expression`). A `void`-typed operand
    /// next to a *value* operand is rejected earlier by the operand-type selection
    /// (`P0021`) - a separate question from the truthiness itself.
    #[test]
    fn truthiness_of_null_and_void_is_false() {
        let mut converter = ExprConverter::new();
        let mut types = TypeTable::new();
        let value = MirExpr::Bool(false);
        for ty in [Type::Null, Type::Void] {
            let ty = types.intern(&ty);
            let mut out = Vec::new();
            let cond = converter.truthy_condition(&value, ty, &mut out, &mut types);
            assert_eq!(
                cond,
                Some(MirExpr::Bool(false)),
                "`{ty:?}` is falsy in JavaScript"
            );
            assert!(
                out.is_empty(),
                "a constant condition must not emit statements"
            );
        }
    }

    /// The always-truthy fallback stays for the values that really are always
    /// truthy, so the nullish arm cannot swallow an object or an array.
    #[test]
    fn truthiness_of_an_object_is_still_true() {
        let mut converter = ExprConverter::new();
        let mut types = TypeTable::new();
        let value = MirExpr::Bool(false);
        for ty in [
            Type::Struct {
                id: StructId::from_raw(0),
            },
            Type::Array {
                element: TypeId::from_raw(0),
            },
        ] {
            let ty = types.intern(&ty);
            let mut out = Vec::new();
            let cond = converter.truthy_condition(&value, ty, &mut out, &mut types);
            assert_eq!(cond, Some(MirExpr::Bool(true)), "`{ty:?}` is always truthy");
        }
    }
}
