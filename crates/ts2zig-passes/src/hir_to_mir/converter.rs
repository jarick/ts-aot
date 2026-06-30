use std::collections::HashMap;

use ts2zig_core::{FunctionId, LocalId, Span, StructId, SymbolId, TypeId};
use ts2zig_ir_hir::HirCallee;
use ts2zig_ir_mir::{MirExpr, MirLocalDecl};

use crate::PassContext;
use crate::hir_to_mir::PLACEHOLDER_FUNCTION;

pub struct ExprConverter {
    pub(super) local_map: HashMap<LocalId, LocalId>,
    pub(super) local_names: HashMap<LocalId, SymbolId>,
    pub(super) function_remap: HashMap<FunctionId, FunctionId>,
    pub(super) next_local: u32,
    pub(super) next_state: i32,
    pub(super) next_await: u32,
    pub(super) temp_locals: Vec<MirLocalDecl>,
    pub(super) struct_ids: HashMap<TypeId, StructId>,
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
            next_local,
            next_state: 0,
            next_await: 0,
            temp_locals: Vec::new(),
            struct_ids: HashMap::new(),
        }
    }

    pub(super) fn take_temp_locals(&mut self) -> Vec<MirLocalDecl> {
        std::mem::take(&mut self.temp_locals)
    }

    pub(super) fn push_temp_local(&mut self, id: LocalId, ty: TypeId) {
        self.temp_locals.push(MirLocalDecl {
            id,
            name: SymbolId::from_raw(0),
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

    pub fn register_local_name(&mut self, id: LocalId, name: SymbolId) {
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

    pub(super) fn fresh_await_dest(&mut self) -> LocalId {
        self.fresh_local()
    }

    pub(super) fn push_await(&mut self) -> (LocalId, i32) {
        let next_state = self.next_state + 1;
        let dest = self.fresh_await_dest();
        self.next_state = next_state;
        (dest, next_state)
    }

    pub(super) fn resolve_callee(
        &mut self,
        callee: &HirCallee,
        ctx: &mut PassContext,
    ) -> FunctionId {
        match callee {
            HirCallee::Function(fid) => self.function_remap.get(fid).copied().unwrap_or(*fid),
            HirCallee::Indirect(_) => {
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
                    "closure callee is not yet supported in HIR→MIR",
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
}

impl Default for ExprConverter {
    fn default() -> Self {
        Self::new()
    }
}
