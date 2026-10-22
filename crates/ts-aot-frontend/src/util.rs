use oxc_ast::ast::{BindingPattern, FormalParameter};
use oxc_span::{SourceType, Span as OxcSpan};
use ts_aot_core::{Atom, Diagnostic, FieldId, LocalId, Span as CoreSpan, Type, TypeId};
use ts_aot_ir_hir::{HirDecl, HirExpr, HirParam};

use crate::skeleton::{SkeletonBuilder, SkeletonEnv};
use crate::type_resolver::TypeParamMap;

pub(crate) fn binding_pattern_name(pattern: &BindingPattern<'_>) -> Option<oxc_str::CompactStr> {
    match pattern {
        BindingPattern::BindingIdentifier(id) => Some(id.name.to_compact_str()),
        BindingPattern::AssignmentPattern(ap) => binding_pattern_name(&ap.left),
        BindingPattern::ObjectPattern(_) | BindingPattern::ArrayPattern(_) => None,
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
            if param.initializer.is_some() {
                senv.diagnostics.push(Diagnostic::warning(
                    "P0017",
                    "default values on parameters are not supported in this AOT target and \
                     are ignored; the default expression is not evaluated",
                    core_span_from_oxc(param.span),
                ));
            }
            if let Some(name) = binding_pattern_name(&param.pattern) {
                params.push(HirParam {
                    name: Atom::from(name),
                    ty,
                });
            } else {
                params.push(HirParam {
                    name: Atom::from("_"),
                    ty,
                });
                let span = core_span_from_oxc(param.span);
                let index = first_param_index + u32::try_from(offset).unwrap_or(u32::MAX);
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
        let ann = param.type_annotation.as_deref();
        match type_param_map {
            Some(map) => self.resolve_ts_type_from_annotation_with_params(senv, ann, Some(map)),
            None => self.resolve_ts_type_from_annotation(senv, ann),
        }
    }
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
                    Some(Type::Optional { inner }) => *inner,
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
        Some(Type::Optional { inner }) => *inner,
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
