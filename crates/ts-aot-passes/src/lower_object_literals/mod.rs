use std::collections::{HashMap, HashSet};

use ts_aot_core::{
    Atom, FieldId, LocalId, Span, StructId, Type, TypeId, TypeTable, sanitize_rust_ident,
};
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
    let mut used_names = HashSet::new();
    let mut used_sanitized_names = HashSet::new();
    collect_decl_names(
        &program.declarations,
        &mut used_names,
        &mut used_sanitized_names,
    );
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
        global_ty: HashMap::new(),
        used_names,
        used_sanitized_names,
    };
    let tla_main_name = program.tla_main_name.clone();
    let tla_main_index = program.declarations.iter().position(
        |decl| matches!(decl, HirDecl::Function(f) if Some(&f.name) == tla_main_name.as_ref()),
    );
    if let Some(index) = tla_main_index {
        rewriter.visit_decl(&mut program.declarations[index]);
    }
    for (index, decl) in program.declarations.iter_mut().enumerate() {
        if Some(index) == tla_main_index {
            continue;
        }
        rewriter.visit_decl(decl);
    }
    let global_ty = std::mem::take(&mut rewriter.global_ty);
    for decl in &mut program.declarations {
        if let HirDecl::Global { name, ty, .. } = decl
            && matches!(rewriter.types.resolve(*ty), Some(Type::Error) | None)
            && let Some(&inferred) = global_ty.get(name)
        {
            *ty = inferred;
        }
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

fn collect_decl_names(decls: &[HirDecl], raw: &mut HashSet<Atom>, sanitized: &mut HashSet<String>) {
    for decl in decls {
        let name = match decl {
            HirDecl::Function(f) => &f.name,
            HirDecl::Class(c) => &c.name,
            HirDecl::TypeAlias { name, .. }
            | HirDecl::Enum { name, .. }
            | HirDecl::Global { name, .. }
            | HirDecl::Interface { name, .. } => name,
            HirDecl::Namespace { members, .. } => {
                collect_decl_names(members, raw, sanitized);
                continue;
            }
        };
        raw.insert(name.clone());
        sanitized.insert(sanitize_rust_ident(name.as_str()));
    }
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
    global_ty: HashMap<Atom, TypeId>,
    used_names: HashSet<Atom>,
    used_sanitized_names: HashSet<String>,
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

    fn fresh_synth_name(&mut self, base: &str) -> Atom {
        let mut suffix = 0u32;
        loop {
            let candidate = if suffix == 0 {
                base.to_owned()
            } else {
                format!("{base}_{suffix}")
            };
            let sanitized = sanitize_rust_ident(&candidate);
            let atom = Atom::from(candidate.as_str());
            if !self.used_names.contains(&atom) && !self.used_sanitized_names.contains(&sanitized) {
                self.used_names.insert(atom.clone());
                self.used_sanitized_names.insert(sanitized);
                return atom;
            }
            suffix += 1;
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
            let name = self.fresh_synth_name(&format!("ObjectLit{}", struct_id.raw()));
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
            if let HirExpr::Global { name, .. } = target.as_ref() {
                let name = name.clone();
                if let Some(result_ty) = self.expr_result_ty(value) {
                    self.global_ty.entry(name).or_insert(result_ty);
                }
            }
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

#[cfg(test)]
mod tests {
    use super::*;
    use ts_aot_core::ModuleId;
    use ts_aot_ir_hir::HirFunction;

    #[test]
    fn synthetic_object_lit_name_avoids_user_declaration_collision() {
        assert_synthetic_name_avoids_collision(0);
    }

    #[test]
    fn synthetic_object_lit_name_avoids_namespace_member_collision() {
        assert_synthetic_name_avoids_collision(1);
    }

    #[test]
    fn synthetic_object_lit_name_avoids_nested_namespace_member_collision() {
        assert_synthetic_name_avoids_collision(2);
    }

    fn assert_synthetic_name_avoids_collision(namespace_depth: usize) {
        let mut types = TypeTable::new();
        let mut program = HirProgram::new(ModuleId::from_raw(0));
        let user_ty = types.intern(&Type::Struct {
            id: StructId::from_raw(100),
        });
        let error_ty = types.intern(&Type::Error);
        let mut user_decl = HirDecl::Class(HirClass {
            name: Atom::from("ObjectLit0"),
            ty: user_ty,
            pre_pass_ty: None,
            fields: Vec::new(),
            methods: Vec::new(),
            extends: None,
            type_params: Vec::new(),
        });
        for _ in 0..namespace_depth {
            user_decl = HirDecl::Namespace {
                name: Atom::from("N"),
                members: vec![user_decl],
            };
        }
        program.push_decl(user_decl.clone());
        let lit = HirExpr::ObjectLiteral {
            fields: vec![ObjectLiteralField::Property {
                name: Atom::from("a"),
                value: HirExpr::Int(1, Span::default()),
            }],
            ty: error_ty,
            span: Span::default(),
        };
        program.push_decl(HirDecl::Function(HirFunction {
            name: Atom::from("f"),
            ret: error_ty,
            body: vec![HirStmt::Return { value: Some(lit) }],
            ..HirFunction::default()
        }));
        program.next_class_struct_id = 0;

        let mut cx = PassContext::new();
        lower_object_literals(&mut program, &mut types, &mut cx);

        assert_eq!(program.declarations[0], user_decl);
        let class_names: Vec<String> = program
            .declarations
            .iter()
            .filter_map(|d| match d {
                HirDecl::Class(c) => Some(c.name.as_str().to_string()),
                _ => None,
            })
            .collect();
        assert_eq!(
            class_names
                .iter()
                .filter(|n| n.as_str() == "ObjectLit0")
                .count(),
            usize::from(namespace_depth == 0),
            "the user-declared `ObjectLit0` must remain the only declaration with that name; got \
             {class_names:?}"
        );
        assert!(
            class_names.iter().any(|n| n == "ObjectLit0_1"),
            "the synthetic object-literal class must sidestep the user `ObjectLit0` collision by \
             picking `ObjectLit0_1`; got {class_names:?}"
        );
    }

    #[test]
    fn synthetic_object_lit_name_probes_suffixes_past_multiple_collisions() {
        let mut types = TypeTable::new();
        let mut program = HirProgram::new(ModuleId::from_raw(0));
        let error_ty = types.intern(&Type::Error);
        for (i, name) in ["ObjectLit0", "ObjectLit0_1"].iter().enumerate() {
            let ty = types.intern(&Type::Struct {
                id: StructId::from_raw(200 + i as u32),
            });
            program.push_decl(HirDecl::Class(HirClass {
                name: Atom::from(*name),
                ty,
                pre_pass_ty: None,
                fields: Vec::new(),
                methods: Vec::new(),
                extends: None,
                type_params: Vec::new(),
            }));
        }
        let lit = HirExpr::ObjectLiteral {
            fields: vec![ObjectLiteralField::Property {
                name: Atom::from("a"),
                value: HirExpr::Int(1, Span::default()),
            }],
            ty: error_ty,
            span: Span::default(),
        };
        program.push_decl(HirDecl::Function(HirFunction {
            name: Atom::from("f"),
            ret: error_ty,
            body: vec![HirStmt::Return { value: Some(lit) }],
            ..HirFunction::default()
        }));
        program.next_class_struct_id = 0;

        let mut cx = PassContext::new();
        lower_object_literals(&mut program, &mut types, &mut cx);

        let class_names: Vec<String> = program
            .declarations
            .iter()
            .filter_map(|d| match d {
                HirDecl::Class(c) => Some(c.name.as_str().to_string()),
                _ => None,
            })
            .collect();
        assert!(
            class_names.iter().any(|n| n == "ObjectLit0_2"),
            "the synthetic class must probe past both `ObjectLit0` and `ObjectLit0_1` to \
             `ObjectLit0_2`; got {class_names:?}"
        );
    }
}
