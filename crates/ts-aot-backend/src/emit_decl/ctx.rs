use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap, HashSet};

use proc_macro2::{Ident, TokenStream};
use quote::{format_ident, quote};

use ts_aot_core::{Atom, FieldId, FunctionId, LocalId, StructId, Type, TypeId, TypeTable};
use ts_aot_ir_mir::{
    FunctionKind, MirBlock, MirDecl, MirExpr, MirFunctionDecl, MirLocalDecl, MirParam, MirPlace,
    MirPlaceBase, MirProgram, MirStmt, RuntimeOp,
};

use super::ident::{ident_from, sanitize_ident};

pub(super) struct RuntimeCallSpec<'a> {
    pub(super) op: RuntimeOp,
    pub(super) args: &'a [MirExpr],
    pub(super) dest: Option<LocalId>,
    pub(super) ty: TypeId,
    pub(super) target_ty: Option<TypeId>,
}

pub(super) struct EmitCtx {
    struct_names: HashMap<StructId, Ident>,
    function_names: HashMap<FunctionId, Ident>,
    struct_fields: HashMap<(StructId, FieldId), Ident>,
    mutable_globals: HashSet<Atom>,
    fn_value_index: HashMap<TypeId, usize>,
    fn_value_sigs: Vec<FnValueSig>,
    fn_value_variants: Vec<Vec<(Atom, Ident)>>,
    fn_value_of_fn: HashMap<Atom, (usize, Ident)>,
    closure_variants: RefCell<HashMap<usize, Vec<ClosureVariantInfo>>>,
    closure_fns: RefCell<Vec<TokenStream>>,
    env_structs: RefCell<Vec<TokenStream>>,
    dispatch_names: RefCell<HashSet<String>>,
    dispatch_sources: RefCell<HashSet<String>>,
    next_closure_id: Cell<u32>,
}

pub(super) type ClosureVariantInfo = (u32, Ident, Vec<TokenStream>);

pub(super) struct FnValueSig {
    pub(super) params: Vec<TypeId>,
    pub(super) ret: TypeId,
}

pub(super) struct EmitEnv<'a> {
    pub(super) types: &'a TypeTable,
    pub(super) program: &'a MirProgram,
}

impl EmitCtx {
    pub(super) fn build(program: &MirProgram, types: &TypeTable) -> Self {
        let mut struct_names = HashMap::new();
        let mut function_names = HashMap::new();
        let mut struct_fields: HashMap<(StructId, FieldId), Ident> = HashMap::new();
        let mut mutable_globals = HashSet::new();
        for decl in &program.declarations {
            match decl {
                MirDecl::Function(f) => {
                    function_names.insert(f.id, ident_from(&f.name));
                }
                MirDecl::Struct(s) => {
                    struct_names.insert(s.id, ident_from(&s.name));
                    for field in &s.fields {
                        struct_fields.insert((s.id, field.id), ident_from(&field.name));
                    }
                    for method in &s.methods {
                        function_names.insert(method.id, ident_from(&method.name));
                    }
                }
                MirDecl::Global(g) => {
                    if g.mutable {
                        mutable_globals.insert(g.name.clone());
                    }
                }
            }
        }
        let (fn_value_index, fn_value_sigs, fn_value_variants, fn_value_of_fn) =
            collect_fn_values(program, types);
        Self {
            struct_names,
            function_names,
            struct_fields,
            mutable_globals,
            fn_value_index,
            fn_value_sigs,
            fn_value_variants,
            fn_value_of_fn,
            closure_variants: RefCell::new(HashMap::new()),
            closure_fns: RefCell::new(Vec::new()),
            env_structs: RefCell::new(Vec::new()),
            dispatch_names: RefCell::new(HashSet::new()),
            dispatch_sources: RefCell::new(HashSet::new()),
            next_closure_id: Cell::new(0),
        }
    }

    pub(super) fn alloc_closure_id(&self) -> u32 {
        let id = self.next_closure_id.get();
        self.next_closure_id
            .set(id.checked_add(1).expect("closure id space exhausted"));
        id
    }

    pub(super) fn register_dispatch_source(&self, name: &str) -> bool {
        self.dispatch_sources.borrow_mut().insert(name.to_owned())
    }

    pub(super) fn dispatch_wrapper_ident(&self, name: &Ident) -> Ident {
        let composed = format!("__ts_aot_dispatch_{name}");
        let base = normalize_dispatch_name(&composed);
        let mut used = self.dispatch_names.borrow_mut();
        let mut candidate = base.clone();
        let mut suffix = 0u32;
        while used.contains(&candidate) {
            suffix = suffix.saturating_add(1);
            candidate = format!("{base}_{suffix}");
        }
        used.insert(candidate.clone());
        format_ident!("{candidate}")
    }

    pub(super) fn push_env_struct(&self, tokens: TokenStream) {
        self.env_structs.borrow_mut().push(tokens);
    }

    pub(super) fn take_env_structs(&self) -> Vec<TokenStream> {
        std::mem::take(&mut self.env_structs.borrow_mut())
    }

    pub(super) fn register_closure(
        &self,
        id: u32,
        sig: usize,
        capture_types: Vec<TokenStream>,
    ) -> Ident {
        let variant = format_ident!("C{}", id);
        self.closure_variants
            .borrow_mut()
            .entry(sig)
            .or_default()
            .push((id, variant.clone(), capture_types));
        variant
    }

    pub(super) fn closure_variants_for(&self, sig: usize) -> Vec<ClosureVariantInfo> {
        self.closure_variants
            .borrow()
            .get(&sig)
            .cloned()
            .unwrap_or_default()
    }

    pub(super) fn push_closure_fn(&self, tokens: TokenStream) {
        self.closure_fns.borrow_mut().push(tokens);
    }

    pub(super) fn take_closure_fns(&self) -> Vec<TokenStream> {
        std::mem::take(&mut self.closure_fns.borrow_mut())
    }

    pub(super) fn fn_value_ident(k: usize) -> Ident {
        format_ident!("__TsFnValue{}", k)
    }

    pub(super) fn fn_call_ident(k: usize) -> Ident {
        format_ident!("__ts_fn_call_{}", k)
    }

    pub(super) fn fn_value_kind(&self, ty: TypeId) -> Option<usize> {
        self.fn_value_index.get(&ty).copied()
    }

    pub(super) fn fn_value_for_name(&self, name: &Atom) -> Option<(usize, Ident)> {
        self.fn_value_of_fn.get(name).cloned()
    }

    pub(super) fn fn_value_count(&self) -> usize {
        self.fn_value_sigs.len()
    }

    pub(super) fn fn_value_sig(&self, k: usize) -> &FnValueSig {
        &self.fn_value_sigs[k]
    }

    pub(super) fn fn_value_variants(&self, k: usize) -> &[(Atom, Ident)] {
        &self.fn_value_variants[k]
    }

    /// Whether the named global is emitted as a `static mut` (and therefore its
    /// reads and writes require an `unsafe` block in the generated Rust).
    ///
    /// Keyed by the global's MIR name (`MirGlobalDecl::name`). References
    /// (`MirExpr::Global` / `MirPlace::Global`) carry the same name: HIR→MIR
    /// never renames a global reference, and a global whose sanitized name would
    /// collide with another declaration is rejected earlier with `E0503`, so the
    /// decl name and the reference name cannot diverge in a compiled program.
    pub(super) fn is_mutable_global(&self, name: &Atom) -> bool {
        self.mutable_globals.contains(name)
    }

    pub(super) fn struct_ident(&self, id: StructId) -> Ident {
        self.struct_names
            .get(&id)
            .cloned()
            .unwrap_or_else(|| format_ident!("__struct{}", id.raw()))
    }

    pub(super) fn function_ident(&self, id: FunctionId) -> Ident {
        self.function_names
            .get(&id)
            .cloned()
            .unwrap_or_else(|| format_ident!("__fn{}", id.raw()))
    }

    pub(super) fn struct_field_ident(&self, struct_id: StructId, field_id: FieldId) -> Ident {
        self.struct_fields
            .get(&(struct_id, field_id))
            .cloned()
            .unwrap_or_else(|| format_ident!("__field{}", field_id.raw()))
    }
}

pub(super) struct BodyCtx {
    locals: HashMap<LocalId, Ident>,
    locals_ty: HashMap<LocalId, TypeId>,
    locals_mut: HashMap<LocalId, bool>,
    reserved_idents: HashSet<Ident>,
    self_param: Option<LocalId>,
    is_generator: bool,
    is_async_generator: bool,
    gen_co: Option<Ident>,
    ret_inner: TypeId,
    in_try: Cell<bool>,
    in_tla_main: bool,
    pub(super) weakmap_liveness_per_key: RefCell<BTreeMap<LocalId, Ident>>,
    continue_label: RefCell<Option<Ident>>,
    try_label: RefCell<Option<Ident>>,
    return_slot: RefCell<Option<Ident>>,
    next_try_id: Cell<u32>,
    shared_by_id: HashMap<LocalId, SharedAccess>,
    shared_by_name: HashMap<Ident, SharedAccess>,
    own_env: Option<OwnEnv>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum SharedStorage {
    Cell,
    RefCell,
}

#[derive(Clone)]
pub(super) struct SharedAccess {
    pub(super) env_ident: Ident,
    pub(super) env_type: Ident,
    pub(super) field: Ident,
    pub(super) storage: SharedStorage,
}

#[derive(Clone)]
pub(super) struct EnvField {
    pub(super) field: Ident,
    pub(super) ty: TypeId,
    pub(super) storage: SharedStorage,
    pub(super) param_init: Option<Ident>,
}

pub(super) struct OwnEnv {
    pub(super) env_ident: Ident,
    pub(super) env_type: Ident,
    pub(super) fields: Vec<EnvField>,
}

fn shared_read_tokens(acc: &SharedAccess) -> TokenStream {
    let env = &acc.env_ident;
    let field = &acc.field;
    match acc.storage {
        SharedStorage::Cell => {
            quote!(#env.#field.get().expect("shared variable read before initialization"))
        }
        SharedStorage::RefCell => quote!(
            #env
                .#field
                .borrow()
                .as_ref()
                .expect("shared variable read before initialization")
                .clone()
        ),
    }
}

fn shared_place_base_tokens(acc: &SharedAccess) -> Option<TokenStream> {
    let env = &acc.env_ident;
    let field = &acc.field;
    match acc.storage {
        SharedStorage::RefCell => Some(quote!((*#env.#field.borrow_mut()).as_mut().unwrap())),
        SharedStorage::Cell => None,
    }
}

pub(super) fn shared_write_tokens(acc: &SharedAccess, value: &TokenStream) -> TokenStream {
    let env = &acc.env_ident;
    let field = &acc.field;
    let tmp = format_ident!("__ts_shared_write_{}", field);
    match acc.storage {
        SharedStorage::Cell => quote! {
            let #tmp = #value;
            #env.#field.set(Some(#tmp));
        },
        SharedStorage::RefCell => quote! {
            let #tmp = #value;
            *#env.#field.borrow_mut() = Some(#tmp);
        },
    }
}

pub(super) fn shared_borrow_tokens(acc: &SharedAccess) -> Option<TokenStream> {
    let env = &acc.env_ident;
    let field = &acc.field;
    match acc.storage {
        SharedStorage::RefCell => Some(quote!(
            #env
                .#field
                .borrow()
                .as_ref()
                .expect("shared variable read before initialization")
        )),
        SharedStorage::Cell => None,
    }
}

pub(super) fn shared_reset_tokens(acc: &SharedAccess) -> TokenStream {
    let env = &acc.env_ident;
    let field = &acc.field;
    match acc.storage {
        SharedStorage::Cell => quote!(#env.#field.set(None);),
        SharedStorage::RefCell => quote!(*#env.#field.borrow_mut() = None;),
    }
}

fn shared_storage_for(ty: Option<&Type>) -> SharedStorage {
    match ty {
        Some(
            Type::Bool
            | Type::I8
            | Type::I16
            | Type::I32
            | Type::I64
            | Type::U8
            | Type::U16
            | Type::U32
            | Type::U64
            | Type::F32
            | Type::F64
            | Type::Date
            | Type::Symbol,
        ) => SharedStorage::Cell,
        _ => SharedStorage::RefCell,
    }
}

impl BodyCtx {
    pub(super) fn new(f: &MirFunctionDecl, types: &TypeTable) -> Self {
        let self_param = match f.kind {
            FunctionKind::Method { self_param, .. }
            | FunctionKind::GeneratorMethod { self_param, .. } => Some(self_param),
            _ => None,
        };
        let is_generator = matches!(
            f.kind,
            FunctionKind::Generator | FunctionKind::GeneratorMethod { .. }
        );
        let is_async_generator = is_generator && f.effects.is_async;
        let ret_inner = if is_generator {
            match types.resolve(f.ret) {
                Some(Type::Generator { inner, .. }) => *inner,
                _ => f.ret,
            }
        } else {
            f.ret
        };
        let gen_co = is_generator.then(|| format_ident!("__gen_co_{}", f.id.raw()));
        let written = collect_written_locals(&f.body.block, types);
        let mut locals = HashMap::new();
        let mut locals_ty = HashMap::new();
        let mut locals_mut = HashMap::new();
        for param in &f.params {
            if Some(param.id) != self_param {
                locals.insert(param.id, ident_from(&param.name));
            }
            locals_ty.insert(param.id, param.ty);
            locals_mut.insert(param.id, written.contains(&param.id));
        }
        for local in &f.body.locals {
            locals.insert(local.id, ident_from(&local.name));
            locals_ty.insert(local.id, local.ty);
            locals_mut.insert(local.id, local.mutable || written.contains(&local.id));
        }
        let reserved_idents = locals.values().cloned().collect::<HashSet<_>>();
        let param_ids: HashSet<LocalId> = f
            .params
            .iter()
            .map(|p| p.id)
            .filter(|id| Some(*id) != self_param)
            .collect();
        let mut fields: Vec<(LocalId, Ident, EnvField)> = Vec::new();
        for id in collect_captured_locals(&f.body.block) {
            if Some(id) == self_param {
                continue;
            }
            let (Some(name), Some(&ty)) = (locals.get(&id), locals_ty.get(&id)) else {
                continue;
            };
            if fields.iter().any(|(fid, _, _)| *fid == id) {
                continue;
            }
            let storage = shared_storage_for(types.resolve(ty));
            let field = format_ident!("{}_{}", name, id.raw());
            let param_init = param_ids.contains(&id).then(|| name.clone());
            fields.push((
                id,
                name.clone(),
                EnvField {
                    field,
                    ty,
                    storage,
                    param_init,
                },
            ));
        }
        let mut shared_by_id: HashMap<LocalId, SharedAccess> = HashMap::new();
        let mut shared_by_name: HashMap<Ident, SharedAccess> = HashMap::new();
        let mut own_env = None;
        if !fields.is_empty() {
            let env_ident = format_ident!("__env_{}", f.id.raw());
            let env_type = format_ident!("__ScopeEnv{}", f.id.raw());
            for (id, name, ef) in &fields {
                let acc = SharedAccess {
                    env_ident: env_ident.clone(),
                    env_type: env_type.clone(),
                    field: ef.field.clone(),
                    storage: ef.storage,
                };
                shared_by_id.insert(*id, acc.clone());
                shared_by_name.insert(name.clone(), acc);
            }
            own_env = Some(OwnEnv {
                env_ident,
                env_type,
                fields: fields.iter().map(|(_, _, ef)| ef.clone()).collect(),
            });
        }
        Self {
            locals,
            locals_ty,
            locals_mut,
            reserved_idents,
            self_param,
            is_generator,
            is_async_generator,
            gen_co,
            ret_inner,
            in_try: Cell::new(false),
            in_tla_main: false,
            weakmap_liveness_per_key: RefCell::new(BTreeMap::new()),
            continue_label: RefCell::new(None),
            try_label: RefCell::new(None),
            return_slot: RefCell::new(None),
            next_try_id: Cell::new(0),
            shared_by_id,
            shared_by_name,
            own_env,
        }
    }

    pub(super) fn alloc_try_id(&self) -> u32 {
        let id = self.next_try_id.get();
        self.next_try_id.set(id + 1);
        id
    }

    pub(super) fn for_closure(
        ret: TypeId,
        inherited_names: HashMap<Ident, SharedAccess>,
        inherited_ids: HashMap<LocalId, SharedAccess>,
    ) -> Self {
        Self {
            locals: HashMap::new(),
            locals_ty: HashMap::new(),
            locals_mut: HashMap::new(),
            reserved_idents: HashSet::new(),
            self_param: None,
            is_generator: false,
            is_async_generator: false,
            gen_co: None,
            ret_inner: ret,
            in_try: Cell::new(false),
            in_tla_main: false,
            weakmap_liveness_per_key: RefCell::new(BTreeMap::new()),
            continue_label: RefCell::new(None),
            try_label: RefCell::new(None),
            return_slot: RefCell::new(None),
            next_try_id: Cell::new(0),
            shared_by_id: inherited_ids,
            shared_by_name: inherited_names,
            own_env: None,
        }
    }

    pub(super) fn shared_name_map(&self) -> HashMap<Ident, SharedAccess> {
        self.shared_by_name.clone()
    }

    pub(super) fn shared_id_map(&self) -> HashMap<LocalId, SharedAccess> {
        self.shared_by_id.clone()
    }

    pub(super) fn forget_inherited_name(&mut self, name: &Ident) {
        self.shared_by_name.remove(name);
    }

    pub(super) fn own_env(&self) -> Option<&OwnEnv> {
        self.own_env.as_ref()
    }

    /// A closure body gets its own scope env for locals of that closure which
    /// are themselves captured by nested closures.
    pub(super) fn detect_own_env(
        &mut self,
        key: &str,
        block: &MirBlock,
        params: &[MirParam],
        locals: &[MirLocalDecl],
        types: &TypeTable,
    ) {
        let mut by_id: HashMap<LocalId, (Ident, TypeId)> = HashMap::new();
        for p in params {
            by_id.insert(p.id, (ident_from(&p.name), p.ty));
        }
        for l in locals {
            by_id.insert(l.id, (ident_from(&l.name), l.ty));
        }
        let param_ids: HashSet<LocalId> = params.iter().map(|p| p.id).collect();
        let mut fields: Vec<(LocalId, Ident, EnvField)> = Vec::new();
        for id in collect_captured_locals(block) {
            let Some((name, ty)) = by_id.get(&id).cloned() else {
                continue;
            };
            if fields.iter().any(|(fid, _, _)| *fid == id) {
                continue;
            }
            let storage = shared_storage_for(types.resolve(ty));
            let field = format_ident!("{}_{}", name, id.raw());
            let param_init = param_ids.contains(&id).then(|| name.clone());
            fields.push((
                id,
                name,
                EnvField {
                    field,
                    ty,
                    storage,
                    param_init,
                },
            ));
        }
        if fields.is_empty() {
            return;
        }
        let env_ident = format_ident!("__env_c{}", key);
        let env_type = format_ident!("__ScopeEnvC{}", key);
        for (id, name, ef) in &fields {
            let acc = SharedAccess {
                env_ident: env_ident.clone(),
                env_type: env_type.clone(),
                field: ef.field.clone(),
                storage: ef.storage,
            };
            self.shared_by_id.insert(*id, acc.clone());
            self.shared_by_name.insert(name.clone(), acc);
        }
        self.own_env = Some(OwnEnv {
            env_ident,
            env_type,
            fields: fields.iter().map(|(_, _, ef)| ef.clone()).collect(),
        });
    }

    pub(super) fn shared_access_for_local(&self, id: LocalId) -> Option<SharedAccess> {
        self.shared_by_id.get(&id).cloned()
    }

    pub(super) fn shared_access_for_name(&self, name: &Atom) -> Option<SharedAccess> {
        self.shared_by_name.get(&ident_from(name)).cloned()
    }

    pub(super) fn local_read(&self, id: LocalId) -> TokenStream {
        if Some(id) == self.self_param {
            return quote!(self);
        }
        if let Some(acc) = self.shared_access_for_local(id) {
            return shared_read_tokens(&acc);
        }
        let ident = self.local_ident(id);
        quote!(#ident)
    }

    pub(super) fn shared_read_for_name(&self, name: &Atom) -> Option<TokenStream> {
        self.shared_access_for_name(name)
            .map(|acc| shared_read_tokens(&acc))
    }

    pub(super) fn local_place_base(&self, id: LocalId) -> TokenStream {
        if Some(id) == self.self_param {
            return quote!(self);
        }
        if let Some(acc) = self.shared_access_for_local(id) {
            let base = shared_place_base_tokens(&acc);
            debug_assert!(
                base.is_some(),
                "Cell-backed shared local cannot be used as a place base"
            );
            if let Some(tokens) = base {
                return tokens;
            }
        }
        let ident = self.local_ident(id);
        quote!(#ident)
    }

    pub(super) fn shared_place_base_for_name(&self, name: &Atom) -> Option<TokenStream> {
        self.shared_access_for_name(name)
            .and_then(|acc| shared_place_base_tokens(&acc))
    }

    pub(super) fn register_local(&mut self, id: LocalId, name: Ident, ty: TypeId, mutable: bool) {
        self.locals.insert(id, name.clone());
        self.locals_ty.insert(id, ty);
        self.locals_mut.insert(id, mutable);
        self.reserved_idents.insert(name);
    }

    pub(super) fn weakmap_liveness_for_key(&self, key_local: LocalId) -> Ident {
        {
            let cache = self.weakmap_liveness_per_key.borrow();
            if let Some(ident) = cache.get(&key_local) {
                return ident.clone();
            }
        }
        let ident = self.alloc_weakmap_liveness_tmp();
        self.weakmap_liveness_per_key
            .borrow_mut()
            .insert(key_local, ident.clone());
        ident
    }

    pub(super) fn alloc_weakmap_liveness_tmp(&self) -> Ident {
        let used_in_cache: std::collections::HashSet<Ident> = self
            .weakmap_liveness_per_key
            .borrow()
            .values()
            .cloned()
            .collect();
        let mut suffix: u32 = 0;
        loop {
            let candidate = format_ident!("__wm_liveness_{}", suffix);
            if !self.reserved_idents.contains(&candidate) && !used_in_cache.contains(&candidate) {
                return candidate;
            }
            suffix += 1;
        }
    }

    pub(super) fn has_weakmap_liveness(&self) -> bool {
        !self.weakmap_liveness_per_key.borrow().is_empty()
    }

    pub(super) fn has_weakmap_liveness_for_key(&self, key_local: LocalId) -> bool {
        self.weakmap_liveness_per_key
            .borrow()
            .contains_key(&key_local)
    }

    pub(super) fn is_generator(&self) -> bool {
        self.is_generator
    }

    pub(super) fn is_async_generator(&self) -> bool {
        self.is_async_generator
    }

    pub(super) fn gen_co(&self) -> Option<Ident> {
        self.gen_co.clone()
    }

    pub(super) fn local_mut(&self, id: LocalId) -> bool {
        self.locals_mut.get(&id).copied().unwrap_or(false)
    }

    pub(super) fn local_ident(&self, id: LocalId) -> Ident {
        match self.locals.get(&id) {
            Some(ident) if ident == "_" => {
                let mut candidate = format_ident!("__tmp{}", id.raw());
                while self.reserved_idents.contains(&candidate) {
                    candidate = format_ident!("{}_", candidate);
                }
                candidate
            }
            Some(ident) => ident.clone(),
            None => format_ident!("__local{}", id.raw()),
        }
    }

    pub(super) fn self_param(&self) -> Option<LocalId> {
        self.self_param
    }

    pub(super) fn local_ref(&self, id: LocalId) -> TokenStream {
        if Some(id) == self.self_param {
            quote!(self)
        } else {
            let ident = self.local_ident(id);
            quote!(#ident)
        }
    }

    pub(super) fn local_ty(&self, id: LocalId) -> Option<TypeId> {
        self.locals_ty.get(&id).copied()
    }

    pub(super) fn set_in_try(&self, value: bool) {
        self.in_try.set(value);
    }

    pub(super) fn in_try(&self) -> bool {
        self.in_try.get()
    }

    pub(super) fn set_in_tla_main(&mut self, value: bool) {
        self.in_tla_main = value;
    }

    pub(super) fn in_tla_main(&self) -> bool {
        self.in_tla_main
    }

    pub(super) fn set_continue_label(&self, label: Option<Ident>) {
        *self.continue_label.borrow_mut() = label;
    }

    pub(super) fn continue_label(&self) -> Option<Ident> {
        self.continue_label.borrow().clone()
    }

    pub(super) fn set_try_label(&self, label: Option<Ident>) {
        *self.try_label.borrow_mut() = label;
    }

    pub(super) fn try_label(&self) -> Option<Ident> {
        self.try_label.borrow().clone()
    }

    pub(super) fn set_return_slot(&self, slot: Option<Ident>) {
        *self.return_slot.borrow_mut() = slot;
    }

    pub(super) fn return_slot(&self) -> Option<Ident> {
        self.return_slot.borrow().clone()
    }

    pub(super) fn return_type_id(&self) -> TypeId {
        self.ret_inner
    }
}

fn place_root_local_id(place: &MirPlace) -> Option<LocalId> {
    match place {
        MirPlace::Local { id } => Some(*id),
        MirPlace::Global(_) => None,
        MirPlace::Field { base, .. } => place_base_root_local_id(base),
        MirPlace::Index { base, .. } => expr_root_local_id(base),
    }
}

/// Returns the name of the global at the root of a place, if any
/// (e.g. `global`, `global.field`, `global[i]`).
pub(super) fn place_root_global_name(place: &MirPlace) -> Option<&Atom> {
    match place {
        MirPlace::Global(name) => Some(name),
        MirPlace::Local { .. } => None,
        MirPlace::Field { base, .. } => place_base_root_global_name(base),
        MirPlace::Index { base, .. } => expr_root_global_name(base),
    }
}

fn place_base_root_global_name(base: &MirPlaceBase) -> Option<&Atom> {
    match base {
        MirPlaceBase::Global(name) => Some(name),
        MirPlaceBase::Local(_) => None,
        MirPlaceBase::Field { base: inner, .. } => place_base_root_global_name(inner),
        MirPlaceBase::Index { base: inner, .. } | MirPlaceBase::Chain { base: inner, .. } => {
            expr_root_global_name(inner)
        }
    }
}

fn expr_root_global_name(expr: &MirExpr) -> Option<&Atom> {
    match expr {
        MirExpr::Global(name) => Some(name),
        MirExpr::Field { base, .. }
        | MirExpr::Index { base, .. }
        | MirExpr::OptionalChain { base, .. } => expr_root_global_name(base),
        _ => None,
    }
}

fn place_base_root_local_id(base: &MirPlaceBase) -> Option<LocalId> {
    match base {
        MirPlaceBase::Local(id) => Some(*id),
        MirPlaceBase::Global(_) => None,
        MirPlaceBase::Field { base: inner, .. } => place_base_root_local_id(inner),
        MirPlaceBase::Index { base: inner, .. } | MirPlaceBase::Chain { base: inner, .. } => {
            expr_root_local_id(inner)
        }
    }
}

fn expr_root_local_id(expr: &MirExpr) -> Option<LocalId> {
    match expr {
        MirExpr::Local(id) => Some(*id),
        MirExpr::Field { base, .. }
        | MirExpr::Index { base, .. }
        | MirExpr::OptionalChain { base, .. } => expr_root_local_id(base),
        _ => None,
    }
}

pub(super) fn mutably_borrowed_arg_index(op: RuntimeOp) -> Option<usize> {
    match op {
        RuntimeOp::GeneratorNext
        | RuntimeOp::ArrayPush
        | RuntimeOp::ArraySet
        | RuntimeOp::MapSet => Some(0),
        _ => None,
    }
}

pub(super) fn collect_written_locals(block: &MirBlock, types: &TypeTable) -> HashSet<LocalId> {
    let mut written = HashSet::new();
    collect_from_block(block, types, &mut written);
    written
}

pub(super) fn collect_assigned_locals(
    stmts: &[MirStmt],
    types: &TypeTable,
    candidates: &HashSet<LocalId>,
) -> HashSet<LocalId> {
    let mut all_written = HashSet::new();
    for stmt in stmts {
        collect_from_stmt(stmt, types, &mut all_written);
    }
    all_written.intersection(candidates).copied().collect()
}

fn collect_from_block(block: &MirBlock, types: &TypeTable, written: &mut HashSet<LocalId>) {
    for stmt in &block.stmts {
        collect_from_stmt(stmt, types, written);
    }
}

fn collect_from_stmt(stmt: &MirStmt, types: &TypeTable, written: &mut HashSet<LocalId>) {
    match stmt {
        MirStmt::Assign { target, .. } => {
            if let Some(root) = place_root_local_id(target) {
                written.insert(root);
            }
        }
        MirStmt::Runtime { op, args, dest, .. } => {
            if let Some(d) = dest {
                written.insert(*d);
            }
            if let Some(idx) = mutably_borrowed_arg_index(*op)
                && let Some(root) = args.get(idx).and_then(expr_root_local_id)
            {
                written.insert(root);
            }
        }
        MirStmt::If {
            then_block,
            else_block,
            ..
        } => {
            collect_from_block(then_block, types, written);
            if let Some(b) = else_block {
                collect_from_block(b, types, written);
            }
        }
        MirStmt::While { body, .. }
        | MirStmt::DoWhile { body, .. }
        | MirStmt::ForIn { body, .. } => {
            collect_from_block(body, types, written);
        }
        MirStmt::ForOf {
            iterable,
            iter_ty,
            body,
            ..
        }
        | MirStmt::ForAwaitOf {
            iterable,
            iter_ty,
            body,
            ..
        } => {
            if matches!(types.resolve(*iter_ty), Some(Type::Generator { .. }))
                && let Some(id) = expr_root_local_id(iterable)
            {
                written.insert(id);
            }
            collect_from_block(body, types, written);
        }
        MirStmt::Switch { cases, default, .. } => {
            for c in cases {
                collect_from_block(&c.body, types, written);
            }
            if let Some(b) = default {
                collect_from_block(b, types, written);
            }
        }
        MirStmt::Try {
            body,
            catch,
            finally,
            ..
        } => {
            collect_from_block(body, types, written);
            if let Some(b) = catch {
                collect_from_block(b, types, written);
            }
            if let Some(b) = finally {
                collect_from_block(b, types, written);
            }
        }
        MirStmt::Expr(expr) | MirStmt::Return(Some(expr)) => {
            collect_from_expr(expr, types, written);
        }
        MirStmt::Throw { error, .. } | MirStmt::ReturnResultErr { error, .. } => {
            collect_from_expr(error, types, written);
        }
        MirStmt::Let {
            init: Some(init), ..
        } => collect_from_expr(init, types, written),
        MirStmt::Let { init: None, .. }
        | MirStmt::Return(None)
        | MirStmt::Break
        | MirStmt::Continue => {}
    }
}

type FnValueRegistry = (
    HashMap<TypeId, usize>,
    Vec<FnValueSig>,
    Vec<Vec<(Atom, Ident)>>,
    HashMap<Atom, (usize, Ident)>,
);

fn normalize_dispatch_name(composed: &str) -> String {
    let mut out = String::with_capacity(composed.len());
    let mut prev_underscore = false;
    for (idx, ch) in composed.chars().enumerate() {
        if ch == '_' {
            if idx < 2 || !prev_underscore {
                out.push('_');
            }
            prev_underscore = true;
        } else {
            out.push(ch);
            prev_underscore = false;
        }
    }
    out
}

fn pascal_variant_ident(name: &str) -> Ident {
    let mut out = String::with_capacity(name.len());
    let mut upper = true;
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            if upper {
                out.push(ch.to_ascii_uppercase());
                upper = false;
            } else {
                out.push(ch);
            }
        } else {
            upper = true;
        }
    }
    if out.is_empty() || out.as_bytes()[0].is_ascii_digit() {
        out.insert(0, 'V');
    }
    format_ident!("{}", sanitize_ident(&out))
}

fn collect_fn_values(program: &MirProgram, types: &TypeTable) -> FnValueRegistry {
    let mut seen = HashSet::new();
    let mut used = Vec::new();
    for decl in &program.declarations {
        match decl {
            MirDecl::Function(f) => collect_fn_decl_tys(f, types, &mut seen, &mut used),
            MirDecl::Struct(s) => {
                for field in &s.fields {
                    collect_type_ids(field.ty, types, &mut seen, &mut used);
                }
                for m in &s.methods {
                    collect_fn_decl_tys(m, types, &mut seen, &mut used);
                }
            }
            MirDecl::Global(g) => {
                collect_type_ids(g.ty, types, &mut seen, &mut used);
                if let Some(init) = &g.init {
                    collect_expr_tys(init, types, &mut seen, &mut used);
                }
            }
        }
    }

    let mut fn_value_index: HashMap<TypeId, usize> = HashMap::new();
    let mut fn_value_sigs: Vec<FnValueSig> = Vec::new();
    for id in used {
        if let Some(Type::Fn { params, ret, .. }) = types.resolve(id) {
            if fn_value_index.contains_key(&id) {
                continue;
            }
            let k = fn_value_sigs.len();
            fn_value_index.insert(id, k);
            fn_value_sigs.push(FnValueSig {
                params: params.clone(),
                ret: *ret,
            });
        }
    }

    let mut fn_value_variants: Vec<Vec<(Atom, Ident)>> = vec![Vec::new(); fn_value_sigs.len()];
    let mut fn_value_of_fn: HashMap<Atom, (usize, Ident)> = HashMap::new();
    for decl in &program.declarations {
        let funcs: Vec<&MirFunctionDecl> = match decl {
            MirDecl::Function(f) => vec![f],
            MirDecl::Struct(s) => s.methods.iter().collect(),
            MirDecl::Global(_) => Vec::new(),
        };
        for f in funcs {
            if !matches!(
                f.kind,
                FunctionKind::Plain | FunctionKind::Closure | FunctionKind::RuntimeShim
            ) || f.effects.is_async
            {
                continue;
            }
            if program.tla_main_name.as_ref().is_some_and(|n| *n == f.name) {
                continue;
            }
            let params: Vec<TypeId> = f.params.iter().map(|p| p.ty).collect();
            let Some(k) = fn_value_sigs
                .iter()
                .position(|s| s.params == params && s.ret == f.ret)
            else {
                continue;
            };
            if fn_value_of_fn.contains_key(&f.name) {
                continue;
            }
            let base = pascal_variant_ident(f.name.as_str());
            let mut variant = base.clone();
            let mut suffix = 0u32;
            while fn_value_variants[k].iter().any(|(_, v)| *v == variant) {
                suffix += 1;
                variant = format_ident!("{}{}", base, suffix);
            }
            fn_value_variants[k].push((f.name.clone(), variant.clone()));
            fn_value_of_fn.insert(f.name.clone(), (k, variant));
        }
    }

    (
        fn_value_index,
        fn_value_sigs,
        fn_value_variants,
        fn_value_of_fn,
    )
}

fn collect_fn_decl_tys(
    f: &MirFunctionDecl,
    types: &TypeTable,
    seen: &mut HashSet<TypeId>,
    out: &mut Vec<TypeId>,
) {
    for p in &f.params {
        collect_type_ids(p.ty, types, seen, out);
    }
    collect_type_ids(f.ret, types, seen, out);
    if let Some(t) = f.throws {
        collect_type_ids(t, types, seen, out);
    }
    for l in &f.body.locals {
        collect_type_ids(l.ty, types, seen, out);
    }
    collect_block_tys(&f.body.block, types, seen, out);
}

fn collect_type_ids(
    id: TypeId,
    types: &TypeTable,
    seen: &mut HashSet<TypeId>,
    out: &mut Vec<TypeId>,
) {
    if !seen.insert(id) {
        return;
    }
    out.push(id);
    match types.resolve(id) {
        Some(Type::Array { element } | Type::Optional { inner: element }) => {
            collect_type_ids(*element, types, seen, out);
        }
        Some(Type::Result { ok, err }) => {
            collect_type_ids(*ok, types, seen, out);
            collect_type_ids(*err, types, seen, out);
        }
        Some(Type::Promise { ok, err }) => {
            collect_type_ids(*ok, types, seen, out);
            if let Some(err) = err {
                collect_type_ids(*err, types, seen, out);
            }
        }
        Some(Type::Fn { params, ret, err }) => {
            for p in params {
                collect_type_ids(*p, types, seen, out);
            }
            collect_type_ids(*ret, types, seen, out);
            if let Some(err) = err {
                collect_type_ids(*err, types, seen, out);
            }
        }
        Some(Type::WeakMap { key, value }) => {
            collect_type_ids(*key, types, seen, out);
            collect_type_ids(*value, types, seen, out);
        }
        Some(Type::Union { variants } | Type::Intersection { parts: variants }) => {
            for v in variants {
                collect_type_ids(*v, types, seen, out);
            }
        }
        Some(Type::Tuple { elements }) => {
            for e in elements {
                collect_type_ids(*e, types, seen, out);
            }
        }
        Some(Type::Generator { inner, .. } | Type::GeneratorResult { inner }) => {
            collect_type_ids(*inner, types, seen, out);
        }
        _ => {}
    }
}

fn collect_block_tys(
    block: &MirBlock,
    types: &TypeTable,
    seen: &mut HashSet<TypeId>,
    out: &mut Vec<TypeId>,
) {
    for stmt in &block.stmts {
        collect_stmt_tys(stmt, types, seen, out);
    }
}

fn collect_stmt_tys(
    stmt: &MirStmt,
    types: &TypeTable,
    seen: &mut HashSet<TypeId>,
    out: &mut Vec<TypeId>,
) {
    match stmt {
        MirStmt::Let { ty, init, .. } => {
            collect_type_ids(*ty, types, seen, out);
            if let Some(init) = init {
                collect_expr_tys(init, types, seen, out);
            }
        }
        MirStmt::Assign { target, value } => {
            if let Some(ty) = target.ty() {
                collect_type_ids(ty, types, seen, out);
            }
            collect_place_tys(target, types, seen, out);
            collect_expr_tys(value, types, seen, out);
        }
        MirStmt::Expr(expr) | MirStmt::Return(Some(expr)) => {
            collect_expr_tys(expr, types, seen, out);
        }
        MirStmt::ReturnResultErr { error, err_ty: t } | MirStmt::Throw { error, error_ty: t } => {
            collect_type_ids(*t, types, seen, out);
            collect_expr_tys(error, types, seen, out);
        }
        MirStmt::If {
            cond,
            then_block,
            else_block,
        } => {
            collect_expr_tys(cond, types, seen, out);
            collect_block_tys(then_block, types, seen, out);
            if let Some(b) = else_block {
                collect_block_tys(b, types, seen, out);
            }
        }
        MirStmt::While { cond, body } | MirStmt::DoWhile { body, cond } => {
            collect_expr_tys(cond, types, seen, out);
            collect_block_tys(body, types, seen, out);
        }
        MirStmt::ForOf {
            iterable,
            iter_ty,
            body,
            ..
        }
        | MirStmt::ForAwaitOf {
            iterable,
            iter_ty,
            body,
            ..
        } => {
            collect_type_ids(*iter_ty, types, seen, out);
            collect_expr_tys(iterable, types, seen, out);
            collect_block_tys(body, types, seen, out);
        }
        MirStmt::ForIn { object, body, .. } => {
            collect_expr_tys(object, types, seen, out);
            collect_block_tys(body, types, seen, out);
        }
        MirStmt::Return(None) | MirStmt::Break | MirStmt::Continue => {}
        MirStmt::Runtime {
            args,
            ty,
            target_ty,
            ..
        } => {
            collect_type_ids(*ty, types, seen, out);
            if let Some(t) = target_ty {
                collect_type_ids(*t, types, seen, out);
            }
            for a in args {
                collect_expr_tys(a, types, seen, out);
            }
        }
        MirStmt::Switch {
            disc,
            cases,
            default,
        } => {
            collect_expr_tys(disc, types, seen, out);
            for c in cases {
                collect_block_tys(&c.body, types, seen, out);
            }
            if let Some(b) = default {
                collect_block_tys(b, types, seen, out);
            }
        }
        MirStmt::Try {
            body,
            catch,
            finally,
            ..
        } => {
            collect_block_tys(body, types, seen, out);
            if let Some(b) = catch {
                collect_block_tys(b, types, seen, out);
            }
            if let Some(b) = finally {
                collect_block_tys(b, types, seen, out);
            }
        }
    }
}

fn collect_place_tys(
    place: &MirPlace,
    types: &TypeTable,
    seen: &mut HashSet<TypeId>,
    out: &mut Vec<TypeId>,
) {
    match place {
        MirPlace::Local { .. } | MirPlace::Global(_) => {}
        MirPlace::Field { base, ty, .. } => {
            collect_type_ids(*ty, types, seen, out);
            collect_place_base_tys(base, types, seen, out);
        }
        MirPlace::Index { base, index, ty } => {
            collect_type_ids(*ty, types, seen, out);
            collect_expr_tys(base, types, seen, out);
            collect_expr_tys(index, types, seen, out);
        }
    }
}

fn collect_place_base_tys(
    base: &MirPlaceBase,
    types: &TypeTable,
    seen: &mut HashSet<TypeId>,
    out: &mut Vec<TypeId>,
) {
    match base {
        MirPlaceBase::Local(_) | MirPlaceBase::Global(_) => {}
        MirPlaceBase::Field { base, ty, .. } => {
            collect_type_ids(*ty, types, seen, out);
            collect_place_base_tys(base, types, seen, out);
        }
        MirPlaceBase::Index { base, index, ty } => {
            collect_type_ids(*ty, types, seen, out);
            collect_expr_tys(base, types, seen, out);
            collect_expr_tys(index, types, seen, out);
        }
        MirPlaceBase::Chain { base, ty } => {
            collect_type_ids(*ty, types, seen, out);
            collect_expr_tys(base, types, seen, out);
        }
    }
}

fn collect_expr_tys(
    expr: &MirExpr,
    types: &TypeTable,
    seen: &mut HashSet<TypeId>,
    out: &mut Vec<TypeId>,
) {
    if let Some(ty) = expr.ty() {
        collect_type_ids(ty, types, seen, out);
    }
    match expr {
        MirExpr::Field { base, .. }
        | MirExpr::OptionalChain { base, .. }
        | MirExpr::ResultOk { value: base, .. }
        | MirExpr::Unary { expr: base, .. }
        | MirExpr::Await { expr: base, .. }
        | MirExpr::TypeOf { expr: base, .. }
        | MirExpr::Cast { expr: base, .. }
        | MirExpr::Import { source: base, .. }
        | MirExpr::ResultErr { error: base, .. } => collect_expr_tys(base, types, seen, out),
        MirExpr::Index { base, index, .. } => {
            collect_expr_tys(base, types, seen, out);
            collect_expr_tys(index, types, seen, out);
        }
        MirExpr::Binary { left, right, .. } => {
            collect_expr_tys(left, types, seen, out);
            collect_expr_tys(right, types, seen, out);
        }
        MirExpr::Yield { expr: Some(e), .. } => collect_expr_tys(e, types, seen, out),
        MirExpr::Call { args, .. } => {
            for a in args {
                collect_expr_tys(a, types, seen, out);
            }
        }
        MirExpr::IndirectCall { callee, args, .. } => {
            collect_expr_tys(callee, types, seen, out);
            for a in args {
                collect_expr_tys(a, types, seen, out);
            }
        }
        MirExpr::StructLiteral { fields, .. } => {
            for (_, v) in fields {
                collect_expr_tys(v, types, seen, out);
            }
        }
        MirExpr::Closure {
            params,
            locals,
            body,
            ret_ty,
            ..
        } => {
            for p in params {
                collect_type_ids(p.ty, types, seen, out);
            }
            for l in locals {
                collect_type_ids(l.ty, types, seen, out);
            }
            collect_type_ids(*ret_ty, types, seen, out);
            collect_block_tys(body, types, seen, out);
        }
        MirExpr::Unit
        | MirExpr::Bool(_)
        | MirExpr::Int { .. }
        | MirExpr::Float { .. }
        | MirExpr::String { .. }
        | MirExpr::Null { .. }
        | MirExpr::Local(_)
        | MirExpr::Global(_)
        | MirExpr::TemplateStringsArray { .. }
        | MirExpr::RegExp { .. }
        | MirExpr::BigInt { .. }
        | MirExpr::Yield { expr: None, .. } => {}
    }
}

fn collect_captured_locals(block: &MirBlock) -> Vec<LocalId> {
    let mut out = HashSet::new();
    collect_captures_block(block, &mut out);
    let mut ids: Vec<LocalId> = out.into_iter().collect();
    ids.sort_unstable_by_key(|id| id.raw());
    ids
}

fn collect_captures_block(block: &MirBlock, out: &mut HashSet<LocalId>) {
    for stmt in &block.stmts {
        collect_captures_stmt(stmt, out);
    }
}

fn collect_captures_stmt(stmt: &MirStmt, out: &mut HashSet<LocalId>) {
    match stmt {
        MirStmt::Let { init, .. } => {
            if let Some(init) = init {
                collect_captures_expr(init, out);
            }
        }
        MirStmt::Assign { target, value } => {
            collect_captures_place(target, out);
            collect_captures_expr(value, out);
        }
        MirStmt::Expr(expr) | MirStmt::Return(Some(expr)) => collect_captures_expr(expr, out),
        MirStmt::Return(None) | MirStmt::Break | MirStmt::Continue => {}
        MirStmt::ReturnResultErr { error, .. } | MirStmt::Throw { error, .. } => {
            collect_captures_expr(error, out);
        }
        MirStmt::If {
            cond,
            then_block,
            else_block,
        } => {
            collect_captures_expr(cond, out);
            collect_captures_block(then_block, out);
            if let Some(b) = else_block {
                collect_captures_block(b, out);
            }
        }
        MirStmt::While { cond, body } | MirStmt::DoWhile { body, cond } => {
            collect_captures_expr(cond, out);
            collect_captures_block(body, out);
        }
        MirStmt::ForOf { iterable, body, .. } | MirStmt::ForAwaitOf { iterable, body, .. } => {
            collect_captures_expr(iterable, out);
            collect_captures_block(body, out);
        }
        MirStmt::ForIn { object, body, .. } => {
            collect_captures_expr(object, out);
            collect_captures_block(body, out);
        }
        MirStmt::Runtime { args, .. } => {
            for a in args {
                collect_captures_expr(a, out);
            }
        }
        MirStmt::Switch {
            disc,
            cases,
            default,
        } => {
            collect_captures_expr(disc, out);
            for c in cases {
                collect_captures_block(&c.body, out);
            }
            if let Some(b) = default {
                collect_captures_block(b, out);
            }
        }
        MirStmt::Try {
            body,
            catch,
            finally,
            ..
        } => {
            collect_captures_block(body, out);
            if let Some(b) = catch {
                collect_captures_block(b, out);
            }
            if let Some(b) = finally {
                collect_captures_block(b, out);
            }
        }
    }
}

fn collect_captures_place(place: &MirPlace, out: &mut HashSet<LocalId>) {
    match place {
        MirPlace::Local { .. } | MirPlace::Global(_) => {}
        MirPlace::Field { base, .. } => collect_captures_place_base(base, out),
        MirPlace::Index { base, index, .. } => {
            collect_captures_expr(base, out);
            collect_captures_expr(index, out);
        }
    }
}

fn collect_captures_place_base(base: &MirPlaceBase, out: &mut HashSet<LocalId>) {
    match base {
        MirPlaceBase::Local(_) | MirPlaceBase::Global(_) => {}
        MirPlaceBase::Field { base, .. } => collect_captures_place_base(base, out),
        MirPlaceBase::Index { base, index, .. } => {
            collect_captures_expr(base, out);
            collect_captures_expr(index, out);
        }
        MirPlaceBase::Chain { base, .. } => collect_captures_expr(base, out),
    }
}

fn collect_captures_expr(expr: &MirExpr, out: &mut HashSet<LocalId>) {
    match expr {
        MirExpr::Closure { captures, body, .. } => {
            for c in captures {
                out.insert(*c);
            }
            collect_captures_block(body, out);
        }
        MirExpr::Field { base, .. }
        | MirExpr::OptionalChain { base, .. }
        | MirExpr::ResultOk { value: base, .. }
        | MirExpr::Unary { expr: base, .. }
        | MirExpr::Await { expr: base, .. }
        | MirExpr::TypeOf { expr: base, .. }
        | MirExpr::Cast { expr: base, .. }
        | MirExpr::Import { source: base, .. }
        | MirExpr::ResultErr { error: base, .. } => collect_captures_expr(base, out),
        MirExpr::Index { base, index, .. } => {
            collect_captures_expr(base, out);
            collect_captures_expr(index, out);
        }
        MirExpr::Binary { left, right, .. } => {
            collect_captures_expr(left, out);
            collect_captures_expr(right, out);
        }
        MirExpr::Yield { expr: Some(e), .. } => collect_captures_expr(e, out),
        MirExpr::Call { args, .. } => {
            for a in args {
                collect_captures_expr(a, out);
            }
        }
        MirExpr::IndirectCall { callee, args, .. } => {
            collect_captures_expr(callee, out);
            for a in args {
                collect_captures_expr(a, out);
            }
        }
        MirExpr::StructLiteral { fields, .. } => {
            for (_, v) in fields {
                collect_captures_expr(v, out);
            }
        }
        MirExpr::Unit
        | MirExpr::Bool(_)
        | MirExpr::Int { .. }
        | MirExpr::Float { .. }
        | MirExpr::String { .. }
        | MirExpr::Null { .. }
        | MirExpr::Local(_)
        | MirExpr::Global(_)
        | MirExpr::TemplateStringsArray { .. }
        | MirExpr::RegExp { .. }
        | MirExpr::BigInt { .. }
        | MirExpr::Yield { expr: None, .. } => {}
    }
}

fn collect_from_expr(expr: &MirExpr, types: &TypeTable, written: &mut HashSet<LocalId>) {
    match expr {
        MirExpr::Field { base, .. }
        | MirExpr::OptionalChain { base, .. }
        | MirExpr::ResultOk { value: base, .. } => collect_from_expr(base, types, written),
        MirExpr::Index { base, index, .. } => {
            collect_from_expr(base, types, written);
            collect_from_expr(index, types, written);
        }
        MirExpr::Binary { left, right, .. } => {
            collect_from_expr(left, types, written);
            collect_from_expr(right, types, written);
        }
        MirExpr::Unary { expr, .. }
        | MirExpr::Await { expr, .. }
        | MirExpr::TypeOf { expr, .. }
        | MirExpr::Cast { expr, .. }
        | MirExpr::ResultErr { error: expr, .. }
        | MirExpr::Import { source: expr, .. } => collect_from_expr(expr, types, written),
        MirExpr::Yield { expr: Some(e), .. } => collect_from_expr(e, types, written),
        MirExpr::Call { args, .. } => {
            for a in args {
                collect_from_expr(a, types, written);
            }
        }
        MirExpr::IndirectCall { callee, args, .. } => {
            collect_from_expr(callee, types, written);
            for a in args {
                collect_from_expr(a, types, written);
            }
        }
        MirExpr::StructLiteral { fields, .. } => {
            for (_, v) in fields {
                collect_from_expr(v, types, written);
            }
        }
        MirExpr::Closure { body, .. } => {
            collect_from_block(body, types, written);
        }
        MirExpr::Unit
        | MirExpr::Bool(_)
        | MirExpr::Int { .. }
        | MirExpr::Float { .. }
        | MirExpr::String { .. }
        | MirExpr::Null { .. }
        | MirExpr::Local(_)
        | MirExpr::Global(_)
        | MirExpr::TemplateStringsArray { .. }
        | MirExpr::RegExp { .. }
        | MirExpr::BigInt { .. }
        | MirExpr::Yield { expr: None, .. } => {}
    }
}
