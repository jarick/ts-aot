//! Nullish-kind (`null` vs `undefined`) compatibility checks.
//!
//! The AOT representation of an optional value is a plain `Option<T>`: it can
//! tell "a value" from "no value", but it cannot tell `null` from `undefined`
//! at runtime. The type table therefore carries [`NullishKinds`] on
//! `Type::Optional`, and the backend answers `x === null` / `x === undefined`
//! from that static information (`Option::is_none()` when the kind admits the
//! literal, `false`/`true` when it provably does not).
//!
//! That static answer is only sound while the runtime value really belongs to
//! the kind its type claims. A `null` written into a `T | undefined` slot (or
//! vice versa) is a TypeScript type error, but this frontend does not run a full
//! assignability check, so it used to be accepted silently and then made
//! `=== null` / `=== undefined` answer the opposite of JavaScript. These
//! helpers reject such writes instead of miscompiling them.

use ts_aot_core::{NullishKinds, Span, Type, TypeId, TypeTable};
use ts_aot_ir_hir::HirExpr;

use super::convert_expr::util::hir_expr_type_id;
use crate::PassContext;

/// A nullish value (or an optional value that may be nullish) is used where a
/// non-optional value is required.
pub(super) const NULLISH_NARROWING_DIAG: &str = "P0018";

/// The written nullish kind is not part of the target type's kind.
pub(super) const NULLISH_KIND_MISMATCH_DIAG: &str = "P0019";

/// A strict comparison against `null`/`undefined` whose operand admits both
/// kinds, so the AOT representation cannot decide the answer.
pub(super) const NULLISH_UNDECIDABLE_DIAG: &str = "P0020";

/// `??`, `||` or `&&` over an operand whose type the AOT target cannot lower
/// (unknown type, or a union that is not representable as an optional value).
pub(super) const LOGICAL_DIAG: &str = "P0021";

/// The nullish kinds a type can hold (`Type::Optional`, and unions that carry
/// `null`/`undefined` variants directly).
pub(super) fn kinds_of_ty(ty: TypeId, types: &TypeTable) -> NullishKinds {
    match types.resolve(ty) {
        Some(Type::Optional { nullish, .. }) => *nullish,
        Some(Type::Union { variants }) => {
            let mut kinds = NullishKinds {
                null: false,
                undefined: false,
            };
            for variant in variants {
                let variant_kinds = kinds_of_ty(*variant, types);
                kinds.null |= variant_kinds.null;
                kinds.undefined |= variant_kinds.undefined;
            }
            kinds
        }
        Some(Type::Null) => NullishKinds::NULL_ONLY,
        Some(Type::Void) => NullishKinds::UNDEFINED_ONLY,
        _ => NullishKinds {
            null: false,
            undefined: false,
        },
    }
}

/// The nullish kinds a value expression can produce: `null`/`undefined`
/// literals are exact, everything else falls back to its static type.
pub(super) fn kinds_of_hir(value: &HirExpr, types: &TypeTable) -> NullishKinds {
    match value {
        HirExpr::Null(_) => NullishKinds::NULL_ONLY,
        HirExpr::Unit(_) | HirExpr::Undefined(_) => NullishKinds::UNDEFINED_ONLY,
        // A conditional yields either branch, so its nullish kinds are the union
        // of theirs: `b ? x : null` is `null`-capable even when the *declared*
        // type it is written into only mentions `undefined`.
        HirExpr::Ternary {
            then_branch,
            else_branch,
            ..
        } => {
            let then_kinds = kinds_of_hir(then_branch, types);
            let else_kinds = kinds_of_hir(else_branch, types);
            NullishKinds {
                null: then_kinds.null || else_kinds.null,
                undefined: then_kinds.undefined || else_kinds.undefined,
            }
        }
        _ => hir_expr_type_id(value).map_or(
            NullishKinds {
                null: false,
                undefined: false,
            },
            |ty| kinds_of_ty(ty, types),
        ),
    }
}

/// Whether the expression is the `null` / `undefined` literal itself (as opposed
/// to a typed optional value).
pub(super) fn is_nullish_literal(value: &HirExpr) -> bool {
    matches!(
        value,
        HirExpr::Null(_) | HirExpr::Unit(_) | HirExpr::Undefined(_)
    )
}

fn describe(kinds: NullishKinds) -> &'static str {
    match (kinds.null, kinds.undefined) {
        (true, true) => "`null | undefined`",
        (true, false) => "`null`",
        (false, true) => "`undefined`",
        (false, false) => "a non-nullish value",
    }
}

/// Whether the nullish kinds a type can hold are *unknown* because the type
/// mentions a generic parameter (or an error/unresolved type), also through an
/// `Optional`/`Union` wrapper: `T`, `T | null` and `T | undefined` may all admit
/// the nullish part of an argument depending on how `T` is inferred, so a check
/// over their `NullishKinds` would be unsound.
fn nullish_kinds_unknown(ty: TypeId, types: &TypeTable) -> bool {
    match types.resolve(ty) {
        None | Some(Type::Error | Type::GenericParam { .. }) => true,
        Some(Type::Optional { inner, .. }) => nullish_kinds_unknown(*inner, types),
        Some(Type::Union { variants }) => variants
            .iter()
            .any(|variant| nullish_kinds_unknown(*variant, types)),
        _ => false,
    }
}

/// The type a call argument is checked against.
///
/// A defaulted parameter (`x: T = <default>`) arrives as the hidden `Option<T>`
/// ABI parameter whose *outer* `Option` is only the "was the argument supplied?"
/// marker; `T` itself may be nullable. The argument is a value of `T`, and both
/// `null` (when `T` allows it) and `undefined` (which selects the default) are
/// acceptable, so the check uses `T | undefined` rather than the marker type.
pub(super) fn argument_check_ty(target_ty: TypeId, types: &mut TypeTable) -> TypeId {
    let Some(Type::Optional { inner, nullish }) = types.resolve(target_ty).cloned() else {
        return target_ty;
    };
    if !nullish.undefined || nullish.null {
        return target_ty;
    }
    let Some(Type::Optional {
        inner: value,
        nullish: declared,
    }) = types.resolve(inner).cloned()
    else {
        return target_ty;
    };
    types.intern(&Type::Optional {
        inner: value,
        nullish: NullishKinds {
            null: declared.null,
            undefined: true,
        },
    })
}

/// Reject a write of `value` into a slot of type `target_ty` when the nullish
/// kinds cannot be represented soundly. Returns `false` (and emits a
/// diagnostic) when the write must not be lowered.
///
/// `site` names the position for the message ("this variable", "this
/// parameter", "the return type of this function", ...).
///
/// `allow_nullish_literal` keeps the long-standing relaxed path for *assignment*
/// targets (a bare `null`/`undefined` literal is lowered as-is with the `E0410`
/// warning); declaration, return and argument positions have no such warning and
/// therefore reject the literal instead of emitting `()` where `T` is expected.
pub(super) fn check_nullish_write(
    value: &HirExpr,
    target_ty: TypeId,
    types: &TypeTable,
    span: Span,
    ctx: &mut PassContext,
    site: &str,
    allow_nullish_literal: bool,
) -> bool {
    check_nullish_kinds(
        kinds_of_hir(value, types),
        is_nullish_literal(value),
        target_ty,
        types,
        span,
        ctx,
        site,
        allow_nullish_literal,
    )
}

/// Like [`check_nullish_write`], for a value whose nullish kinds are already
/// known: the *converted* value is used by the `return` check, because the HIR
/// node of a call or `||` carries no type of its own (`Error`), so the HIR form
/// above cannot see an optional value there.
pub(super) fn check_nullish_write_kinds(
    source: NullishKinds,
    target_ty: TypeId,
    types: &TypeTable,
    span: Span,
    ctx: &mut PassContext,
    site: &str,
) -> bool {
    check_nullish_kinds(source, false, target_ty, types, span, ctx, site, false)
}

#[allow(clippy::too_many_arguments)]
fn check_nullish_kinds(
    source: NullishKinds,
    source_is_literal: bool,
    target_ty: TypeId,
    types: &TypeTable,
    span: Span,
    ctx: &mut PassContext,
    site: &str,
    allow_nullish_literal: bool,
) -> bool {
    let Some(target) = types.resolve(target_ty) else {
        return true;
    };
    // An unknown target (a generic parameter, an unresolved/error type, or an
    // optional/union over one) has no *known* nullish kinds: the empty
    // `NullishKinds` of `kinds_of_ty` mean "unknown", not "this type cannot hold
    // nullish". Passing an optional value to `T` is legal TypeScript (`T` is
    // inferred with the nullish part), and `T | null` may also admit
    // `undefined` when `T` does - so the check must be permissive instead of
    // reporting a false `P0018`/`P0019` here.
    if nullish_kinds_unknown(target_ty, types) {
        return true;
    }
    if !source.null && !source.undefined {
        return true;
    }
    // `void`/`null` targets have no value to carry, so a nullish literal is the
    // natural (and already supported) lowering.
    if matches!(target, Type::Void | Type::Null) {
        return true;
    }
    let target_kinds = kinds_of_ty(target_ty, types);
    if !target_kinds.null && !target_kinds.undefined {
        // A bare `null`/`undefined` literal keeps the long-standing relaxed
        // warning path (`E0410`, emitted by the assignment compatibility check):
        // it is lowered as-is. An *optional-typed* value has no lowering at all
        // (the generated Rust would compare/assign `Option<T>` to `T`), so it is
        // rejected here.
        if allow_nullish_literal && source_is_literal {
            return true;
        }
        let what = if source_is_literal {
            format!("a {} value", describe(source))
        } else {
            format!("an optional value (it may be {})", describe(source))
        };
        ctx.error(
            NULLISH_NARROWING_DIAG,
            format!(
                "{what} is used where a non-optional value is required ({site}); the AOT target \
                 represents optional values as `Option<T>` and has no implicit \
                 nullish-to-value unwrapping. Handle the nullish case first (for example with \
                 `x!` after a check, or by widening the target type)"
            ),
            span,
        );
        return false;
    }
    let missing = NullishKinds {
        null: source.null && !target_kinds.null,
        undefined: source.undefined && !target_kinds.undefined,
    };
    if missing.null || missing.undefined {
        ctx.error(
            NULLISH_KIND_MISMATCH_DIAG,
            format!(
                "a {} value is written into a type that only allows {} ({site}); `null` and \
                 `undefined` share one `Option::None` representation, so accepting this write \
                 would make `=== null` / `=== undefined` answer incorrectly. Widen the target \
                 type to `T | null | undefined`, or use the literal the target allows",
                describe(source),
                describe(target_kinds)
            ),
            span,
        );
        return false;
    }
    true
}

/// Convenience wrapper for the callers that already have the operand's HIR type
/// (the comparison check itself lives in [`check_nullish_comparison_ty`]).
#[cfg(test)]
pub(super) fn check_nullish_comparison(
    operand: &HirExpr,
    types: &TypeTable,
    ctx: &mut PassContext,
    op_name: &str,
) -> bool {
    check_nullish_comparison_ty(operand, None, types, ctx, op_name)
}

/// Reject a strict `=== null` / `!== null` (`undefined` likewise) comparison
/// whose operand type admits *both* kinds: `Option<T>` cannot tell which one the
/// runtime value holds, so neither answer is correct for every input.
///
/// Unlike the test-only wrapper above, the operand's type is resolved by the
/// caller when the HIR node carries none of its own - a call operand has
/// `Type::Error` in HIR, and accepting `f() === null` merely because its type is
/// unknown is wrong when `f` returns a type that includes `undefined`.
pub(super) fn check_nullish_comparison_ty(
    operand: &HirExpr,
    resolved_ty: Option<TypeId>,
    types: &TypeTable,
    ctx: &mut PassContext,
    op_name: &str,
) -> bool {
    let Some(ty) = resolved_ty.or_else(|| hir_expr_type_id(operand)) else {
        return true;
    };
    let kinds = kinds_of_ty(ty, types);
    if !(kinds.null && kinds.undefined) {
        return true;
    }
    ctx.error(
        NULLISH_UNDECIDABLE_DIAG,
        format!(
            "`{op_name}` compares against a `null`/`undefined` literal, but the other operand's \
             type allows both `null` and `undefined`; the AOT representation stores both as \
             `Option::None`, so this strict comparison cannot be decided. Use a loose comparison \
             (`== null` / `!= null`), narrow the value's type, or split the type so only one \
             nullish kind remains"
        ),
        operand.span(),
    );
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use ts_aot_core::TypeTable;
    use ts_aot_ir_hir::HirExpr;

    fn span() -> Span {
        Span::new(0, 1)
    }

    #[test]
    fn kinds_cover_optional_union_and_plain_types() {
        let mut types = TypeTable::new();
        let number = types.intern(&Type::F64);
        let null_only = types.intern(&Type::Optional {
            inner: number,
            nullish: NullishKinds::NULL_ONLY,
        });
        let undefined_only = types.intern(&Type::Optional {
            inner: number,
            nullish: NullishKinds::UNDEFINED_ONLY,
        });
        let both = types.intern(&Type::Optional {
            inner: number,
            nullish: NullishKinds::BOTH,
        });
        let null_ty = types.intern(&Type::Null);
        let union = types.intern(&Type::Union {
            variants: vec![number, null_ty],
        });
        assert_eq!(kinds_of_ty(null_only, &types), NullishKinds::NULL_ONLY);
        assert_eq!(
            kinds_of_ty(undefined_only, &types),
            NullishKinds::UNDEFINED_ONLY
        );
        assert_eq!(kinds_of_ty(both, &types), NullishKinds::BOTH);
        assert_eq!(
            kinds_of_ty(union, &types),
            NullishKinds::NULL_ONLY,
            "a union with a `null` variant is nullable"
        );
        assert_eq!(
            kinds_of_ty(number, &types),
            NullishKinds {
                null: false,
                undefined: false
            }
        );
    }

    #[test]
    fn nullish_literals_map_to_their_kind() {
        let types = TypeTable::new();
        assert_eq!(
            kinds_of_hir(&HirExpr::Null(span()), &types),
            NullishKinds::NULL_ONLY
        );
        assert_eq!(
            kinds_of_hir(&HirExpr::Undefined(span()), &types),
            NullishKinds::UNDEFINED_ONLY
        );
        assert_eq!(
            kinds_of_hir(&HirExpr::Unit(span()), &types),
            NullishKinds::UNDEFINED_ONLY
        );
    }

    #[test]
    fn a_literal_of_the_other_kind_is_rejected() {
        let mut types = TypeTable::new();
        let number = types.intern(&Type::F64);
        let undefined_only = types.intern(&Type::Optional {
            inner: number,
            nullish: NullishKinds::UNDEFINED_ONLY,
        });
        let mut ctx = PassContext::new();
        assert!(
            !check_nullish_write(
                &HirExpr::Null(span()),
                undefined_only,
                &types,
                span(),
                &mut ctx,
                "test target",
                false
            ),
            "`null` written into `number | undefined` must be rejected"
        );
        let mismatch = ctx
            .diagnostics()
            .iter()
            .find(|d| d.code.as_str() == NULLISH_KIND_MISMATCH_DIAG)
            .expect("expected a kind-mismatch diagnostic");
        assert!(
            mismatch.message.contains("only allows `undefined`"),
            "got: {}",
            mismatch.message
        );
    }

    #[test]
    fn a_literal_of_the_same_kind_is_accepted() {
        let mut types = TypeTable::new();
        let number = types.intern(&Type::F64);
        for nullish in [NullishKinds::NULL_ONLY, NullishKinds::BOTH] {
            let target = types.intern(&Type::Optional {
                inner: number,
                nullish,
            });
            let mut ctx = PassContext::new();
            assert!(
                check_nullish_write(
                    &HirExpr::Null(span()),
                    target,
                    &types,
                    span(),
                    &mut ctx,
                    "test target",
                    false
                ),
                "`null` written into a target that allows `null` must be accepted ({nullish:?})"
            );
            assert!(ctx.diagnostics().is_empty(), "{:?}", ctx.diagnostics());
        }
    }

    #[test]
    fn an_optional_value_needs_a_non_optional_target_check() {
        let mut types = TypeTable::new();
        let number = types.intern(&Type::F64);
        let optional = types.intern(&Type::Optional {
            inner: number,
            nullish: NullishKinds::NULL_ONLY,
        });
        let value = HirExpr::Local {
            id: ts_aot_core::LocalId::from_raw(0),
            ty: optional,
            span: span(),
        };
        let mut ctx = PassContext::new();
        assert!(
            !check_nullish_write(
                &value,
                number,
                &types,
                span(),
                &mut ctx,
                "test target",
                false
            ),
            "an optional value must not be written into a non-optional target"
        );
        assert!(
            ctx.diagnostics()
                .iter()
                .any(|d| d.code.as_str() == NULLISH_NARROWING_DIAG),
            "{:?}",
            ctx.diagnostics()
        );
    }

    #[test]
    fn a_bare_nullish_literal_into_a_declaration_or_argument_is_rejected() {
        let mut types = TypeTable::new();
        let number = types.intern(&Type::F64);
        let mut ctx = PassContext::new();
        assert!(
            !check_nullish_write(
                &HirExpr::Null(span()),
                number,
                &types,
                span(),
                &mut ctx,
                "the annotation of `x`",
                false
            ),
            "a bare `null` literal written into a non-optional declaration/argument must be \
             rejected (it used to emit `()` where `T` is expected)"
        );
        let diag = ctx
            .diagnostics()
            .iter()
            .find(|d| d.code.as_str() == NULLISH_NARROWING_DIAG)
            .expect("expected a narrowing diagnostic");
        assert!(
            diag.message.contains("a `null` value"),
            "the message must name the literal; got: {}",
            diag.message
        );
    }

    #[test]
    fn a_bare_nullish_literal_into_a_non_optional_target_keeps_the_relaxed_path() {
        let mut types = TypeTable::new();
        let number = types.intern(&Type::F64);
        let mut ctx = PassContext::new();
        assert!(
            check_nullish_write(
                &HirExpr::Null(span()),
                number,
                &types,
                span(),
                &mut ctx,
                "test target",
                true
            ),
            "a bare `null` literal keeps the pre-existing relaxed E0410 warning path"
        );
        assert!(ctx.diagnostics().is_empty(), "{:?}", ctx.diagnostics());
    }

    #[test]
    fn strict_comparison_of_a_both_kinds_operand_is_rejected() {
        let mut types = TypeTable::new();
        let number = types.intern(&Type::F64);
        let both = types.intern(&Type::Optional {
            inner: number,
            nullish: NullishKinds::BOTH,
        });
        let operand = HirExpr::Local {
            id: ts_aot_core::LocalId::from_raw(0),
            ty: both,
            span: span(),
        };
        let mut ctx = PassContext::new();
        assert!(
            !check_nullish_comparison(&operand, &types, &mut ctx, "==="),
            "`x === null` cannot be decided when x allows both null and undefined"
        );
        assert!(
            ctx.diagnostics()
                .iter()
                .any(|d| d.code.as_str() == NULLISH_UNDECIDABLE_DIAG),
            "{:?}",
            ctx.diagnostics()
        );
    }

    #[test]
    fn strict_comparison_of_a_single_kind_operand_is_allowed() {
        let mut types = TypeTable::new();
        let number = types.intern(&Type::F64);
        for nullish in [NullishKinds::NULL_ONLY, NullishKinds::UNDEFINED_ONLY] {
            let operand_ty = types.intern(&Type::Optional {
                inner: number,
                nullish,
            });
            let operand = HirExpr::Local {
                id: ts_aot_core::LocalId::from_raw(0),
                ty: operand_ty,
                span: span(),
            };
            let mut ctx = PassContext::new();
            assert!(
                check_nullish_comparison(&operand, &types, &mut ctx, "==="),
                "a single-kind optional can be compared strictly ({nullish:?})"
            );
            assert!(ctx.diagnostics().is_empty(), "{:?}", ctx.diagnostics());
        }
    }
}
