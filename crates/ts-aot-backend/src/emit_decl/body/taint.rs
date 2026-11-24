use super::super::ctx::{BodyCtx, EmitEnv};
use super::taint_scan::{expr_may_hold_self_capture, self_capture_scan};
use std::collections::HashSet;
use ts_aot_core::{FieldId, LocalId};
use ts_aot_ir_mir::{MirBlock, MirExpr, MirPlace, MirPlaceBase};

#[derive(Default, Clone)]
pub(super) struct SelfCaptureTaint {
    pub(super) whole: HashSet<LocalId>,
    pub(super) members: HashSet<(LocalId, FieldId)>,
    pub(super) receiver_aliases: HashSet<LocalId>,
    pub(super) receiver_owned: HashSet<LocalId>,
}

impl SelfCaptureTaint {
    pub(super) fn holds_local(&self, id: LocalId) -> bool {
        self.whole.contains(&id) || self.members.iter().any(|(local, _)| *local == id)
    }

    pub(super) fn kill_local(&mut self, id: LocalId) {
        self.whole.remove(&id);
        self.members.retain(|(local, _)| *local != id);
        self.receiver_aliases.remove(&id);
        self.receiver_owned.remove(&id);
    }

    pub(super) fn union(&mut self, other: &SelfCaptureTaint) {
        self.whole.extend(other.whole.iter().copied());
        self.members.extend(other.members.iter().copied());
        self.receiver_aliases
            .extend(other.receiver_aliases.iter().copied());
        self.receiver_owned
            .extend(other.receiver_owned.iter().copied());
    }

    pub(super) fn is_subset_of(&self, other: &SelfCaptureTaint) -> bool {
        self.whole.is_subset(&other.whole)
            && self.members.is_subset(&other.members)
            && self.receiver_aliases.is_subset(&other.receiver_aliases)
            && self.receiver_owned.is_subset(&other.receiver_owned)
    }

    pub(super) fn is_receiver_root(&self, id: LocalId, self_param: LocalId) -> bool {
        id == self_param || self.receiver_aliases.contains(&id) || self.receiver_owned.contains(&id)
    }

    pub(super) fn mark_receiver_alias(&mut self, id: LocalId) {
        self.receiver_aliases.insert(id);
    }

    pub(super) fn mark_receiver_owned(&mut self, id: LocalId) {
        self.receiver_owned.insert(id);
    }

    pub(super) fn taint_whole(&mut self, id: LocalId) -> bool {
        self.whole.insert(id)
    }

    pub(super) fn taint_member(&mut self, id: LocalId, field: FieldId) -> bool {
        self.members.insert((id, field))
    }

    pub(super) fn copy_members(&mut self, from: LocalId, to: LocalId) -> bool {
        let fields: Vec<FieldId> = self
            .members
            .iter()
            .filter(|(local, _)| *local == from)
            .map(|(_, field)| *field)
            .collect();
        let mut changed = false;
        for field in fields {
            changed |= self.members.insert((to, field));
        }
        changed
    }
}

pub(super) enum TaintTarget {
    Whole(LocalId),
    Member(LocalId, FieldId),
}

pub(super) fn expr_taint_target(expr: &MirExpr) -> Option<TaintTarget> {
    match expr {
        MirExpr::Local(id) => Some(TaintTarget::Whole(*id)),
        MirExpr::Field { base, field, .. } => match expr_taint_target(base) {
            Some(TaintTarget::Whole(local)) => Some(TaintTarget::Member(local, *field)),
            member @ Some(TaintTarget::Member(..)) => member,
            None => None,
        },
        MirExpr::Index { base, .. }
        | MirExpr::Cast { expr: base, .. }
        | MirExpr::OptionalChain { base, .. }
        | MirExpr::UnwrapOptional { expr: base, .. } => expr_taint_target(base),
        _ => None,
    }
}

pub(super) fn place_taint_target(place: &MirPlace) -> Option<TaintTarget> {
    match place {
        MirPlace::Local { id } => Some(TaintTarget::Whole(*id)),
        MirPlace::Global(_) => None,
        MirPlace::Field { base, field, .. } => match place_base_taint_target(base) {
            Some(TaintTarget::Whole(local)) => Some(TaintTarget::Member(local, *field)),
            member @ Some(TaintTarget::Member(..)) => member,
            None => None,
        },
        MirPlace::Index { base, .. } => expr_taint_target(base),
    }
}

fn place_base_taint_target(base: &MirPlaceBase) -> Option<TaintTarget> {
    match base {
        MirPlaceBase::Local(id) => Some(TaintTarget::Whole(*id)),
        MirPlaceBase::Global(_) => None,
        MirPlaceBase::Field { base, field, .. } => match place_base_taint_target(base) {
            Some(TaintTarget::Whole(local)) => Some(TaintTarget::Member(local, *field)),
            member @ Some(TaintTarget::Member(..)) => member,
            None => None,
        },
        MirPlaceBase::Index { base, .. } | MirPlaceBase::Chain { base, .. } => {
            expr_taint_target(base)
        }
    }
}

pub(super) fn apply_taint_target(
    taint: &mut SelfCaptureTaint,
    target: &TaintTarget,
    self_param: LocalId,
) {
    match *target {
        TaintTarget::Whole(id) if id != self_param => {
            taint.taint_whole(id);
        }
        TaintTarget::Member(id, field) if id != self_param => {
            taint.taint_member(id, field);
        }
        _ => {}
    }
}

pub(super) fn propagate_taint_to_local(
    taint: &mut SelfCaptureTaint,
    dest: LocalId,
    value: &MirExpr,
    self_param: LocalId,
    emit_env: &EmitEnv,
) {
    if let Some(src) = local_source(value) {
        if src == self_param || taint.receiver_aliases.contains(&src) {
            taint.mark_receiver_alias(dest);
        }
        if taint.receiver_owned.contains(&src) {
            taint.mark_receiver_owned(dest);
        }
        if !taint.whole.contains(&src) {
            taint.copy_members(src, dest);
            return;
        }
    } else if expr_is_receiver_rooted(value, self_param, taint) {
        taint.mark_receiver_owned(dest);
    }
    if expr_may_hold_self_capture(value, self_param, taint, emit_env) {
        taint.taint_whole(dest);
    }
}

pub(super) fn local_source(expr: &MirExpr) -> Option<LocalId> {
    match expr {
        MirExpr::Local(id) => Some(*id),
        MirExpr::Cast { expr, .. } => local_source(expr),
        _ => None,
    }
}

fn expr_is_receiver_rooted(expr: &MirExpr, self_param: LocalId, taint: &SelfCaptureTaint) -> bool {
    match expr {
        MirExpr::Local(id) => taint.is_receiver_root(*id, self_param),
        MirExpr::Field { base, .. }
        | MirExpr::Index { base, .. }
        | MirExpr::OptionalChain { base, .. }
        | MirExpr::UnwrapOptional { expr: base, .. }
        | MirExpr::Cast { expr: base, .. } => expr_is_receiver_rooted(base, self_param, taint),
        _ => false,
    }
}

pub(super) fn place_is_escaping_target(
    place: &MirPlace,
    self_param: LocalId,
    taint: &SelfCaptureTaint,
) -> bool {
    match place {
        MirPlace::Local { id } => *id == self_param,
        MirPlace::Global(_) => true,
        MirPlace::Field { base, .. } => place_base_is_escaping_target(base, self_param, taint),
        MirPlace::Index { base, .. } => expr_is_escaping_target(base, self_param, taint),
    }
}

fn place_base_is_escaping_target(
    base: &MirPlaceBase,
    self_param: LocalId,
    taint: &SelfCaptureTaint,
) -> bool {
    match base {
        MirPlaceBase::Local(id) => taint.is_receiver_root(*id, self_param),
        MirPlaceBase::Global(_) => true,
        MirPlaceBase::Chain { base, .. } | MirPlaceBase::Index { base, .. } => {
            expr_is_escaping_target(base, self_param, taint)
        }
        MirPlaceBase::Field { base, .. } => place_base_is_escaping_target(base, self_param, taint),
    }
}

pub(super) fn expr_is_escaping_target(
    expr: &MirExpr,
    self_param: LocalId,
    taint: &SelfCaptureTaint,
) -> bool {
    match expr {
        MirExpr::Global(_) => true,
        MirExpr::Local(id) => taint.is_receiver_root(*id, self_param),
        MirExpr::Field { base, .. }
        | MirExpr::Index { base, .. }
        | MirExpr::OptionalChain { base, .. }
        | MirExpr::IsSome { expr: base, .. }
        | MirExpr::UnwrapOptional { expr: base, .. }
        | MirExpr::Cast { expr: base, .. } => expr_is_escaping_target(base, self_param, taint),
        _ => false,
    }
}

pub(crate) fn self_capture_may_escape(
    block: &MirBlock,
    self_param: LocalId,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
) -> bool {
    let mut taint = SelfCaptureTaint::default();
    self_capture_scan(block, self_param, &mut taint, emit_env, body_ctx)
}
