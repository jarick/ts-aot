use ts_aot_core::{LocalId, Span};
use ts_aot_ir_mir::{MirExpr, MirPlace, MirPlaceBase, MirStmt};

use crate::PassContext;
use crate::hir_to_mir::convert_expr::util::mir_expr_ty;
use crate::hir_to_mir::converter::ExprConverter;

pub(super) fn mir_place_to_expr(p: MirPlace) -> MirExpr {
    match p {
        MirPlace::Local { id } => MirExpr::Local(id),
        MirPlace::Field { base, field, ty } => MirExpr::Field {
            base: Box::new(mir_place_base_to_expr(*base)),
            field,
            ty,
        },
        MirPlace::Index { base, index, ty } => MirExpr::Index { base, index, ty },
    }
}

pub(super) fn mir_place_base_to_expr(b: MirPlaceBase) -> MirExpr {
    match b {
        MirPlaceBase::Local(id) => MirExpr::Local(id),
        MirPlaceBase::Field { base, field, ty } => MirExpr::Field {
            base: Box::new(mir_place_base_to_expr(*base)),
            field,
            ty,
        },
        MirPlaceBase::Index { base, index, ty } => MirExpr::Index { base, index, ty },
        MirPlaceBase::Chain { base, ty } => MirExpr::OptionalChain { base, ty },
    }
}

pub(super) fn mir_expr_to_place<F>(
    e: MirExpr,
    ctx: &mut PassContext,
    materialize: F,
) -> Option<MirPlace>
where
    F: FnMut(MirExpr) -> LocalId,
{
    match e {
        MirExpr::Local(id) => Some(MirPlace::Local { id }),
        MirExpr::Field { base, field, ty } => {
            let base_pb = mir_expr_to_place_base(*base, ctx, materialize)?;
            Some(MirPlace::Field {
                base: Box::new(base_pb),
                field,
                ty,
            })
        }
        MirExpr::Index { base, index, ty } => Some(MirPlace::Index { base, index, ty }),
        _ => {
            ctx.error(
                "P0006",
                "expression is not a valid assignment target",
                Span::new(0, 0),
            );
            None
        }
    }
}

pub(super) fn mir_expr_to_place_base<F>(
    e: MirExpr,
    ctx: &mut PassContext,
    materialize: F,
) -> Option<MirPlaceBase>
where
    F: FnMut(MirExpr) -> LocalId,
{
    let mut materialize = materialize;
    materialize_place_base(e, ctx, &mut materialize)
}

#[allow(clippy::only_used_in_recursion)]
pub(super) fn materialize_place_base<F>(
    e: MirExpr,
    ctx: &mut PassContext,
    materialize: &mut F,
) -> Option<MirPlaceBase>
where
    F: FnMut(MirExpr) -> LocalId,
{
    match e {
        MirExpr::Local(id) => Some(MirPlaceBase::Local(id)),
        MirExpr::Field { base, field, ty } => {
            let inner = materialize_place_base(*base, ctx, materialize)?;
            Some(MirPlaceBase::Field {
                base: Box::new(inner),
                field,
                ty,
            })
        }
        MirExpr::Index { base, index, ty } => Some(MirPlaceBase::Index { base, index, ty }),
        MirExpr::OptionalChain { base, ty } => {
            let inner = materialize_place_base(*base, ctx, materialize)?;
            Some(MirPlaceBase::Chain {
                base: Box::new(mir_place_base_to_expr(inner)),
                ty,
            })
        }
        non_place => Some(MirPlaceBase::Local(materialize(non_place))),
    }
}

impl ExprConverter {
    pub(super) fn ensure_place_pure_components(
        &mut self,
        place: MirPlace,
        out: &mut Vec<MirStmt>,
    ) -> MirPlace {
        match place {
            MirPlace::Local { id } => MirPlace::Local { id },
            MirPlace::Field { base, field, ty } => {
                let new_base = self.ensure_place_base_pure_components(*base, out);
                MirPlace::Field {
                    base: Box::new(new_base),
                    field,
                    ty,
                }
            }
            MirPlace::Index { base, index, ty } => {
                let new_base = self.ensure_mir_expr_pure_components(*base, out);
                let new_index = self.ensure_mir_expr_pure_components(*index, out);
                MirPlace::Index {
                    base: Box::new(new_base),
                    index: Box::new(new_index),
                    ty,
                }
            }
        }
    }

    fn ensure_place_base_pure_components(
        &mut self,
        base: MirPlaceBase,
        out: &mut Vec<MirStmt>,
    ) -> MirPlaceBase {
        match base {
            MirPlaceBase::Local(id) => MirPlaceBase::Local(id),
            MirPlaceBase::Field { base, field, ty } => {
                let inner = self.ensure_place_base_pure_components(*base, out);
                MirPlaceBase::Field {
                    base: Box::new(inner),
                    field,
                    ty,
                }
            }
            MirPlaceBase::Index { base, index, ty } => {
                let new_base = self.ensure_mir_expr_pure_components(*base, out);
                let new_index = self.ensure_mir_expr_pure_components(*index, out);
                MirPlaceBase::Index {
                    base: Box::new(new_base),
                    index: Box::new(new_index),
                    ty,
                }
            }
            MirPlaceBase::Chain { base, ty } => {
                let new_base = self.ensure_mir_expr_pure_components(*base, out);
                MirPlaceBase::Chain {
                    base: Box::new(new_base),
                    ty,
                }
            }
        }
    }

    fn ensure_mir_expr_pure_components(
        &mut self,
        expr: MirExpr,
        out: &mut Vec<MirStmt>,
    ) -> MirExpr {
        match expr {
            MirExpr::Local(id) => MirExpr::Local(id),
            MirExpr::Field { base, field, ty } => {
                let new_base = self.ensure_mir_expr_pure_components(*base, out);
                MirExpr::Field {
                    base: Box::new(new_base),
                    field,
                    ty,
                }
            }
            MirExpr::Index { base, index, ty } => {
                let new_base = self.ensure_mir_expr_pure_components(*base, out);
                let new_index = self.ensure_mir_expr_pure_components(*index, out);
                MirExpr::Index {
                    base: Box::new(new_base),
                    index: Box::new(new_index),
                    ty,
                }
            }
            other => {
                let local = self.fresh_local();
                let local_ty = mir_expr_ty(&other);
                self.push_temp_local(local, local_ty);
                out.push(MirStmt::Let {
                    local,
                    ty: local_ty,
                    init: Some(other),
                    mutable: false,
                });
                MirExpr::Local(local)
            }
        }
    }
}
