use std::collections::{HashMap, HashSet, VecDeque};

use ts_aot_core::{Atom, FunctionId, NullishKinds, Type, TypeId, TypeTable};
use ts_aot_ir_hir::{
    HirBinaryOp, HirCallee, HirDecl, HirExpr, HirFunction, HirProgram, HirStmt, ObjectLiteralField,
    Visitor, VisitorMut, walk_expr, walk_expr_mut, walk_stmt_mut,
};

use crate::PassContext;
use crate::hir_to_mir::{ExprConverter, optional_chain_result};

use super::MonomorphizeStats;
use super::infer::{
    build_mapping, format_type_args, hir_expr_ty, infer_type_args, type_args_resolved,
    type_resolved,
};
use super::substitute::{TypeParamMap, TypeSubstitutionResult, substitute_func};
use super::substitute_ty::substitute_type;

type SpecializationKey = (FunctionId, Vec<TypeId>);
#[derive(Default)]
struct MethodIndex {
    generic: HashMap<(TypeId, Atom, bool), FunctionId>,
    class_types: HashMap<Atom, TypeId>,
}

impl MethodIndex {
    fn insert(&mut self, key: (TypeId, Atom, bool), fid: FunctionId) {
        self.generic.insert(key, fid);
    }

    fn get(&self, key: &(TypeId, Atom, bool)) -> Option<&FunctionId> {
        self.generic.get(key)
    }
}

type OnCallee<'a> = &'a mut dyn FnMut(&mut HirCallee, &mut [HirExpr], &[TypeId]);

pub fn monomorphize(
    program: &mut HirProgram,
    types: &mut TypeTable,
    ctx: &mut PassContext,
) -> MonomorphizeStats {
    let mut stats = MonomorphizeStats::default();
    let mut next_fid: u32 = 0;
    let mut generic_fn_ids: HashSet<FunctionId> = HashSet::new();
    let mut fn_index: HashMap<FunctionId, HirFunction> = HashMap::new();

    let mut name_to_generic: HashMap<Atom, FunctionId> = HashMap::new();
    let mut method_to_generic: MethodIndex = MethodIndex::default();
    let mut method_owner: HashMap<FunctionId, (TypeId, bool)> = HashMap::new();
    classify_decls(
        &program.declarations,
        &mut next_fid,
        &mut generic_fn_ids,
        &mut fn_index,
        &mut name_to_generic,
        &mut method_to_generic,
        &mut method_owner,
        &mut stats,
    );

    let mut worklist: VecDeque<SpecializationKey> = VecDeque::new();
    let mut inference = ExprConverter::new();
    if !generic_fn_ids.is_empty() {
        inference.set_program(std::sync::Arc::new(program.clone()));
    }
    {
        let mut on_callee =
            |callee: &mut HirCallee, args: &mut [HirExpr], explicit_type_args: &[TypeId]| {
                push_specialization(
                    callee,
                    args,
                    explicit_type_args,
                    &generic_fn_ids,
                    &name_to_generic,
                    &method_to_generic,
                    &fn_index,
                    &inference,
                    types,
                    &mut worklist,
                );
            };
        visit_callees(&mut program.declarations, &mut on_callee);
        for inits in program.class_field_inits.values_mut() {
            for (_, init) in inits.iter_mut() {
                visit_expr_callees(init, &mut on_callee);
            }
        }
    }

    let mut mono_for_specialization: HashMap<SpecializationKey, FunctionId> = HashMap::new();
    let mut specialization_names: HashMap<FunctionId, Atom> = HashMap::new();
    let mut new_decls: Vec<HirDecl> = Vec::new();
    let mut method_specializations: Vec<(TypeId, HirFunction)> = Vec::new();
    let mut processed: HashSet<SpecializationKey> = HashSet::new();

    while let Some(key) = worklist.pop_front() {
        if !processed.insert(key.clone()) {
            continue;
        }
        let (generic_fid, type_args) = key.clone();
        let Some(generic_fn) = fn_index.get(&generic_fid).cloned() else {
            continue;
        };
        let mapping = build_mapping(&generic_fn, &type_args);
        let mono_fid = FunctionId::from_raw(next_fid);
        next_fid += 1;
        let mut mono_subst_result = TypeSubstitutionResult::default();
        let mut mono = substitute_func(&generic_fn, &mapping, types, &mut mono_subst_result);
        mono.name = Atom::from(format!(
            "{}_mono_{}",
            generic_fn.name.as_str(),
            format_type_args(&type_args)
        ));
        specialization_names.insert(mono_fid, mono.name.clone());
        let mono_decl = HirDecl::Function(mono.clone());

        let mut mono_for_scan = mono_decl.clone();
        let mut on_callee =
            |callee: &mut HirCallee, args: &mut [HirExpr], explicit_type_args: &[TypeId]| {
                push_specialization(
                    callee,
                    args,
                    explicit_type_args,
                    &generic_fn_ids,
                    &name_to_generic,
                    &method_to_generic,
                    &fn_index,
                    &inference,
                    types,
                    &mut worklist,
                );
            };
        visit_decl_callees(&mut mono_for_scan, &mut on_callee);

        if let Some((owner_ty, _)) = method_owner.get(&generic_fid).copied() {
            method_specializations.push((owner_ty, mono));
        } else {
            new_decls.push(mono_decl);
        }
        mono_for_specialization.insert(key, mono_fid);
        stats.monomorphized += 1;
    }

    program.declarations.extend(new_decls);
    for (owner_ty, method) in method_specializations {
        let name = method.name.clone();
        let class = program.declarations.iter_mut().find_map(|decl| match decl {
            HirDecl::Class(c) if c.ty == owner_ty => Some(c),
            _ => None,
        });
        match class {
            Some(class) => class.methods.push(method),
            None => ctx.error(
                "E0300",
                format!(
                    "the class of method `{}` is not emitted (it was dropped as unemittable), \
                     so its specialization has nowhere to go",
                    name.as_str()
                ),
                ts_aot_core::Span::new(0, 0),
            ),
        }
    }

    {
        let mut on_callee = |callee: &mut HirCallee,
                             args: &mut [HirExpr],
                             explicit_type_args: &[TypeId]| {
            let Some(fid) = generic_callee(
                callee,
                &generic_fn_ids,
                &name_to_generic,
                &method_to_generic,
                &inference,
                types,
            ) else {
                return;
            };
            let Some(generic_fn) = fn_index.get(&fid) else {
                return;
            };
            if let Some(span) = narrow_optional_chain_receiver_args(callee, generic_fn, args, types)
            {
                ctx.error(
                    "E0300",
                    "another argument of this optional-chain call reassigns the receiver, so the \
                     argument that reads the receiver cannot be narrowed to the non-nullish type \
                     the guard checked; the AOT target would not pass the receiver's pre-call \
                     value. Move the reading argument out of the call, or pass a value that is not \
                     the receiver local",
                    span,
                );
                return;
            }
            let type_args = infer_for_callee(
                callee,
                &method_to_generic,
                generic_fn,
                args,
                explicit_type_args,
                &inference,
                types,
            );
            let key: SpecializationKey = (fid, type_args);
            if let Some(&mono_fid) = mono_for_specialization.get(&key) {
                let renamed = match (&mut *callee, specialization_names.get(&mono_fid)) {
                    (HirCallee::Indirect(inner), Some(mono_name)) => match inner.as_mut() {
                        HirExpr::Field { field_name, .. } if method_owner.contains_key(&fid) => {
                            *field_name = mono_name.clone();
                            true
                        }
                        _ => false,
                    },
                    _ => false,
                };
                if !renamed {
                    *callee = HirCallee::Function(mono_fid);
                }
                stats.calls_rewritten += 1;
            }
        };
        visit_callees(&mut program.declarations, &mut on_callee);
        for inits in program.class_field_inits.values_mut() {
            for (_, init) in inits.iter_mut() {
                visit_expr_callees(init, &mut on_callee);
            }
        }
    }

    let mut mapping_by_fid: HashMap<FunctionId, TypeParamMap> = HashMap::new();
    let mut mapping_by_name: HashMap<Atom, TypeParamMap> = HashMap::new();
    let mut mapping_by_owner: HashMap<(TypeId, Atom, bool), TypeParamMap> = HashMap::new();
    for ((generic_fid, type_args), mono_fid) in &mono_for_specialization {
        let Some(generic_fn) = fn_index.get(generic_fid) else {
            continue;
        };
        let mapping = build_mapping(generic_fn, type_args);
        mapping_by_fid.insert(*mono_fid, mapping.clone());
        if let Some(&(owner_ty, is_static)) = method_owner.get(generic_fid)
            && let Some(name) = specialization_names.get(mono_fid)
        {
            mapping_by_owner.insert((owner_ty, name.clone(), is_static), mapping.clone());
            mapping_by_name.insert(name.clone(), mapping);
        }
    }
    let mut struct_shapes: StructShapes = HashMap::new();
    repair_specialized_call_types(
        &mut program.declarations,
        types,
        &mapping_by_fid,
        &mapping_by_name,
        &mapping_by_owner,
        &method_to_generic.class_types,
        &inference,
        &mut struct_shapes,
    );
    for inits in program.class_field_inits.values_mut() {
        for (_, init) in inits.iter_mut() {
            let mut repair = CallTypeRepair {
                types: &mut *types,
                by_fid: &mapping_by_fid,
                by_name: &mapping_by_name,
                by_owner: &mapping_by_owner,
                class_types: &method_to_generic.class_types,
                converter: &inference,
                struct_shapes: &mut struct_shapes,
                local_tys: HashMap::new(),
                retyped_locals: HashMap::new(),
            };
            repair.visit_expr_mut(init);
        }
    }
    repair_struct_field_types(&mut program.declarations, &struct_shapes, types);

    stats
}

type StructShapes = HashMap<TypeId, Option<Vec<TypeId>>>;

struct CallTypeRepair<'a> {
    types: &'a mut TypeTable,
    by_fid: &'a HashMap<FunctionId, TypeParamMap>,
    by_name: &'a HashMap<Atom, TypeParamMap>,
    by_owner: &'a HashMap<(TypeId, Atom, bool), TypeParamMap>,
    class_types: &'a HashMap<Atom, TypeId>,
    converter: &'a ExprConverter,
    struct_shapes: &'a mut StructShapes,
    local_tys: HashMap<ts_aot_core::LocalId, TypeId>,
    retyped_locals: HashMap<ts_aot_core::LocalId, (TypeId, TypeId)>,
}

impl CallTypeRepair<'_> {
    fn mapping_of(&mut self, callee: &HirCallee) -> Option<TypeParamMap> {
        match callee {
            HirCallee::Function(fid) => self.by_fid.get(fid).cloned(),
            HirCallee::Indirect(inner) => match inner.as_ref() {
                HirExpr::Field {
                    owner, field_name, ..
                } => {
                    let (ty, is_static) =
                        receiver_owner(owner, self.class_types, self.converter, &mut *self.types);
                    let primary = self
                        .by_owner
                        .get(&(ty, field_name.clone(), is_static))
                        .cloned();
                    if primary.is_some() {
                        return primary;
                    }
                    self.by_name.get(field_name).cloned()
                }
                HirExpr::Global { name, .. } => self.by_name.get(name).cloned(),
                _ => None,
            },
            HirCallee::Closure(_) | HirCallee::Runtime { .. } => None,
        }
    }

    fn repair_ty(&mut self, ty: &mut TypeId, mapping: &TypeParamMap) {
        let mut result = TypeSubstitutionResult::default();
        let substituted = substitute_type(*ty, mapping, self.types, &mut result);
        if substituted != *ty {
            *ty = substituted;
        }
    }

    fn optional_chain_call(&self, callee: &HirCallee) -> bool {
        let HirCallee::Indirect(inner) = callee else {
            return false;
        };
        let HirExpr::Field { owner, .. } = inner.as_ref() else {
            return false;
        };
        matches!(owner.as_ref(), HirExpr::OptionalChain { .. })
            && matches!(
                self.types.resolve(owner.ty()),
                Some(Type::Null | Type::Void | Type::Optional { .. })
            )
    }

    fn repair_expr(&mut self, expr: &mut HirExpr) -> Option<TypeId> {
        match expr {
            HirExpr::Call {
                callee, args, ty, ..
            } => {
                if let HirCallee::Indirect(inner) = callee {
                    self.visit_expr_mut(inner);
                }
                for arg in args.iter_mut() {
                    self.visit_expr_mut(arg);
                }
                let mapping = self.mapping_of(callee)?;
                self.repair_ty(ty, &mapping);
                if self.optional_chain_call(callee) {
                    *ty = optional_chain_result(*ty, self.types);
                }
                Some(*ty)
            }
            HirExpr::OptionalChain { base, ty, .. } => {
                let repaired = self.repair_expr(base);
                *ty = base.ty();
                repaired.map(|_| *ty)
            }
            HirExpr::Binary {
                op: HirBinaryOp::Coalesce,
                lhs,
                rhs,
                ty,
                ..
            } => {
                self.repair_expr(lhs);
                self.repair_expr(rhs);
                match self.types.resolve(lhs.ty()).cloned() {
                    Some(Type::Null | Type::Void) => {
                        if let Some(rhs_ty) = hir_expr_ty(rhs, self.types) {
                            *ty = rhs_ty;
                        }
                    }
                    Some(Type::Optional { inner, .. }) => {
                        let mut present = inner;
                        while let Some(Type::Optional { inner: next, .. }) =
                            self.types.resolve(present).cloned()
                        {
                            if matches!(self.types.resolve(next), None | Some(Type::Error)) {
                                break;
                            }
                            present = next;
                        }
                        match self.types.resolve(rhs.ty()).cloned() {
                            Some(Type::Optional {
                                inner: rhs_inner,
                                nullish,
                            }) if rhs_inner == inner => {
                                *ty = self.types.intern(&Type::Optional { inner, nullish });
                            }
                            Some(Type::Optional {
                                inner: rhs_inner,
                                nullish,
                            }) if rhs_inner == present => {
                                *ty = self.types.intern(&Type::Optional {
                                    inner: present,
                                    nullish,
                                });
                            }
                            Some(Type::Optional {
                                inner: rhs_inner,
                                nullish,
                            }) => {
                                *ty = if matches!(
                                    self.types.resolve(rhs_inner),
                                    Some(Type::Optional { .. })
                                ) {
                                    rhs.ty()
                                } else {
                                    self.types.intern(&Type::Optional {
                                        inner: present,
                                        nullish,
                                    })
                                };
                            }
                            Some(Type::Null) => {
                                *ty = self.types.intern(&Type::Optional {
                                    inner: present,
                                    nullish: NullishKinds::NULL_ONLY,
                                });
                            }
                            Some(Type::Void) => {
                                *ty = self.types.intern(&Type::Optional {
                                    inner: present,
                                    nullish: NullishKinds::UNDEFINED_ONLY,
                                });
                            }
                            _ => match rhs.as_ref() {
                                HirExpr::Null(_) => {
                                    *ty = self.types.intern(&Type::Optional {
                                        inner: present,
                                        nullish: NullishKinds::NULL_ONLY,
                                    });
                                }
                                HirExpr::Undefined(_) | HirExpr::Unit(_) => {
                                    *ty = self.types.intern(&Type::Optional {
                                        inner: present,
                                        nullish: NullishKinds::UNDEFINED_ONLY,
                                    });
                                }
                                _ => {
                                    *ty = present;
                                }
                            },
                        }
                    }
                    _ => {
                        *ty = lhs.ty();
                    }
                }
                Some(*ty)
            }
            HirExpr::Binary {
                op: HirBinaryOp::And | HirBinaryOp::Or,
                lhs,
                rhs,
                ty,
                ..
            } => {
                let left = self.repair_expr(lhs);
                let right = self.repair_expr(rhs);
                match (left, right) {
                    (Some(left), Some(right)) if left == right => {
                        if !type_resolved(*ty, self.types) {
                            *ty = left;
                        }
                        Some(*ty)
                    }
                    _ => None,
                }
            }
            HirExpr::ArrayLiteral { elements, ty, .. } => {
                for element in elements.iter_mut() {
                    self.repair_expr(element);
                }
                self.repair_array_element(ty, elements);
                Some(*ty)
            }
            HirExpr::Index {
                owner, index, ty, ..
            } => {
                self.repair_expr(owner);
                self.repair_expr(index);
                self.repair_index_element(owner, ty);
                Some(*ty)
            }
            HirExpr::Local { id, ty, .. } => {
                if !type_resolved(*ty, self.types)
                    && let Some(&declared) = self.local_tys.get(id)
                    && type_resolved(declared, self.types)
                {
                    *ty = declared;
                }
                if let Some(&(from, to)) = self.retyped_locals.get(id)
                    && *ty == from
                {
                    *ty = to;
                }
                Some(*ty)
            }
            HirExpr::Closure { captures, body, .. } => {
                for capture in captures.iter_mut() {
                    self.repair_expr(capture);
                }
                let saved = std::mem::take(&mut self.local_tys);
                for stmt in body.iter_mut() {
                    self.visit_stmt_mut(stmt);
                }
                self.local_tys = saved;
                None
            }
            HirExpr::StructLiteral { fields, ty, .. } => {
                for (_, value) in fields.iter_mut() {
                    self.repair_expr(value);
                }
                self.record_struct_shape(*ty, fields);
                Some(*ty)
            }
            HirExpr::TypeAssertion { expr, target, .. } => {
                let repaired = self.repair_expr(expr);
                if !type_resolved(*target, self.types)
                    && let Some(mut value_ty) = repaired
                {
                    while let Some(Type::Optional { inner, .. }) =
                        self.types.resolve(value_ty).cloned()
                    {
                        value_ty = inner;
                    }
                    *target = value_ty;
                }
                if matches!(self.types.resolve(*target), None | Some(Type::Error)) {
                    repaired
                } else {
                    Some(*target)
                }
            }
            other => {
                walk_expr_mut(self, other);
                None
            }
        }
    }

    fn repair_array_element(&mut self, ty: &mut TypeId, elements: &[HirExpr]) {
        let Some(Type::Array { element }) = self.types.resolve(*ty).cloned() else {
            return;
        };
        let Some(inferred) = self.uniform_element_ty(elements) else {
            return;
        };
        if inferred == element {
            return;
        }
        let widens_to_optional = type_resolved(element, self.types)
            && matches!(
                self.types.resolve(inferred),
                Some(Type::Optional { inner, .. }) if *inner == element
            );
        if type_resolved(element, self.types) && !widens_to_optional {
            return;
        }
        *ty = self.types.intern(&Type::Array { element: inferred });
    }

    fn uniform_element_ty(&mut self, elements: &[HirExpr]) -> Option<TypeId> {
        let mut inferred: Option<TypeId> = None;
        for element_expr in elements {
            let element_ty = hir_expr_ty(element_expr, self.types)?;
            if !type_resolved(element_ty, self.types)
                || matches!(self.types.resolve(element_ty), Some(Type::Error) | None)
            {
                return None;
            }
            match inferred {
                None => inferred = Some(element_ty),
                Some(previous) if previous == element_ty => {}
                Some(_) => return None,
            }
        }
        inferred
    }

    fn repair_index_element(&mut self, owner: &HirExpr, ty: &mut TypeId) {
        if type_resolved(*ty, self.types) {
            return;
        }
        let owner_ty = match owner {
            HirExpr::Local { id, ty: stale, .. } if !type_resolved(*stale, self.types) => {
                self.local_tys.get(id).copied().unwrap_or(*stale)
            }
            other => other.ty(),
        };
        if let Some(Type::Array { element }) = self.types.resolve(owner_ty).cloned() {
            *ty = element;
        }
    }

    fn record_struct_shape(&mut self, ty: TypeId, fields: &[(ts_aot_core::FieldId, HirExpr)]) {
        if !matches!(self.types.resolve(ty), Some(Type::Struct { .. })) {
            return;
        }
        let mut slots: Vec<Option<TypeId>> = vec![None; fields.len()];
        for (field, value) in fields {
            let Some(slot) = slots.get_mut(field.raw() as usize) else {
                return;
            };
            let Some(field_ty) = hir_expr_ty(value, self.types) else {
                return;
            };
            *slot = Some(field_ty);
        }
        let mut shape = Vec::with_capacity(slots.len());
        for field_ty in slots {
            let Some(field_ty) = field_ty else {
                return;
            };
            shape.push(field_ty);
        }
        match self.struct_shapes.get(&ty).cloned() {
            None => {
                self.struct_shapes.insert(ty, Some(shape));
            }
            Some(Some(previous)) if previous != shape => {
                self.struct_shapes.insert(ty, None);
            }
            _ => {}
        }
    }
}

impl VisitorMut for CallTypeRepair<'_> {
    fn visit_stmt_mut(&mut self, stmt: &mut HirStmt) {
        match stmt {
            HirStmt::Let { id, ty, init, .. } => {
                if let Some(init) = init.as_mut() {
                    let inferred_from_init = *ty == init.ty();
                    let nullish_literal_fallback = matches!(
                        init,
                        HirExpr::Binary {
                            op: HirBinaryOp::Coalesce,
                            rhs,
                            ..
                        } if matches!(
                            rhs.as_ref(),
                            HirExpr::Null(_) | HirExpr::Undefined(_) | HirExpr::Unit(_)
                        )
                    );
                    let widened_container = matches!(init, HirExpr::ArrayLiteral { .. });
                    let reads_retyped_local = matches!(init, HirExpr::Local { id, .. } if self.retyped_locals.contains_key(id));
                    let coalescing = matches!(
                        init,
                        HirExpr::Binary {
                            op: HirBinaryOp::Coalesce,
                            ..
                        }
                    );
                    if let Some(repaired) = self.repair_expr(init)
                        && (!type_resolved(*ty, self.types)
                            || (inferred_from_init
                                && repaired != *ty
                                && (nullish_literal_fallback
                                    || widened_container
                                    || reads_retyped_local
                                    || (coalescing
                                        && matches!(
                                            self.types.resolve(repaired),
                                            Some(Type::Optional { .. })
                                        )))))
                    {
                        if type_resolved(*ty, self.types) {
                            self.retyped_locals.insert(*id, (*ty, repaired));
                        }
                        *ty = repaired;
                    }
                }
                self.local_tys.insert(*id, *ty);
            }
            other => walk_stmt_mut(self, other),
        }
    }

    fn visit_expr_mut(&mut self, expr: &mut HirExpr) {
        self.repair_expr(expr);
    }
}

fn unify_field_ty(
    pattern: TypeId,
    actual: TypeId,
    mapping: &mut TypeParamMap,
    types: &TypeTable,
) -> bool {
    match types.resolve(pattern) {
        Some(Type::GenericParam { id }) => match mapping.get(id) {
            Some(existing) => *existing == actual,
            None => {
                mapping.insert(*id, actual);
                true
            }
        },
        Some(Type::Optional { inner, .. }) => match types.resolve(actual) {
            Some(Type::Optional {
                inner: actual_inner,
                ..
            }) => unify_field_ty(*inner, *actual_inner, mapping, types),
            _ => false,
        },
        Some(Type::Array { element }) => match types.resolve(actual) {
            Some(Type::Array {
                element: actual_element,
            }) => unify_field_ty(*element, *actual_element, mapping, types),
            _ => false,
        },
        _ => pattern == actual,
    }
}

fn repair_struct_field_types(decls: &mut [HirDecl], shapes: &StructShapes, types: &mut TypeTable) {
    for decl in decls {
        match decl {
            HirDecl::Namespace { members, .. } => {
                repair_struct_field_types(members, shapes, types);
            }
            HirDecl::Class(class) => {
                let Some(Some(shape)) = shapes.get(&class.ty) else {
                    continue;
                };
                if class.fields.len() != shape.len() {
                    continue;
                }
                let mut mapping = TypeParamMap::new();
                let mut unified = true;
                for (field, actual) in class.fields.iter().zip(shape.iter()) {
                    if unify_field_ty(field.ty, *actual, &mut mapping, types) {
                        continue;
                    }
                    if !matches!(
                        types.resolve(*actual),
                        Some(Type::Optional { inner, .. }) if *inner == field.ty
                    ) {
                        unified = false;
                        break;
                    }
                }
                if !unified {
                    continue;
                }
                if !mapping.is_empty() {
                    for field in class.fields.iter_mut() {
                        let mut result = TypeSubstitutionResult::default();
                        let substituted = substitute_type(field.ty, &mapping, types, &mut result);
                        if substituted != field.ty {
                            field.ty = substituted;
                        }
                    }
                }
                for (field, actual) in class.fields.iter_mut().zip(shape.iter()) {
                    if field.ty != *actual
                        && matches!(
                            types.resolve(*actual),
                            Some(Type::Optional { inner, .. }) if *inner == field.ty
                        )
                    {
                        field.ty = *actual;
                    }
                }
            }
            HirDecl::Function(_)
            | HirDecl::TypeAlias { .. }
            | HirDecl::Enum { .. }
            | HirDecl::Global { .. }
            | HirDecl::Interface { .. } => {}
        }
    }
}

fn repair_specialized_call_types(
    decls: &mut [HirDecl],
    types: &mut TypeTable,
    by_fid: &HashMap<FunctionId, TypeParamMap>,
    by_name: &HashMap<Atom, TypeParamMap>,
    by_owner: &HashMap<(TypeId, Atom, bool), TypeParamMap>,
    class_types: &HashMap<Atom, TypeId>,
    converter: &ExprConverter,
    struct_shapes: &mut StructShapes,
) {
    for decl in decls {
        if let HirDecl::Namespace { members, .. } = decl {
            repair_specialized_call_types(
                members,
                types,
                by_fid,
                by_name,
                by_owner,
                class_types,
                converter,
                struct_shapes,
            );
            continue;
        }
        let mut repair = CallTypeRepair {
            types: &mut *types,
            by_fid,
            by_name,
            by_owner,
            class_types,
            converter,
            struct_shapes,
            local_tys: HashMap::new(),
            retyped_locals: HashMap::new(),
        };
        match decl {
            HirDecl::Function(f) => {
                repair.local_tys.clear();
                for stmt in f.body.iter_mut() {
                    repair.visit_stmt_mut(stmt);
                }
            }
            HirDecl::Class(c) => {
                for m in c.methods.iter_mut() {
                    repair.local_tys.clear();
                    repair.retyped_locals.clear();
                    for stmt in m.body.iter_mut() {
                        repair.visit_stmt_mut(stmt);
                    }
                }
            }
            HirDecl::Global {
                init: Some(expr), ..
            } => repair.visit_expr_mut(expr),
            HirDecl::TypeAlias { .. }
            | HirDecl::Enum { .. }
            | HirDecl::Interface { .. }
            | HirDecl::Namespace { .. }
            | HirDecl::Global { init: None, .. } => {}
        }
    }
}

fn receiver_class_ty(owner: &HirExpr, converter: &ExprConverter, types: &mut TypeTable) -> TypeId {
    let ty = owner.ty();
    let ty = if matches!(types.resolve(ty), Some(ts_aot_core::Type::Error) | None) {
        converter.expr_type(owner, types).unwrap_or(ty)
    } else {
        ty
    };
    match types.resolve(ty) {
        Some(ts_aot_core::Type::Optional { inner, .. }) => *inner,
        _ => ty,
    }
}

fn receiver_owner(
    owner: &HirExpr,
    class_types: &HashMap<Atom, TypeId>,
    converter: &ExprConverter,
    types: &mut TypeTable,
) -> (TypeId, bool) {
    match owner {
        HirExpr::Global { name, .. } => match class_types.get(name) {
            Some(&class_ty) => (class_ty, true),
            None => (receiver_class_ty(owner, converter, types), false),
        },
        _ => (receiver_class_ty(owner, converter, types), false),
    }
}

fn method_generic(
    index: &MethodIndex,
    owner: &HirExpr,
    field_name: &Atom,
    converter: &ExprConverter,
    types: &mut TypeTable,
) -> Option<FunctionId> {
    let (ty, is_static) = receiver_owner(owner, &index.class_types, converter, types);
    index.get(&(ty, field_name.clone(), is_static)).copied()
}

fn generic_callee(
    callee: &HirCallee,
    generic_fn_ids: &HashSet<FunctionId>,
    name_to_generic: &HashMap<Atom, FunctionId>,
    method_to_generic: &MethodIndex,
    converter: &ExprConverter,
    types: &mut TypeTable,
) -> Option<FunctionId> {
    match callee {
        HirCallee::Function(fid) if generic_fn_ids.contains(fid) => Some(*fid),
        HirCallee::Function(_) => None,
        HirCallee::Indirect(inner) => match inner.as_ref() {
            HirExpr::Global { name, .. } => name_to_generic.get(name).copied(),
            HirExpr::Field {
                owner, field_name, ..
            } => method_generic(method_to_generic, owner, field_name, converter, types),
            _ => None,
        },
        HirCallee::Closure(_) | HirCallee::Runtime { .. } => None,
    }
}

fn infer_for_callee(
    callee: &HirCallee,
    method_to_generic: &MethodIndex,
    generic_fn: &HirFunction,
    args: &[HirExpr],
    explicit_type_args: &[TypeId],
    converter: &ExprConverter,
    types: &mut TypeTable,
) -> Vec<TypeId> {
    if !explicit_type_args.is_empty() && explicit_type_args.len() == generic_fn.type_params.len() {
        return explicit_type_args.to_vec();
    }
    if let HirCallee::Indirect(inner) = callee
        && let HirExpr::Field {
            owner, field_name, ..
        } = inner.as_ref()
        && method_generic(method_to_generic, owner, field_name, converter, types).is_some()
        && generic_fn
            .params
            .first()
            .is_some_and(|param| param.name.as_str() == "this")
    {
        let mut all = Vec::with_capacity(args.len() + 1);
        all.push(owner.as_ref().clone());
        all.extend(args.iter().cloned());
        return infer_type_args(generic_fn, &all, explicit_type_args, types, converter);
    }
    infer_type_args(generic_fn, args, explicit_type_args, types, converter)
}

fn expr_writes_local(expr: &HirExpr, id: ts_aot_core::LocalId) -> bool {
    struct Scan {
        id: ts_aot_core::LocalId,
        found: bool,
    }
    impl Visitor for Scan {
        fn visit_expr(&mut self, expr: &HirExpr) {
            match expr {
                HirExpr::Assignment { target, value, .. } => {
                    if matches!(target.as_ref(), HirExpr::Local { id, .. } if *id == self.id) {
                        self.found = true;
                    }
                    self.visit_expr(target);
                    self.visit_expr(value);
                }
                HirExpr::CompoundUpdate { target, rhs, .. } => {
                    if matches!(target.as_ref(), HirExpr::Local { id, .. } if *id == self.id) {
                        self.found = true;
                    }
                    self.visit_expr(target);
                    self.visit_expr(rhs);
                }
                HirExpr::Closure { .. } => {}
                other => walk_expr(self, other),
            }
        }
    }
    let mut scan = Scan { id, found: false };
    scan.visit_expr(expr);
    scan.found
}

fn narrow_optional_chain_receiver_args(
    callee: &HirCallee,
    generic_fn: &HirFunction,
    args: &mut [HirExpr],
    types: &TypeTable,
) -> Option<ts_aot_core::Span> {
    let HirCallee::Indirect(inner) = callee else {
        return None;
    };
    let HirExpr::Field { owner, .. } = inner.as_ref() else {
        return None;
    };
    let HirExpr::OptionalChain { base, ty, .. } = owner.as_ref() else {
        return None;
    };
    let HirExpr::Local { id: base_id, .. } = base.as_ref() else {
        return None;
    };
    let Some(Type::Optional {
        inner: narrowed, ..
    }) = types.resolve(*ty).cloned()
    else {
        return None;
    };
    let receiver_param_offset = usize::from(
        generic_fn
            .params
            .first()
            .is_some_and(|param| param.name.as_str() == "this"),
    );
    let writes: Vec<bool> = args
        .iter()
        .map(|arg| expr_writes_local(arg, *base_id))
        .collect();
    let mut reassigned_before = Vec::with_capacity(writes.len());
    let mut reassigned = false;
    for write in &writes {
        reassigned_before.push(reassigned);
        reassigned |= *write;
    }
    let mut reassigned_after = vec![false; writes.len()];
    let mut reassigned = false;
    for (index, write) in writes.iter().enumerate().rev() {
        reassigned_after[index] = reassigned;
        reassigned |= *write;
    }
    let mut blocked = None;
    for (index, arg) in args.iter_mut().enumerate() {
        if arg.ty() != *ty {
            continue;
        }
        let same_receiver = matches!(&*arg, HirExpr::Local { id, .. } if id == base_id);
        if !same_receiver {
            continue;
        }
        let Some(param) = generic_fn.params.get(index + receiver_param_offset) else {
            continue;
        };
        if !matches!(types.resolve(param.ty), Some(Type::GenericParam { .. })) {
            continue;
        }
        if reassigned_before[index] || reassigned_after[index] {
            blocked = Some(arg.span());
            continue;
        }
        let span = arg.span();
        let value = std::mem::replace(arg, HirExpr::Unit(span));
        *arg = HirExpr::TypeAssertion {
            expr: Box::new(value),
            target: narrowed,
            span,
        };
    }
    blocked
}

fn push_specialization(
    callee: &HirCallee,
    args: &mut [HirExpr],
    explicit_type_args: &[TypeId],
    generic_fn_ids: &HashSet<FunctionId>,
    name_to_generic: &HashMap<Atom, FunctionId>,
    method_to_generic: &MethodIndex,
    fn_index: &HashMap<FunctionId, HirFunction>,
    converter: &ExprConverter,
    types: &mut TypeTable,
    worklist: &mut VecDeque<SpecializationKey>,
) {
    let Some(fid) = generic_callee(
        callee,
        generic_fn_ids,
        name_to_generic,
        method_to_generic,
        converter,
        types,
    ) else {
        return;
    };
    let Some(generic_fn) = fn_index.get(&fid) else {
        return;
    };
    let _ = narrow_optional_chain_receiver_args(callee, generic_fn, args, types);
    let type_args = infer_for_callee(
        callee,
        method_to_generic,
        generic_fn,
        args,
        explicit_type_args,
        converter,
        types,
    );
    if type_args_resolved(&type_args, types) {
        worklist.push_back((fid, type_args));
    }
}

fn classify_decls(
    decls: &[HirDecl],
    next_fid: &mut u32,
    generic_fn_ids: &mut HashSet<FunctionId>,
    fn_index: &mut HashMap<FunctionId, HirFunction>,
    top_level_names: &mut HashMap<Atom, FunctionId>,
    method_to_generic: &mut MethodIndex,
    method_owner: &mut HashMap<FunctionId, (TypeId, bool)>,
    stats: &mut MonomorphizeStats,
) {
    for decl in decls {
        match decl {
            HirDecl::Function(f) => {
                let fid = FunctionId::from_raw(*next_fid);
                *next_fid += 1;
                if !f.type_params.is_empty() {
                    generic_fn_ids.insert(fid);
                    top_level_names.insert(f.name.clone(), fid);
                    stats.generic_functions += 1;
                }
                fn_index.insert(fid, f.clone());
            }
            HirDecl::Class(c) => {
                method_to_generic.class_types.insert(c.name.clone(), c.ty);
                for m in &c.methods {
                    let fid = FunctionId::from_raw(*next_fid);
                    *next_fid += 1;
                    let is_static = m.params.first().is_none_or(|p| p.name.as_str() != "this");
                    let shadowed_by_field = !is_static && c.fields.iter().any(|f| f.name == m.name);
                    if !m.type_params.is_empty() && !shadowed_by_field {
                        generic_fn_ids.insert(fid);
                        method_to_generic.insert((c.ty, m.name.clone(), is_static), fid);
                        method_owner.insert(fid, (c.ty, is_static));
                        stats.generic_functions += 1;
                    }
                    fn_index.insert(fid, m.clone());
                }
            }
            HirDecl::Namespace { .. } => {}
            _ => {}
        }
    }
}
fn visit_callees(decls: &mut [HirDecl], on_callee: OnCallee<'_>) {
    for decl in decls {
        visit_decl_callees(decl, on_callee);
    }
}

fn visit_decl_callees(decl: &mut HirDecl, on_callee: OnCallee<'_>) {
    match decl {
        HirDecl::Function(f) => visit_body_callees(&mut f.body, on_callee),
        HirDecl::Class(c) => {
            for m in &mut c.methods {
                visit_body_callees(&mut m.body, on_callee);
            }
        }
        HirDecl::Global {
            init: Some(expr), ..
        } => visit_expr_callees(expr, on_callee),
        HirDecl::Namespace { members, .. } => visit_callees(members, on_callee),
        HirDecl::TypeAlias { .. }
        | HirDecl::Enum { .. }
        | HirDecl::Interface { .. }
        | HirDecl::Global { init: None, .. } => {}
    }
}

fn visit_body_callees(body: &mut [HirStmt], on_callee: OnCallee<'_>) {
    for stmt in body {
        visit_stmt_callees(stmt, on_callee);
    }
}

fn visit_stmt_callees(stmt: &mut HirStmt, on_callee: OnCallee<'_>) {
    match stmt {
        HirStmt::Block(stmts) => visit_body_callees(stmts, on_callee),
        HirStmt::Let {
            init: Some(expr), ..
        } => visit_expr_callees(expr, on_callee),
        HirStmt::Let { init: None, .. } => {}
        HirStmt::Expr { expr } => visit_expr_callees(expr, on_callee),
        HirStmt::If {
            cond,
            then,
            otherwise,
        } => {
            visit_expr_callees(cond, on_callee);
            visit_stmt_callees(then, on_callee);
            if let Some(e) = otherwise.as_mut() {
                visit_stmt_callees(e, on_callee);
            }
        }
        HirStmt::While { cond, body } | HirStmt::DoWhile { body, cond } => {
            visit_expr_callees(cond, on_callee);
            visit_stmt_callees(body, on_callee);
        }
        HirStmt::ForOf { iter, body, .. }
        | HirStmt::ForAwaitOf { iter, body, .. }
        | HirStmt::ForIn { iter, body, .. } => {
            visit_expr_callees(iter, on_callee);
            visit_stmt_callees(body, on_callee);
        }
        HirStmt::Switch { disc, cases } => {
            visit_expr_callees(disc, on_callee);
            for case in cases {
                if let Some(test) = case.test.as_mut() {
                    visit_expr_callees(test, on_callee);
                }
                visit_body_callees(&mut case.body, on_callee);
            }
        }
        HirStmt::Return { value: Some(expr) } => visit_expr_callees(expr, on_callee),
        HirStmt::Return { value: None } => {}
        HirStmt::Throw { expr } => visit_expr_callees(expr, on_callee),
        HirStmt::Try {
            body,
            catch,
            finally,
        } => {
            visit_stmt_callees(body, on_callee);
            if let Some(c) = catch.as_mut() {
                visit_stmt_callees(&mut c.body, on_callee);
            }
            if let Some(f) = finally.as_mut() {
                visit_stmt_callees(f, on_callee);
            }
        }
        HirStmt::Break { .. } | HirStmt::Continue { .. } => {}
        HirStmt::Decl(decl) => visit_decl_callees(decl, on_callee),
    }
}

fn visit_expr_callees(expr: &mut HirExpr, on_callee: OnCallee<'_>) {
    match expr {
        HirExpr::DefaultConstruct { .. } => {}
        HirExpr::Call {
            callee,
            args,
            type_args,
            ..
        } => {
            if let HirCallee::Indirect(inner) = callee {
                visit_expr_callees(inner, on_callee);
            }
            on_callee(callee, args, type_args);
            for arg in args {
                visit_expr_callees(arg, on_callee);
            }
        }
        HirExpr::Binary { lhs, rhs, .. } => {
            visit_expr_callees(lhs, on_callee);
            visit_expr_callees(rhs, on_callee);
        }
        HirExpr::Unary { expr, .. } => visit_expr_callees(expr, on_callee),
        HirExpr::Field { owner, .. } => visit_expr_callees(owner, on_callee),
        HirExpr::Index { owner, index, .. } => {
            visit_expr_callees(owner, on_callee);
            visit_expr_callees(index, on_callee);
        }
        HirExpr::StructLiteral { fields, .. } => {
            for (_, v) in fields {
                visit_expr_callees(v, on_callee);
            }
        }
        HirExpr::ArrayLiteral { elements, .. } => {
            for el in elements {
                visit_expr_callees(el, on_callee);
            }
        }
        HirExpr::ObjectLiteral { fields, .. } => {
            for f in fields {
                match f {
                    ObjectLiteralField::Property { value, .. } => {
                        visit_expr_callees(value, on_callee);
                    }
                    ObjectLiteralField::Spread(value) => {
                        visit_expr_callees(value, on_callee);
                    }
                }
            }
        }
        HirExpr::Ternary {
            cond,
            then_branch,
            else_branch,
            ..
        } => {
            visit_expr_callees(cond, on_callee);
            visit_expr_callees(then_branch, on_callee);
            visit_expr_callees(else_branch, on_callee);
        }
        HirExpr::Sequence { exprs, .. } => {
            for e in exprs {
                visit_expr_callees(e, on_callee);
            }
        }
        HirExpr::Closure { body, captures, .. } => {
            for cap in captures {
                visit_expr_callees(cap, on_callee);
            }
            visit_body_callees(body, on_callee);
        }
        HirExpr::Await { expr, .. }
        | HirExpr::Yield {
            expr: Some(expr), ..
        } => visit_expr_callees(expr, on_callee),
        HirExpr::Yield { expr: None, .. } => {}
        HirExpr::Template {
            tag, expressions, ..
        } => {
            if let Some(t) = tag.as_mut() {
                visit_expr_callees(t, on_callee);
            }
            for p in expressions {
                visit_expr_callees(p, on_callee);
            }
        }
        HirExpr::New { callee, args, .. } => {
            visit_expr_callees(callee, on_callee);
            for arg in args {
                visit_expr_callees(arg, on_callee);
            }
        }
        HirExpr::OptionalChain { base, .. } => visit_expr_callees(base, on_callee),
        HirExpr::TypeAssertion { expr, .. } => visit_expr_callees(expr, on_callee),
        HirExpr::Assignment { target, value, .. } => {
            visit_expr_callees(target, on_callee);
            visit_expr_callees(value, on_callee);
        }
        HirExpr::CompoundUpdate { target, rhs, .. } => {
            visit_expr_callees(target, on_callee);
            visit_expr_callees(rhs, on_callee);
        }
        HirExpr::Import { source, .. } => visit_expr_callees(source, on_callee),
        HirExpr::Unit(_)
        | HirExpr::Bool(_, _)
        | HirExpr::Int(_, _)
        | HirExpr::Float(_, _)
        | HirExpr::String(_, _)
        | HirExpr::Null(_)
        | HirExpr::Undefined(_)
        | HirExpr::Local { .. }
        | HirExpr::Global { .. }
        | HirExpr::RegExp { .. }
        | HirExpr::BigInt { .. } => {}
    }
}
