use std::collections::HashMap;

use oxc_ast::ast::{TSTupleElement, TSType, TSTypeName};
use oxc_span::GetSpan;
use ts_aot_core::{Diagnostic, DiagnosticBag, Type, TypeId, TypeTable};

use crate::util::core_span_from_oxc;

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
    ty: Option<&oxc_ast::ast::TSType<'_>>,
    types: &mut TypeTable,
    aliases: Option<&HashMap<String, TypeId>>,
    type_params: Option<&TypeParamMap>,
    mut diagnostics: Option<&mut DiagnosticBag>,
) -> Option<TypeId> {
    Some(match ty? {
        TSType::TSNeverKeyword(_) => types.intern(&Type::Never),
        TSType::TSNumberKeyword(_) => types.intern(&Type::I32),
        TSType::TSStringKeyword(_) => types.intern(&Type::String),
        TSType::TSBooleanKeyword(_) => types.intern(&Type::Bool),
        TSType::TSVoidKeyword(_) | TSType::TSUndefinedKeyword(_) => types.intern(&Type::Void),
        TSType::TSNullKeyword(_) => types.intern(&Type::Null),
        TSType::TSTypeReference(r) => match &r.type_name {
            TSTypeName::IdentifierReference(id) => {
                let name = id.name.as_str();
                type_params
                    .and_then(|m| m.get(name).copied())
                    .or_else(|| aliases.and_then(|m| m.get(name).copied()))
                    .unwrap_or_else(|| match type_from_ident(name) {
                        Some(t) => types.intern(&t),
                        None => types.intern(&Type::Error),
                    })
            }
            TSTypeName::QualifiedName(_) | TSTypeName::ThisExpression(_) => {
                types.intern(&Type::Error)
            }
        },
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
            types.intern(&Type::Error)
        }
        TSType::TSUnionType(u) => {
            let mut variants: Vec<TypeId> = Vec::with_capacity(u.types.len());
            for variant in &u.types {
                let id = resolve_simple_type(
                    Some(variant),
                    types,
                    aliases,
                    type_params,
                    diagnostics.as_deref_mut(),
                )
                .unwrap_or_else(|| types.intern(&Type::Error));
                variants.push(id);
            }
            types.intern(&Type::Union { variants })
        }
        TSType::TSIntersectionType(i) => {
            let mut parts: Vec<TypeId> = Vec::with_capacity(i.types.len());
            for part in &i.types {
                let id = resolve_simple_type(
                    Some(part),
                    types,
                    aliases,
                    type_params,
                    diagnostics.as_deref_mut(),
                )
                .unwrap_or_else(|| types.intern(&Type::Error));
                parts.push(id);
            }
            parts.sort_unstable_by_key(|id| id.raw());
            parts.dedup();
            types.intern(&Type::Intersection { parts })
        }
        TSType::TSTupleType(t) => {
            let mut elements: Vec<TypeId> = Vec::with_capacity(t.element_types.len());
            for element in &t.element_types {
                if let Some(ty) = element.as_ts_type() {
                    let id = resolve_simple_type(
                        Some(ty),
                        types,
                        aliases,
                        type_params,
                        diagnostics.as_deref_mut(),
                    )
                    .unwrap_or_else(|| types.intern(&Type::Error));
                    elements.push(id);
                } else {
                    report_unsupported_tuple_element(element, diagnostics.as_deref_mut());
                    elements.push(types.intern(&Type::Error));
                }
            }
            types.intern(&Type::Tuple { elements })
        }
        _ => types.intern(&Type::Error),
    })
}

fn report_unsupported_tuple_element(
    element: &TSTupleElement<'_>,
    diagnostics: Option<&mut DiagnosticBag>,
) {
    if let Some(diag) = diagnostics {
        let (code, message) = match element {
            TSTupleElement::TSRestType(_) => (
                "E0402",
                "rest tuple element `...T` is not supported in Phase 4".to_owned(),
            ),
            TSTupleElement::TSOptionalType(_) => (
                "E0402",
                "optional tuple element `T?` is not supported in Phase 4".to_owned(),
            ),
            _ => (
                "E0402",
                "extended tuple element is not supported in Phase 4".to_owned(),
            ),
        };
        diag.push(Diagnostic::warning(
            code,
            message,
            core_span_from_oxc(element.span()),
        ));
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
