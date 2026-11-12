use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use ts_aot_core::{
    Atom, FieldId, FunctionId, LocalId, Span, StructId, Type, TypeId, TypeTable, Visibility,
    sanitize_rust_ident,
};
use ts_aot_ir_hir::{
    HirClass, HirDecl, HirExpr, HirFunction, HirProgram, HirStmt, HirSwitchCase,
    ObjectLiteralField, Visitor, walk_expr, walk_stmt,
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
pub(crate) fn convert_function(
    f: &HirFunction,
    id: FunctionId,
    export_name: Option<String>,
    function_remap: HashMap<FunctionId, FunctionId>,
    struct_id_map: &mut HashMap<TypeId, StructId>,
    next_struct_id: &mut u32,
    converter: &mut ExprConverter,
    types: &mut TypeTable,
    ctx: &mut PassContext,
    namespace_path: &[String],
    throws_key: &Atom,
) -> MirFunctionDecl {
    let param_ids: Vec<LocalId> = f.params.iter().map(|p| p.id).collect();
    converter.reset_for_function(function_remap, &param_ids, namespace_path);
    converter.set_return_ty(f.ret);
    for p in &f.params {
        converter.register_local_name(p.id, p.name.clone());
    }
    // The error type must be known *before* the body is converted: a call to a
    // function that may throw is only propagatable when the enclosing function
    // returns a `Result` of the same error type (`module_throws` propagates the
    // error type along the call graph for that reason).
    let throws = if f.is_generator {
        None
    } else {
        converter.ensure_throws_types(types);
        let inferred = infer_throws(&f.body, f.throws, types)
            .or_else(|| converter.function_throws(throws_key));
        if inferred.is_some() || f.throws.is_some() {
            inferred
        } else {
            // Last resort: the frontend types every indirect call as
            // `Type::Error`, so `throw g()` (or `throw c ? g() : 0`) has no
            // static type of its own. The value thrown at runtime is what the
            // expression *yields*, and `expr_type` recovers that from the
            // callee's declaration and the conditional's branches - without it
            // the function got `throws = None` and the backend emitted
            // `return Err(..)` inside a plain (`f64`) function.
            let mut resolve = |expr: &HirExpr, types: &mut TypeTable| {
                let ty = converter.expr_type(expr, types)?;
                match types.resolve(ty) {
                    // A generic parameter is not a concrete error type either.
                    None | Some(Type::Error | Type::GenericParam { .. }) => None,
                    Some(_) => Some(ty),
                }
            };
            first_throw_type(&f.body, types, &mut resolve)
        }
    };
    if let Some(expected) = throws
        && let Some((other, span)) = conflicting_throw_type(&f.body, expected, types)
    {
        ctx.error(
            "E0417",
            format!(
                "this function throws more than one type (`{}` and `{}`), but the AOT target \
                 keeps one error type per function, so the generated `Result` could not hold both; \
                 make every `throw` in it use the same type",
                throw_type_name(types, expected),
                throw_type_name(types, other),
            ),
            span,
        );
    }
    converter.set_current_throws(throws);
    let (block, locals) = converter.convert_block_with_shared_struct_ids(
        &f.body,
        struct_id_map,
        next_struct_id,
        types,
        ctx,
    );

    let params: Vec<MirParam> = build_params(&f.params);
    let can_throw = body_can_throw(&f.body);

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

pub(super) fn extend_qualified(base: &Atom, leaf: &str) -> Atom {
    let mut s = String::with_capacity(base.as_str().len() + 2 + leaf.len());
    s.push_str(base.as_str());
    s.push_str("::");
    s.push_str(leaf);
    Atom::from(s)
}

fn build_params(params: &[ts_aot_ir_hir::HirParam]) -> Vec<MirParam> {
    params
        .iter()
        .map(|p| MirParam {
            id: p.id,
            name: p.name.clone(),
            ty: p.ty,
        })
        .collect()
}

struct PreAssignState<'a> {
    pre_id: u32,
    name_to_function: HashMap<Atom, FunctionId>,
    struct_id_map: HashMap<TypeId, StructId>,
    method_map: HashMap<(StructId, Atom, bool), FunctionId>,
    method_ret: HashMap<(StructId, Atom, bool), TypeId>,
    method_param_tys: HashMap<(StructId, Atom, bool), Vec<TypeId>>,
    method_param_optional: HashMap<(StructId, Atom, bool), Vec<bool>>,
    class_struct_by_name: HashMap<Atom, StructId>,
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
    struct_id_map: HashMap<TypeId, StructId>,
    next_struct_id: u32,
    converter: ExprConverter,
    types: &'a mut TypeTable,
    ctx: &'a mut PassContext,
    converted_names: HashSet<Atom>,
    seen_sanitized_names: HashSet<String>,
    mutated_globals: HashSet<Atom>,
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
    mir.is_module = hir.is_module;
    mir.tla_main_name = hir.tla_main_name.clone();
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
        method_map: HashMap::new(),
        method_ret: HashMap::new(),
        method_param_tys: HashMap::new(),
        method_param_optional: HashMap::new(),
        class_struct_by_name: HashMap::new(),
        next_struct_id: hir.next_class_struct_id.max(1),
        seen_names: HashSet::new(),
        seen_sanitized_names: HashSet::new(),
        ctx,
    };
    pre_assign_ids_recursive(&hir.declarations, &mut pre_state, types, &[]);
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
        method_map,
        method_ret,
        method_param_tys,
        method_param_optional,
        class_struct_by_name,
        next_struct_id,
        seen_names: _,
        seen_sanitized_names: _,
        ctx,
    } = pre_state;
    let name_to_function = Arc::new(name_to_function);
    let mut converter = ExprConverter::with_function_remap_and_offset(HashMap::new(), 0);
    converter.name_to_function = Arc::clone(&name_to_function);
    converter.set_program(Arc::new(HirProgram::clone(hir)));
    converter.set_program_tables(
        field_id_lookup,
        method_map,
        method_ret,
        method_param_tys,
        method_param_optional,
        class_struct_by_name,
    );
    let mut convert_state = ConvertState {
        next_function_id: 0,
        name_to_function,
        struct_id_map,
        next_struct_id,
        converter,
        types,
        ctx,
        converted_names: HashSet::new(),
        seen_sanitized_names: HashSet::new(),
        mutated_globals: collect_mutated_globals(&hir.declarations),
    };
    convert_decls_recursive(&hir.declarations, &mut mir, &mut convert_state, &[]);
    debug_check_name_to_function(&mir, &convert_state.name_to_function);
    mir
}

struct MutatedGlobalCollector<'a> {
    names: &'a mut HashSet<Atom>,
}

impl MutatedGlobalCollector<'_> {
    fn note_target(&mut self, target: &HirExpr) {
        if let HirExpr::Global { name, .. } = target {
            self.names.insert(name.clone());
        }
    }
}

impl Visitor for MutatedGlobalCollector<'_> {
    fn visit_stmt(&mut self, stmt: &HirStmt) {
        walk_stmt(self, stmt);
    }
    fn visit_expr(&mut self, expr: &HirExpr) {
        match expr {
            HirExpr::Assignment { target, .. } | HirExpr::CompoundUpdate { target, .. } => {
                self.note_target(target);
            }
            _ => {}
        }
        walk_expr(self, expr);
    }
}

fn collect_mutated_globals(decls: &[HirDecl]) -> HashSet<Atom> {
    let mut names = HashSet::new();
    fn scan_decls(decls: &[HirDecl], names: &mut HashSet<Atom>) {
        for decl in decls {
            match decl {
                HirDecl::Function(f) => {
                    let mut v = MutatedGlobalCollector { names };
                    v.visit_block(&f.body);
                }
                HirDecl::Class(c) => {
                    for m in &c.methods {
                        let mut v = MutatedGlobalCollector { names };
                        v.visit_block(&m.body);
                    }
                }
                HirDecl::Namespace { members, .. } => scan_decls(members, names),
                _ => {}
            }
        }
    }
    scan_decls(decls, &mut names);
    names
}

fn pre_assign_ids_recursive(
    decls: &[HirDecl],
    state: &mut PreAssignState,
    types: &TypeTable,
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
                let sid = match types.resolve(c.ty) {
                    Some(Type::Struct { id }) => *id,
                    _ => {
                        let sid = StructId::from_raw(state.next_struct_id);
                        state.next_struct_id += 1;
                        sid
                    }
                };
                state.struct_id_map.insert(c.ty, sid);
                if let Some(pre_pass_ty) = c.pre_pass_ty
                    && pre_pass_ty != c.ty
                {
                    state.struct_id_map.insert(pre_pass_ty, sid);
                }
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
                state.class_struct_by_name.insert(class_key.clone(), sid);
                for method in &c.methods {
                    let id = state.alloc_function_id();
                    if class_collides {
                        continue;
                    }
                    let method_base = extend_qualified(&class_key, method.name.as_str());
                    let (method_key, method_key_sanitized) = match disambiguate_rust_ident(
                        &method_base,
                        &state.seen_names,
                        &state.seen_sanitized_names,
                        state.ctx,
                    ) {
                        Ok(pair) => pair,
                        Err(()) => continue,
                    };
                    state.seen_names.insert(method_key.clone());
                    state.seen_sanitized_names.insert(method_key_sanitized);
                    let is_static = method
                        .params
                        .first()
                        .is_none_or(|p| p.name.as_str() != "this");
                    state.name_to_function.insert(method_key, id);
                    state
                        .method_map
                        .insert((sid, method.name.clone(), is_static), id);
                    state
                        .method_ret
                        .insert((sid, method.name.clone(), is_static), method.ret);
                    state.method_param_tys.insert(
                        (sid, method.name.clone(), is_static),
                        method.params.iter().map(|p| p.ty).collect(),
                    );
                    state.method_param_optional.insert(
                        (sid, method.name.clone(), is_static),
                        method.params.iter().map(|p| p.optional).collect(),
                    );
                }
            }
            HirDecl::Namespace { name, members } => {
                let mut child_path: Vec<String> = namespace_path.to_vec();
                child_path.push(name.as_str().to_owned());
                pre_assign_ids_recursive(members, state, types, &child_path);
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
            let function_key = qualified_name(namespace_path, f.name.as_str());
            let mut mir_fn = convert_function(
                f,
                id,
                export_name,
                HashMap::new(),
                &mut state.struct_id_map,
                &mut state.next_struct_id,
                &mut state.converter,
                state.types,
                state.ctx,
                namespace_path,
                &function_key,
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
            let mir_init = init
                .as_ref()
                .and_then(|e| lower_global_init(e, state.ctx, state.types));
            Some(MirDecl::Global(MirGlobalDecl {
                name: final_mir_name,
                ty: *ty,
                mutable: state.mutated_globals.contains(name),
                visibility: Visibility::Public,
                export_name: None,
                init: mir_init,
            }))
        }
        HirDecl::Namespace { .. } => None,
    }
}
fn skip_class_method_ids(c: &HirClass, state: &mut ConvertState) {
    for _ in &c.methods {
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
        let name = method.name.as_str();
        let emitted_leaf = name
            .strip_prefix("__get_")
            .map(|prop| format!("get_{prop}"))
            .or_else(|| {
                name.strip_prefix("__set_")
                    .map(|prop| format!("set_{prop}"))
            })
            .unwrap_or_else(|| name.to_owned());
        let method_qualified = extend_qualified(&final_class_name, &emitted_leaf);
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
        // `module_throws` keys methods by their class-qualified name (`A::m`),
        // so the lookup must use exactly that key - a bare method name resolves
        // to no entry and the method would look like it never throws.
        let method_key = extend_qualified(
            &qualified_name(namespace_path, c.name.as_str()),
            method.name.as_str(),
        );
        let m = convert_function(
            method,
            id,
            export_name,
            method_remap,
            &mut state.struct_id_map,
            &mut state.next_struct_id,
            &mut state.converter,
            state.types,
            state.ctx,
            namespace_path,
            &method_key,
        );
        let mut m = m;
        m.name = final_method_name;
        let is_static = method
            .params
            .first()
            .is_none_or(|p| p.name.as_str() != "this");
        m.kind = if is_static {
            if method.is_generator {
                FunctionKind::Generator
            } else {
                FunctionKind::Plain
            }
        } else if method.is_generator {
            FunctionKind::GeneratorMethod {
                owner: sid,
                self_param,
            }
        } else if name == "constructor" {
            // A constructor mutates the freshly allocated instance, so the
            // backend takes the receiver by mutable reference (`&mut self`).
            FunctionKind::Constructor { owner: sid }
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

fn infer_throws(
    body: &[HirStmt],
    declared: Option<TypeId>,
    types: &mut TypeTable,
) -> Option<TypeId> {
    if declared.is_some() {
        declared
    } else {
        body_throws_type(body, types)
    }
}

/// A human-readable name of a type for the throw diagnostics: the internal
/// `Type` names (`F64`, `String`) must not leak into user-facing messages.
pub(super) fn throw_type_name(types: &TypeTable, ty: TypeId) -> String {
    match types.resolve(ty) {
        None | Some(Type::Error) => "an unknown type".to_owned(),
        Some(Type::Void) => "void".to_owned(),
        Some(Type::Bool) => "bool".to_owned(),
        Some(
            Type::I8
            | Type::I16
            | Type::I32
            | Type::I64
            | Type::U8
            | Type::U16
            | Type::U32
            | Type::U64
            | Type::F32
            | Type::F64,
        ) => "number".to_owned(),
        Some(Type::String) => "string".to_owned(),
        Some(Type::Null) => "null".to_owned(),
        Some(Type::Optional { inner, nullish }) => {
            let mut name = throw_type_name(types, *inner);
            if nullish.null {
                name.push_str(" | null");
            }
            if nullish.undefined {
                name.push_str(" | undefined");
            }
            name
        }
        Some(Type::Union { variants }) => variants
            .iter()
            .map(|variant| throw_type_name(types, *variant))
            .collect::<Vec<_>>()
            .join(" | "),
        Some(Type::Named { symbol }) => symbol.as_str().to_owned(),
        Some(Type::GenericParam { .. }) => "a type parameter".to_owned(),
        Some(other) => format!("{other:?}"),
    }
}

/// The first `throw` whose value has a different type than `expected`, together
/// with the span of that `throw`.
///
/// The AOT target keeps one error type per function, so a function that throws
/// both a number and a string would emit `Result<T, f64>` with a `JsString`
/// `Err` arm - code that cannot compile, reported here instead.
pub(super) fn conflicting_throw_type(
    body: &[HirStmt],
    expected: TypeId,
    types: &mut TypeTable,
) -> Option<(TypeId, Span)> {
    // An unknown expected type cannot be compared against anything: everything
    // would "conflict", which is what produced a false `E0417` for a `throw`
    // whose value is a call the frontend typed as `Type::Error`.
    if matches!(types.resolve(expected), None | Some(Type::Error)) {
        return None;
    }
    fn visit(
        s: &HirStmt,
        expected: TypeId,
        types: &mut TypeTable,
        out: &mut Option<(TypeId, Span)>,
    ) {
        if out.is_some() {
            return;
        }
        match s {
            HirStmt::Throw { expr } => {
                let ty = throw_expr_type(expr, types);
                // A rethrown caught value is dynamic (`catch (e) { throw e; }`):
                // it is not a second error type, so it does not conflict.
                if matches!(types.resolve(ty), None | Some(Type::Error)) {
                    return;
                }
                if types.resolve(ty) != types.resolve(expected) {
                    *out = Some((ty, expr.span()));
                }
            }
            HirStmt::If {
                then, otherwise, ..
            } => {
                visit(then, expected, types, out);
                if let Some(otherwise) = otherwise {
                    visit(otherwise, expected, types, out);
                }
            }
            HirStmt::While { body, .. }
            | HirStmt::DoWhile { body, .. }
            | HirStmt::ForOf { body, .. }
            | HirStmt::ForAwaitOf { body, .. }
            | HirStmt::ForIn { body, .. } => visit(body, expected, types, out),
            HirStmt::Block(stmts) => {
                for s in stmts {
                    visit(s, expected, types, out);
                }
            }
            HirStmt::Try {
                body,
                catch,
                finally,
            } => {
                // A `throw` inside the `try` body is handled by its `catch` and
                // does not escape the function. Only a `try` without a `catch`,
                // and a throwing `catch`/`finally`, escape.
                if catch.is_none() {
                    visit(body, expected, types, out);
                }
                if let Some(catch) = catch {
                    visit(&catch.body, expected, types, out);
                }
                if let Some(finally) = finally {
                    visit(finally, expected, types, out);
                }
            }
            HirStmt::Switch { cases, .. } => {
                for case in cases {
                    for s in &case.body {
                        visit(s, expected, types, out);
                    }
                }
            }
            _ => {}
        }
    }
    let mut out = None;
    for s in body {
        visit(s, expected, types, &mut out);
    }
    out
}

/// The type of a `throw` expression when it is statically known.
///
/// A call the frontend could not type (a throwing callee, an unresolved
/// generic, ...) carries `Type::Error`; taking it as "the" error type produced
/// a false `E0417` (and a `Result<T, ()>`) whenever the first `throw` of a
/// function was such a call, so an unknown type is skipped in favour of the
/// first statically known one.
fn known_throw_type(expr: &HirExpr, types: &mut TypeTable) -> Option<TypeId> {
    let ty = throw_expr_type(expr, types);
    match types.resolve(ty) {
        Some(Type::Error) => None,
        _ => Some(ty),
    }
}

/// The type of the first `throw` in `body` (declaration order, `if` branches
/// left to right) for which `resolve` yields a static type.
fn first_throw_type(
    body: &[HirStmt],
    types: &mut TypeTable,
    resolve: &mut dyn FnMut(&HirExpr, &mut TypeTable) -> Option<TypeId>,
) -> Option<TypeId> {
    fn check(
        s: &HirStmt,
        types: &mut TypeTable,
        resolve: &mut dyn FnMut(&HirExpr, &mut TypeTable) -> Option<TypeId>,
    ) -> Option<TypeId> {
        match s {
            HirStmt::Throw { expr } => resolve(expr, types),
            HirStmt::If {
                then, otherwise, ..
            } => check(then, types, resolve)
                .or_else(|| otherwise.as_deref().and_then(|s| check(s, types, resolve))),
            HirStmt::While { body, .. } | HirStmt::DoWhile { body, .. } => {
                check(body, types, resolve)
            }
            HirStmt::ForOf { body, .. }
            | HirStmt::ForAwaitOf { body, .. }
            | HirStmt::ForIn { body, .. } => check(body, types, resolve),
            HirStmt::Block(stmts) => stmts.iter().find_map(|s| check(s, types, resolve)),
            // A `throw` in the `try` body of a guarded `try` is handled by its
            // `catch`, so it is not an escaping error; `catch`/`finally` bodies
            // carry real escaping `throw`s (a `throw` inside `finally` is emitted
            // outside the protected region) and are searched - like
            // `conflicting_throw_type` does.
            HirStmt::Try {
                body,
                catch,
                finally,
            } => catch
                .is_none()
                .then(|| check(body, types, resolve))
                .flatten()
                .or_else(|| catch.as_ref().and_then(|c| check(&c.body, types, resolve)))
                .or_else(|| finally.as_deref().and_then(|f| check(f, types, resolve))),
            HirStmt::Switch { cases, .. } => cases
                .iter()
                .find_map(|c| c.body.iter().find_map(|s| check(s, types, resolve))),
            _ => None,
        }
    }
    body.iter().find_map(|s| check(s, types, resolve))
}

/// The first `throw <call>` operand of `body`.
///
/// The frontend leaves a call's HIR type as `Type::Error`, so `body_throws_type`
/// cannot name the thrown type; the throws fixpoint resolves the operand through
/// the callee's declared return type instead of skipping the function.
pub(super) fn thrown_call_operand(body: &[HirStmt]) -> Option<&HirExpr> {
    fn check(s: &HirStmt) -> Option<&HirExpr> {
        match s {
            HirStmt::Throw { expr } if matches!(expr, HirExpr::Call { .. }) => Some(expr),
            HirStmt::If {
                then, otherwise, ..
            } => check(then).or_else(|| otherwise.as_deref().and_then(check)),
            HirStmt::While { body, .. }
            | HirStmt::DoWhile { body, .. }
            | HirStmt::ForOf { body, .. }
            | HirStmt::ForAwaitOf { body, .. }
            | HirStmt::ForIn { body, .. } => check(body),
            HirStmt::Block(stmts) => stmts.iter().find_map(check),
            HirStmt::Try {
                body,
                catch,
                finally,
            } => catch
                .is_none()
                .then(|| check(body))
                .flatten()
                .or_else(|| catch.as_ref().and_then(|c| check(&c.body)))
                .or_else(|| finally.as_deref().and_then(check)),
            HirStmt::Switch { cases, .. } => {
                cases.iter().find_map(|c| c.body.iter().find_map(check))
            }
            _ => None,
        }
    }
    body.iter().find_map(check)
}

pub(super) fn body_throws_type(body: &[HirStmt], types: &mut TypeTable) -> Option<TypeId> {
    let mut resolve = known_throw_type;
    first_throw_type(body, types, &mut resolve)
}

fn throw_expr_type(expr: &HirExpr, types: &mut TypeTable) -> TypeId {
    match expr {
        // A literal has no type of its own, and the old `TypeId::from_raw(0)`
        // fallback collided with a real interned type (the first one in the
        // table), so `throw 1` inside a method was typed as the enclosing class.
        HirExpr::Int(..) => types.intern(&Type::F64),
        HirExpr::Float(..) => types.intern(&Type::F64),
        HirExpr::String(..) => types.intern(&Type::String),
        HirExpr::Bool(..) => types.intern(&Type::Bool),
        HirExpr::Null(_) | HirExpr::Unit(_) | HirExpr::Undefined(_) => types.intern(&Type::Void),
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
        _ => types.intern(&Type::Error),
    }
}

fn lower_global_init(
    init: &HirExpr,
    ctx: &mut PassContext,
    types: &mut TypeTable,
) -> Option<MirExpr> {
    let mir_init = match init {
        HirExpr::Int(v, _) => MirExpr::Int {
            value: i128::from(*v),
            ty: types.intern(&Type::I64),
        },
        HirExpr::Float(bits, _) => MirExpr::Float {
            value: f64::from_bits(*bits),
            ty: types.intern(&Type::F64),
        },
        HirExpr::Bool(b, _) => MirExpr::Bool(*b),
        HirExpr::String(id, _) => MirExpr::String {
            id: id.clone(),
            ty: types.intern(&Type::String),
        },
        HirExpr::Null(_) => MirExpr::Null {
            ty: types.intern(&Type::Null),
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
