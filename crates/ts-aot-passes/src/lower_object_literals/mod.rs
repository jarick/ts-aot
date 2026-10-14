use std::collections::{HashMap, HashSet};

use ts_aot_core::{Atom, FieldId, LocalId, Span, StructId, Type, TypeId, TypeTable};
use ts_aot_ir_hir::{
    HirCallee, HirClass, HirDecl, HirExpr, HirField, HirProgram, HirStmt, ObjectLiteralField,
    VisitorMut, walk_expr_mut, walk_stmt_mut,
};

use crate::PassContext;
use crate::monomorphize::hir_expr_ty;

pub fn lower_object_literals(
    program: &mut HirProgram,
    types: &mut TypeTable,
    _ctx: &mut PassContext,
) {
    let struct_fields = collect_class_field_order(types, &program.declarations);
    let struct_field_types = struct_fields
        .iter()
        .map(|(sid, fields)| {
            let by_name = fields
                .iter()
                .map(|(name, ty)| (name.clone(), *ty))
                .collect();
            (*sid, by_name)
        })
        .collect();
    let fn_ret = collect_fn_ret(&program.declarations);
    let fn_params = collect_fn_params(&program.declarations);
    let mut rewriter = ObjectLitRewriter {
        types,
        next_struct: program.next_class_struct_id,
        synth: Vec::new(),
        local_ty: HashMap::new(),
        struct_cache: HashMap::new(),
        struct_fields,
        struct_field_types,
        fn_ret,
        fn_params,
        current_ret_ty: None,
    };
    for decl in &mut program.declarations {
        rewriter.visit_decl(decl);
    }
    program.next_class_struct_id = rewriter.next_struct;
    program.declarations.extend(rewriter.synth);
}

fn collect_class_field_order(
    types: &TypeTable,
    decls: &[HirDecl],
) -> HashMap<StructId, Vec<(Atom, TypeId)>> {
    let mut map = HashMap::new();
    fn scan(
        types: &TypeTable,
        decls: &[HirDecl],
        map: &mut HashMap<StructId, Vec<(Atom, TypeId)>>,
    ) {
        for decl in decls {
            match decl {
                HirDecl::Class(c) => {
                    if let Some(sid) = types.struct_id(c.ty) {
                        let fields: Vec<(Atom, TypeId)> =
                            c.fields.iter().map(|f| (f.name.clone(), f.ty)).collect();
                        map.insert(sid, fields);
                    }
                }
                HirDecl::Namespace { members, .. } => scan(types, members, map),
                _ => {}
            }
        }
    }
    scan(types, decls, &mut map);
    map
}

fn collect_fn_ret(decls: &[HirDecl]) -> HashMap<Atom, TypeId> {
    let mut map = HashMap::new();
    fn scan(decls: &[HirDecl], map: &mut HashMap<Atom, TypeId>) {
        for decl in decls {
            match decl {
                HirDecl::Function(f) => {
                    map.insert(f.name.clone(), f.ret);
                }
                HirDecl::Namespace { members, .. } => scan(members, map),
                _ => {}
            }
        }
    }
    scan(decls, &mut map);
    map
}

fn collect_fn_params(decls: &[HirDecl]) -> HashMap<Atom, Vec<TypeId>> {
    let mut map = HashMap::new();
    fn scan(decls: &[HirDecl], map: &mut HashMap<Atom, Vec<TypeId>>) {
        for decl in decls {
            match decl {
                HirDecl::Function(f) => {
                    map.insert(f.name.clone(), f.params.iter().map(|p| p.ty).collect());
                }
                HirDecl::Namespace { members, .. } => scan(members, map),
                _ => {}
            }
        }
    }
    scan(decls, &mut map);
    map
}

struct ObjectLitRewriter<'a> {
    types: &'a mut TypeTable,
    next_struct: u32,
    synth: Vec<HirDecl>,
    local_ty: HashMap<LocalId, TypeId>,
    struct_cache: HashMap<Vec<(Atom, TypeId)>, TypeId>,
    struct_fields: HashMap<StructId, Vec<(Atom, TypeId)>>,
    struct_field_types: HashMap<StructId, HashMap<Atom, TypeId>>,
    fn_ret: HashMap<Atom, TypeId>,
    fn_params: HashMap<Atom, Vec<TypeId>>,
    current_ret_ty: Option<TypeId>,
}

impl ObjectLitRewriter<'_> {
    fn visit_decl(&mut self, decl: &mut HirDecl) {
        match decl {
            HirDecl::Function(f) => {
                self.local_ty.clear();
                self.current_ret_ty = Some(f.ret);
                self.visit_block_mut(&mut f.body);
                self.current_ret_ty = None;
            }
            HirDecl::Class(c) => {
                for m in &mut c.methods {
                    self.local_ty.clear();
                    self.current_ret_ty = Some(m.ret);
                    self.visit_block_mut(&mut m.body);
                    self.current_ret_ty = None;
                }
            }
            HirDecl::Global { init: Some(e), .. } => self.visit_expr_mut(e),
            HirDecl::Namespace { members, .. } => {
                for m in members {
                    self.visit_decl(m);
                }
            }
            _ => {}
        }
    }

    fn infer_value_ty(&mut self, value: &HirExpr) -> TypeId {
        if let Some(t) = hir_expr_ty(value, self.types)
            && !matches!(self.types.resolve(t), Some(Type::Error) | None)
        {
            return t;
        }
        if let HirExpr::Call {
            callee: HirCallee::Indirect(inner),
            ..
        } = value
            && let HirExpr::Global { name, .. } = inner.as_ref()
            && let Some(&ret) = self.fn_ret.get(name)
            && !matches!(self.types.resolve(ret), Some(Type::Error) | None)
        {
            return ret;
        }
        self.types.intern(&Type::Error)
    }

    fn build_struct(
        &mut self,
        fields: &[ObjectLiteralField],
        span: Span,
        target_ty: Option<TypeId>,
    ) -> Option<HirExpr> {
        if fields
            .iter()
            .any(|f| matches!(f, ObjectLiteralField::Spread(_)))
        {
            return None;
        }
        let mut lit_fields = Vec::with_capacity(fields.len());
        let mut seen = HashSet::with_capacity(fields.len());
        for field in fields.iter() {
            let ObjectLiteralField::Property { name, value } = field else {
                return None;
            };
            if !seen.insert(name.clone()) {
                return None;
            }
            lit_fields.push((name.clone(), value.clone()));
        }
        if let Some((sid, ty)) = self.resolve_target_struct(target_ty) {
            let field_order = self.struct_fields.get(&sid)?;
            if lit_fields.len() != field_order.len() {
                return None;
            }
            let mut out = Vec::with_capacity(lit_fields.len());
            for (name, value) in lit_fields {
                let index = field_order.iter().position(|(n, _)| n == name)?;
                out.push((
                    FieldId::from_raw(u32::try_from(index).unwrap_or(u32::MAX)),
                    value,
                ));
            }
            return Some(HirExpr::StructLiteral {
                ty,
                fields: out,
                span,
            });
        }
        if let Some(ty) = target_ty
            && let Some(t) = self.types.resolve(ty)
            && is_concrete_non_struct(t)
        {
            return None;
        }
        let mut hir_fields = Vec::with_capacity(lit_fields.len());
        let mut shaped: Vec<(Atom, TypeId)> = Vec::with_capacity(lit_fields.len());
        for (name, value) in &lit_fields {
            let ty = self.infer_value_ty(value);
            hir_fields.push(HirField {
                name: name.clone(),
                ty,
            });
            shaped.push((name.clone(), ty));
        }
        let ty = if let Some(&existing) = self.struct_cache.get(&shaped) {
            existing
        } else {
            let struct_id = StructId::from_raw(self.next_struct);
            self.next_struct += 1;
            let ty = self.types.intern(&Type::Struct { id: struct_id });
            let name = Atom::from(format!("__object_lit_{}", struct_id.raw()));
            self.synth.push(HirDecl::Class(HirClass {
                name,
                ty,
                pre_pass_ty: None,
                fields: hir_fields,
                methods: Vec::new(),
                extends: None,
                type_params: Vec::new(),
            }));
            self.struct_cache.insert(shaped, ty);
            ty
        };
        Some(HirExpr::StructLiteral {
            ty,
            fields: lit_fields
                .into_iter()
                .enumerate()
                .map(|(i, (_, value))| {
                    (
                        FieldId::from_raw(u32::try_from(i).unwrap_or(u32::MAX)),
                        value,
                    )
                })
                .collect(),
            span,
        })
    }

    fn resolve_target_struct(&self, target_ty: Option<TypeId>) -> Option<(StructId, TypeId)> {
        let ty = target_ty?;
        match self.types.resolve(ty) {
            Some(Type::Struct { id }) if self.struct_fields.contains_key(id) => Some((*id, ty)),
            _ => None,
        }
    }

    fn struct_field_ty(&self, sid: StructId, name: &Atom) -> Option<TypeId> {
        self.struct_field_types
            .get(&sid)
            .and_then(|fields| fields.get(name))
            .copied()
    }
}

impl VisitorMut for ObjectLitRewriter<'_> {
    fn visit_stmt_mut(&mut self, stmt: &mut HirStmt) {
        if let HirStmt::Let {
            id,
            ty,
            init: Some(init_expr),
            ..
        } = stmt
        {
            let declared_ty = *ty;
            self.visit_expr_with_expected(init_expr, Some(declared_ty));
            if matches!(self.types.resolve(*ty), Some(Type::Error) | None)
                && let Some(result_ty) = self.expr_result_ty(init_expr)
            {
                *ty = result_ty;
                self.local_ty.insert(*id, result_ty);
            }
            return;
        }
        if let HirStmt::Return { value, .. } = stmt {
            if let Some(e) = value {
                let ret = self.current_ret_ty;
                self.visit_expr_with_expected(e, ret);
            }
            return;
        }
        walk_stmt_mut(self, stmt);
    }

    fn visit_expr_mut(&mut self, expr: &mut HirExpr) {
        self.visit_expr_with_expected(expr, None);
    }
}

impl ObjectLitRewriter<'_> {
    fn expr_result_ty(&self, expr: &HirExpr) -> Option<TypeId> {
        match expr {
            HirExpr::StructLiteral { ty, .. } => Some(*ty),
            HirExpr::TypeAssertion { target, .. } => Some(*target),
            _ => None,
        }
    }

    fn visit_expr_with_expected(&mut self, expr: &mut HirExpr, expected: Option<TypeId>) {
        if let HirExpr::Closure { body, captures, .. } = expr {
            for c in captures.iter_mut() {
                self.visit_expr_mut(c);
            }
            let saved = std::mem::take(&mut self.local_ty);
            let saved_ret = self.current_ret_ty.take();
            for s in body.iter_mut() {
                self.visit_stmt_mut(s);
            }
            self.local_ty = saved;
            self.current_ret_ty = saved_ret;
            return;
        }
        if let HirExpr::Call { callee, args, .. } = expr {
            if is_array_from(callee) {
                let mut iter = args.iter_mut();
                if let Some(first) = iter.next() {
                    walk_expr_mut(self, first);
                }
                for a in iter {
                    self.visit_expr_mut(a);
                }
                return;
            }
            let param_types = match callee {
                HirCallee::Indirect(inner) => match inner.as_ref() {
                    HirExpr::Global { name, .. } => self.fn_params.get(name).cloned(),
                    _ => None,
                },
                _ => None,
            };
            if let Some(param_types) = param_types
                && param_types.len() >= args.len()
            {
                for (arg, param_ty) in args.iter_mut().zip(param_types.iter()) {
                    self.visit_expr_with_expected(arg, Some(*param_ty));
                }
                return;
            }
        }
        if let HirExpr::TypeAssertion {
            expr: inner,
            target,
            ..
        } = expr
        {
            self.visit_expr_with_expected(inner, Some(*target));
            return;
        }
        if let HirExpr::Assignment { target, value, .. } = expr {
            self.visit_expr_mut(target);
            let target_ty = target.ty();
            self.visit_expr_with_expected(value, Some(target_ty));
            return;
        }

        if let HirExpr::ObjectLiteral { fields, .. } = expr
            && let Some((sid, _)) = self.resolve_target_struct(expected)
        {
            let field_tys: Vec<Option<TypeId>> = fields
                .iter()
                .map(|field| match field {
                    ObjectLiteralField::Property { name, .. } => self.struct_field_ty(sid, name),
                    ObjectLiteralField::Spread(_) => None,
                })
                .collect();
            for (field, field_ty) in fields.iter_mut().zip(field_tys) {
                self.visit_expr_with_expected(field.value_mut(), field_ty);
            }
        } else {
            walk_expr_mut(self, expr);
        }
        if let HirExpr::Local { id, ty, .. } = expr
            && matches!(self.types.resolve(*ty), Some(Type::Error) | None)
            && let Some(&t) = self.local_ty.get(&*id)
        {
            *ty = t;
        }
        if let HirExpr::ObjectLiteral { fields, span, .. } = expr
            && let Some(new_expr) = self.build_struct(fields, *span, expected)
        {
            *expr = new_expr;
        }
    }
}

fn is_concrete_non_struct(ty: &Type) -> bool {
    !matches!(
        ty,
        Type::Struct { .. }
            | Type::Error
            | Type::GenericParam { .. }
            | Type::Named { .. }
            | Type::Optional { .. }
            | Type::Union { .. }
            | Type::Intersection { .. }
    )
}

fn is_array_from(callee: &HirCallee) -> bool {
    let HirCallee::Indirect(inner) = callee else {
        return false;
    };
    let HirExpr::Field {
        owner, field_name, ..
    } = inner.as_ref()
    else {
        return false;
    };
    matches!(owner.as_ref(), HirExpr::Global { name, .. } if name.as_str() == "Array")
        && field_name.as_str() == "from"
}
