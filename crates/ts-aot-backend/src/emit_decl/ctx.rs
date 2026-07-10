use std::collections::HashMap;

use proc_macro2::{Ident, TokenStream};
use quote::{format_ident, quote};

use ts_aot_core::{FieldId, FunctionId, LocalId, StructId, TypeId, TypeTable};
use ts_aot_ir_mir::{FunctionKind, MirDecl, MirFunctionDecl, MirProgram};

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
    self_param: Option<LocalId>,
}

impl BodyCtx {
    pub(super) fn new(f: &MirFunctionDecl) -> Self {
        let self_param = match f.kind {
            FunctionKind::Method { self_param, .. } => Some(self_param),
            _ => None,
        };
        let mut locals = HashMap::new();
        let mut locals_ty = HashMap::new();
        for param in &f.params {
            if Some(param.id) != self_param {
                locals.insert(param.id, ident_from(&param.name));
            }
            locals_ty.insert(param.id, param.ty);
        }
        for local in &f.body.locals {
            locals.insert(local.id, ident_from(&local.name));
            locals_ty.insert(local.id, local.ty);
        }
        Self {
            locals,
            locals_ty,
            self_param,
        }
    }

    pub(super) fn local_ident(&self, id: LocalId) -> Ident {
        self.locals
            .get(&id)
            .cloned()
            .unwrap_or_else(|| format_ident!("__local{}", id.raw()))
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
}
