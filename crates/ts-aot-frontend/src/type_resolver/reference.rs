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

fn resolve_single_arg_generic(
    name: &str,
    r: &oxc_ast::ast::TSTypeReference<'_>,
    types: &mut TypeTable,
    aliases: Option<&HashMap<String, TypeId>>,
    type_params: Option<&TypeParamMap>,
    diagnostics: &mut Option<&mut DiagnosticBag>,
    build: impl FnOnce(TypeId) -> Type,
) -> TypeId {
    let type_args = r.type_arguments.as_ref();
    if let Some(args) = type_args {
        if args.params.len() == 1 {
            let inner_id = resolve_simple_type(
                Some(&args.params[0]),
                types,
                aliases,
                type_params,
                (*diagnostics).as_deref_mut(),
            )
            .unwrap_or_else(|| types.intern(&Type::Error));
            return types.intern(&build(inner_id));
        }
        if let Some(diag) = (*diagnostics).as_deref_mut() {
            diag.push(Diagnostic::warning(
                "E0403",
                format!(
                    "{name}<T> requires exactly one type argument, got {}",
                    args.params.len()
                ),
                core_span_from_oxc(r.span),
            ));
        }
        types.intern(&Type::Error)
    } else {
        if let Some(diag) = (*diagnostics).as_deref_mut() {
            diag.push(Diagnostic::warning(
                "E0403",
                format!("{name} used without type arguments"),
                core_span_from_oxc(r.span),
            ));
        }
        types.intern(&Type::Error)
    }
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
        Some(resolve_single_arg_generic(
            self.name(),
            r,
            types,
            aliases,
            type_params,
            diagnostics,
            |element_id| Type::Array {
                element: element_id,
            },
        ))
    }
}

struct PromiseGeneric;

impl BuiltInGeneric for PromiseGeneric {
    fn name(&self) -> &'static str {
        "Promise"
    }
    fn try_resolve(
        &self,
        r: &oxc_ast::ast::TSTypeReference<'_>,
        types: &mut TypeTable,
        aliases: Option<&HashMap<String, TypeId>>,
        type_params: Option<&TypeParamMap>,
        diagnostics: &mut Option<&mut DiagnosticBag>,
    ) -> Option<TypeId> {
        Some(resolve_single_arg_generic(
            self.name(),
            r,
            types,
            aliases,
            type_params,
            diagnostics,
            |ok_id| Type::Promise {
                ok: ok_id,
                err: None,
            },
        ))
    }
}

struct WeakMapGeneric;

impl BuiltInGeneric for WeakMapGeneric {
    fn name(&self) -> &'static str {
        "WeakMap"
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
        match type_args {
            Some(args) if args.params.len() == 2 => {
                let key_id = resolve_simple_type(
                    Some(&args.params[0]),
                    types,
                    aliases,
                    type_params,
                    (*diagnostics).as_deref_mut(),
                )
                .unwrap_or_else(|| types.intern(&Type::Error));
                let value_id = resolve_simple_type(
                    Some(&args.params[1]),
                    types,
                    aliases,
                    type_params,
                    (*diagnostics).as_deref_mut(),
                )
                .unwrap_or_else(|| types.intern(&Type::Error));
                if !is_supported_weakmap_key_type(key_id, types) {
                    if let Some(diag) = (*diagnostics).as_deref_mut() {
                        let key_ty = types.resolve(key_id);
                        let detail = match key_ty {
                            Some(Type::Error) => "the bare `object` type is not supported as a WeakMap key until a full identity-bearing handle (an object wrapper that preserves address across function boundaries) is available; use a concrete struct type instead.".to_string(),
                            _ => format!("WeakMap keys must be concrete struct types so the compiler can emit an identity-bearing handle; got key type {key_ty:?}"),
                        };
                        diag.push(Diagnostic::warning(
                            "E0403",
                            format!("WeakMap<K, V> key must be a struct type. {detail}"),
                            core_span_from_oxc(r.span),
                        ));
                    }
                    return Some(types.intern(&Type::Error));
                }
                if !is_supported_weakmap_value_type(value_id, types) {
                    if let Some(diag) = (*diagnostics).as_deref_mut() {
                        diag.push(Diagnostic::warning(
                            "E0403",
                            format!(
                                "WeakMap<K, V> value must resolve to i64; runtime helpers take i64 by value. Got value type {:?}",
                                types.resolve(value_id)
                            ),
                            core_span_from_oxc(r.span),
                        ));
                    }
                    return Some(types.intern(&Type::Error));
                }
                Some(types.intern(&Type::WeakMap {
                    key: key_id,
                    value: value_id,
                }))
            }
            Some(args) => {
                if let Some(diag) = (*diagnostics).as_deref_mut() {
                    diag.push(Diagnostic::warning(
                        "E0403",
                        format!(
                            "WeakMap<K, V> requires exactly two type arguments, got {}",
                            args.params.len()
                        ),
                        core_span_from_oxc(r.span),
                    ));
                }
                Some(types.intern(&Type::Error))
            }
            None => {
                if let Some(diag) = (*diagnostics).as_deref_mut() {
                    diag.push(Diagnostic::warning(
                        "E0403",
                        "WeakMap<K, V> requires type arguments; bare `WeakMap` is not a valid type annotation. Use `WeakMap<K, V>` with concrete key/value types.".to_string(),
                        core_span_from_oxc(r.span),
                    ));
                }
                Some(types.intern(&Type::Error))
            }
        }
    }
}

fn is_supported_weakmap_value_type(value: TypeId, types: &TypeTable) -> bool {
    if value.raw() == 0 {
        return true;
    }
    matches!(types.resolve(value), Some(Type::I64))
}

fn is_supported_weakmap_key_type(key: TypeId, types: &TypeTable) -> bool {
    if let Some(Type::Error) = types.resolve(key) {
        return false;
    }
    if key.raw() == 0 {
        return true;
    }
    matches!(types.resolve(key), Some(Type::Struct { .. }))
}

const BUILTIN_GENERICS: &[&dyn BuiltInGeneric] = &[&ArrayGeneric, &PromiseGeneric, &WeakMapGeneric];
