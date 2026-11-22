use super::super::ctx::{BodyCtx, EmitEnv};
use super::helpers::{array_from_mapped_can_forward_element, ty_may_hold_fn};
use super::taint::{
    SelfCaptureTaint, TaintTarget, apply_taint_target, expr_is_escaping_target, expr_taint_target,
    local_source, place_is_escaping_target, place_taint_target, propagate_taint_to_local,
};
use ts_aot_core::{LocalId, Type, TypeId};
use ts_aot_ir_mir::{MirBlock, MirExpr, MirPlace, MirPlaceBase, MirStmt, RuntimeOp};

fn array_element_may_hold_fn(ty: TypeId, emit_env: &EmitEnv) -> bool {
    matches!(
        emit_env.types.resolve(ty),
        Some(Type::Array { element }) if ty_may_hold_fn(*element, emit_env)
    )
}

pub(super) fn expr_may_hold_self_capture(
    expr: &MirExpr,
    self_param: LocalId,
    taint: &SelfCaptureTaint,
    emit_env: &EmitEnv,
) -> bool {
    match expr {
        MirExpr::Closure { captures, .. } => {
            captures.contains(&self_param) || captures.iter().any(|id| taint.holds_local(*id))
        }
        MirExpr::Local(id) => taint.holds_local(*id),
        MirExpr::StructLiteral { fields, .. } => fields
            .iter()
            .any(|(_, value)| expr_may_hold_self_capture(value, self_param, taint, emit_env)),
        MirExpr::Field { base, field, ty } => {
            if !ty_may_hold_fn(*ty, emit_env) {
                return false;
            }
            match base.as_ref() {
                MirExpr::Local(local) => {
                    taint.whole.contains(local) || taint.members.contains(&(*local, *field))
                }
                _ => match expr_taint_target(base) {
                    Some(TaintTarget::Member(local, outer)) => {
                        taint.whole.contains(&local) || taint.members.contains(&(local, outer))
                    }
                    _ => expr_may_hold_self_capture(base, self_param, taint, emit_env),
                },
            }
        }
        MirExpr::Index { base, ty, .. } => {
            if !ty_may_hold_fn(*ty, emit_env) {
                return false;
            }
            match expr_taint_target(base) {
                Some(TaintTarget::Whole(local)) => taint.holds_local(local),
                Some(TaintTarget::Member(local, member)) => {
                    taint.whole.contains(&local) || taint.members.contains(&(local, member))
                }
                None => expr_may_hold_self_capture(base, self_param, taint, emit_env),
            }
        }
        MirExpr::ResultOk { value: base, .. }
        | MirExpr::ResultErr { error: base, .. }
        | MirExpr::Propagate { expr: base, .. }
        | MirExpr::Await { expr: base, .. }
        | MirExpr::Cast { expr: base, .. }
        | MirExpr::OptionalChain { base, .. }
        | MirExpr::UnwrapOptional { expr: base, .. } => {
            expr_may_hold_self_capture(base, self_param, taint, emit_env)
        }
        MirExpr::Yield { expr, .. } => expr
            .as_ref()
            .is_some_and(|value| expr_may_hold_self_capture(value, self_param, taint, emit_env)),
        MirExpr::Call { args, .. } => {
            expr.ty().is_some_and(|ty| ty_may_hold_fn(ty, emit_env))
                && args
                    .iter()
                    .any(|arg| expr_may_hold_self_capture(arg, self_param, taint, emit_env))
        }
        MirExpr::IndirectCall { callee, args, .. } => {
            expr.ty().is_some_and(|ty| ty_may_hold_fn(ty, emit_env))
                && (expr_may_hold_self_capture(callee, self_param, taint, emit_env)
                    || args
                        .iter()
                        .any(|arg| expr_may_hold_self_capture(arg, self_param, taint, emit_env)))
        }
        MirExpr::Unit
        | MirExpr::Bool(_)
        | MirExpr::Int { .. }
        | MirExpr::Float { .. }
        | MirExpr::String { .. }
        | MirExpr::Null { .. }
        | MirExpr::Global(_)
        | MirExpr::TemplateStringsArray { .. }
        | MirExpr::RegExp { .. }
        | MirExpr::BigInt { .. }
        | MirExpr::Import { .. }
        | MirExpr::Binary { .. }
        | MirExpr::Unary { .. }
        | MirExpr::IsSome { .. }
        | MirExpr::TypeOf { .. } => false,
    }
}

fn place_base_may_throw(base: &MirPlaceBase) -> bool {
    match base {
        MirPlaceBase::Local(_) | MirPlaceBase::Global(_) => false,
        MirPlaceBase::Field { base, .. } => place_base_may_throw(base),
        MirPlaceBase::Index { base, index, .. } => expr_may_throw(base) || expr_may_throw(index),
        MirPlaceBase::Chain { base, .. } => expr_may_throw(base),
    }
}

fn place_may_throw(place: &MirPlace) -> bool {
    match place {
        MirPlace::Local { .. } | MirPlace::Global(_) => false,
        MirPlace::Field { base, .. } => place_base_may_throw(base),
        MirPlace::Index { base, index, .. } => expr_may_throw(base) || expr_may_throw(index),
    }
}

fn expr_may_throw(expr: &MirExpr) -> bool {
    match expr {
        MirExpr::Call { .. }
        | MirExpr::IndirectCall { .. }
        | MirExpr::Propagate { .. }
        | MirExpr::Await { .. }
        | MirExpr::Yield { .. } => true,
        MirExpr::Field { base, .. }
        | MirExpr::OptionalChain { base, .. }
        | MirExpr::IsSome { expr: base, .. }
        | MirExpr::UnwrapOptional { expr: base, .. }
        | MirExpr::TypeOf { expr: base, .. }
        | MirExpr::Cast { expr: base, .. }
        | MirExpr::ResultOk { value: base, .. }
        | MirExpr::ResultErr { error: base, .. }
        | MirExpr::Unary { expr: base, .. } => expr_may_throw(base),
        MirExpr::Index { base, index, .. } => expr_may_throw(base) || expr_may_throw(index),
        MirExpr::Binary { left, right, .. } => expr_may_throw(left) || expr_may_throw(right),
        MirExpr::StructLiteral { fields, .. } => {
            fields.iter().any(|(_, value)| expr_may_throw(value))
        }
        MirExpr::Import { source, .. } => expr_may_throw(source),
        _ => false,
    }
}

fn stmt_may_throw(stmt: &MirStmt) -> bool {
    match stmt {
        MirStmt::Throw { .. } | MirStmt::ReturnResultErr { .. } => true,
        MirStmt::Runtime { op, .. } => matches!(op, RuntimeOp::ThrowTypeError),
        MirStmt::Let {
            init: Some(init), ..
        }
        | MirStmt::LetInfer { init, .. } => expr_may_throw(init),
        MirStmt::Assign { target, value } => place_may_throw(target) || expr_may_throw(value),
        MirStmt::Expr(expr) | MirStmt::Return(Some(expr)) => expr_may_throw(expr),
        MirStmt::If { cond, .. } | MirStmt::While { cond, .. } | MirStmt::DoWhile { cond, .. } => {
            expr_may_throw(cond)
        }
        MirStmt::ForOf { iterable, .. } | MirStmt::ForAwaitOf { iterable, .. } => {
            expr_may_throw(iterable)
        }
        MirStmt::ForIn { object, .. } => expr_may_throw(object),
        MirStmt::Switch { disc, .. } => expr_may_throw(disc),
        _ => false,
    }
}

fn self_capture_loop_fixpoint(
    body: &MirBlock,
    self_param: LocalId,
    header_taint: &mut SelfCaptureTaint,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
    thrown: &mut SelfCaptureTaint,
) -> bool {
    loop {
        let mut pass_taint = header_taint.clone();
        if self_capture_scan_throwing(
            body,
            self_param,
            &mut pass_taint,
            emit_env,
            body_ctx,
            thrown,
        ) {
            return true;
        }
        if pass_taint.is_subset_of(header_taint) {
            return false;
        }
        header_taint.union(&pass_taint);
    }
}

pub(super) fn self_capture_scan(
    block: &MirBlock,
    self_param: LocalId,
    taint: &mut SelfCaptureTaint,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
) -> bool {
    let mut thrown = SelfCaptureTaint::default();
    self_capture_scan_throwing(block, self_param, taint, emit_env, body_ctx, &mut thrown)
}

fn self_capture_scan_throwing(
    block: &MirBlock,
    self_param: LocalId,
    taint: &mut SelfCaptureTaint,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
    thrown: &mut SelfCaptureTaint,
) -> bool {
    for stmt in &block.stmts {
        if stmt_may_throw(stmt) {
            thrown.union(taint);
        }
        match stmt {
            MirStmt::Let { local, init, .. } => {
                taint.kill_local(*local);
                if let Some(value) = init {
                    propagate_taint_to_local(taint, *local, value, self_param, emit_env);
                }
            }
            MirStmt::LetInfer { local, init } => {
                taint.kill_local(*local);
                propagate_taint_to_local(taint, *local, init, self_param, emit_env);
            }
            MirStmt::Assign { target, value } => match target {
                MirPlace::Local { id } if *id == self_param => {
                    if expr_may_hold_self_capture(value, self_param, taint, emit_env) {
                        return true;
                    }
                }
                MirPlace::Local { id } => {
                    if local_source(value) != Some(*id) {
                        taint.kill_local(*id);
                        propagate_taint_to_local(taint, *id, value, self_param, emit_env);
                    }
                }
                _ => {
                    if place_is_escaping_target(target, self_param, taint)
                        && expr_may_hold_self_capture(value, self_param, taint, emit_env)
                    {
                        return true;
                    }
                    if let Some(target) = place_taint_target(target)
                        && expr_may_hold_self_capture(value, self_param, taint, emit_env)
                    {
                        apply_taint_target(taint, &target, self_param);
                    }
                }
            },
            MirStmt::Return(Some(value)) => {
                if expr_may_hold_self_capture(value, self_param, taint, emit_env) {
                    return true;
                }
            }
            MirStmt::Throw { error, .. } | MirStmt::ReturnResultErr { error, .. } => {
                if expr_may_hold_self_capture(error, self_param, taint, emit_env) {
                    return true;
                }
            }
            MirStmt::Runtime {
                op, args, dest, ty, ..
            } => {
                if matches!(
                    op,
                    RuntimeOp::ArrayPush
                        | RuntimeOp::ArraySet
                        | RuntimeOp::MapSet
                        | RuntimeOp::WeakMapSet
                ) && args
                    .iter()
                    .skip(1)
                    .any(|arg| expr_may_hold_self_capture(arg, self_param, taint, emit_env))
                    && args.first().is_some_and(|container| {
                        expr_is_escaping_target(container, self_param, taint)
                    })
                {
                    return true;
                }
                let carries = args
                    .iter()
                    .any(|arg| expr_may_hold_self_capture(arg, self_param, taint, emit_env));
                if matches!(
                    op,
                    RuntimeOp::ArrayCreate
                        | RuntimeOp::ArraySlice
                        | RuntimeOp::ArrayConcat
                        | RuntimeOp::ArrayFrom
                ) && let Some(dest) = dest
                    && carries
                {
                    taint.taint_whole(*dest);
                }
                if matches!(op, RuntimeOp::ArrayFromMapped)
                    && let Some(dest) = dest
                    && carries
                    && (args.first().is_some_and(|source| {
                        array_from_mapped_can_forward_element(*ty, source, emit_env, body_ctx)
                    }) || (args.get(1).is_some_and(|mapper| {
                        expr_may_hold_self_capture(mapper, self_param, taint, emit_env)
                    }) && array_element_may_hold_fn(*ty, emit_env)))
                {
                    taint.taint_whole(*dest);
                }
                if matches!(op, RuntimeOp::ArrayGet | RuntimeOp::ArrayGetOrDefault)
                    && let Some(dest) = dest
                    && args.first().is_some_and(|container| {
                        expr_may_hold_self_capture(container, self_param, taint, emit_env)
                    })
                {
                    taint.taint_whole(*dest);
                }
                if matches!(
                    op,
                    RuntimeOp::ArrayPush
                        | RuntimeOp::ArraySet
                        | RuntimeOp::MapSet
                        | RuntimeOp::WeakMapSet
                ) && args
                    .iter()
                    .skip(1)
                    .any(|arg| expr_may_hold_self_capture(arg, self_param, taint, emit_env))
                    && let Some(target) = args.first().and_then(expr_taint_target)
                {
                    apply_taint_target(taint, &target, self_param);
                }
            }
            MirStmt::If {
                then_block,
                else_block,
                ..
            } => {
                let mut then_taint = (*taint).clone();
                let then_escape = self_capture_scan_throwing(
                    then_block,
                    self_param,
                    &mut then_taint,
                    emit_env,
                    body_ctx,
                    thrown,
                );
                let mut else_taint = (*taint).clone();
                let else_escape = else_block.as_ref().is_some_and(|else_block| {
                    self_capture_scan_throwing(
                        else_block,
                        self_param,
                        &mut else_taint,
                        emit_env,
                        body_ctx,
                        thrown,
                    )
                });
                *taint = then_taint;
                taint.union(&else_taint);
                if then_escape || else_escape {
                    return true;
                }
            }
            MirStmt::ForOf {
                item,
                iterable,
                body,
                ..
            }
            | MirStmt::ForAwaitOf {
                item,
                iterable,
                body,
                ..
            } => {
                if expr_may_hold_self_capture(iterable, self_param, taint, emit_env) {
                    taint.taint_whole(*item);
                }
                let mut body_taint = (*taint).clone();
                let escaped = self_capture_loop_fixpoint(
                    body,
                    self_param,
                    &mut body_taint,
                    emit_env,
                    body_ctx,
                    thrown,
                );
                taint.union(&body_taint);
                if escaped {
                    return true;
                }
            }
            MirStmt::While { body, .. }
            | MirStmt::DoWhile { body, .. }
            | MirStmt::ForIn { body, .. } => {
                let mut body_taint = (*taint).clone();
                let escaped = self_capture_loop_fixpoint(
                    body,
                    self_param,
                    &mut body_taint,
                    emit_env,
                    body_ctx,
                    thrown,
                );
                taint.union(&body_taint);
                if escaped {
                    return true;
                }
            }
            MirStmt::Switch { cases, default, .. } => {
                let mut merged = (*taint).clone();
                let mut escaped = false;
                for case in cases {
                    let mut case_taint = (*taint).clone();
                    escaped |= self_capture_scan_throwing(
                        &case.body,
                        self_param,
                        &mut case_taint,
                        emit_env,
                        body_ctx,
                        thrown,
                    );
                    merged.union(&case_taint);
                }
                if let Some(default) = default {
                    let mut default_taint = (*taint).clone();
                    escaped |= self_capture_scan_throwing(
                        default,
                        self_param,
                        &mut default_taint,
                        emit_env,
                        body_ctx,
                        thrown,
                    );
                    merged.union(&default_taint);
                }
                taint.union(&merged);
                if escaped {
                    return true;
                }
            }
            MirStmt::Try {
                body,
                catch,
                finally,
                ..
            } => {
                let mut body_taint = (*taint).clone();
                let mut body_thrown = SelfCaptureTaint::default();
                let body_escape = self_capture_scan_throwing(
                    body,
                    self_param,
                    &mut body_taint,
                    emit_env,
                    body_ctx,
                    &mut body_thrown,
                );
                let mut catch_taint = (*taint).clone();
                catch_taint.union(&body_taint);
                catch_taint.union(&body_thrown);
                let catch_escape = catch.as_ref().is_some_and(|catch| {
                    self_capture_scan_throwing(
                        catch,
                        self_param,
                        &mut catch_taint,
                        emit_env,
                        body_ctx,
                        thrown,
                    )
                });
                if catch.is_none() {
                    thrown.union(&body_thrown);
                }
                taint.union(&body_taint);
                taint.union(&catch_taint);
                if let Some(finally) = finally {
                    let mut finally_taint = (*taint).clone();
                    let finally_escape = self_capture_scan_throwing(
                        finally,
                        self_param,
                        &mut finally_taint,
                        emit_env,
                        body_ctx,
                        thrown,
                    );
                    taint.union(&finally_taint);
                    if finally_escape {
                        return true;
                    }
                }
                if body_escape || catch_escape {
                    return true;
                }
            }
            _ => {}
        }
    }
    false
}
