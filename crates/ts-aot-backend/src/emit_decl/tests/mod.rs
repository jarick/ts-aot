use super::body::self_capture_may_escape;
use super::ctx::{BodyCtx, EmitEnv};
use super::emit_decls;
use ts_aot_core::{Atom, FunctionId, LocalId, ModuleId, Type, TypeId, TypeTable};
use ts_aot_ir_mir::{
    BinaryOp, FunctionEffects, FunctionKind, MirBlock, MirBody, MirDecl, MirExpr, MirFunctionDecl,
    MirLocalDecl, MirParam, MirProgram, MirStmt, RuntimeOp,
};

mod arena_crate;
mod call;
mod call_order;
mod coerce;
mod dispatch;
mod dispatch_types;
mod equality;
mod equality_fold;
mod function;
mod globals;
mod globals_string;
mod literal;
mod method_self;
mod numbers;
mod object_is;
mod param_mut;
mod place;
mod place_optional;
mod promise;
mod runtime_array;
mod stmt_async_generator;
mod stmt_async_generator_await;
mod stmt_catch;
mod stmt_control;
mod stmt_try;
mod stmt_try_nested;
mod stmt_try_return;
mod stmt_yield;
mod struct_decl;
mod struct_generic;
mod taint;
mod taint_closure;
mod taint_scan;
mod types;

pub(super) fn empty_func(name: &str) -> MirFunctionDecl {
    MirFunctionDecl {
        id: FunctionId::from_raw(0),
        name: Atom::from(name),
        export_name: None,
        params: Vec::new(),
        ret: TypeId::from_raw(0),
        throws: None,
        body: MirBody::default(),
        kind: FunctionKind::Plain,
        effects: FunctionEffects::default(),
    }
}

pub(super) fn signature_fragment<'a>(s: &'a str, fn_name: &str) -> &'a str {
    let after_fn = s.split(&format!("fn {fn_name}")).nth(1).unwrap_or("");
    let after_generics = if let Some(rest) = after_fn.trim_start().strip_prefix('<') {
        rest.split_once('>').map_or(rest, |(_, r)| r)
    } else {
        after_fn
    };
    let trimmed = after_generics.trim_start();
    let after_paren = trimmed.strip_prefix('(').unwrap_or(trimmed);
    after_paren.split('{').next().unwrap_or("")
}

pub(super) fn for_header_fragment<'a>(s: &'a str, fn_name: &str) -> &'a str {
    let after_fn = s.split(&format!("fn {fn_name}")).nth(1).unwrap_or("");
    after_fn
        .split(" for ")
        .nth(1)
        .and_then(|rest| rest.split('{').next())
        .unwrap_or("")
}

pub(super) fn extract_dispatch_table(s: &str) -> String {
    let after_table = s.find("__TS_AOT_DISPATCH_TABLE").unwrap_or(0);
    let from = s[after_table..]
        .find('&')
        .map_or(after_table, |i| after_table + i);
    let to = s[from..].find(';').map_or(s.len(), |i| from + i);
    s[from..to].to_string()
}

pub(super) fn optional_binary_func(
    name: &str,
    param_ty: TypeId,
    ret_ty: TypeId,
) -> MirFunctionDecl {
    let mut f = empty_func(name);
    f.params = vec![MirParam {
        id: LocalId::from_raw(0),
        name: Atom::from("x"),
        ty: param_ty,
    }];
    f.ret = ret_ty;
    f
}

pub(super) fn opt_eq_body(op: BinaryOp, left: MirExpr, right: MirExpr, bool_ty: TypeId) -> MirBody {
    MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Return(Some(MirExpr::Binary {
                op,
                left: Box::new(left),
                right: Box::new(right),
                ty: bool_ty,
            }))],
        },
    }
}

pub(super) fn norm(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace()).collect()
}

pub(super) fn emit_object_is(args: Vec<MirExpr>) -> String {
    let mut types = TypeTable::new();
    let bool_ty = types.intern(&Type::Bool);
    let temp = LocalId::from_raw(0);
    let mut f = empty_func("is");
    f.ret = bool_ty;
    f.body = MirBody {
        locals: vec![MirLocalDecl {
            id: temp,
            name: Atom::from(""),
            ty: bool_ty,
            mutable: false,
            narrowed: false,
        }],
        block: MirBlock {
            stmts: vec![MirStmt::Runtime {
                op: RuntimeOp::ObjectIs,
                args,
                dest: Some(temp),
                ty: bool_ty,
                target_ty: None,
            }],
        },
    };
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(f));
    emit_decls(&prog, &types)
        .expect("object-is emission should succeed")
        .to_string()
}

pub(super) fn self_capture_probe(block: &MirBlock, self_param: LocalId) -> bool {
    let types = TypeTable::new();
    let prog = MirProgram::new(ModuleId::from_raw(0));
    self_capture_probe_with_env(block, self_param, &types, &prog)
}

pub(super) fn self_capture_probe_with_env(
    block: &MirBlock,
    self_param: LocalId,
    types: &TypeTable,
    prog: &MirProgram,
) -> bool {
    let emit_env = EmitEnv {
        types,
        program: prog,
    };
    let body_ctx = BodyCtx::for_closure(
        TypeId::from_raw(0),
        std::collections::HashMap::new(),
        std::collections::HashMap::new(),
    );
    self_capture_may_escape(block, self_param, &emit_env, &body_ctx)
}

pub(super) fn capturing_closure(self_param: LocalId, ty: TypeId) -> MirExpr {
    MirExpr::Closure {
        id: 0,
        params: Vec::new(),
        captures: vec![self_param],
        locals: Vec::new(),
        body: MirBlock { stmts: Vec::new() },
        ret_ty: ty,
        fn_ty: ty,
    }
}

pub(super) fn capture_free_closure(ty: TypeId) -> MirExpr {
    MirExpr::Closure {
        id: 0,
        params: Vec::new(),
        captures: Vec::new(),
        locals: Vec::new(),
        body: MirBlock { stmts: Vec::new() },
        ret_ty: ty,
        fn_ty: ty,
    }
}

pub(super) fn normalize_token_whitespace(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_space = false;
    for c in s.chars() {
        if c.is_whitespace() {
            if !prev_space {
                out.push(' ');
                prev_space = true;
            }
        } else {
            out.push(c);
            prev_space = false;
        }
    }
    out
}
