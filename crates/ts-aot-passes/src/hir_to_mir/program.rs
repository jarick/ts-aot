use std::collections::HashMap;
use std::sync::Arc;

use ts_aot_core::{
    Atom, FieldId, FunctionId, LocalId, Span, StructId, TypeId, TypeTable, Visibility,
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

#[allow(clippy::too_many_arguments)]
pub fn convert_function(
    f: &HirFunction,
    id: FunctionId,
    export_name: Option<String>,
    function_remap: HashMap<FunctionId, FunctionId>,
    name_to_function: &Arc<HashMap<Atom, FunctionId>>,
    struct_id_map: &mut HashMap<TypeId, StructId>,
    next_struct_id: &mut u32,
    field_id_lookup: &HashMap<(StructId, Atom), FieldId>,
    types: &mut TypeTable,
    ctx: &mut PassContext,
) -> MirFunctionDecl {
    let param_count = f.params.len();
    let mut converter =
        ExprConverter::with_function_remap_and_offset(function_remap, param_count as u32);
    converter.name_to_function = Arc::clone(name_to_function);
    converter.set_field_id_lookup(field_id_lookup.clone());
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
        kind: FunctionKind::Plain,
        effects: FunctionEffects {
            can_throw,
            is_async: f.is_async,
        },
    }
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
    let mut next_function_id: u32 = 0;
    let mut struct_id_map: HashMap<TypeId, StructId> = HashMap::new();
    let mut next_struct_id: u32 = 0;
    let mut name_to_function: HashMap<Atom, FunctionId> = HashMap::new();
    let mut pre_id: u32 = 0;
    for decl in &hir.declarations {
        match decl {
            HirDecl::Function(f) => {
                let id = FunctionId::from_raw(pre_id);
                name_to_function.insert(f.name.clone(), id);
                pre_id += 1;
            }
            HirDecl::Class(c) => {
                let sid = StructId::from_raw(next_struct_id);
                next_struct_id += 1;
                struct_id_map.insert(c.ty, sid);
                for method in &c.methods {
                    if method.params.is_empty() {
                        continue;
                    }
                    let id = FunctionId::from_raw(pre_id);
                    name_to_function.entry(method.name.clone()).or_insert(id);
                    pre_id += 1;
                }
            }
            HirDecl::TypeAlias { .. }
            | HirDecl::Interface { .. }
            | HirDecl::Enum { .. }
            | HirDecl::Global { .. }
            | HirDecl::Namespace { .. } => {}
        }
    }
    let mut field_id_lookup: HashMap<(StructId, Atom), FieldId> = HashMap::new();
    for decl in &hir.declarations {
        if let HirDecl::Class(c) = decl
            && let Some(&sid) = struct_id_map.get(&c.ty)
        {
            for (i, f) in c.fields.iter().enumerate() {
                field_id_lookup.insert((sid, f.name.clone()), FieldId::from_raw(i as u32));
            }
        }
    }
    let name_to_function = Arc::new(name_to_function);
    for decl in &hir.declarations {
        if let Some(mir_decl) = convert_decl(
            decl,
            &mut next_function_id,
            &name_to_function,
            &mut struct_id_map,
            &mut next_struct_id,
            &field_id_lookup,
            types,
            ctx,
        ) {
            mir.push_decl(mir_decl);
        }
    }
    debug_check_name_to_function(&mir, &name_to_function);
    mir
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

#[allow(clippy::too_many_arguments)]
fn convert_decl(
    decl: &HirDecl,
    next_function_id: &mut u32,
    name_to_function: &Arc<HashMap<Atom, FunctionId>>,
    struct_id_map: &mut HashMap<TypeId, StructId>,
    next_struct_id: &mut u32,
    field_id_lookup: &HashMap<(StructId, Atom), FieldId>,
    types: &mut TypeTable,
    ctx: &mut PassContext,
) -> Option<MirDecl> {
    match decl {
        HirDecl::Function(f) => {
            let id = FunctionId::from_raw(*next_function_id);
            *next_function_id += 1;
            let export_name = if f.is_exported {
                Some(f.name.as_str().to_owned())
            } else {
                None
            };
            Some(MirDecl::Function(convert_function(
                f,
                id,
                export_name,
                HashMap::new(),
                name_to_function,
                struct_id_map,
                next_struct_id,
                field_id_lookup,
                types,
                ctx,
            )))
        }
        HirDecl::Class(c) => Some(MirDecl::Struct(convert_struct(
            c,
            next_function_id,
            name_to_function,
            struct_id_map,
            next_struct_id,
            field_id_lookup,
            types,
            ctx,
        ))),
        HirDecl::TypeAlias { .. } | HirDecl::Interface { .. } => None,
        HirDecl::Enum { .. } => None,
        HirDecl::Global { name, ty, init } => {
            let mir_init = init.as_ref().and_then(|e| lower_global_init(e, ctx));
            Some(MirDecl::Global(MirGlobalDecl {
                name: name.clone(),
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

#[allow(clippy::too_many_arguments)]
fn convert_struct(
    c: &HirClass,
    next_function_id: &mut u32,
    name_to_function: &Arc<HashMap<Atom, FunctionId>>,
    struct_id_map: &mut HashMap<TypeId, StructId>,
    next_struct_id: &mut u32,
    field_id_lookup: &HashMap<(StructId, Atom), FieldId>,
    types: &mut TypeTable,
    ctx: &mut PassContext,
) -> MirStructDecl {
    let sid = struct_id_map[&c.ty];
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
        let id = FunctionId::from_raw(*next_function_id);
        *next_function_id += 1;
        let export_name = if method.is_exported {
            Some(method.name.as_str().to_owned())
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
            name_to_function,
            struct_id_map,
            next_struct_id,
            field_id_lookup,
            types,
            ctx,
        );
        let mut m = m;
        m.kind = FunctionKind::Method {
            owner: sid,
            self_param,
        };
        methods.push(m);
    }
    MirStructDecl {
        id: sid,
        name: c.name.clone(),
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
            HirStmt::ForOf { iter, body, .. } | HirStmt::ForIn { iter, body, .. } => {
                expr_can_throw(iter) || stmt_can_throw(body)
            }
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
            HirStmt::ForOf { body, .. } | HirStmt::ForIn { body, .. } => check(body),
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
