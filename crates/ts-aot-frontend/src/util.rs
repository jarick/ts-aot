use oxc_ast::ast::{BindingPattern, Directive, FormalParameter, FormalParameters, FunctionBody};
use oxc_span::{SourceType, Span as OxcSpan};
use ts_aot_core::{
    Atom, Diagnostic, FieldId, LocalId, NullishKinds, Span as CoreSpan, Type, TypeId, TypeTable,
};
use ts_aot_ir_hir::{HirCallee, HirDecl, HirExpr, HirParam, HirProgram, HirStmt};

use crate::skeleton::{SkeletonBuilder, SkeletonEnv};
use crate::type_resolver::TypeParamMap;

pub(crate) fn binding_pattern_name(pattern: &BindingPattern<'_>) -> Option<oxc_str::CompactStr> {
    match pattern {
        BindingPattern::BindingIdentifier(id) => Some(id.name.to_compact_str()),
        BindingPattern::AssignmentPattern(ap) => binding_pattern_name(&ap.left),
        BindingPattern::ObjectPattern(_) | BindingPattern::ArrayPattern(_) => None,
    }
}

pub(crate) fn collect_binding_names(pattern: &BindingPattern<'_>, out: &mut Vec<Atom>) {
    match pattern {
        BindingPattern::BindingIdentifier(id) => out.push(Atom::from(id.name.as_str())),
        BindingPattern::ObjectPattern(obj) => {
            for prop in &obj.properties {
                collect_binding_names(&prop.value, out);
            }
            if let Some(rest) = &obj.rest {
                collect_binding_names(&rest.argument, out);
            }
        }
        BindingPattern::ArrayPattern(arr) => {
            for pattern in arr.elements.iter().flatten() {
                collect_binding_names(pattern, out);
            }
            if let Some(rest) = &arr.rest {
                collect_binding_names(&rest.argument, out);
            }
        }
        BindingPattern::AssignmentPattern(ap) => collect_binding_names(&ap.left, out),
    }
}

pub(crate) fn directives_have_use_strict(directives: &[Directive<'_>]) -> bool {
    directives
        .iter()
        .any(|d| d.directive.as_str() == "use strict")
}

pub(crate) fn body_has_use_strict(body: Option<&FunctionBody<'_>>) -> bool {
    body.is_some_and(|b| directives_have_use_strict(&b.directives))
}

pub(crate) fn validate_unique_param_bindings(
    senv: &mut SkeletonEnv,
    params: &FormalParameters<'_>,
    strict: bool,
) {
    let non_simple = params.rest.is_some()
        || params.items.iter().any(|param| {
            param.initializer.is_some()
                || !matches!(param.pattern, BindingPattern::BindingIdentifier(_))
        });
    if !strict && !non_simple {
        return;
    }
    let mut names: Vec<Atom> = Vec::new();
    for param in &params.items {
        collect_binding_names(&param.pattern, &mut names);
    }
    if let Some(rest) = &params.rest {
        collect_binding_names(&rest.rest.argument, &mut names);
    }
    let mut seen: std::collections::HashSet<Atom> = std::collections::HashSet::new();
    for name in &names {
        if !seen.insert(name.clone()) {
            senv.diagnostics.push(Diagnostic::error(
                "E0200",
                format!("parse error: duplicate parameter name `{name}`"),
                core_span_from_oxc(params.span),
            ));
        }
    }
}

impl SkeletonBuilder {
    pub(crate) fn build_params_and_locals(
        &mut self,
        senv: &mut SkeletonEnv,
        items: &[FormalParameter<'_>],
        first_param_index: u32,
        type_param_map: Option<&TypeParamMap>,
    ) -> (Vec<HirParam>, Vec<(Atom, TypeId, HirExpr)>) {
        let mut params = Vec::with_capacity(items.len());
        let mut extra_locals: Vec<(Atom, TypeId, HirExpr)> = Vec::new();
        for (offset, param) in items.iter().enumerate() {
            let ty = self.resolve_param_ty(senv, param, type_param_map);
            let index = first_param_index.saturating_add(u32::try_from(offset).unwrap_or(u32::MAX));
            let name = binding_pattern_name(&param.pattern);
            // `x: T = <default>` makes the argument optional for callers; the body
            // sees a local of type `T` that `walk_function_body` initialises with
            // `param ?? <default>` (see `declare_params_with_prelude`).
            let has_default = param.initializer.is_some() && name.is_some();
            if param.initializer.is_some() && name.is_none() {
                // A destructured parameter has no single ABI slot that could carry
                // the "was the argument supplied?" marker, so its default cannot be
                // applied: report it instead of silently dropping it. The argument
                // stays required, exactly as before.
                senv.report_unwalked(
                    "a default value on a destructured parameter is not supported by the AOT \
                     target; the default is ignored and the argument stays required",
                    param.span,
                );
            }
            let default_name = name.clone();
            if has_default {
                if param.optional {
                    senv.diagnostics.push(Diagnostic::error(
                        "E0200",
                        "parse error: a parameter cannot have both a question mark and an \
                         initializer; use `x: T = <default>` (the argument becomes optional)",
                        core_span_from_oxc(param.span),
                    ));
                }
                let abi_ty = wrap_optional_param(senv.types, ty);
                params.push(HirParam {
                    id: LocalId::from_raw(index),
                    // The parameter keeps its name (diagnostics and dumps show
                    // it); the body's references are bound to the initialised
                    // local of the same name, which shadows it.
                    name: default_name
                        .clone()
                        .map_or_else(|| Atom::from("_"), Atom::from),
                    ty: abi_ty,
                    optional: true,
                });
            } else if let Some(name) = name {
                params.push(HirParam {
                    id: LocalId::from_raw(index),
                    name: Atom::from(name),
                    ty,
                    optional: param.optional,
                });
            } else {
                params.push(HirParam {
                    id: LocalId::from_raw(index),
                    name: Atom::from("_"),
                    ty,
                    optional: param.optional,
                });
                let span = core_span_from_oxc(param.span);
                let base = HirExpr::Local {
                    id: LocalId::from_raw(index),
                    ty,
                    span,
                };
                destructure(senv, &param.pattern, &base, ty, span, &mut extra_locals);
            }
        }
        (params, extra_locals)
    }

    fn resolve_param_ty(
        &mut self,
        senv: &mut SkeletonEnv,
        param: &FormalParameter<'_>,
        type_param_map: Option<&TypeParamMap>,
    ) -> TypeId {
        if param.type_annotation.is_none() {
            let subject = match binding_pattern_name(&param.pattern) {
                Some(name) => format!("parameter `{name}`"),
                None => "parameter".to_string(),
            };
            senv.diagnostics.push(Diagnostic::error(
                "E0405",
                format!(
                    "{subject} has no type annotation; strict AOT mode requires an explicit type"
                ),
                core_span_from_oxc(param.span),
            ));
            return senv.error_ty();
        }
        let ann = param.type_annotation.as_deref();
        let ty = match type_param_map {
            Some(map) => self.resolve_ts_type_from_annotation_with_params(senv, ann, Some(map)),
            None => self.resolve_ts_type_from_annotation(senv, ann),
        };
        if param.optional {
            wrap_optional_param(senv.types, ty)
        } else {
            ty
        }
    }
}

/// The ABI type of a parameter the caller may omit: an `Option<T>` that only
/// `undefined` (`null` stays distinguishable when the annotation had it).
pub(crate) fn wrap_optional_param(types: &mut TypeTable, ty: TypeId) -> TypeId {
    // The hidden ABI parameter is `Option<T>` where the *outer* `Option` answers
    // "was the argument supplied?" and the inner value is the declared type
    // unchanged. A nullable declared type is therefore **nested**
    // (`Option<Option<_>>`) rather than flattened: flattening collapses
    // `f(null)` into "no argument", and a JavaScript default applies only to
    // `undefined` - a supplied `null` must survive.
    types.intern(&Type::Optional {
        inner: ty,
        nullish: NullishKinds::UNDEFINED_ONLY,
    })
}

fn destructure(
    senv: &mut SkeletonEnv,
    pattern: &BindingPattern<'_>,
    base: &HirExpr,
    base_ty: TypeId,
    span: CoreSpan,
    out: &mut Vec<(Atom, TypeId, HirExpr)>,
) {
    match pattern {
        BindingPattern::BindingIdentifier(id) => {
            let name = Atom::from(id.name.to_compact_str());
            if !out
                .iter()
                .any(|(existing, _, _)| existing.as_str() == name.as_str())
            {
                out.push((name, base_ty, base.clone()));
            }
        }
        BindingPattern::AssignmentPattern(ap) => {
            senv.diagnostics.push(Diagnostic::warning(
                "P0016",
                "default values in destructured parameters are not supported in this AOT \
                 target and are ignored; the default expression is not evaluated",
                core_span_from_oxc(ap.span),
            ));
            destructure(senv, &ap.left, base, base_ty, span, out);
        }
        BindingPattern::ObjectPattern(obj) => {
            for prop in &obj.properties {
                if prop.computed {
                    continue;
                }
                let Some(key) = prop.key.static_name() else {
                    continue;
                };
                let field_name = Atom::from(key.as_ref());
                let owner = match senv.types.resolve(base_ty) {
                    Some(Type::Optional { inner, .. }) => *inner,
                    _ => base_ty,
                };
                let field_ty = struct_field_ty(senv, owner, field_name.as_str())
                    .unwrap_or_else(|| senv.error_ty());
                let field = HirExpr::Field {
                    owner: Box::new(base.clone()),
                    field: FieldId::from_raw(0),
                    field_name,
                    ty: field_ty,
                    span,
                };
                destructure(senv, &prop.value, &field, field_ty, span, out);
            }
            if let Some(rest) = &obj.rest {
                declare_rest_binding(senv, &rest.argument, base, base_ty, span, out);
            }
        }
        BindingPattern::ArrayPattern(arr) => {
            let element_ty = match senv.types.resolve(base_ty) {
                Some(Type::Array { element }) => *element,
                _ => senv.error_ty(),
            };
            for (i, element) in arr.elements.iter().enumerate() {
                let Some(element) = element else {
                    continue;
                };
                let index = HirExpr::Index {
                    owner: Box::new(base.clone()),
                    index: Box::new(HirExpr::Int(i64::try_from(i).unwrap_or(i64::MAX), span)),
                    ty: element_ty,
                    span,
                };
                destructure(senv, element, &index, element_ty, span, out);
            }
            if let Some(rest) = &arr.rest {
                declare_rest_binding(senv, &rest.argument, base, base_ty, span, out);
            }
        }
    }
}

fn declare_rest_binding(
    senv: &mut SkeletonEnv,
    pattern: &BindingPattern<'_>,
    base: &HirExpr,
    base_ty: TypeId,
    span: CoreSpan,
    out: &mut Vec<(Atom, TypeId, HirExpr)>,
) {
    senv.diagnostics.push(Diagnostic::warning(
        "P0015",
        "rest elements in destructuring are not supported in this AOT target; the rest \
         binding aliases the whole destructured value (it still includes the destructured \
         keys and shares storage with the source)",
        span,
    ));
    let owner_ty = match senv.types.resolve(base_ty) {
        Some(Type::Optional { inner, .. }) => *inner,
        _ => base_ty,
    };
    destructure(senv, pattern, base, owner_ty, span, out);
}

fn struct_field_ty(senv: &SkeletonEnv, owner_ty: TypeId, name: &str) -> Option<TypeId> {
    fn walk(decls: &[HirDecl], owner_ty: TypeId, name: &str) -> Option<TypeId> {
        for decl in decls {
            match decl {
                HirDecl::Class(c) => {
                    if (c.ty == owner_ty || c.pre_pass_ty == Some(owner_ty))
                        && let Some(field) = c.fields.iter().find(|f| f.name.as_str() == name)
                    {
                        return Some(field.ty);
                    }
                }
                HirDecl::Namespace { members, .. } => {
                    if let Some(ty) = walk(members, owner_ty, name) {
                        return Some(ty);
                    }
                }
                _ => {}
            }
        }
        None
    }
    walk(&senv.program.declarations, owner_ty, name)
}
pub(crate) fn source_type_for(name: &str, module: bool) -> SourceType {
    SourceType::from_path(name).map_or_else(
        |_| {
            let mut s = SourceType::default().with_typescript(true);
            if module {
                s = s.with_module(true);
            }
            s
        },
        |s| if module { s.with_module(true) } else { s },
    )
}

pub(crate) fn core_span_from_oxc(span: OxcSpan) -> CoreSpan {
    CoreSpan::new(span.start, span.end)
}

/// What the frontend could work out about a function's return type from its
/// body alone (used when there is no return type annotation).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReturnInference {
    /// Every value-returning path yields the same, statically known type and the
    /// body cannot fall through past them.
    Inferred(TypeId),
    /// The body never returns a value, so the function returns `undefined`.
    Void,
    /// The body returns values whose type cannot be determined here (or mixes
    /// value returns with a bare/fall-through return): an explicit annotation is
    /// required.
    Unknown,
}

#[derive(Default)]
struct ReturnScan {
    returns: Vec<TypeId>,
    value_returns: bool,
    bare_return: bool,
    unknown: bool,
}

pub(crate) fn infer_function_return_ty(
    stmts: &[HirStmt],
    types: &mut TypeTable,
    program: &HirProgram,
) -> ReturnInference {
    let mut scan = ReturnScan::default();
    collect_return_tys_slice(stmts, types, program, &mut scan);
    if scan.unknown || (scan.value_returns && scan.bare_return) {
        return ReturnInference::Unknown;
    }
    if !scan.value_returns {
        return ReturnInference::Void;
    }
    if !stmts.last().is_some_and(HirStmt::is_terminal) {
        // Falling out of the function returns `undefined`, which the inferred
        // value type does not describe.
        return ReturnInference::Unknown;
    }
    let first = scan.returns[0];
    if scan.returns.iter().all(|ty| *ty == first) {
        ReturnInference::Inferred(first)
    } else {
        ReturnInference::Unknown
    }
}

fn return_expr_ty(expr: &HirExpr, types: &mut TypeTable, program: &HirProgram) -> Option<TypeId> {
    if let Some(literal) = crate::expr::literal_hir_ty(types, expr) {
        return Some(literal);
    }
    if !expr.has_static_type() {
        return None;
    }
    let ty = expr.ty();
    match types.resolve(ty) {
        None | Some(Type::Error) => {
            // The skeleton does not type every expression up front: a call or a
            // global read carries `Error` until the callee/global itself is
            // lowered. Declarations already seen by the frontend still know their
            // type, which is enough for the common `return helper()` /
            // `return someGlobal` shapes.
            let declared = match expr {
                HirExpr::Call { callee, .. } => callee_declared_ret(callee, program),
                HirExpr::Global { name, .. } => global_declared_ty(name, program),
                _ => None,
            };
            declared.and_then(|declared| match types.resolve(declared) {
                None | Some(Type::Error) => None,
                Some(_) => Some(declared),
            })
        }
        Some(_) => Some(ty),
    }
}

fn callee_declared_ret(callee: &HirCallee, program: &HirProgram) -> Option<TypeId> {
    let HirCallee::Indirect(inner) = callee else {
        return None;
    };
    let HirExpr::Global { name, .. } = inner.as_ref() else {
        return None;
    };
    program.find_function_by_name(name).map(|f| f.ret)
}

fn global_declared_ty(name: &Atom, program: &HirProgram) -> Option<TypeId> {
    fn scan(decls: &[HirDecl], name: &Atom) -> Option<TypeId> {
        for decl in decls {
            match decl {
                HirDecl::Global {
                    name: decl_name,
                    ty,
                    ..
                } if decl_name == name => {
                    return Some(*ty);
                }
                HirDecl::Namespace { members, .. } => {
                    if let Some(ty) = scan(members, name) {
                        return Some(ty);
                    }
                }
                _ => {}
            }
        }
        None
    }
    scan(&program.declarations, name)
}

fn collect_return_tys_slice(
    stmts: &[HirStmt],
    types: &mut TypeTable,
    program: &HirProgram,
    scan: &mut ReturnScan,
) {
    for stmt in stmts {
        collect_return_tys(stmt, types, program, scan);
    }
}

fn collect_return_tys(
    stmt: &HirStmt,
    types: &mut TypeTable,
    program: &HirProgram,
    scan: &mut ReturnScan,
) {
    match stmt {
        HirStmt::Return { value: Some(expr) } => match expr {
            // `return undefined` (and the synthesized placeholder for an
            // unsupported construct) is the same as a bare `return`.
            HirExpr::Undefined(_) | HirExpr::Unit(_) => scan.bare_return = true,
            HirExpr::Null(_) => {
                scan.value_returns = true;
                let null_ty = types.intern(&Type::Null);
                scan.returns.push(null_ty);
            }
            _ => {
                scan.value_returns = true;
                let Some(ty) = return_expr_ty(expr, types, program) else {
                    scan.unknown = true;
                    return;
                };
                scan.returns.push(ty);
            }
        },
        HirStmt::Return { value: None } => scan.bare_return = true,
        HirStmt::Block(stmts) => collect_return_tys_slice(stmts, types, program, scan),
        HirStmt::If {
            then, otherwise, ..
        } => {
            collect_return_tys(then, types, program, scan);
            if let Some(otherwise) = otherwise {
                collect_return_tys(otherwise, types, program, scan);
            }
        }
        HirStmt::While { body, .. }
        | HirStmt::DoWhile { body, .. }
        | HirStmt::ForOf { body, .. }
        | HirStmt::ForAwaitOf { body, .. }
        | HirStmt::ForIn { body, .. } => collect_return_tys(body, types, program, scan),
        HirStmt::Switch { cases, .. } => {
            for case in cases {
                collect_return_tys_slice(&case.body, types, program, scan);
            }
        }
        HirStmt::Try {
            body,
            catch,
            finally,
        } => {
            collect_return_tys(body, types, program, scan);
            if let Some(catch) = catch {
                collect_return_tys(&catch.body, types, program, scan);
            }
            if let Some(finally) = finally {
                collect_return_tys(finally, types, program, scan);
            }
        }
        _ => {}
    }
}
