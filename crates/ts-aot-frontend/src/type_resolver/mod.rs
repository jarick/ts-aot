use std::collections::HashMap;

use oxc_ast::ast::TSType;
use ts_aot_core::{Diagnostic, DiagnosticBag, Type, TypeId, TypeTable};

use crate::util::core_span_from_oxc;

mod aggregate;
mod function;
mod reference;

pub(crate) struct TypeParamMap {
    tys: HashMap<String, TypeId>,
}

impl TypeParamMap {
    #[must_use]
    pub fn new() -> Self {
        Self {
            tys: HashMap::new(),
        }
    }

    pub fn bind(&mut self, name: impl Into<String>, ty: TypeId) {
        self.tys.insert(name.into(), ty);
    }

    #[must_use]
    pub fn get(&self, name: &str) -> Option<&TypeId> {
        self.tys.get(name)
    }

    #[must_use]
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.tys.is_empty()
    }

    pub fn iter_bindings(&self) -> impl Iterator<Item = (&str, TypeId)> + '_ {
        self.tys.iter().map(|(k, v)| (k.as_str(), *v))
    }
}

impl Default for TypeParamMap {
    fn default() -> Self {
        Self::new()
    }
}

#[must_use]
pub(crate) fn type_from_ident(s: &str) -> Option<Type> {
    match s {
        "i8" => Some(Type::I8),
        "i16" => Some(Type::I16),
        "i32" | "number" => Some(Type::I32),
        "i64" => Some(Type::I64),
        "u8" => Some(Type::U8),
        "u16" => Some(Type::U16),
        "u32" => Some(Type::U32),
        "u64" => Some(Type::U64),
        "f32" => Some(Type::F32),
        "f64" => Some(Type::F64),
        "string" => Some(Type::String),
        "boolean" | "bool" => Some(Type::Bool),
        "void" | "undefined" => Some(Type::Void),
        "null" => Some(Type::Null),
        "never" => Some(Type::Never),
        _ => None,
    }
}

#[must_use]
pub(crate) fn resolve_simple_type(
    ty: Option<&TSType<'_>>,
    types: &mut TypeTable,
    aliases: Option<&HashMap<String, TypeId>>,
    type_params: Option<&TypeParamMap>,
    mut diagnostics: Option<&mut DiagnosticBag>,
) -> Option<TypeId> {
    match ty? {
        TSType::TSNeverKeyword(_) => Some(types.intern(&Type::Never)),
        TSType::TSNumberKeyword(_) => Some(types.intern(&Type::I32)),
        TSType::TSStringKeyword(_) => Some(types.intern(&Type::String)),
        TSType::TSBooleanKeyword(_) => Some(types.intern(&Type::Bool)),
        TSType::TSVoidKeyword(_) | TSType::TSUndefinedKeyword(_) => Some(types.intern(&Type::Void)),
        TSType::TSNullKeyword(_) => Some(types.intern(&Type::Null)),
        TSType::TSTypeReference(r) => Some(reference::resolve_type_reference(
            r,
            types,
            aliases,
            type_params,
            diagnostics,
        )),
        TSType::TSNamedTupleMember(m) => {
            if let Some(diag) = diagnostics.as_deref_mut() {
                diag.push(Diagnostic::warning(
                    "E0402",
                    format!(
                        "named tuple element `{}:` is not supported in Phase 4 — use `[T]` instead",
                        m.label.name
                    ),
                    core_span_from_oxc(m.span),
                ));
            }
            Some(types.intern(&Type::Error))
        }
        TSType::TSUnionType(u) => Some(aggregate::resolve_union(
            &u.types,
            types,
            aliases,
            type_params,
            &mut diagnostics,
        )),
        TSType::TSIntersectionType(i) => Some(aggregate::resolve_intersection(
            &i.types,
            types,
            aliases,
            type_params,
            &mut diagnostics,
        )),
        TSType::TSTupleType(t) => Some(aggregate::resolve_tuple(
            &t.element_types,
            types,
            aliases,
            type_params,
            &mut diagnostics,
        )),
        TSType::TSArrayType(a) => Some(aggregate::resolve_array(
            &a.element_type,
            types,
            aliases,
            type_params,
            &mut diagnostics,
        )),
        TSType::TSFunctionType(f) => {
            function::resolve_function(f, types, aliases, type_params, &mut diagnostics)
        }
        _ => Some(types.intern(&Type::Error)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn type_from_ident_maps_primitives() {
        assert_eq!(type_from_ident("i32"), Some(Type::I32));
        assert_eq!(type_from_ident("string"), Some(Type::String));
        assert_eq!(type_from_ident("bool"), Some(Type::Bool));
        assert_eq!(type_from_ident("boolean"), Some(Type::Bool));
        assert_eq!(type_from_ident("void"), Some(Type::Void));
        assert_eq!(type_from_ident("f64"), Some(Type::F64));
    }

    #[test]
    fn type_from_ident_unknown_returns_none() {
        assert_eq!(type_from_ident("Promise"), None);
        assert_eq!(type_from_ident("MyClass"), None);
        assert_eq!(type_from_ident(""), None);
    }

    #[test]
    fn type_param_map_binds_and_resolves() {
        let mut m = TypeParamMap::new();
        let ty = TypeId::from_raw(7);
        m.bind("T", ty);
        assert_eq!(m.get("T"), Some(&ty));
        assert_eq!(m.get("U"), None);
        assert!(!m.is_empty());
    }

    #[test]
    fn type_param_map_empty_by_default() {
        let m = TypeParamMap::default();
        assert!(m.is_empty());
        assert_eq!(m.get("anything"), None);
    }

    #[test]
    fn resolve_simple_type_returns_none_for_none_input() {
        let mut types = TypeTable::new();
        let result = resolve_simple_type(None, &mut types, None, None, None);
        assert!(result.is_none());
        assert!(types.is_empty());
    }
}
