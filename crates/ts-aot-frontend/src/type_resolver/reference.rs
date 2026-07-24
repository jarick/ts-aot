use std::collections::HashMap;

use ts_aot_core::{Diagnostic, DiagnosticBag, Type, TypeId, TypeTable};

use crate::util::core_span_from_oxc;

use super::{TypeParamMap, resolve_simple_type, type_from_ident};

pub(super) fn resolve_type_reference(
    r: &oxc_ast::ast::TSTypeReference<'_>,
    types: &mut TypeTable,
    aliases: Option<&HashMap<String, TypeId>>,
    type_params: Option<&TypeParamMap>,
    mut diagnostics: Option<&mut DiagnosticBag>,
) -> TypeId {
    match &r.type_name {
        oxc_ast::ast::TSTypeName::IdentifierReference(id) => {
            let name = id.name.as_str();
            if let Some(id) = type_params
                .and_then(|m| m.get(name).copied())
                .or_else(|| aliases.and_then(|m| m.get(name).copied()))
            {
                return id;
            }
            if let Some(id) =
                try_resolve_builtin_generic(name, r, types, aliases, type_params, &mut diagnostics)
            {
                return id;
            }
            match type_from_ident(name) {
                Some(t) => types.intern(&t),
                None => types.intern(&Type::Error),
            }
        }
        oxc_ast::ast::TSTypeName::QualifiedName(_)
        | oxc_ast::ast::TSTypeName::ThisExpression(_) => types.intern(&Type::Error),
    }
}

pub(super) fn try_resolve_builtin_generic(
    name: &str,
    r: &oxc_ast::ast::TSTypeReference<'_>,
    types: &mut TypeTable,
    aliases: Option<&HashMap<String, TypeId>>,
    type_params: Option<&TypeParamMap>,
    diagnostics: &mut Option<&mut DiagnosticBag>,
) -> Option<TypeId> {
    for builtin in BUILTIN_GENERICS {
        if builtin.name() == name
            && let Some(id) = builtin.try_resolve(r, types, aliases, type_params, diagnostics)
        {
            return Some(id);
        }
    }
    None
}

trait BuiltInGeneric {
    fn name(&self) -> &'static str;
    fn try_resolve(
        &self,
        r: &oxc_ast::ast::TSTypeReference<'_>,
        types: &mut TypeTable,
        aliases: Option<&HashMap<String, TypeId>>,
        type_params: Option<&TypeParamMap>,
        diagnostics: &mut Option<&mut DiagnosticBag>,
    ) -> Option<TypeId>;
}

struct ArrayGeneric;

impl BuiltInGeneric for ArrayGeneric {
    fn name(&self) -> &'static str {
        "Array"
    }
    fn try_resolve(
        &self,
        r: &oxc_ast::ast::TSTypeReference<'_>,
        types: &mut TypeTable,
        aliases: Option<&HashMap<String, TypeId>>,
        type_params: Option<&TypeParamMap>,
        diagnostics: &mut Option<&mut DiagnosticBag>,
    ) -> Option<TypeId> {
        let type_args = r.type_arguments.as_ref();
        if let Some(args) = type_args {
            if args.params.len() == 1 {
                let element_id = resolve_simple_type(
                    Some(&args.params[0]),
                    types,
                    aliases,
                    type_params,
                    (*diagnostics).as_deref_mut(),
                )
                .unwrap_or_else(|| types.intern(&Type::Error));
                return Some(types.intern(&Type::Array {
                    element: element_id,
                }));
            }
            if let Some(diag) = (*diagnostics).as_deref_mut() {
                diag.push(Diagnostic::warning(
                    "E0403",
                    format!(
                        "Array<T> requires exactly one type argument, got {}",
                        args.params.len()
                    ),
                    core_span_from_oxc(r.span),
                ));
            }
            Some(types.intern(&Type::Error))
        } else {
            if let Some(diag) = (*diagnostics).as_deref_mut() {
                diag.push(Diagnostic::warning(
                    "E0403",
                    "Array used without type arguments",
                    core_span_from_oxc(r.span),
                ));
            }
            Some(types.intern(&Type::Error))
        }
    }
}

const BUILTIN_GENERICS: &[&dyn BuiltInGeneric] = &[&ArrayGeneric];
