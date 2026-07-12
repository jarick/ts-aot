use std::collections::HashMap;
use std::sync::Arc;

use ts_aot_core::{Atom, FieldId, FunctionId, LocalId, Span, StructId, TypeId};
use ts_aot_ir_hir::{HirCallee, HirExpr};
use ts_aot_ir_mir::{MirExpr, MirLocalDecl};

use crate::PassContext;
use crate::hir_to_mir::PLACEHOLDER_FUNCTION;

pub struct ExprConverter {
    pub(super) local_map: HashMap<LocalId, LocalId>,
    pub(super) local_names: HashMap<LocalId, Atom>,
    pub(super) function_remap: HashMap<FunctionId, FunctionId>,
    pub(super) name_to_function: Arc<HashMap<Atom, FunctionId>>,
    pub(super) next_local: u32,
    pub(super) temp_locals: Vec<MirLocalDecl>,
    pub(super) struct_ids: HashMap<TypeId, StructId>,
    pub(super) field_id_lookup: HashMap<(StructId, Atom), FieldId>,
}

impl ExprConverter {
    #[must_use]
    pub fn new() -> Self {
        Self::with_function_remap(HashMap::new())
    }

    #[must_use]
    pub fn with_function_remap(remap: HashMap<FunctionId, FunctionId>) -> Self {
        Self::with_function_remap_and_offset(remap, 0)
    }

    #[must_use]
    pub fn with_function_remap_and_offset(
        remap: HashMap<FunctionId, FunctionId>,
        next_local: u32,
    ) -> Self {
        Self {
            local_map: HashMap::new(),
            local_names: HashMap::new(),
            function_remap: remap,
            name_to_function: Arc::new(HashMap::new()),
            next_local,
            temp_locals: Vec::new(),
            struct_ids: HashMap::new(),
            field_id_lookup: HashMap::new(),
        }
    }

    pub fn set_field_id_lookup(&mut self, lookup: HashMap<(StructId, Atom), FieldId>) {
        self.field_id_lookup = lookup;
    }

    pub(super) fn take_temp_locals(&mut self) -> Vec<MirLocalDecl> {
        std::mem::take(&mut self.temp_locals)
    }

    pub(super) fn push_temp_local(&mut self, id: LocalId, ty: TypeId) {
        self.temp_locals.push(MirLocalDecl {
            id,
            name: Atom::from(""),
            ty,
            mutable: true,
        });
    }

    #[must_use]
    pub fn peek_next_local(&self) -> u32 {
        self.next_local
    }

    pub(super) fn fresh_local(&mut self) -> LocalId {
        let id = LocalId::from_raw(self.next_local);
        self.next_local += 1;
        id
    }

    pub(super) fn map_local(&mut self, old: LocalId) -> MirExpr {
        if let Some(&new) = self.local_map.get(&old) {
            MirExpr::Local(new)
        } else {
            let new_id = self.fresh_local();
            self.local_map.insert(old, new_id);
            MirExpr::Local(new_id)
        }
    }

    #[must_use]
    pub fn map_local_id(&mut self, old: LocalId) -> LocalId {
        if let Some(&new) = self.local_map.get(&old) {
            new
        } else {
            let new_id = self.fresh_local();
            self.local_map.insert(old, new_id);
            new_id
        }
    }

    pub fn register_local_name(&mut self, id: LocalId, name: Atom) {
        self.local_names.insert(id, name);
    }

    pub fn seed_params(&mut self, count: u32) {
        for i in 0..count {
            self.local_map
                .insert(LocalId::from_raw(i), LocalId::from_raw(i));
        }
        if count > self.next_local {
            self.next_local = count;
        }
    }

    pub(super) fn resolve_callee(
        &mut self,
        callee: &HirCallee,
        ctx: &mut PassContext,
    ) -> FunctionId {
        match callee {
            HirCallee::Function(fid) => self.function_remap.get(fid).copied().unwrap_or(*fid),
            HirCallee::Indirect(inner) => {
                if let HirExpr::Global { name, .. } = inner.as_ref()
                    && let Some(&fid) = self.name_to_function.get(name)
                {
                    return fid;
                }
                ctx.error(
                    "P0005",
                    "indirect (computed) callee is not yet supported in HIR→MIR",
                    Span::new(0, 0),
                );
                PLACEHOLDER_FUNCTION
            }
            HirCallee::Closure(_) => {
                ctx.error(
                    "P0005",
                    "closure callee should have been rewritten to Indirect(Global) by lower_closures before HIR→MIR",
                    Span::new(0, 0),
                );
                PLACEHOLDER_FUNCTION
            }
            HirCallee::Runtime { .. } => {
                ctx.error(
                    "P0005",
                    "runtime callee is not yet supported in HIR→MIR",
                    Span::new(0, 0),
                );
                PLACEHOLDER_FUNCTION
            }
        }
    }

    pub(super) fn lookup_or_alloc_struct_id(
        &mut self,
        ty: TypeId,
        shared_ids: &mut HashMap<TypeId, StructId>,
        shared_next: &mut u32,
    ) -> StructId {
        if let Some(&id) = self.struct_ids.get(&ty) {
            return id;
        }
        if let Some(&id) = shared_ids.get(&ty) {
            self.struct_ids.insert(ty, id);
            return id;
        }
        let id = StructId::from_raw(*shared_next);
        *shared_next += 1;
        shared_ids.insert(ty, id);
        self.struct_ids.insert(ty, id);
        id
    }

    pub(super) fn resolve_field_id(
        &self,
        owner: &HirExpr,
        field_name: &Atom,
        placeholder: FieldId,
        shared_ids: &HashMap<TypeId, StructId>,
        ctx: &mut PassContext,
    ) -> FieldId {
        let owner_ty = match owner {
            HirExpr::Local { ty, .. }
            | HirExpr::Global { ty, .. }
            | HirExpr::Field { ty, .. }
            | HirExpr::Index { ty, .. }
            | HirExpr::Call { ty, .. }
            | HirExpr::Binary { ty, .. }
            | HirExpr::Unary { ty, .. }
            | HirExpr::StructLiteral { ty, .. }
            | HirExpr::ArrayLiteral { ty, .. }
            | HirExpr::Closure { ty, .. }
            | HirExpr::Await { ty, .. }
            | HirExpr::Yield { ty, .. }
            | HirExpr::Template { ty, .. }
            | HirExpr::New { ty, .. }
            | HirExpr::OptionalChain { ty, .. }
            | HirExpr::Assignment { ty, .. }
            | HirExpr::CompoundUpdate { ty, .. } => Some(*ty),
            HirExpr::TypeAssertion { target, .. } => Some(*target),
            _ => None,
        };
        let Some(ty) = owner_ty else {
            ctx.error(
                "P0011",
                format!(
                    "owner expression has no static type for field access `{}`",
                    field_name.as_str()
                ),
                Span::new(0, 0),
            );
            return placeholder;
        };
        let Some(&sid) = self.struct_ids.get(&ty).or_else(|| shared_ids.get(&ty)) else {
            ctx.error(
                "P0012",
                format!(
                    "owner type {:?} has no registered struct id; the class must be lowered before field access on it",
                    ty
                ),
                Span::new(0, 0),
            );
            return placeholder;
        };
        match self.field_id_lookup.get(&(sid, field_name.clone())) {
            Some(id) => *id,
            None => {
                ctx.error(
                    "P0010",
                    format!(
                        "field `{}` is not declared on the static type of the owner",
                        field_name.as_str()
                    ),
                    Span::new(0, 0),
                );
                placeholder
            }
        }
    }
}

impl Default for ExprConverter {
    fn default() -> Self {
        Self::new()
    }
}
