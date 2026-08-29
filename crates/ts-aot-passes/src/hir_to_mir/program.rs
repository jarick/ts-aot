use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use ts_aot_core::{
    Atom, FieldId, FunctionId, LocalId, Span, StructId, TypeId, TypeTable, Visibility,
    sanitize_rust_ident,
};
use ts_aot_ir_hir::{
    HirClass, HirDecl, HirExpr, HirFunction, HirProgram, HirStmt, HirSwitchCase, ObjectLiteralField,
};
use ts_aot_ir_mir::{
    FunctionEffects, FunctionKind, MirBody, MirDecl, MirExpr, MirFieldDecl, MirFunctionDecl,
    MirGlobalDecl, MirImport, MirParam, MirProgram, MirStructDecl,
};

use crate::PassContext;
use crate::hir_to_mir::converter::ExprConverter;

const NAMESPACE_PATH_COLLISION_DIAG: &str = "E0503";
const RUST_IDENT_SUFFIX_OVERFLOW_DIAG: &str = "E0504";

fn report_collision(ctx: &mut PassContext, key: &Atom) {
    ctx.error(
        NAMESPACE_PATH_COLLISION_DIAG,
        format!(
            "namespace path collision: qualified name `{}` already used by another declaration; \
             rename one of the namespaces to avoid the collision",
            key.as_str()
        ),
        Span::default(),
    );
}

fn disambiguate_rust_ident(
    base: &Atom,
    converted_names: &HashSet<Atom>,
    seen_sanitized_names: &HashSet<String>,
    ctx: &mut PassContext,
) -> Result<(Atom, String), ()> {
    let base_sanitized = sanitize_rust_ident(base.as_str());
    if !seen_sanitized_names.contains(&base_sanitized) {
        return Ok((base.clone(), base_sanitized));
    }
    let mut suffix: u32 = 2;
    loop {
        let candidate_str = format!("{}_{}", base.as_str(), suffix);
        let candidate = Atom::from(candidate_str);
        let candidate_sanitized = sanitize_rust_ident(candidate.as_str());
        if !converted_names.contains(&candidate)
            && !seen_sanitized_names.contains(&candidate_sanitized)
        {
            return Ok((candidate, candidate_sanitized));
        }
        suffix = match suffix.checked_add(1) {
            Some(n) => n,
            None => {
                ctx.error(
                    RUST_IDENT_SUFFIX_OVERFLOW_DIAG,
                    format!(
                        "rust identifier collision handler exhausted u32 suffix space for `{}`; \
                         this AOT target cannot emit more than {} functions sharing the same base name",
                        base.as_str(),
                        u32::MAX
                    ),
                    Span::default(),
                );
                return Err(());
            }
        };
    }
}
pub fn convert_function(
    f: &HirFunction,
    id: FunctionId,
    export_name: Option<String>,
    function_remap: HashMap<FunctionId, FunctionId>,
    name_to_function: &Arc<HashMap<Atom, FunctionId>>,
    hir: &Arc<HirProgram>,
    struct_id_map: &mut HashMap<TypeId, StructId>,
    next_struct_id: &mut u32,
    field_id_lookup: &HashMap<(StructId, Atom), FieldId>,
    types: &mut TypeTable,
    ctx: &mut PassContext,
    namespace_path: &[String],
) -> MirFunctionDecl {
    let param_count = f.params.len();
    let mut converter =
        ExprConverter::with_function_remap_and_offset(function_remap, param_count as u32);
    converter.name_to_function = Arc::clone(name_to_function);
    converter.set_program(Arc::clone(hir));
    converter.set_field_id_lookup(field_id_lookup.clone());
    converter.set_namespace_path(namespace_path);
    converter.seed_params(param_count as u32);
    let (block, locals) = converter.convert_block_with_shared_struct_ids(
        &f.body,
        struct_id_map,
        next_struct_id,
        types,
        ctx,
    );

    let params: Vec<MirParam> = build_params(&f.params);
    let can_throw = body_can_throw(&f.body);
    let throws = infer_throws(&f.body, f.throws);

    MirFunctionDecl {
        id,
        name: f.name.clone(),
        export_name,
        params,
        ret: f.ret,
        throws,
        body: MirBody { locals, block },
        kind: if f.is_generator {
            FunctionKind::Generator
        } else {
            FunctionKind::Plain
        },
        effects: FunctionEffects {
            can_throw,
            is_async: f.is_async,
        },
    }
}

pub(crate) use ts_aot_ir_hir::qualified_name;

fn extend_qualified(base: &Atom, leaf: &str) -> Atom {
    let mut s = String::with_capacity(base.as_str().len() + 2 + leaf.len());
    s.push_str(base.as_str());
    s.push_str("::");
    s.push_str(leaf);
    Atom::from(s)
}

fn build_params(params: &[ts_aot_ir_hir::HirParam]) -> Vec<MirParam> {
    params
        .iter()
        .enumerate()
        .map(|(i, p)| MirParam {
            id: LocalId::from_raw(i as u32),
            name: p.name.clone(),
            ty: p.ty,
        })
        .collect()
}

struct PreAssignState<'a> {
    pre_id: u32,
    name_to_function: HashMap<Atom, FunctionId>,
    struct_id_map: HashMap<TypeId, StructId>,
    next_struct_id: u32,
    seen_names: HashSet<Atom>,
    seen_sanitized_names: HashSet<String>,
    ctx: &'a mut PassContext,
}

impl<'a> PreAssignState<'a> {
    fn alloc_function_id(&mut self) -> FunctionId {
        let id = FunctionId::from_raw(self.pre_id);
        self.pre_id += 1;
        id
    }
}

struct ConvertState<'a> {
    next_function_id: u32,
    name_to_function: Arc<HashMap<Atom, FunctionId>>,
    hir: Arc<HirProgram>,
    struct_id_map: HashMap<TypeId, StructId>,
    next_struct_id: u32,
    field_id_lookup: HashMap<(StructId, Atom), FieldId>,
    types: &'a mut TypeTable,
    ctx: &'a mut PassContext,
    converted_names: HashSet<Atom>,
    seen_sanitized_names: HashSet<String>,
}

impl<'a> ConvertState<'a> {
    fn alloc_function_id(&mut self) -> FunctionId {
        let id = FunctionId::from_raw(self.next_function_id);
        self.next_function_id += 1;
        id
    }

    fn skip_function_id(&mut self) {
        self.next_function_id += 1;
    }
}

pub fn convert_program(
    hir: &HirProgram,
    types: &mut TypeTable,
    ctx: &mut PassContext,
) -> MirProgram {
    let mut mir = MirProgram::new(hir.module);
    for export in &hir.exports {
        mir.exports.push(ts_aot_ir_mir::MirExport {
            symbol: export.name.clone(),
            alias: export.alias.clone(),
        });
    }
    for import in &hir.imports {
        mir.imports.push(MirImport {
            module: import.module.as_str().to_owned(),
            symbol: import.name.clone(),
            alias: import.alias.clone(),
        });
    }
    let mut pre_state = PreAssignState {
        pre_id: 0,
        name_to_function: HashMap::new(),
        struct_id_map: HashMap::new(),
        next_struct_id: 0,
        seen_names: HashSet::new(),
        seen_sanitized_names: HashSet::new(),
        ctx,
    };
    pre_assign_ids_recursive(&hir.declarations, &mut pre_state, &[]);
    let mut field_id_lookup: HashMap<(StructId, Atom), FieldId> = HashMap::new();
    collect_field_id_lookup_recursive(
        &hir.declarations,
        &pre_state.struct_id_map,
        &mut field_id_lookup,
    );
    let PreAssignState {
        pre_id: _,
        name_to_function,
        struct_id_map,
        next_struct_id,
        seen_names: _,
        seen_sanitized_names: _,
        ctx,
    } = pre_state;
    let mut convert_state = ConvertState {
        next_function_id: 0,
        name_to_function: Arc::new(name_to_function),
        hir: Arc::new(HirProgram::clone(hir)),
        struct_id_map,
        next_struct_id,
        field_id_lookup,
        types,
        ctx,
        converted_names: HashSet::new(),
        seen_sanitized_names: HashSet::new(),
    };
    convert_decls_recursive(&hir.declarations, &mut mir, &mut convert_state, &[]);
    debug_check_name_to_function(&mir, &convert_state.name_to_function);
    mir
}

fn pre_assign_ids_recursive(
    decls: &[HirDecl],
    state: &mut PreAssignState,
    namespace_path: &[String],
) {
    for decl in decls {
        match decl {
            HirDecl::Function(f) => {
                let id = state.alloc_function_id();
                let key = qualified_name(namespace_path, f.name.as_str());
                let key_sanitized = sanitize_rust_ident(key.as_str());
                if state.seen_names.contains(&key)
                    || state.seen_sanitized_names.contains(&key_sanitized)
                {
                    report_collision(state.ctx, &key);
                } else {
                    state.seen_names.insert(key.clone());
                    state.seen_sanitized_names.insert(key_sanitized);
                    state.name_to_function.insert(key, id);
                }
            }
            HirDecl::Class(c) => {
                let sid = StructId::from_raw(state.next_struct_id);
                state.next_struct_id += 1;
                state.struct_id_map.insert(c.ty, sid);
                let class_key = qualified_name(namespace_path, c.name.as_str());
                let class_key_sanitized = sanitize_rust_ident(class_key.as_str());
                let class_collides = state.seen_names.contains(&class_key)
                    || state.seen_sanitized_names.contains(&class_key_sanitized);
                if class_collides {
                    report_collision(state.ctx, &class_key);
                } else {
                    state.seen_names.insert(class_key.clone());
                    state.seen_sanitized_names.insert(class_key_sanitized);
                }
                for method in &c.methods {
                    if method.params.is_empty() {
                        continue;
                    }
                    let id = state.alloc_function_id();
                    if class_collides {
                        continue;
                    }
                    let method_key = extend_qualified(&class_key, method.name.as_str());
                    let method_key_sanitized = sanitize_rust_ident(method_key.as_str());
                    if state.seen_names.contains(&method_key)
                        || state.seen_sanitized_names.contains(&method_key_sanitized)
                    {
                        report_collision(state.ctx, &method_key);
                    } else {
                        state.seen_names.insert(method_key.clone());
                        state.seen_sanitized_names.insert(method_key_sanitized);
                        state.name_to_function.insert(method_key, id);
                    }
                }
            }
            HirDecl::Namespace { name, members } => {
                let mut child_path: Vec<String> = namespace_path.to_vec();
                child_path.push(name.as_str().to_owned());
                pre_assign_ids_recursive(members, state, &child_path);
            }
            HirDecl::TypeAlias { .. } | HirDecl::Interface { .. } | HirDecl::Enum { .. } => {}
            HirDecl::Global { name, .. } => {
                let key = qualified_name(namespace_path, name.as_str());
                let key_sanitized = sanitize_rust_ident(key.as_str());
                if state.seen_names.contains(&key)
                    || state.seen_sanitized_names.contains(&key_sanitized)
                {
                    report_collision(state.ctx, &key);
                } else {
                    state.seen_names.insert(key);
                    state.seen_sanitized_names.insert(key_sanitized);
                }
            }
        }
    }
}

fn collect_field_id_lookup_recursive(
    decls: &[HirDecl],
    struct_id_map: &HashMap<TypeId, StructId>,
    field_id_lookup: &mut HashMap<(StructId, Atom), FieldId>,
) {
    for decl in decls {
        match decl {
            HirDecl::Class(c) => {
                if let Some(&sid) = struct_id_map.get(&c.ty) {
                    for (i, f) in c.fields.iter().enumerate() {
                        field_id_lookup.insert((sid, f.name.clone()), FieldId::from_raw(i as u32));
                    }
                }
            }
            HirDecl::Namespace { members, .. } => {
                collect_field_id_lookup_recursive(members, struct_id_map, field_id_lookup);
            }
            HirDecl::Function(_)
            | HirDecl::TypeAlias { .. }
            | HirDecl::Interface { .. }
            | HirDecl::Enum { .. }
            | HirDecl::Global { .. } => {}
        }
    }
}

fn convert_decls_recursive(
    decls: &[HirDecl],
    mir: &mut MirProgram,
    state: &mut ConvertState,
    namespace_path: &[String],
) {
    for decl in decls {
        if let HirDecl::Namespace { name, members } = decl {
            let mut child_path: Vec<String> = namespace_path.to_vec();
            child_path.push(name.as_str().to_owned());
            convert_decls_recursive(members, mir, state, &child_path);
            continue;
        }
        if let Some(mir_decl) = convert_decl(decl, state, namespace_path) {
            mir.push_decl(mir_decl);
        }
    }
}

#[cfg(debug_assertions)]
fn debug_check_name_to_function(
    mir: &MirProgram,
    name_to_function: &Arc<HashMap<Atom, FunctionId>>,
) {
    use std::collections::HashMap;
    let mut actual: HashMap<&Atom, FunctionId> = HashMap::new();
    for f in mir.functions() {
        actual.insert(&f.name, f.id);
    }
    for s in mir.structs() {
        for m in &s.methods {
            actual.insert(&m.name, m.id);
        }
    }
    for (name, expected_id) in name_to_function.iter() {
        if let Some(&actual_id) = actual.get(name) {
            assert_eq!(
                actual_id, *expected_id,
                "name_to_function[{name:?}] = {expected_id:?} but MIR has id {actual_id:?}"
            );
        }
    }
}

#[cfg(not(debug_assertions))]
fn debug_check_name_to_function(
    _mir: &MirProgram,
    _name_to_function: &Arc<HashMap<Atom, FunctionId>>,
) {
}
fn convert_decl(
    decl: &HirDecl,
    state: &mut ConvertState,
    namespace_path: &[String],
) -> Option<MirDecl> {
    match decl {
        HirDecl::Function(f) => {
            let mir_name = qualified_name(namespace_path, f.name.as_str());
            if state.converted_names.contains(&mir_name) {
                state.skip_function_id();
                return None;
            }
            let (final_mir_name, final_sanitized) = match disambiguate_rust_ident(
                &mir_name,
                &state.converted_names,
                &state.seen_sanitized_names,
                state.ctx,
            ) {
                Ok(pair) => pair,
                Err(()) => {
                    state.skip_function_id();
                    return None;
                }
            };
            state.converted_names.insert(final_mir_name.clone());
            state.seen_sanitized_names.insert(final_sanitized);
            let id = state.alloc_function_id();
            let export_name = if f.is_exported {
                Some(mir_name.as_str().to_owned())
            } else {
                None
            };
            let mut mir_fn = convert_function(
                f,
                id,
                export_name,
                HashMap::new(),
                &state.name_to_function,
                &state.hir,
                &mut state.struct_id_map,
                &mut state.next_struct_id,
                &state.field_id_lookup,
                state.types,
                state.ctx,
                namespace_path,
            );
            mir_fn.name = final_mir_name;
            Some(MirDecl::Function(mir_fn))
        }
        HirDecl::Class(c) => {
            let class_name = qualified_name(namespace_path, c.name.as_str());
            if state.converted_names.contains(&class_name) {
                skip_class_method_ids(c, state);
                return None;
            }
            let (final_class_name, final_class_sanitized) = match disambiguate_rust_ident(
                &class_name,
                &state.converted_names,
                &state.seen_sanitized_names,
                state.ctx,
            ) {
                Ok(pair) => pair,
                Err(()) => {
                    skip_class_method_ids(c, state);
                    return None;
                }
            };
            state.converted_names.insert(final_class_name.clone());
            state.seen_sanitized_names.insert(final_class_sanitized);
            let mir_struct = convert_struct(c, state, namespace_path, final_class_name);
            Some(MirDecl::Struct(mir_struct))
        }
        HirDecl::TypeAlias { .. } | HirDecl::Interface { .. } => None,
        HirDecl::Enum { .. } => None,
        HirDecl::Global { name, ty, init } => {
            let mir_name = qualified_name(namespace_path, name.as_str());
            if state.converted_names.contains(&mir_name) {
                return None;
            }
            let (final_mir_name, final_sanitized) = match disambiguate_rust_ident(
                &mir_name,
                &state.converted_names,
                &state.seen_sanitized_names,
                state.ctx,
            ) {
                Ok(pair) => pair,
                Err(()) => return None,
            };
            state.converted_names.insert(final_mir_name.clone());
            state.seen_sanitized_names.insert(final_sanitized);
            let mir_init = init.as_ref().and_then(|e| lower_global_init(e, state.ctx));
            Some(MirDecl::Global(MirGlobalDecl {
                name: final_mir_name,
                ty: *ty,
                mutable: false,
                visibility: Visibility::Public,
                export_name: None,
                init: mir_init,
            }))
        }
        HirDecl::Namespace { .. } => None,
    }
}
fn skip_class_method_ids(c: &HirClass, state: &mut ConvertState) {
    for method in &c.methods {
        if method.params.is_empty() {
            continue;
        }
        state.skip_function_id();
    }
}

fn convert_struct(
    c: &HirClass,
    state: &mut ConvertState,
    namespace_path: &[String],
    final_class_name: Atom,
) -> MirStructDecl {
    let sid = state.struct_id_map[&c.ty];
    let fields: Vec<MirFieldDecl> = c
        .fields
        .iter()
        .enumerate()
        .map(|(i, f)| MirFieldDecl {
            id: FieldId::from_raw(i as u32),
            name: f.name.clone(),
            ty: f.ty,
            mutable: false,
            visibility: Visibility::Public,
        })
        .collect();
    let mut methods = Vec::new();
    for method in &c.methods {
        if method.params.is_empty() {
            continue;
        }
        let method_qualified = extend_qualified(&final_class_name, method.name.as_str());
        if state.converted_names.contains(&method_qualified) {
            state.skip_function_id();
            continue;
        }
        let (final_method_name, final_method_sanitized) = match disambiguate_rust_ident(
            &method_qualified,
            &state.converted_names,
            &state.seen_sanitized_names,
            state.ctx,
        ) {
            Ok(pair) => pair,
            Err(()) => {
                state.skip_function_id();
                continue;
            }
        };
        state.converted_names.insert(final_method_name.clone());
        state.seen_sanitized_names.insert(final_method_sanitized);
        let id = state.alloc_function_id();
        let export_name = if method.is_exported {
            Some(method_qualified.as_str().to_owned())
        } else {
            None
        };
        let mut method_remap: HashMap<FunctionId, FunctionId> = HashMap::new();
        method_remap.insert(FunctionId::from_raw(u32::MAX), id);
        let self_param = LocalId::from_raw(0);
        let m = convert_function(
            method,
            id,
            export_name,
            method_remap,
            &state.name_to_function,
            &state.hir,
            &mut state.struct_id_map,
            &mut state.next_struct_id,
            &state.field_id_lookup,
            state.types,
            state.ctx,
            namespace_path,
        );
        let mut m = m;
        m.name = final_method_name;
        m.kind = if method.is_generator {
            FunctionKind::GeneratorMethod {
                owner: sid,
                self_param,
            }
        } else {
            FunctionKind::Method {
                owner: sid,
                self_param,
            }
        };
        methods.push(m);
    }
    MirStructDecl {
        id: sid,
        name: final_class_name,
        fields,
        methods,
    }
}

fn body_can_throw(body: &[HirStmt]) -> bool {
    fn expr_can_throw(e: &HirExpr) -> bool {
        match e {
            HirExpr::Call { .. }
            | HirExpr::New { .. }
            | HirExpr::Await { .. }
            | HirExpr::Yield { .. } => true,
            HirExpr::StructLiteral { fields, .. } => fields.iter().any(|(_, e)| expr_can_throw(e)),
            HirExpr::Assignment { target, value, .. } => {
                expr_can_throw(target) || expr_can_throw(value)
            }
            HirExpr::CompoundUpdate { target, rhs, .. } => {
                expr_can_throw(target) || expr_can_throw(rhs)
            }
            HirExpr::Index { owner, index, .. } => expr_can_throw(owner) || expr_can_throw(index),
            HirExpr::Field { owner, .. } => expr_can_throw(owner),
            HirExpr::Binary { lhs, rhs, .. } => expr_can_throw(lhs) || expr_can_throw(rhs),
            HirExpr::Unary { expr, .. } => expr_can_throw(expr),
            HirExpr::Template { expressions, .. } => expressions.iter().any(expr_can_throw),
            HirExpr::ArrayLiteral { elements, .. } => elements.iter().any(expr_can_throw),
            HirExpr::ObjectLiteral { fields, .. } => fields.iter().any(|f| match f {
                ObjectLiteralField::Property { value, .. } => expr_can_throw(value),
                ObjectLiteralField::Spread(value) => expr_can_throw(value),
            }),
            HirExpr::Ternary {
                cond,
                then_branch,
                else_branch,
                ..
            } => expr_can_throw(cond) || expr_can_throw(then_branch) || expr_can_throw(else_branch),
            HirExpr::Sequence { exprs, .. } => exprs.iter().any(expr_can_throw),
            HirExpr::TypeAssertion { expr, .. } => expr_can_throw(expr),
            HirExpr::OptionalChain { base, .. } => expr_can_throw(base),
            HirExpr::Closure { captures, .. } => captures.iter().any(expr_can_throw),
            _ => false,
        }
    }
    fn switch_case_can_throw(c: &HirSwitchCase) -> bool {
        c.test.as_ref().is_some_and(expr_can_throw) || block_can_throw(&c.body)
    }
    fn block_can_throw(stmts: &[HirStmt]) -> bool {
        stmts.iter().any(stmt_can_throw)
    }
    fn stmt_can_throw(s: &HirStmt) -> bool {
        match s {
            HirStmt::Expr { expr } => expr_can_throw(expr),
            HirStmt::Throw { .. } => true,
            HirStmt::If {
                cond,
                then,
                otherwise,
            } => {
                expr_can_throw(cond)
                    || stmt_can_throw(then)
                    || otherwise.as_deref().is_some_and(stmt_can_throw)
            }
            HirStmt::While { cond, body } | HirStmt::DoWhile { body, cond } => {
                expr_can_throw(cond) || stmt_can_throw(body)
            }
            HirStmt::ForOf { iter, body, .. }
            | HirStmt::ForAwaitOf { iter, body, .. }
            | HirStmt::ForIn { iter, body, .. } => expr_can_throw(iter) || stmt_can_throw(body),
            HirStmt::Switch { disc, cases } => {
                expr_can_throw(disc) || cases.iter().any(switch_case_can_throw)
            }
            HirStmt::Try {
                body,
                catch,
                finally,
            } => {
                stmt_can_throw(body)
                    || catch.as_ref().is_some_and(|c| stmt_can_throw(&c.body))
                    || finally.as_deref().is_some_and(stmt_can_throw)
            }
            HirStmt::Block(stmts) => block_can_throw(stmts),
            HirStmt::Let {
                init: Some(expr), ..
            } => expr_can_throw(expr),
            HirStmt::Return { value: Some(expr) } => expr_can_throw(expr),
            HirStmt::Decl(_) | HirStmt::Break { .. } | HirStmt::Continue { .. } => false,
            HirStmt::Let { init: None, .. } | HirStmt::Return { value: None } => false,
        }
    }
    block_can_throw(body)
}

fn infer_throws(body: &[HirStmt], declared: Option<TypeId>) -> Option<TypeId> {
    if declared.is_some() {
        declared
    } else {
        body_throws_type(body)
    }
}

fn body_throws_type(body: &[HirStmt]) -> Option<TypeId> {
    fn check(s: &HirStmt) -> Option<TypeId> {
        match s {
            HirStmt::Throw { expr } => Some(throw_expr_type(expr)),
            HirStmt::If {
                then, otherwise, ..
            } => check(then).or_else(|| otherwise.as_deref().and_then(check)),
            HirStmt::While { body, .. } | HirStmt::DoWhile { body, .. } => check(body),
            HirStmt::ForOf { body, .. }
            | HirStmt::ForAwaitOf { body, .. }
            | HirStmt::ForIn { body, .. } => check(body),
            HirStmt::Block(stmts) => stmts.iter().find_map(check),
            HirStmt::Try { body, .. } => check(body),
            HirStmt::Switch { cases, .. } => {
                cases.iter().find_map(|c| c.body.iter().find_map(check))
            }
            _ => None,
        }
    }
    body.iter().find_map(check)
}

fn throw_expr_type(expr: &HirExpr) -> TypeId {
    match expr {
        HirExpr::Local { ty, .. }
        | HirExpr::Global { ty, .. }
        | HirExpr::Field { ty, .. }
        | HirExpr::Index { ty, .. }
        | HirExpr::Call { ty, .. }
        | HirExpr::Binary { ty, .. }
        | HirExpr::Unary { ty, .. }
        | HirExpr::StructLiteral { ty, .. }
        | HirExpr::ObjectLiteral { ty, .. }
        | HirExpr::ArrayLiteral { ty, .. }
        | HirExpr::Closure { ty, .. }
        | HirExpr::Await { ty, .. }
        | HirExpr::Yield { ty, .. }
        | HirExpr::Template { ty, .. }
        | HirExpr::New { ty, .. }
        | HirExpr::OptionalChain { ty, .. }
        | HirExpr::Assignment { ty, .. }
        | HirExpr::CompoundUpdate { ty, .. }
        | HirExpr::Ternary { ty, .. }
        | HirExpr::Sequence { ty, .. } => *ty,
        HirExpr::TypeAssertion { target, .. } => *target,
        _ => TypeId::from_raw(0),
    }
}

fn lower_global_init(init: &HirExpr, ctx: &mut PassContext) -> Option<MirExpr> {
    let mir_init = match init {
        HirExpr::Int(v, _) => MirExpr::Int {
            value: i128::from(*v),
            ty: TypeId::from_raw(0),
        },
        HirExpr::Float(bits, _) => MirExpr::Float {
            value: f64::from_bits(*bits),
            ty: TypeId::from_raw(0),
        },
        HirExpr::Bool(b, _) => MirExpr::Bool(*b),
        HirExpr::String(id, _) => MirExpr::String {
            id: id.clone(),
            ty: TypeId::from_raw(0),
        },
        HirExpr::Null(_) => MirExpr::Null {
            ty: TypeId::from_raw(0),
        },
        HirExpr::Undefined(_) | HirExpr::Unit(_) => MirExpr::Unit,
        HirExpr::Global { name, .. } => MirExpr::Global(name.clone()),
        other => {
            ctx.warning(
                "P0006",
                format!("global initializer must be a compile-time constant, got {other:?}"),
                Span::new(0, 0),
            );
            return None;
        }
    };
    Some(mir_init)
}
