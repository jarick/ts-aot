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
use super::types::emit_type_id_with_ctx;

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
    function_ret: HashMap<FunctionId, TypeId>,
    function_params: HashMap<FunctionId, Vec<TypeId>>,
    function_async: HashSet<FunctionId>,
    function_owner: HashMap<FunctionId, StructId>,
    constructors: HashSet<FunctionId>,
    /// Methods whose body writes their receiver: they take &mut self and their
    /// call sites borrow the receiver instead of cloning it.
    pub(super) mutating_methods: HashSet<FunctionId>,
    struct_fields: HashMap<(StructId, FieldId), Ident>,
    struct_field_names: HashMap<StructId, Vec<Atom>>,
    mutable_globals: HashSet<Atom>,
    function_arity: HashMap<Atom, usize>,
    global_tys: HashMap<Atom, TypeId>,
    struct_lifetimes: HashMap<StructId, bool>,
    fn_value_index: HashMap<TypeId, usize>,
    fn_value_sigs: Vec<FnValueSig>,
    /// Per bucket, `(callee, variant, is_throwing, is_async)`: the Rust item the
    /// bucket's dispatch arm calls, the enum variant a value is built with, and
    /// the two call shims that arm needs.
    ///
    /// `callee` is normally the declaration's own name. For a declaration whose
    /// ABI parameter list is one `Option` level *wider* than the bucket's
    /// (`x: number | null = 7` against `(x: number | null) => ...`) it is a
    /// generated `FnValueAdapter` instead - the bucket's flat argument has to be
    /// re-wrapped before the call - and both flags are then `false`, because the
    /// adapter applies the throwing/async mapping itself.
    fn_value_variants: Vec<Vec<(Atom, Ident, bool, bool)>>,
    fn_value_of_fn: HashMap<Atom, (usize, Ident)>,
    closure_variants: RefCell<HashMap<usize, Vec<ClosureVariantInfo>>>,
    closure_fns: RefCell<Vec<TokenStream>>,
    env_structs: RefCell<Vec<TokenStream>>,
    dispatch_names: RefCell<HashSet<String>>,
    dispatch_sources: RefCell<HashSet<String>>,
    next_closure_id: Cell<u32>,
    used_runtime: Cell<bool>,
}

pub(super) type ClosureVariantInfo = (u32, Ident, Vec<TokenStream>);

pub(super) struct FnValueSig {
    pub(super) params: Vec<TypeId>,
    /// Positional "the caller may omit this argument" flags copied from the
    /// `Type::Fn` this signature was built from; an indirect call pads a missing
    /// trailing argument with `undefined` only where a flag is set.
    pub(super) optional: Vec<bool>,
    pub(super) ret: TypeId,
}

pub(super) struct EmitEnv<'a> {
    pub(super) types: &'a TypeTable,
    pub(super) program: &'a MirProgram,
}

fn collect_struct_lifetimes(program: &MirProgram, types: &TypeTable) -> HashMap<StructId, bool> {
    let mut needs: HashMap<StructId, bool> = HashMap::new();
    for decl in &program.declarations {
        if let MirDecl::Struct(s) = decl {
            needs.insert(s.id, false);
        }
    }
    loop {
        let mut changed = false;
        for decl in &program.declarations {
            let MirDecl::Struct(s) = decl else {
                continue;
            };
            if needs.get(&s.id).copied().unwrap_or(false) {
                continue;
            }
            if s.fields
                .iter()
                .any(|f| type_needs_lifetime(f.ty, types, &needs))
            {
                needs.insert(s.id, true);
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    needs
}

fn type_needs_lifetime(ty: TypeId, types: &TypeTable, needs: &HashMap<StructId, bool>) -> bool {
    match types.resolve(ty) {
        Some(Type::String | Type::Array { .. } | Type::Fn { .. }) => true,
        Some(Type::Struct { id }) => needs.get(id).copied().unwrap_or(true),
        Some(
            Type::Optional { inner, .. }
            | Type::GeneratorResult { inner }
            | Type::AsyncGenYield { inner }
            | Type::Generator { inner, .. }
            | Type::Promise { ok: inner, .. }
            | Type::WeakMap { value: inner, .. },
        ) => type_needs_lifetime(*inner, types, needs),
        Some(Type::Result { ok, err }) => {
            type_needs_lifetime(*ok, types, needs) || type_needs_lifetime(*err, types, needs)
        }
        Some(Type::Named { .. }) => types
            .struct_id(ty)
            .is_some_and(|id| needs.get(&id).copied().unwrap_or(true)),
        _ => false,
    }
}

impl EmitCtx {
    pub(super) fn build(program: &MirProgram, types: &TypeTable) -> Self {
        let mut struct_names = HashMap::new();
        let mut function_names = HashMap::new();
        let mut function_ret = HashMap::new();
        let mut function_async = HashSet::new();
        let mut function_owner = HashMap::new();
        let mut constructors = HashSet::new();
        let mut mutating_methods = HashSet::new();
        // A method that writes a field of `this` mutates the instance it is called
        // on - directly, or through another mutating method (`this.w ??= 7` calls
        // the setter). Such a method takes `&mut self` and its call sites borrow the
        // receiver: the by-value `mut self` wrote into a clone and the mutation was
        // silently lost. Which methods qualify is a fixpoint, and it has to be the
        // same predicate the signature is emitted from (`BodyCtx::local_mut`).
        loop {
            let mut changed = false;
            for decl in &program.declarations {
                let MirDecl::Struct(s) = decl else {
                    continue;
                };
                for method in &s.methods {
                    let FunctionKind::Method { self_param, .. } = method.kind else {
                        continue;
                    };
                    if mutating_methods.contains(&method.id) {
                        continue;
                    }
                    if collect_written_locals(&method.body.block, types, &mutating_methods)
                        .contains(&self_param)
                    {
                        mutating_methods.insert(method.id);
                        changed = true;
                    }
                }
            }
            if !changed {
                break;
            }
        }
        let mut struct_fields: HashMap<(StructId, FieldId), Ident> = HashMap::new();
        let mut struct_field_names: HashMap<StructId, Vec<Atom>> = HashMap::new();
        let mut mutable_globals = HashSet::new();
        let mut global_tys = HashMap::new();
        let mut function_arity = HashMap::new();
        let mut function_params: HashMap<FunctionId, Vec<TypeId>> = HashMap::new();
        for decl in &program.declarations {
            match decl {
                MirDecl::Function(f) => {
                    function_names.insert(f.id, ident_from(&f.name));
                    function_ret.insert(f.id, f.ret);
                    function_params.insert(f.id, f.params.iter().map(|p| p.ty).collect());
                    function_arity
                        .entry(f.name.clone())
                        .or_insert(f.params.len());
                    if f.effects.is_async && f.kind != FunctionKind::Generator {
                        function_async.insert(f.id);
                    }
                }
                MirDecl::Struct(s) => {
                    struct_names.insert(s.id, ident_from(&s.name));
                    for field in &s.fields {
                        struct_fields.insert((s.id, field.id), ident_from(&field.name));
                    }
                    struct_field_names
                        .insert(s.id, s.fields.iter().map(|f| f.name.clone()).collect());
                    for method in &s.methods {
                        function_names.insert(method.id, ident_from(&method.name));
                        function_ret.insert(method.id, method.ret);
                        function_params
                            .insert(method.id, method.params.iter().map(|p| p.ty).collect());
                        if method.effects.is_async
                            && !matches!(
                                method.kind,
                                FunctionKind::Generator | FunctionKind::GeneratorMethod { .. }
                            )
                        {
                            function_async.insert(method.id);
                        }
                        if let FunctionKind::Method { owner, .. }
                        | FunctionKind::GeneratorMethod { owner, .. }
                        | FunctionKind::Constructor { owner } = method.kind
                        {
                            function_owner.insert(method.id, owner);
                        }
                        if matches!(method.kind, FunctionKind::Constructor { .. }) {
                            constructors.insert(method.id);
                        }
                    }
                }
                MirDecl::Global(g) => {
                    global_tys.insert(g.name.clone(), g.ty);
                    if g.mutable {
                        mutable_globals.insert(g.name.clone());
                    }
                }
            }
        }
        let (fn_value_index, fn_value_sigs, fn_value_variants, fn_value_of_fn, fn_value_adapters) =
            collect_fn_values(program, types);
        let struct_lifetimes = collect_struct_lifetimes(program, types);
        let ctx = Self {
            struct_names,
            function_names,
            function_ret,
            function_params,
            function_async,
            function_owner,
            constructors,
            mutating_methods,
            struct_fields,
            struct_field_names,
            mutable_globals,
            global_tys,
            function_arity,
            struct_lifetimes,
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
            used_runtime: Cell::new(false),
        };
        // The enum and the `__ts_fn_call_k` helper are emitted from
        // `fn_value_variants` alone and only know the *bucket's* parameter types,
        // so a declaration registered under a wider ABI gets its argument
        // re-wrapping through a generated adapter fn, emitted next to the closure
        // bodies (the same slot `emit_closure_value` uses).
        if !fn_value_adapters.is_empty() {
            let emit_env = EmitEnv { types, program };
            for adapter in &fn_value_adapters {
                let tokens = adapter.emit(&emit_env, &ctx);
                ctx.push_closure_fn(tokens);
            }
        }
        ctx
    }

    pub(super) fn mark_runtime_used(&self) {
        self.used_runtime.set(true);
    }

    pub(super) fn runtime_used(&self) -> bool {
        self.used_runtime.get()
    }

    pub(super) fn struct_needs_lifetime(&self, id: StructId) -> bool {
        self.struct_lifetimes.get(&id).copied().unwrap_or(true)
    }

    pub(super) fn type_needs_lifetime(&self, ty: TypeId, types: &TypeTable) -> bool {
        type_needs_lifetime(ty, types, &self.struct_lifetimes)
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

    pub(super) fn fn_value_variants(&self, k: usize) -> &[(Atom, Ident, bool, bool)] {
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

    pub(super) fn global_ty(&self, name: &Atom) -> Option<TypeId> {
        self.global_tys.get(name).copied()
    }

    pub(super) fn struct_field_names(&self, id: StructId) -> Option<&[Atom]> {
        self.struct_field_names.get(&id).map(Vec::as_slice)
    }

    pub(super) fn function_arity(&self, name: &Atom) -> Option<usize> {
        self.function_arity.get(name).copied()
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

    pub(super) fn function_ret(&self, id: FunctionId) -> Option<TypeId> {
        self.function_ret.get(&id).copied()
    }

    pub(super) fn function_params(&self, id: FunctionId) -> Option<&[TypeId]> {
        self.function_params.get(&id).map(Vec::as_slice)
    }

    pub(super) fn function_is_async(&self, id: FunctionId) -> bool {
        self.function_async.contains(&id)
    }

    pub(super) fn function_owner(&self, id: FunctionId) -> Option<StructId> {
        self.function_owner.get(&id).copied()
    }

    /// A class constructor: its receiver is passed by mutable reference and it
    /// returns `()` (the instance is built by the caller).
    pub(super) fn function_is_constructor(&self, id: FunctionId) -> bool {
        self.constructors.contains(&id)
    }

    /// A method that mutates the instance it is called on: its receiver is passed
    /// by mutable reference, so the write reaches the caller's object.
    pub(super) fn function_is_mutating_method(&self, id: FunctionId) -> bool {
        self.mutating_methods.contains(&id)
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
    async_plain_ok: Option<TypeId>,
    in_try: Cell<bool>,
    in_tla_main: bool,
    pub(super) weakmap_liveness_per_key: RefCell<BTreeMap<LocalId, Ident>>,
    continue_label: RefCell<Option<Ident>>,
    continue_target: RefCell<Option<Ident>>,
    break_label: RefCell<Option<Ident>>,
    try_label: RefCell<Option<Ident>>,
    return_slot: RefCell<Option<Ident>>,
    loop_depth: Cell<u32>,
    switch_depth: Cell<u32>,
    try_jump_floor: RefCell<Vec<(u32, u32)>>,
    next_try_id: Cell<u32>,
    next_block_id: Cell<u32>,
    shared_by_id: HashMap<LocalId, SharedAccess>,
    /// Value types of the enclosing closure's captures. A capture read is a
    /// `MirExpr::Local` of an outer local, which the closure's own ctx has no
    /// declaration for, so without this its value type (and therefore every
    /// type-driven emission decision, e.g. numeric coercion of a literal
    /// operand) would be unknown.
    capture_tys: HashMap<LocalId, TypeId>,
    shared_by_name: HashMap<Ident, SharedAccess>,
    own_env: Option<OwnEnv>,
    dynamic_locals: RefCell<HashSet<LocalId>>,
    dynamic_names: RefCell<HashSet<Atom>>,
    move_safe_locals: RefCell<HashSet<LocalId>>,
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
    /// The declared type of the shared variable. A name-based capture
    /// (`MirExpr::Global`/`MirPlace::Global` of a variable captured through two
    /// levels of `ScopeEnv`) carries no type of its own, so this is the only
    /// place the emitter can learn it; without it the write is emitted without
    /// the `Option`/numeric coercion the field type requires.
    pub(super) ty: TypeId,
}

#[derive(Clone)]
pub(super) struct EnvField {
    pub(super) field: Ident,
    pub(super) ty: TypeId,
    pub(super) storage: SharedStorage,
    pub(super) param_init: Option<Ident>,
    pub(super) dynamic: bool,
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
    pub(super) fn new(
        f: &MirFunctionDecl,
        types: &TypeTable,
        mutating_methods: &HashSet<FunctionId>,
    ) -> Self {
        let self_param = match f.kind {
            FunctionKind::Method { self_param, .. }
            | FunctionKind::GeneratorMethod { self_param, .. } => Some(self_param),
            // A constructor's receiver is the first parameter (`this`), the same
            // convention the passes use for methods.
            FunctionKind::Constructor { .. } => Some(LocalId::from_raw(0)),
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
        let async_plain_ok = if !is_generator && f.effects.is_async {
            match types.resolve(f.ret) {
                Some(Type::Promise { ok, .. }) => Some(*ok),
                Some(Type::Result { ok, .. }) => match types.resolve(*ok) {
                    Some(Type::Promise { ok: inner, .. }) => Some(*inner),
                    _ => None,
                },
                _ => None,
            }
        } else {
            None
        };
        let gen_co = is_generator.then(|| format_ident!("__gen_co_{}", f.id.raw()));
        let written = collect_written_locals(&f.body.block, types, mutating_methods);
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
        let dynamic_ids = dynamic_locals_for(&f.body.block, &f.params, &f.body.locals, types);
        let mut dynamic_names: HashSet<Atom> = HashSet::new();
        for p in &f.params {
            if dynamic_ids.contains(&p.id) {
                dynamic_names.insert(p.name.clone());
            }
        }
        for l in &f.body.locals {
            if dynamic_ids.contains(&l.id) {
                dynamic_names.insert(l.name.clone());
            }
        }
        let move_safe_locals = collect_move_safe_locals(&f.body.block, &f.body.locals);
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
                    dynamic: dynamic_ids.contains(&id),
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
                    ty: ef.ty,
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
            async_plain_ok,
            in_try: Cell::new(false),
            in_tla_main: false,
            weakmap_liveness_per_key: RefCell::new(BTreeMap::new()),
            continue_label: RefCell::new(None),
            continue_target: RefCell::new(None),
            break_label: RefCell::new(None),
            try_label: RefCell::new(None),
            return_slot: RefCell::new(None),
            loop_depth: Cell::new(0),
            switch_depth: Cell::new(0),
            try_jump_floor: RefCell::new(Vec::new()),
            next_try_id: Cell::new(0),
            next_block_id: Cell::new(0),
            shared_by_id,
            shared_by_name,
            capture_tys: HashMap::new(),
            own_env,
            dynamic_locals: RefCell::new(dynamic_ids),
            dynamic_names: RefCell::new(dynamic_names),
            move_safe_locals: RefCell::new(move_safe_locals),
        }
    }

    pub(super) fn alloc_try_id(&self) -> u32 {
        let id = self.next_try_id.get();
        self.next_try_id.set(id + 1);
        id
    }

    pub(super) fn alloc_block_id(&self) -> u32 {
        let id = self.next_block_id.get();
        self.next_block_id.set(id + 1);
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
            async_plain_ok: None,
            in_try: Cell::new(false),
            in_tla_main: false,
            weakmap_liveness_per_key: RefCell::new(BTreeMap::new()),
            continue_label: RefCell::new(None),
            continue_target: RefCell::new(None),
            break_label: RefCell::new(None),
            try_label: RefCell::new(None),
            return_slot: RefCell::new(None),
            loop_depth: Cell::new(0),
            switch_depth: Cell::new(0),
            try_jump_floor: RefCell::new(Vec::new()),
            next_try_id: Cell::new(0),
            next_block_id: Cell::new(0),
            shared_by_id: inherited_ids,
            capture_tys: HashMap::new(),
            shared_by_name: inherited_names,
            own_env: None,
            dynamic_locals: RefCell::new(HashSet::new()),
            dynamic_names: RefCell::new(HashSet::new()),
            move_safe_locals: RefCell::new(HashSet::new()),
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
            let dynamic = self.is_dynamic(id);
            fields.push((
                id,
                name,
                EnvField {
                    field,
                    ty,
                    storage,
                    param_init,
                    dynamic,
                },
            ));
        }
        if fields.is_empty() {
            return;
        }
        for p in params {
            if self.is_dynamic(p.id) {
                self.dynamic_names.borrow_mut().insert(p.name.clone());
            }
        }
        for l in locals {
            if self.is_dynamic(l.id) {
                self.dynamic_names.borrow_mut().insert(l.name.clone());
            }
        }
        let env_ident = format_ident!("__env_c{}", key);
        let env_type = format_ident!("__ScopeEnvC{}", key);
        for (id, name, ef) in &fields {
            let acc = SharedAccess {
                env_ident: env_ident.clone(),
                env_type: env_type.clone(),
                field: ef.field.clone(),
                storage: ef.storage,
                ty: ef.ty,
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

    /// The declared type of a variable reached through a name-based capture.
    ///
    /// A capture two closure levels deep is emitted as a `MirExpr::Global`/
    /// `MirPlace::Global` of the captured name, which carries no `TypeId`; the
    /// env field still knows its declared type.
    pub(super) fn shared_ty_for_name(&self, name: &Atom) -> Option<TypeId> {
        self.shared_by_name.get(&ident_from(name)).map(|acc| acc.ty)
    }

    pub(super) fn local_read(&self, id: LocalId) -> TokenStream {
        if Some(id) == self.self_param {
            // A mutating method holds `&mut self`, so an owned read has to clone:
            // `return this` must not move out of the borrow.
            return if self.local_mut(id) {
                quote!(self.clone())
            } else {
                quote!(self)
            };
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

    pub(super) fn capture_ty(&self, id: LocalId) -> Option<TypeId> {
        self.capture_tys.get(&id).copied()
    }

    /// Records the value type of every capture of the closure whose body this
    /// ctx emits, so capture reads can be typed like ordinary locals.
    pub(super) fn seed_capture_types(&mut self, captures: &[LocalId], parent: &BodyCtx) {
        for id in captures {
            if let Some(ty) = parent.local_ty(*id).or_else(|| parent.capture_ty(*id)) {
                self.capture_tys.insert(*id, ty);
            }
        }
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

    pub(super) fn set_continue_target(&self, label: Option<Ident>) {
        *self.continue_target.borrow_mut() = label;
    }

    pub(super) fn continue_target(&self) -> Option<Ident> {
        self.continue_target.borrow().clone()
    }

    pub(super) fn set_break_label(&self, label: Option<Ident>) {
        *self.break_label.borrow_mut() = label;
    }

    pub(super) fn break_label(&self) -> Option<Ident> {
        self.break_label.borrow().clone()
    }

    pub(super) fn with_loop_labels<R>(
        &self,
        continue_target: Option<Ident>,
        f: impl FnOnce() -> R,
    ) -> R {
        let prev_continue = self.continue_label();
        let prev_break = self.break_label();
        let prev_target = self.continue_target();
        self.set_continue_label(None);
        self.set_break_label(None);
        self.set_continue_target(continue_target);
        self.loop_depth.set(self.loop_depth.get() + 1);
        let result = f();
        self.loop_depth.set(self.loop_depth.get() - 1);
        self.set_continue_label(prev_continue);
        self.set_break_label(prev_break);
        self.set_continue_target(prev_target);
        result
    }

    pub(super) fn enter_loop(&self) {
        self.loop_depth.set(self.loop_depth.get() + 1);
    }

    pub(super) fn exit_loop(&self) {
        self.loop_depth.set(self.loop_depth.get() - 1);
    }

    pub(super) fn enter_switch(&self) {
        self.switch_depth.set(self.switch_depth.get() + 1);
    }

    pub(super) fn exit_switch(&self) {
        self.switch_depth.set(self.switch_depth.get() - 1);
    }

    pub(super) fn push_try_floor(&self) {
        self.try_jump_floor
            .borrow_mut()
            .push((self.loop_depth.get(), self.switch_depth.get()));
    }

    pub(super) fn pop_try_floor(&self) {
        self.try_jump_floor.borrow_mut().pop();
    }

    pub(super) fn continue_escapes_try(&self) -> bool {
        self.try_jump_floor
            .borrow()
            .last()
            .is_some_and(|&(loop_floor, _)| loop_floor == self.loop_depth.get())
    }

    pub(super) fn break_escapes_try(&self) -> bool {
        self.try_jump_floor
            .borrow()
            .last()
            .is_some_and(|&(loop_floor, switch_floor)| {
                loop_floor == self.loop_depth.get() && switch_floor == self.switch_depth.get()
            })
    }

    pub(super) fn try_floor_loop(&self) -> Option<u32> {
        self.try_jump_floor
            .borrow()
            .last()
            .map(|&(loop_floor, _)| loop_floor)
    }

    pub(super) fn enclosing_try_loop_floor(&self) -> u32 {
        let floors = self.try_jump_floor.borrow();
        let len = floors.len();
        if len >= 2 { floors[len - 2].0 } else { 0 }
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

    pub(super) fn async_plain_ok(&self) -> Option<TypeId> {
        self.async_plain_ok
    }

    pub(super) fn is_dynamic(&self, id: LocalId) -> bool {
        self.dynamic_locals.borrow().contains(&id)
    }

    pub(super) fn inherit_dynamic(&self, parent: &BodyCtx) {
        self.dynamic_locals
            .borrow_mut()
            .extend(parent.dynamic_locals.borrow().iter().copied());
        self.dynamic_names
            .borrow_mut()
            .extend(parent.dynamic_names.borrow().iter().cloned());
    }

    pub(super) fn is_dynamic_name(&self, name: &Atom) -> bool {
        self.dynamic_names.borrow().contains(name)
    }

    pub(super) fn seed_dynamic(
        &self,
        block: &MirBlock,
        params: &[MirParam],
        locals: &[MirLocalDecl],
        types: &TypeTable,
    ) {
        let ids = dynamic_locals_for(block, params, locals, types);
        for p in params {
            if ids.contains(&p.id) {
                self.dynamic_names.borrow_mut().insert(p.name.clone());
            }
        }
        for l in locals {
            if ids.contains(&l.id) {
                self.dynamic_names.borrow_mut().insert(l.name.clone());
            }
        }
        self.dynamic_locals.borrow_mut().extend(ids);
    }

    pub(super) fn seed_move_safe(&self, block: &MirBlock, locals: &[MirLocalDecl]) {
        let ids = collect_move_safe_locals(block, locals);
        self.move_safe_locals.borrow_mut().extend(ids);
    }

    pub(super) fn is_move_safe(&self, id: LocalId) -> bool {
        self.move_safe_locals.borrow().contains(&id)
    }
}

fn collect_move_safe_locals(block: &MirBlock, locals: &[MirLocalDecl]) -> HashSet<LocalId> {
    let mut uses: HashMap<LocalId, (u32, bool)> = HashMap::new();
    count_uses_block(block, false, &mut uses);
    locals
        .iter()
        .filter(|l| l.name.as_str().is_empty())
        .filter(|l| {
            uses.get(&l.id)
                .is_some_and(|&(n, in_loop)| n == 1 && !in_loop)
        })
        .map(|l| l.id)
        .collect()
}

fn bump_use(uses: &mut HashMap<LocalId, (u32, bool)>, id: LocalId, in_loop: bool) {
    let entry = uses.entry(id).or_insert((0, false));
    entry.0 += 1;
    entry.1 |= in_loop;
}

fn count_uses_block(block: &MirBlock, in_loop: bool, uses: &mut HashMap<LocalId, (u32, bool)>) {
    for stmt in &block.stmts {
        count_uses_stmt(stmt, in_loop, uses);
    }
}

fn count_uses_stmt(stmt: &MirStmt, in_loop: bool, uses: &mut HashMap<LocalId, (u32, bool)>) {
    match stmt {
        MirStmt::Let { init: Some(e), .. } | MirStmt::Expr(e) | MirStmt::Return(Some(e)) => {
            count_uses_expr(e, in_loop, uses);
        }
        MirStmt::Let { init: None, .. }
        | MirStmt::Return(None)
        | MirStmt::Break
        | MirStmt::Continue => {}
        MirStmt::Assign { target, value } => {
            count_uses_place(target, in_loop, uses);
            count_uses_expr(value, in_loop, uses);
        }
        MirStmt::ReturnResultErr { error, .. } | MirStmt::Throw { error, .. } => {
            count_uses_expr(error, in_loop, uses);
        }
        MirStmt::If {
            cond,
            then_block,
            else_block,
        } => {
            count_uses_expr(cond, in_loop, uses);
            count_uses_block(then_block, in_loop, uses);
            if let Some(b) = else_block {
                count_uses_block(b, in_loop, uses);
            }
        }
        MirStmt::While { cond, body } | MirStmt::DoWhile { body, cond } => {
            count_uses_expr(cond, true, uses);
            count_uses_block(body, true, uses);
        }
        MirStmt::ForOf { iterable, body, .. } | MirStmt::ForAwaitOf { iterable, body, .. } => {
            count_uses_expr(iterable, in_loop, uses);
            count_uses_block(body, true, uses);
        }
        MirStmt::ForIn { object, body, .. } => {
            count_uses_expr(object, in_loop, uses);
            count_uses_block(body, true, uses);
        }
        MirStmt::Switch {
            disc,
            cases,
            default,
        } => {
            count_uses_expr(disc, in_loop, uses);
            for case in cases {
                count_uses_block(&case.body, in_loop, uses);
            }
            if let Some(b) = default {
                count_uses_block(b, in_loop, uses);
            }
        }
        MirStmt::Runtime { args, .. } => {
            for a in args {
                count_uses_expr(a, in_loop, uses);
            }
        }
        MirStmt::Try {
            body,
            catch,
            finally,
            ..
        } => {
            count_uses_block(body, in_loop, uses);
            if let Some(c) = catch {
                count_uses_block(c, in_loop, uses);
            }
            if let Some(f) = finally {
                count_uses_block(f, in_loop, uses);
            }
        }
    }
}

fn count_uses_place(place: &MirPlace, in_loop: bool, uses: &mut HashMap<LocalId, (u32, bool)>) {
    match place {
        MirPlace::Local { .. } | MirPlace::Global(_) => {}
        MirPlace::Field { base, .. } => count_uses_place_base(base, in_loop, uses),
        MirPlace::Index { base, index, .. } => {
            count_uses_expr(base, in_loop, uses);
            count_uses_expr(index, in_loop, uses);
        }
    }
}

fn count_uses_place_base(
    base: &MirPlaceBase,
    in_loop: bool,
    uses: &mut HashMap<LocalId, (u32, bool)>,
) {
    match base {
        MirPlaceBase::Local(id) => bump_use(uses, *id, in_loop),
        MirPlaceBase::Global(_) => {}
        MirPlaceBase::Field { base, .. } => count_uses_place_base(base, in_loop, uses),
        MirPlaceBase::Index { base, index, .. } => {
            count_uses_expr(base, in_loop, uses);
            count_uses_expr(index, in_loop, uses);
        }
        MirPlaceBase::Chain { base, .. } => count_uses_expr(base, in_loop, uses),
    }
}

fn count_uses_expr(expr: &MirExpr, in_loop: bool, uses: &mut HashMap<LocalId, (u32, bool)>) {
    match expr {
        MirExpr::Local(id) => bump_use(uses, *id, in_loop),
        MirExpr::Field { base, .. }
        | MirExpr::OptionalChain { base, .. }
        | MirExpr::ResultOk { value: base, .. } => count_uses_expr(base, in_loop, uses),
        MirExpr::Index { base, index, .. } => {
            count_uses_expr(base, in_loop, uses);
            count_uses_expr(index, in_loop, uses);
        }
        MirExpr::Binary { left, right, .. } => {
            count_uses_expr(left, in_loop, uses);
            count_uses_expr(right, in_loop, uses);
        }
        MirExpr::IsSome { expr, .. } | MirExpr::UnwrapOptional { expr, .. } => {
            count_uses_expr(expr, in_loop, uses);
        }
        MirExpr::Unary { expr, .. }
        | MirExpr::Await { expr, .. }
        | MirExpr::TypeOf { expr, .. }
        | MirExpr::Cast { expr, .. }
        | MirExpr::ResultErr { error: expr, .. }
        | MirExpr::Propagate { expr, .. }
        | MirExpr::Import { source: expr, .. } => count_uses_expr(expr, in_loop, uses),
        MirExpr::Yield { expr: Some(e), .. } => count_uses_expr(e, in_loop, uses),
        MirExpr::Call { args, .. } => {
            for a in args {
                count_uses_expr(a, in_loop, uses);
            }
        }
        MirExpr::IndirectCall { callee, args, .. } => {
            count_uses_expr(callee, in_loop, uses);
            for a in args {
                count_uses_expr(a, in_loop, uses);
            }
        }
        MirExpr::StructLiteral { fields, .. } => {
            for (_, v) in fields {
                count_uses_expr(v, in_loop, uses);
            }
        }
        MirExpr::Closure { body, .. } => count_uses_block(body, true, uses),
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
        | MirExpr::Yield { expr: None, .. } => {}
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

pub(super) fn collect_written_locals(
    block: &MirBlock,
    types: &TypeTable,
    mutating: &HashSet<FunctionId>,
) -> HashSet<LocalId> {
    let mut written = HashSet::new();
    collect_from_block(block, types, &mut written, mutating);
    written
}

fn dynamic_locals_for(
    block: &MirBlock,
    params: &[MirParam],
    locals: &[MirLocalDecl],
    types: &TypeTable,
) -> HashSet<LocalId> {
    let mut locals_ty: HashMap<LocalId, TypeId> = HashMap::new();
    for p in params {
        locals_ty.insert(p.id, p.ty);
    }
    for l in locals {
        locals_ty.insert(l.id, l.ty);
    }
    let mut ids = HashSet::new();
    collect_dynamic_locals(block, &locals_ty, types, &mut ids);
    ids
}

fn collect_dynamic_locals(
    block: &MirBlock,
    locals_ty: &HashMap<LocalId, TypeId>,
    types: &TypeTable,
    out: &mut HashSet<LocalId>,
) {
    for stmt in &block.stmts {
        match stmt {
            MirStmt::Try {
                body,
                catch_param,
                catch,
                finally,
            } => {
                if let Some(p) = catch_param
                    && locals_ty
                        .get(p)
                        .and_then(|t| types.resolve(*t))
                        .is_some_and(|t| matches!(t, Type::Error))
                {
                    out.insert(*p);
                }
                collect_dynamic_locals(body, locals_ty, types, out);
                if let Some(c) = catch {
                    collect_dynamic_locals(c, locals_ty, types, out);
                }
                if let Some(f) = finally {
                    collect_dynamic_locals(f, locals_ty, types, out);
                }
            }
            MirStmt::Let {
                local,
                init: Some(init),
                ..
            } => {
                if let MirExpr::Local(id) = init
                    && out.contains(id)
                    && locals_ty
                        .get(local)
                        .and_then(|t| types.resolve(*t))
                        .is_some_and(|t| matches!(t, Type::Error))
                {
                    out.insert(*local);
                }
            }
            MirStmt::Assign {
                target: MirPlace::Local { id },
                value,
            } => {
                if let MirExpr::Local(v) = value
                    && out.contains(v)
                    && locals_ty
                        .get(id)
                        .and_then(|t| types.resolve(*t))
                        .is_some_and(|t| matches!(t, Type::Error))
                {
                    out.insert(*id);
                }
            }
            MirStmt::If {
                then_block,
                else_block,
                ..
            } => {
                collect_dynamic_locals(then_block, locals_ty, types, out);
                if let Some(b) = else_block {
                    collect_dynamic_locals(b, locals_ty, types, out);
                }
            }
            MirStmt::While { body, .. }
            | MirStmt::DoWhile { body, .. }
            | MirStmt::ForOf { body, .. }
            | MirStmt::ForAwaitOf { body, .. }
            | MirStmt::ForIn { body, .. } => {
                collect_dynamic_locals(body, locals_ty, types, out);
            }
            MirStmt::Switch { cases, default, .. } => {
                for case in cases {
                    collect_dynamic_locals(&case.body, locals_ty, types, out);
                }
                if let Some(d) = default {
                    collect_dynamic_locals(d, locals_ty, types, out);
                }
            }
            _ => {}
        }
    }
}

pub(super) fn collect_assigned_locals(
    stmts: &[MirStmt],
    types: &TypeTable,
    candidates: &HashSet<LocalId>,
) -> HashSet<LocalId> {
    let mut all_written = HashSet::new();
    for stmt in stmts {
        collect_from_stmt(stmt, types, &mut all_written, &HashSet::new());
    }
    all_written.intersection(candidates).copied().collect()
}

fn collect_from_block(
    block: &MirBlock,
    types: &TypeTable,
    written: &mut HashSet<LocalId>,
    mutating: &HashSet<FunctionId>,
) {
    for stmt in &block.stmts {
        collect_from_stmt(stmt, types, written, mutating);
    }
}

fn collect_from_stmt(
    stmt: &MirStmt,
    types: &TypeTable,
    written: &mut HashSet<LocalId>,
    mutating: &HashSet<FunctionId>,
) {
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
            collect_from_block(then_block, types, written, mutating);
            if let Some(b) = else_block {
                collect_from_block(b, types, written, mutating);
            }
        }
        MirStmt::While { body, .. }
        | MirStmt::DoWhile { body, .. }
        | MirStmt::ForIn { body, .. } => {
            collect_from_block(body, types, written, mutating);
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
            collect_from_block(body, types, written, mutating);
        }
        MirStmt::Switch { cases, default, .. } => {
            for c in cases {
                collect_from_block(&c.body, types, written, mutating);
            }
            if let Some(b) = default {
                collect_from_block(b, types, written, mutating);
            }
        }
        MirStmt::Try {
            body,
            catch,
            finally,
            ..
        } => {
            collect_from_block(body, types, written, mutating);
            if let Some(b) = catch {
                collect_from_block(b, types, written, mutating);
            }
            if let Some(b) = finally {
                collect_from_block(b, types, written, mutating);
            }
        }
        MirStmt::Expr(expr) | MirStmt::Return(Some(expr)) => {
            collect_from_expr(expr, types, written, mutating);
        }
        MirStmt::Throw { error, .. } | MirStmt::ReturnResultErr { error, .. } => {
            collect_from_expr(error, types, written, mutating);
        }
        MirStmt::Let {
            init: Some(init), ..
        } => collect_from_expr(init, types, written, mutating),
        MirStmt::Let { init: None, .. }
        | MirStmt::Return(None)
        | MirStmt::Break
        | MirStmt::Continue => {}
    }
}

type FnValueRegistry = (
    HashMap<TypeId, usize>,
    Vec<FnValueSig>,
    Vec<Vec<(Atom, Ident, bool, bool)>>,
    HashMap<Atom, (usize, Ident)>,
    Vec<FnValueAdapter>,
);

/// How one bucket argument is adapted for the declaration parameter it feeds.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Lift {
    /// The bucket parameter *is* the declaration's ABI parameter.
    None,
    /// The declaration's ABI parameter is `Option<bucket parameter>`: the
    /// argument is always supplied, so wrap it (`Some(__a0)`).
    Wrap,
    /// Same nesting, but the bucket may also be called *without* this argument,
    /// which its ABI spells `None` (the call site pads an omitted trailing
    /// argument with `undefined`, which `optional_coerce` turns into `None`).
    /// `None` then means "no argument" and has to stay the outer `None`, so that
    /// the callee's default still applies: `__a0.map(Some)`.
    ///
    /// Only a bucket type that admits just `undefined` can use this: `null` and
    /// "no argument" would both be the flat `None`, so a passed `null` would
    /// select the default. A bucket type that admits `null` is rejected instead
    /// of being registered (see [`fn_value_bucket`]).
    WrapIfSupplied,
}

/// A declaration whose ABI parameter list is one `Option` level *wider* than the
/// function-value bucket it is registered in.
///
/// `f(x: number | null = 7)` is emitted with the nested ABI parameter
/// `Option<Option<f64>>`: the outer `Option` answers "was an argument supplied?"
/// (the callee's prologue applies the default when it is `None`) and the inner
/// one is the value's own nullishness. A function *type* can never spell a
/// default, so `(x: number | null) => number | null` has the flat parameter
/// `Option<f64>` and its call sites pass a flat value onward. The bucket arm
/// therefore cannot call the declaration directly: the generated adapter takes
/// the bucket's parameters and re-wraps the lifted ones, which is exactly the
/// "argument supplied" state the nested ABI encodes - a `null` argument reaches
/// the arm as `None` and becomes `Some(None)`, never the outer `None` that would
/// select the default.
struct FnValueAdapter {
    /// Name of the generated adapter fn; also the callee recorded for the
    /// bucket's dispatch arm.
    ident: Ident,
    /// The bucket's parameters, which are the adapter's own parameters.
    params: Vec<TypeId>,
    /// The bucket's return type.
    ret: TypeId,
    /// The declaration the adapter forwards to.
    decl: Ident,
    /// Positional adaptation of the bucket's arguments.
    lifts: Vec<Lift>,
    is_throwing: bool,
    is_async: bool,
}

impl FnValueAdapter {
    /// Rust source of `fn __ts_fn_value_lift_N<'a>(ctx, <bucket params>) -> <bucket ret>`
    /// forwarding to the declaration with the lifted arguments re-wrapped.
    fn emit(&self, emit_env: &EmitEnv<'_>, ctx: &EmitCtx) -> TokenStream {
        let ident = &self.ident;
        let decl = &self.decl;
        let arg_names: Vec<Ident> = (0..self.params.len())
            .map(|i| format_ident!("__a{}", i))
            .collect();
        let param_tys: Vec<TokenStream> = self
            .params
            .iter()
            .map(|ty| emit_type_id_with_ctx(*ty, emit_env, ctx))
            .collect();
        let ret = emit_type_id_with_ctx(self.ret, emit_env, ctx);
        let args: Vec<TokenStream> = arg_names
            .iter()
            .zip(&self.lifts)
            .map(|(name, lift)| match lift {
                Lift::None => quote!(#name),
                Lift::Wrap => quote!(::core::option::Option::Some(#name)),
                Lift::WrapIfSupplied => {
                    quote!(#name.map(::core::option::Option::Some))
                }
            })
            .collect();
        // The same shims `emit_fn_values` puts around a direct arm call; the
        // adapter owns them so the bucket only ever sees its own flat signature.
        let mut call = quote!(#decl(__ctx, #(#args),*));
        if self.is_async {
            call = quote!(ts_aot_runtime::__ts_aot_run_or_poll(#call));
        }
        if self.is_throwing {
            call = quote!(match #call {
                Ok(__v) => __v,
                Err(__e) => ts_aot_runtime::__ts_aot_throw(__e),
            });
        }
        quote! {
            fn #ident<'a>(
                __ctx: &ts_aot_runtime::Ctx<'a>,
                #(#arg_names: #param_tys),*
            ) -> #ret {
                #call
            }
        }
    }
}

/// Whether two types are the *same in the generated Rust* even though they are
/// distinct `TypeId`s.
///
/// Interning keys `Option` types by their nullish kind: `T | null` keeps `null`,
/// `T | undefined` and `x?: T` accept only `undefined`, so `number | null` and
/// `number | undefined` are two `TypeId`s - but one `Option<f64>`. The
/// difference lives in the source language and in `optional_coerce`, not in the
/// ABI, which is why the bucket for `(x?: number) => number` has to accept a
/// declaration parameter declared `number | null`. A function type whose
/// `optional` flags differ is the same ABI too whenever both intern to one
/// value bucket (that is exactly the enum both emit). Every other type is
/// compared by `TypeId`, which is structural equality for the rest.
fn same_abi(
    types: &TypeTable,
    fn_value_index: &HashMap<TypeId, usize>,
    a: TypeId,
    b: TypeId,
) -> bool {
    if a == b {
        return true;
    }
    match (types.resolve(a), types.resolve(b)) {
        (Some(Type::Optional { inner: ia, .. }), Some(Type::Optional { inner: ib, .. })) => {
            same_abi(types, fn_value_index, *ia, *ib)
        }
        (Some(Type::Fn { .. }), Some(Type::Fn { .. })) => matches!(
            (fn_value_index.get(&a), fn_value_index.get(&b)),
            (Some(ka), Some(kb)) if ka == kb
        ),
        _ => false,
    }
}

/// The bucket a declaration's ABI parameter list belongs to, plus the positions
/// that have to be re-wrapped.
///
/// Exact `params`/`ret` equality is tried first, so every declaration that was
/// registered before is still registered in the very same bucket. A declaration
/// whose parameter is one `Option` level *above* the bucket's is accepted as
/// well (see `FnValueAdapter`); the reverse relation never is - a bucket
/// parameter `Option<T>` against a declaration parameter `T` would let a call
/// omit an argument the declaration requires. The levels are compared with
/// `same_abi`, so the bucket's `Option<f64>` is the inner level of `number |
/// null`'s `Option<Option<f64>>` even though the two intern apart.
fn fn_value_bucket(
    types: &TypeTable,
    fn_value_index: &HashMap<TypeId, usize>,
    sigs: &[FnValueSig],
    params: &[TypeId],
    ret: TypeId,
) -> Option<(usize, Vec<Lift>)> {
    if let Some(k) = sigs.iter().position(|s| s.params == params && s.ret == ret) {
        return Some((k, vec![Lift::None; params.len()]));
    }
    sigs.iter().enumerate().find_map(|(k, s)| {
        if s.params.len() != params.len() || s.ret != ret {
            return None;
        }
        let mut lifts: Vec<Lift> = Vec::with_capacity(params.len());
        for (index, (declared, bucket)) in params.iter().zip(&s.params).enumerate() {
            if declared == bucket {
                lifts.push(Lift::None);
                continue;
            }
            let Some(Type::Optional { inner, .. }) = types.resolve(*declared) else {
                return None;
            };
            if !same_abi(types, fn_value_index, *inner, *bucket) {
                return None;
            }
            // Only an `Option` bucket parameter can carry "no argument" as its
            // `None`; a bucket safe to call without the argument therefore has to
            // keep that `None` outside the wrapper.
            let bucket_ty = types.resolve(*bucket);
            let may_omit = s.optional.get(index).copied().unwrap_or(false)
                && matches!(bucket_ty, Some(Type::Optional { .. }));
            if may_omit && matches!(bucket_ty, Some(Type::Optional { nullish, .. }) if nullish.null)
            {
                // The flat `Option<T>` ABI cannot express "the caller passed
                // `null`" and "the caller omitted the argument" at the same time:
                // both are `None`. `Lift::WrapIfSupplied` maps `None` to the outer
                // `None`, so a passed `null` would select the callee's default
                // instead of staying `null`. Reject the registration (the
                // established unsupported path leaves the declaration out of the
                // bucket) rather than emit an adapter that miscompiles one of the
                // two cases. A bucket whose type admits only `undefined` is fine:
                // an omitted argument and an explicit `undefined` both ask for the
                // default, so one `None` can mean both.
                return None;
            }
            lifts.push(if may_omit {
                Lift::WrapIfSupplied
            } else {
                Lift::Wrap
            });
        }
        lifts
            .iter()
            .any(|lift| *lift != Lift::None)
            .then_some((k, lifts))
    })
}

/// A name for a generated adapter that keeps out of the module's value
/// namespace: the adapter fn is emitted next to the declarations, so it must not
/// collide with a function, method or global of the program.
fn next_fn_value_adapter_ident(taken: &mut HashSet<String>, index: usize) -> Ident {
    let mut candidate = format!("__ts_fn_value_lift_{index}");
    let mut suffix = 0u32;
    while !taken.insert(candidate.clone()) {
        suffix = suffix.saturating_add(1);
        candidate = format!("__ts_fn_value_lift_{index}_{suffix}");
    }
    format_ident!("{candidate}")
}

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
        if let Some(Type::Fn {
            params,
            optional,
            ret,
            ..
        }) = types.resolve(id)
        {
            if fn_value_index.contains_key(&id) {
                continue;
            }
            // `optional` takes part in `Type`'s `Hash`/`Eq`, so `x?: number` and
            // `x: number | undefined` intern to two distinct `TypeId`s even though
            // their ABI is identical (`Option<f64>` either way) and a short vector
            // means "the rest is required" just like an explicit `false`. Bucketing
            // by `TypeId` alone therefore left one of the two enums
            // `__Uninhabited`, while a closure declared with the other spelling was
            // stored into it (E0308). All signatures that agree on `params`/`ret`
            // share one bucket, and the bucket keeps the *union* of the omission
            // flags: padding an omitted argument with `undefined` is always safe,
            // whereas dropping a pad a caller relied on would emit a call with a
            // missing argument.
            let k = if let Some(k) = fn_value_sigs
                .iter()
                .position(|s| s.params == *params && s.ret == *ret)
            {
                let flags = &mut fn_value_sigs[k].optional;
                for (index, may_omit) in optional.iter().enumerate() {
                    if !*may_omit {
                        continue;
                    }
                    if index >= flags.len() {
                        flags.resize(index + 1, false);
                    }
                    flags[index] = true;
                }
                k
            } else {
                let k = fn_value_sigs.len();
                fn_value_sigs.push(FnValueSig {
                    params: params.clone(),
                    optional: optional.clone(),
                    ret: *ret,
                });
                k
            };
            fn_value_index.insert(id, k);
        }
    }

    let mut fn_value_variants: Vec<Vec<(Atom, Ident, bool, bool)>> =
        vec![Vec::new(); fn_value_sigs.len()];
    let mut fn_value_of_fn: HashMap<Atom, (usize, Ident)> = HashMap::new();
    let mut fn_value_adapters: Vec<FnValueAdapter> = Vec::new();
    // Generated adapters are emitted next to the declarations, so keep their
    // names away from every name the program already puts in that namespace.
    let mut taken_idents: HashSet<String> = HashSet::new();
    for decl in &program.declarations {
        match decl {
            MirDecl::Function(f) => {
                taken_idents.insert(sanitize_ident(f.name.as_str()));
            }
            MirDecl::Struct(s) => {
                for m in &s.methods {
                    taken_idents.insert(sanitize_ident(m.name.as_str()));
                }
            }
            MirDecl::Global(g) => {
                taken_idents.insert(sanitize_ident(g.name.as_str()));
            }
        }
    }
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
            ) {
                continue;
            }
            if program.tla_main_name.as_ref().is_some_and(|n| *n == f.name) {
                continue;
            }
            let params: Vec<TypeId> = f.params.iter().map(|p| p.ty).collect();
            let is_throwing = matches!(types.resolve(f.ret), Some(Type::Result { .. }));
            let is_async = f.effects.is_async;
            let eff_ret = match types.resolve(f.ret) {
                Some(Type::Result { ok, .. }) => *ok,
                _ => f.ret,
            };
            // A signature is picked by `params`/`ret` alone: a `MirParam` does not
            // carry the "may be omitted" flag, and it does not have to - the
            // registry above keeps exactly one bucket per `(params, ret)` pair, so
            // the position is unique and the bucket's flags already cover every
            // spelling of the same function type. The ABI parameter may still be
            // one `Option` level *wider* than the bucket's (a defaulted nullable
            // parameter), in which case the bucket arm goes through an adapter.
            let Some((k, lifts)) =
                fn_value_bucket(types, &fn_value_index, &fn_value_sigs, &params, eff_ret)
            else {
                continue;
            };
            if fn_value_of_fn.contains_key(&f.name) {
                continue;
            }
            let base = pascal_variant_ident(f.name.as_str());
            let mut variant = base.clone();
            let mut suffix = 0u32;
            while fn_value_variants[k]
                .iter()
                .any(|(_, v, _, _)| *v == variant)
            {
                suffix += 1;
                variant = format_ident!("{}{}", base, suffix);
            }
            let (callee, is_throwing, is_async) = if lifts.iter().any(|lift| *lift != Lift::None) {
                let ident = next_fn_value_adapter_ident(&mut taken_idents, fn_value_adapters.len());
                fn_value_adapters.push(FnValueAdapter {
                    ident: ident.clone(),
                    params: fn_value_sigs[k].params.clone(),
                    ret: fn_value_sigs[k].ret,
                    decl: ident_from(&f.name),
                    lifts,
                    is_throwing,
                    is_async,
                });
                // The adapter owns the throwing/async shims, so the arm must not
                // apply them a second time.
                (Atom::from(ident.to_string()), false, false)
            } else {
                (f.name.clone(), is_throwing, is_async)
            };
            fn_value_variants[k].push((callee, variant.clone(), is_throwing, is_async));
            fn_value_of_fn.insert(f.name.clone(), (k, variant));
        }
    }

    (
        fn_value_index,
        fn_value_sigs,
        fn_value_variants,
        fn_value_of_fn,
        fn_value_adapters,
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
        Some(Type::Array { element } | Type::Optional { inner: element, .. }) => {
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
        Some(Type::Fn {
            params, ret, err, ..
        }) => {
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
        | MirExpr::ResultErr { error: base, .. }
        | MirExpr::Propagate { expr: base, .. } => collect_expr_tys(base, types, seen, out),
        MirExpr::Index { base, index, .. } => {
            collect_expr_tys(base, types, seen, out);
            collect_expr_tys(index, types, seen, out);
        }
        MirExpr::Binary { left, right, .. } => {
            collect_expr_tys(left, types, seen, out);
            collect_expr_tys(right, types, seen, out);
        }
        MirExpr::IsSome { expr, .. } | MirExpr::UnwrapOptional { expr, .. } => {
            collect_expr_tys(expr, types, seen, out);
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
        | MirExpr::ResultErr { error: base, .. }
        | MirExpr::Propagate { expr: base, .. } => collect_captures_expr(base, out),
        MirExpr::Index { base, index, .. } => {
            collect_captures_expr(base, out);
            collect_captures_expr(index, out);
        }
        MirExpr::IsSome { expr, .. } | MirExpr::UnwrapOptional { expr, .. } => {
            collect_captures_expr(expr, out);
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

fn collect_from_expr(
    expr: &MirExpr,
    types: &TypeTable,
    written: &mut HashSet<LocalId>,
    mutating: &HashSet<FunctionId>,
) {
    match expr {
        MirExpr::Field { base, .. }
        | MirExpr::OptionalChain { base, .. }
        | MirExpr::ResultOk { value: base, .. } => {
            collect_from_expr(base, types, written, mutating);
        }
        MirExpr::Index { base, index, .. } => {
            collect_from_expr(base, types, written, mutating);
            collect_from_expr(index, types, written, mutating);
        }
        MirExpr::IsSome { expr, .. } | MirExpr::UnwrapOptional { expr, .. } => {
            collect_from_expr(expr, types, written, mutating);
        }
        MirExpr::Binary { left, right, .. } => {
            collect_from_expr(left, types, written, mutating);
            collect_from_expr(right, types, written, mutating);
        }
        MirExpr::Unary { expr, .. }
        | MirExpr::Await { expr, .. }
        | MirExpr::TypeOf { expr, .. }
        | MirExpr::Cast { expr, .. }
        | MirExpr::ResultErr { error: expr, .. }
        | MirExpr::Propagate { expr, .. }
        | MirExpr::Import { source: expr, .. } => collect_from_expr(expr, types, written, mutating),
        MirExpr::Yield { expr: Some(e), .. } => collect_from_expr(e, types, written, mutating),
        MirExpr::Call { callee, args, .. } => {
            // The receiver of a mutating method is passed as &mut, so its
            // binding has to be mut even though the MIR never assigns it.
            if mutating.contains(callee)
                && let Some(root) = args.first().and_then(expr_root_local_id)
            {
                written.insert(root);
            }
            for a in args {
                collect_from_expr(a, types, written, mutating);
            }
        }
        MirExpr::IndirectCall { callee, args, .. } => {
            collect_from_expr(callee, types, written, mutating);
            for a in args {
                collect_from_expr(a, types, written, mutating);
            }
        }
        MirExpr::StructLiteral { fields, .. } => {
            for (_, v) in fields {
                collect_from_expr(v, types, written, mutating);
            }
        }
        MirExpr::Closure { body, .. } => {
            collect_from_block(body, types, written, mutating);
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
