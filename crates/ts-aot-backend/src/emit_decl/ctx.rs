use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};

use proc_macro2::{Ident, TokenStream};
use quote::{format_ident, quote};

use ts_aot_core::{FieldId, FunctionId, LocalId, StructId, Type, TypeId, TypeTable};
use ts_aot_ir_mir::{
    FunctionKind, MirBlock, MirDecl, MirExpr, MirFunctionDecl, MirPlace, MirPlaceBase, MirProgram,
    MirStmt, RuntimeOp,
};

use super::ident::ident_from;

pub(super) struct EmitCtx<'a> {
    pub(super) types: &'a TypeTable,
    struct_names: HashMap<StructId, Ident>,
    function_names: HashMap<FunctionId, Ident>,
    struct_fields: HashMap<(StructId, FieldId), Ident>,
}

impl<'a> EmitCtx<'a> {
    pub(super) fn new(program: &MirProgram, types: &'a TypeTable) -> Self {
        let mut struct_names = HashMap::new();
        let mut function_names = HashMap::new();
        let mut struct_fields: HashMap<(StructId, FieldId), Ident> = HashMap::new();
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
                MirDecl::Global(_) => {}
            }
        }
        Self {
            types,
            struct_names,
            function_names,
            struct_fields,
        }
    }

    #[cfg(test)]
    pub(super) fn standalone(types: &'a TypeTable) -> Self {
        Self {
            types,
            struct_names: HashMap::new(),
            function_names: HashMap::new(),
            struct_fields: HashMap::new(),
        }
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
    gen_co: Option<Ident>,
    ret: TypeId,
    in_try: Cell<bool>,
    continue_label: RefCell<Option<Ident>>,
    try_label: RefCell<Option<Ident>>,
    return_slot: RefCell<Option<Ident>>,
    next_try_id: Cell<u32>,
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
        Self {
            locals,
            locals_ty,
            locals_mut,
            reserved_idents,
            self_param,
            is_generator,
            gen_co,
            ret: f.ret,
            in_try: Cell::new(false),
            continue_label: RefCell::new(None),
            try_label: RefCell::new(None),
            return_slot: RefCell::new(None),
            next_try_id: Cell::new(0),
        }
    }

    pub(super) fn alloc_try_id(&self) -> u32 {
        let id = self.next_try_id.get();
        self.next_try_id.set(id + 1);
        id
    }

    pub(super) fn is_generator(&self) -> bool {
        self.is_generator
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
        self.ret
    }
}

fn place_root_local_id(place: &MirPlace) -> Option<LocalId> {
    match place {
        MirPlace::Local { id } => Some(*id),
        MirPlace::Field { base, .. } => place_base_root_local_id(base),
        MirPlace::Index { base, .. } => expr_root_local_id(base),
    }
}

fn place_base_root_local_id(base: &MirPlaceBase) -> Option<LocalId> {
    match base {
        MirPlaceBase::Local(id) => Some(*id),
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
        MirStmt::ForOf {
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
        MirStmt::While { body, .. }
        | MirStmt::DoWhile { body, .. }
        | MirStmt::ForIn { body, .. } => {
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
        MirStmt::Let { .. }
        | MirStmt::Expr(_)
        | MirStmt::Return(_)
        | MirStmt::ReturnResultErr { .. }
        | MirStmt::Throw { .. }
        | MirStmt::Break
        | MirStmt::Continue => {}
    }
}
