use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use ts_aot_core::{
    Atom, FieldId, FunctionId, LocalId, ModuleId, Span, StructId, Type, TypeId, TypeTable,
};
use ts_aot_ir_hir::{HirCallee, HirClass, HirDecl, HirExpr, HirProgram};

use ts_aot_ir_mir::{MirExpr, MirLocalDecl};

use crate::PassContext;
use crate::hir_to_mir::{PLACEHOLDER_FUNCTION, qualified_name};

pub struct ExprConverter {
    pub(super) local_map: HashMap<LocalId, LocalId>,
    pub(super) local_names: HashMap<LocalId, Atom>,
    pub(super) function_remap: HashMap<FunctionId, FunctionId>,
    pub(super) name_to_function: Arc<HashMap<Atom, FunctionId>>,
    pub(super) program: Arc<HirProgram>,
    pub(super) namespace_path: Vec<String>,
    pub(super) next_local: u32,
    pub(super) temp_locals: Vec<MirLocalDecl>,
    pub(super) struct_ids: HashMap<TypeId, StructId>,
    pub(super) field_id_lookup: HashMap<(StructId, Atom), FieldId>,
    pub(super) method_map: HashMap<(StructId, Atom, bool), FunctionId>,
    pub(super) method_ret: HashMap<(StructId, Atom, bool), TypeId>,
    pub(super) method_param_tys: HashMap<(StructId, Atom, bool), Vec<TypeId>>,
    pub(super) class_struct_by_name: HashMap<Atom, StructId>,
    pub(super) current_call_type_args: Vec<TypeId>,
    pub(super) class_index: OnceLock<HashMap<TypeId, ClassPath>>,
    pub(super) global_types: OnceLock<HashMap<Atom, TypeId>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ClassPath(pub(crate) Vec<usize>);

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
            program: Arc::new(HirProgram::new(ModuleId::from_raw(0))),
            namespace_path: Vec::new(),
            next_local,
            temp_locals: Vec::new(),
            struct_ids: HashMap::new(),
            field_id_lookup: HashMap::new(),
            method_map: HashMap::new(),
            method_ret: HashMap::new(),
            method_param_tys: HashMap::new(),
            class_struct_by_name: HashMap::new(),
            current_call_type_args: Vec::new(),
            class_index: OnceLock::new(),
            global_types: OnceLock::new(),
        }
    }

    pub fn set_field_id_lookup(&mut self, lookup: HashMap<(StructId, Atom), FieldId>) {
        self.field_id_lookup = lookup;
    }

    pub(crate) fn set_program_tables(
        &mut self,
        field_id_lookup: HashMap<(StructId, Atom), FieldId>,
        method_map: HashMap<(StructId, Atom, bool), FunctionId>,
        method_ret: HashMap<(StructId, Atom, bool), TypeId>,
        method_param_tys: HashMap<(StructId, Atom, bool), Vec<TypeId>>,
        class_struct_by_name: HashMap<Atom, StructId>,
    ) {
        self.field_id_lookup = field_id_lookup;
        self.method_map = method_map;
        self.method_ret = method_ret;
        self.method_param_tys = method_param_tys;
        self.class_struct_by_name = class_struct_by_name;
    }

    pub fn set_namespace_path(&mut self, path: &[String]) {
        self.namespace_path = path.to_vec();
    }

    pub fn set_program(&mut self, program: Arc<HirProgram>) {
        self.program = program;
        self.class_index = OnceLock::new();
        self.global_types = OnceLock::new();
    }

    pub(super) fn global_decl_type(&self, name: &Atom) -> Option<TypeId> {
        let map = self.global_types.get_or_init(|| {
            let mut map = HashMap::new();
            fn scan(
                decls: &[HirDecl],
                namespace_path: &mut Vec<String>,
                map: &mut HashMap<Atom, TypeId>,
            ) {
                for decl in decls {
                    match decl {
                        HirDecl::Global { name, ty, .. } => {
                            map.insert(qualified_name(namespace_path, name.as_str()), *ty);
                        }
                        HirDecl::Namespace { name, members } => {
                            namespace_path.push(name.as_str().to_owned());
                            scan(members, namespace_path, map);
                            namespace_path.pop();
                        }
                        _ => {}
                    }
                }
            }
            let mut namespace_path = Vec::new();
            scan(&self.program.declarations, &mut namespace_path, &mut map);
            map
        });
        for depth in (0..=self.namespace_path.len()).rev() {
            let probe = qualified_name(&self.namespace_path[..depth], name.as_str());
            if let Some(&ty) = map.get(&probe) {
                return Some(ty);
            }
        }
        None
    }

    pub(super) fn take_temp_locals(&mut self) -> Vec<MirLocalDecl> {
        std::mem::take(&mut self.temp_locals)
    }

    pub(super) fn push_temp_local(&mut self, id: LocalId, ty: TypeId) {
        self.push_temp_local_with_mut(id, ty, true);
    }

    pub(super) fn push_temp_local_with_mut(&mut self, id: LocalId, ty: TypeId, mutable: bool) {
        self.temp_locals.push(MirLocalDecl {
            id,
            name: Atom::from(""),
            ty,
            mutable,
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

    pub fn map_local_id_inplace(&mut self, old: LocalId, new: LocalId) {
        self.local_map.insert(old, new);
    }

    pub fn register_local_name(&mut self, id: LocalId, name: Atom) {
        self.local_names.insert(id, name);
    }

    pub fn local_name(&self, id: LocalId) -> Option<Atom> {
        self.local_names.get(&id).cloned()
    }

    pub(super) fn local_name_taken(&self, name: &str) -> bool {
        self.local_names
            .values()
            .any(|existing| existing.as_str() == name)
    }

    pub(super) fn unique_synth_local_name(&mut self, id: LocalId, base: &str) -> Atom {
        let mut name = format!("{}_{}", base, id.raw());
        while self.local_name_taken(&name) {
            name.push('_');
        }
        let name = Atom::from(name);
        self.register_local_name(id, name.clone());
        name
    }

    pub fn seed_params(&mut self, param_ids: &[LocalId]) {
        for &id in param_ids {
            self.local_map.insert(id, id);
        }
        let next = param_ids
            .iter()
            .map(|id| id.raw())
            .max()
            .map_or(0, |m| m.saturating_add(1));
        if next > self.next_local {
            self.next_local = next;
        }
    }

    pub(crate) fn reset_for_function(
        &mut self,
        function_remap: HashMap<FunctionId, FunctionId>,
        param_ids: &[LocalId],
        namespace_path: &[String],
    ) {
        self.local_map.clear();
        self.local_names.clear();
        self.function_remap = function_remap;
        self.namespace_path.clear();
        self.namespace_path.extend_from_slice(namespace_path);
        self.next_local = param_ids
            .iter()
            .map(|id| id.raw())
            .max()
            .map_or(0, |m| m.saturating_add(1));
        self.temp_locals.clear();
        self.struct_ids.clear();
        self.current_call_type_args.clear();
        self.seed_params(param_ids);
    }

    pub(super) fn resolve_callee(
        &mut self,
        callee: &HirCallee,
        ctx: &mut PassContext,
    ) -> FunctionId {
        match callee {
            HirCallee::Function(fid) => self.function_remap.get(fid).copied().unwrap_or(*fid),
            HirCallee::Indirect(inner) => {
                if let HirExpr::Global { name, .. } = inner.as_ref() {
                    for depth in (0..=self.namespace_path.len()).rev() {
                        let probe = qualified_name(&self.namespace_path[..depth], name.as_str());
                        if let Some(&fid) = self.name_to_function.get(&probe) {
                            return fid;
                        }
                    }
                }
                ctx.warning(
                    "P0005",
                    "indirect (computed) callee resolves to MirExpr::IndirectCall at HIR→MIR; \
                     backend emits callee(args) directly. \
                     OptionalChain callees (e.g. obj?.()) go through optional_call_map_arm when ty is Optional.",
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

    pub(super) fn lookup_class_struct_by_name(&self, name: &Atom) -> Option<StructId> {
        for depth in (0..=self.namespace_path.len()).rev() {
            let probe = qualified_name(&self.namespace_path[..depth], name.as_str());
            if let Some(&sid) = self.class_struct_by_name.get(&probe) {
                return Some(sid);
            }
        }
        None
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

    fn global_owner_ty(&self, name: &Atom, ty: TypeId, types: &TypeTable) -> TypeId {
        if matches!(types.resolve(ty), Some(Type::Error)) {
            self.global_decl_type(name).unwrap_or(ty)
        } else {
            ty
        }
    }

    pub(super) fn owner_struct_id(
        &self,
        owner: &HirExpr,
        shared_ids: &HashMap<TypeId, StructId>,
        types: &TypeTable,
    ) -> Option<StructId> {
        let owner_ty = match owner {
            HirExpr::Global { name, ty, .. } => self.global_owner_ty(name, *ty, types),
            _ => owner.ty(),
        };
        let owner_ty = match types.resolve(owner_ty) {
            Some(Type::Optional { inner }) => *inner,
            _ => owner_ty,
        };
        self.struct_ids
            .get(&owner_ty)
            .or_else(|| shared_ids.get(&owner_ty))
            .copied()
    }

    pub(super) fn resolve_field_id(
        &self,
        owner: &HirExpr,
        field_name: &Atom,
        placeholder: FieldId,
        shared_ids: &HashMap<TypeId, StructId>,
        types: &TypeTable,
        ctx: &mut PassContext,
    ) -> FieldId {
        let owner_ty = match owner {
            HirExpr::OptionalChain { .. } | HirExpr::Field { .. } => {
                let declarations = &self.program.declarations;
                let class_index = self
                    .class_index
                    .get_or_init(|| build_class_index(declarations));
                let resolve_global = |name: &Atom| self.global_decl_type(name);
                compute_chain_owner_ty(owner, class_index, declarations, types, &resolve_global)
            }
            HirExpr::Global { name, ty, .. } => Some(self.global_owner_ty(name, *ty, types)),
            HirExpr::Local { ty, .. }
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
            | HirExpr::Assignment { ty, .. }
            | HirExpr::CompoundUpdate { ty, .. } => Some(*ty),
            HirExpr::TypeAssertion { target, .. } => Some(*target),
            _ => None,
        };
        let Some(ty) = owner_ty.map(|t| match types.resolve(t) {
            Some(Type::Optional { inner }) => *inner,
            _ => t,
        }) else {
            if owner.has_static_type() && matches!(types.resolve(owner.ty()), Some(Type::Error)) {
                ctx.warning(
                    "P0013",
                    format!(
                        "field access `{}` on an already-erroneous owner was skipped; \
                         the owner type failed to resolve earlier",
                        field_name.as_str()
                    ),
                    Span::new(0, 0),
                );
                return placeholder;
            }
            ctx.warning(
                "P0011",
                format!(
                    "owner expression has no static type for field access `{}`; the access \
                     is lowered with a placeholder field",
                    field_name.as_str()
                ),
                Span::new(0, 0),
            );
            return placeholder;
        };
        if matches!(types.resolve(ty), Some(Type::AsyncGenYield { .. })) {
            return match field_name.as_str() {
                "value" => FieldId::from_raw(0),
                "done" => FieldId::from_raw(1),
                _ => {
                    ctx.warning(
                        "P0012",
                        format!(
                            "`AsyncGenYield` has no field `{}`; expected `value` or `done`",
                            field_name.as_str()
                        ),
                        Span::new(0, 0),
                    );
                    placeholder
                }
            };
        }
        if matches!(types.resolve(ty), Some(Type::Error)) {
            ctx.warning(
                "P0013",
                format!(
                    "field access `{}` on an already-erroneous owner was skipped; \
                     the owner type failed to resolve earlier",
                    field_name.as_str()
                ),
                Span::new(0, 0),
            );
            return placeholder;
        }
        let Some(&sid) = self.struct_ids.get(&ty).or_else(|| shared_ids.get(&ty)) else {
            ctx.warning(
                "P0012",
                format!(
                    "owner type {:?} has no struct representation for field access; the \
                     access is lowered with a placeholder field",
                    ty
                ),
                Span::new(0, 0),
            );
            return placeholder;
        };
        match self.field_id_lookup.get(&(sid, field_name.clone())) {
            Some(id) => *id,
            None if is_object_prototype_method(field_name) => {
                ctx.error(
                    "E0407",
                    format!(
                        "Object.prototype method `{}` cannot be used as a bare field value; \
                         invoke it on a struct receiver instead",
                        field_name.as_str()
                    ),
                    Span::new(0, 0),
                );
                placeholder
            }
            None => {
                ctx.warning(
                    "P0010",
                    format!(
                        "field `{}` is not declared on the static type of the owner; the \
                         access is lowered with a placeholder field",
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

const OBJECT_PROTOTYPE_METHODS: &[&str] = &["hasOwnProperty"];

fn is_object_prototype_method(field_name: &Atom) -> bool {
    OBJECT_PROTOTYPE_METHODS.contains(&field_name.as_str())
}

fn compute_chain_owner_ty(
    expr: &HirExpr,
    class_index: &HashMap<TypeId, ClassPath>,
    declarations: &[HirDecl],
    types: &TypeTable,
    resolve_global: &dyn Fn(&Atom) -> Option<TypeId>,
) -> Option<TypeId> {
    let class_at = |ty: TypeId| -> Option<&HirClass> {
        let path = class_index.get(&ty)?;
        class_at_path(declarations, path.clone())
    };
    match expr {
        HirExpr::OptionalChain { base, .. } => {
            let inner =
                compute_chain_owner_ty(base, class_index, declarations, types, resolve_global)?;
            Some(match types.resolve(inner) {
                Some(Type::Optional { inner }) => *inner,
                _ => inner,
            })
        }
        HirExpr::Field {
            owner, field_name, ..
        } => {
            let owner_class =
                compute_chain_owner_ty(owner, class_index, declarations, types, resolve_global)?;
            let owner_class = match types.resolve(owner_class) {
                Some(Type::Optional { inner }) => *inner,
                _ => owner_class,
            };
            let class = class_at(owner_class)?;
            let field_ty = class
                .fields
                .iter()
                .find(|f| f.name == *field_name)
                .map(|f| f.ty)?;
            Some(match types.resolve(field_ty) {
                Some(Type::Optional { inner }) => *inner,
                _ => field_ty,
            })
        }
        HirExpr::Index { owner, .. } => {
            let owner_ty =
                compute_chain_owner_ty(owner, class_index, declarations, types, resolve_global)?;
            Some(match types.resolve(owner_ty) {
                Some(Type::Optional { inner }) => *inner,
                _ => owner_ty,
            })
        }
        HirExpr::Call {
            callee: HirCallee::Indirect(inner),
            ..
        } => {
            let inner_ty =
                compute_chain_owner_ty(inner, class_index, declarations, types, resolve_global)?;
            Some(match types.resolve(inner_ty) {
                Some(Type::Optional { inner }) => *inner,
                _ => inner_ty,
            })
        }
        HirExpr::Local { ty, .. } => Some(*ty),
        HirExpr::Global { name, ty, .. } => {
            if matches!(types.resolve(*ty), Some(Type::Error)) {
                Some(resolve_global(name).unwrap_or(*ty))
            } else {
                Some(*ty)
            }
        }
        _ => Some(expr.ty()),
    }
}

pub(crate) fn build_class_index(declarations: &[HirDecl]) -> HashMap<TypeId, ClassPath> {
    let mut index = HashMap::new();
    for (idx, decl) in declarations.iter().enumerate() {
        index_decl(decl, ClassPath(vec![idx]), &mut index);
    }
    index
}

pub(crate) fn index_decl(decl: &HirDecl, path: ClassPath, index: &mut HashMap<TypeId, ClassPath>) {
    match decl {
        HirDecl::Class(c) => {
            index.insert(c.ty, path.clone());
            if let Some(pre_pass_ty) = c.pre_pass_ty
                && pre_pass_ty != c.ty
            {
                index.insert(pre_pass_ty, path.clone());
            }
        }
        HirDecl::Namespace { members, .. } => {
            for (member_idx, member) in members.iter().enumerate() {
                let mut member_path = path.clone();
                member_path.0.push(member_idx);
                index_decl(member, member_path, index);
            }
        }
        _ => {}
    }
}

pub(crate) fn class_at_path(declarations: &[HirDecl], path: ClassPath) -> Option<&HirClass> {
    let (first, rest) = path.0.split_first()?;
    let mut current = declarations.get(*first)?;
    for &idx in rest {
        current = match current {
            HirDecl::Namespace { members, .. } => members.get(idx)?,
            _ => return None,
        };
    }
    match current {
        HirDecl::Class(c) => Some(c),
        _ => None,
    }
}
