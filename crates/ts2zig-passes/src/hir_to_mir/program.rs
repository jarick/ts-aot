use std::collections::HashMap;

use ts2zig_core::{
    FieldId, FunctionId, LocalId, StringTable, StructId, SymbolId, SymbolTable, TypeId, Visibility,
};
use ts2zig_ir_hir::{HirClass, HirDecl, HirField, HirFunction, HirProgram, HirStmt};
use ts2zig_ir_mir::{
    FunctionEffects, FunctionKind, MirBody, MirDecl, MirFieldDecl, MirFunctionDecl, MirImport,
    MirParam, MirProgram, MirStructDecl,
};

use crate::PassContext;
use crate::hir_to_mir::converter::ExprConverter;

#[allow(clippy::too_many_arguments)]
pub fn convert_function(
    f: &HirFunction,
    id: FunctionId,
    export_name: Option<String>,
    function_remap: HashMap<FunctionId, FunctionId>,
    strings: &StringTable,
    symbols: &mut SymbolTable,
    struct_id_map: &mut HashMap<TypeId, StructId>,
    next_struct_id: &mut u32,
    ctx: &mut PassContext,
) -> MirFunctionDecl {
    let param_count = f.params.len();
    let mut converter =
        ExprConverter::with_function_remap_and_offset(function_remap, param_count as u32);
    converter.seed_params(param_count as u32);
    let (block, locals) =
        converter.convert_block_with_shared_struct_ids(&f.body, struct_id_map, next_struct_id, ctx);

    let params: Vec<MirParam> = build_params(&f.params, strings, symbols);

    MirFunctionDecl {
        id,
        name: f.name,
        export_name,
        params,
        ret: f.ret,
        throws: None,
        body: MirBody { locals, block },
        kind: FunctionKind::Plain,
        effects: FunctionEffects {
            can_throw: body_can_throw(&f.body),
            is_async: f.is_async,
        },
    }
}

fn body_can_throw(body: &[HirStmt]) -> bool {
    body.iter().any(stmt_can_throw)
}

fn stmt_can_throw(s: &HirStmt) -> bool {
    match s {
        HirStmt::Throw { .. } => true,
        HirStmt::Block(stmts) => body_can_throw(stmts),
        HirStmt::Let { init: Some(e), .. } => hir_expr_can_throw(e),
        HirStmt::Let { init: None, .. } => false,
        HirStmt::Expr { expr } => hir_expr_can_throw(expr),
        HirStmt::If {
            cond,
            then,
            otherwise,
        } => {
            hir_expr_can_throw(cond)
                || stmt_can_throw(then)
                || otherwise.as_ref().is_some_and(|b| stmt_can_throw(b))
        }
        HirStmt::While { cond, body } | HirStmt::DoWhile { body, cond } => {
            hir_expr_can_throw(cond) || stmt_can_throw(body)
        }
        HirStmt::ForOf { iter, body, .. } | HirStmt::ForIn { iter, body, .. } => {
            hir_expr_can_throw(iter) || stmt_can_throw(body)
        }
        HirStmt::Switch { disc, cases } => {
            hir_expr_can_throw(disc) || cases.iter().any(|c| body_can_throw(&c.body))
        }
        HirStmt::Try {
            body,
            catch,
            finally,
        } => {
            stmt_can_throw(body)
                || catch.as_ref().is_some_and(|c| stmt_can_throw(&c.body))
                || finally.as_ref().is_some_and(|f| stmt_can_throw(f))
        }
        HirStmt::Return { value: Some(e) } => hir_expr_can_throw(e),
        HirStmt::Return { value: None } => false,
        HirStmt::Break { .. } | HirStmt::Continue { .. } | HirStmt::Decl(_) => false,
    }
}

fn hir_expr_can_throw(e: &ts2zig_ir_hir::HirExpr) -> bool {
    use ts2zig_ir_hir::HirExpr;
    match e {
        HirExpr::Call { .. } | HirExpr::Await { .. } | HirExpr::New { .. } => true,
        HirExpr::Field { owner, .. } => hir_expr_can_throw(owner),
        HirExpr::Index { owner, index, .. } => {
            hir_expr_can_throw(owner) || hir_expr_can_throw(index)
        }
        HirExpr::Binary { lhs, rhs, .. } => hir_expr_can_throw(lhs) || hir_expr_can_throw(rhs),
        HirExpr::Unary { expr, .. } => hir_expr_can_throw(expr),
        HirExpr::Template { parts, .. } => parts.iter().any(hir_expr_can_throw),
        HirExpr::OptionalChain { base, .. } => hir_expr_can_throw(base),
        HirExpr::TypeAssertion { expr, .. } => hir_expr_can_throw(expr),
        HirExpr::Assignment { target, value, .. } => {
            hir_expr_can_throw(target) || hir_expr_can_throw(value)
        }
        HirExpr::ArrayLiteral { elements, .. } => elements.iter().any(hir_expr_can_throw),
        HirExpr::StructLiteral { fields, .. } => fields.iter().any(|(_, e)| hir_expr_can_throw(e)),
        HirExpr::Closure { .. } | HirExpr::Yield { .. } => true,
        _ => false,
    }
}

fn build_params(
    params: &[ts2zig_ir_hir::HirParam],
    strings: &StringTable,
    symbols: &mut SymbolTable,
) -> Vec<MirParam> {
    params
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let raw = strings.resolve(p.name).unwrap_or("");
            MirParam {
                id: LocalId::from_raw(i as u32),
                name: symbols.intern(raw),
                ty: p.ty,
            }
        })
        .collect()
}

pub fn convert_program(
    hir: &HirProgram,
    strings: &StringTable,
    symbols: &mut SymbolTable,
    ctx: &mut PassContext,
) -> MirProgram {
    let mut mir = MirProgram::new(hir.module);
    for export in &hir.exports {
        mir.exports.push(ts2zig_ir_mir::MirExport {
            symbol: export.name,
            alias: export.alias.map(|s| SymbolId::from_raw(s.raw())),
        });
    }
    for import in &hir.imports {
        mir.imports.push(MirImport {
            module: strings.resolve(import.module).unwrap_or("").to_owned(),
            symbol: import.name,
            alias: import.alias,
        });
    }
    let mut next_function_id: u32 = 0;
    let mut struct_id_map: HashMap<TypeId, StructId> = HashMap::new();
    let mut next_struct_id: u32 = 0;
    let mut class_struct_ids: HashMap<*const HirClass, StructId> = HashMap::new();
    for decl in &hir.declarations {
        if let HirDecl::Class(c) = decl {
            let sid = StructId::from_raw(next_struct_id);
            next_struct_id += 1;
            struct_id_map.insert(c.ty, sid);
            class_struct_ids.insert(decl_ptr(decl), sid);
        }
    }
    for decl in &hir.declarations {
        if let Some(mir_decl) = convert_decl(
            decl,
            &mut next_function_id,
            &mut struct_id_map,
            &mut next_struct_id,
            &class_struct_ids,
            strings,
            symbols,
            ctx,
        ) {
            mir.push_decl(mir_decl);
        }
    }
    mir
}

fn decl_ptr(decl: &HirDecl) -> *const HirClass {
    match decl {
        HirDecl::Class(c) => c as *const HirClass,
        _ => std::ptr::null(),
    }
}

#[allow(clippy::too_many_arguments)]
fn convert_decl(
    decl: &HirDecl,
    next_function_id: &mut u32,
    struct_id_map: &mut HashMap<TypeId, StructId>,
    next_struct_id: &mut u32,
    class_struct_ids: &HashMap<*const HirClass, StructId>,
    strings: &StringTable,
    symbols: &mut SymbolTable,
    ctx: &mut PassContext,
) -> Option<MirDecl> {
    match decl {
        HirDecl::Function(f) => {
            let id = FunctionId::from_raw(*next_function_id);
            *next_function_id += 1;
            let export_name = if f.is_exported {
                symbols.resolve(f.name).map(str::to_owned)
            } else {
                None
            };
            Some(MirDecl::Function(convert_function(
                f,
                id,
                export_name,
                HashMap::new(),
                strings,
                symbols,
                struct_id_map,
                next_struct_id,
                ctx,
            )))
        }
        HirDecl::Class(c) => {
            let struct_id = *class_struct_ids
                .get(&(c as *const HirClass))
                .expect("class struct_id must be pre-allocated");
            Some(convert_class(
                c,
                struct_id,
                next_function_id,
                struct_id_map,
                next_struct_id,
                strings,
                symbols,
                ctx,
            ))
        }
        HirDecl::TypeAlias { .. }
        | HirDecl::Enum { .. }
        | HirDecl::Global { .. }
        | HirDecl::Interface { .. }
        | HirDecl::Namespace { .. } => None,
    }
}

#[allow(clippy::too_many_arguments)]
fn convert_class(
    c: &HirClass,
    struct_id: StructId,
    next_function_id: &mut u32,
    struct_id_map: &mut HashMap<TypeId, StructId>,
    next_struct_id: &mut u32,
    strings: &StringTable,
    symbols: &mut SymbolTable,
    ctx: &mut PassContext,
) -> MirDecl {
    struct_id_map.insert(c.ty, struct_id);

    let fields: Vec<MirFieldDecl> = c
        .fields
        .iter()
        .enumerate()
        .map(|(i, f): (usize, &HirField)| MirFieldDecl {
            id: FieldId::from_raw(i as u32),
            name: symbols.intern(strings.resolve(f.name).unwrap_or("")),
            ty: f.ty,
            mutable: false,
            visibility: Visibility::Public,
        })
        .collect();

    let methods: Vec<MirFunctionDecl> = c
        .methods
        .iter()
        .map(|m| {
            let id = FunctionId::from_raw(*next_function_id);
            *next_function_id += 1;
            convert_function(
                m,
                id,
                None,
                HashMap::new(),
                strings,
                symbols,
                struct_id_map,
                next_struct_id,
                ctx,
            )
        })
        .collect();

    let methods: Vec<MirFunctionDecl> = methods
        .into_iter()
        .filter_map(|mut m| {
            let self_param = m.params.first().map(|p| p.id)?;
            m.kind = FunctionKind::Method {
                owner: struct_id,
                self_param,
            };
            Some(m)
        })
        .collect();

    MirDecl::Struct(MirStructDecl {
        id: struct_id,
        name: c.name,
        fields,
        methods,
    })
}
