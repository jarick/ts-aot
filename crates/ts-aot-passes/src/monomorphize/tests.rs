use super::*;
use crate::PassContext;
use crate::hir_to_mir::convert_program;
use std::collections::HashSet;
use ts_aot_core::{Atom, FunctionId, GenericParamId, LocalId, ModuleId, Type, TypeId, TypeTable};
use ts_aot_ir_hir::{HirCallee, HirDecl, HirExpr, HirFunction, HirParam, HirProgram, HirStmt};
use ts_aot_ir_mir::MirDecl;

fn setup() -> (HirProgram, TypeTable, PassContext) {
    let types = TypeTable::new();
    let ctx = PassContext::default();
    (HirProgram::new(ModuleId::from_raw(0)), types, ctx)
}

fn simple_fn(name: &str, body: Vec<HirStmt>) -> HirFunction {
    HirFunction {
        name: Atom::from(name),
        params: vec![],
        ret: TypeId::from_raw(0),
        throws: None,
        body,
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: vec![],
        async_info: None,
    }
}

fn generic_fn(name: &str, type_params: Vec<GenericParamId>, body: Vec<HirStmt>) -> HirFunction {
    HirFunction {
        type_params,
        ..simple_fn(name, body)
    }
}

fn find_fn<'a>(program: &'a HirProgram, name: &str) -> Option<&'a HirFunction> {
    program.declarations.iter().find_map(|d| match d {
        HirDecl::Function(f) if f.name.as_str() == name => Some(f),
        HirDecl::Namespace { members, .. } => members.iter().find_map(|m| match m {
            HirDecl::Function(f) if f.name.as_str() == name => Some(f),
            _ => None,
        }),
        _ => None,
    })
}

fn find_mono_for<'a>(program: &'a HirProgram, generic_name: &str) -> Option<&'a HirFunction> {
    let prefix = format!("{}_mono_", generic_name);
    program.declarations.iter().find_map(|d| match d {
        HirDecl::Function(f) if f.name.as_str().starts_with(&prefix) => Some(f),
        _ => None,
    })
}

fn count_mono_for(program: &HirProgram, generic_name: &str) -> usize {
    let prefix = format!("{}_mono_", generic_name);
    program
        .declarations
        .iter()
        .filter(|d| matches!(d, HirDecl::Function(f) if f.name.as_str().starts_with(&prefix)))
        .count()
}

fn find_mono_mir_fn<'a>(
    mir: &'a ts_aot_ir_mir::MirProgram,
    generic_name: &str,
) -> Option<(FunctionId, &'a ts_aot_ir_mir::MirFunctionDecl)> {
    let prefix = format!("{}_mono_", generic_name);
    mir.declarations.iter().find_map(|d| match d {
        MirDecl::Function(f) if f.name.as_str().starts_with(&prefix) => Some((f.id, f)),
        _ => None,
    })
}

#[test]
fn non_generic_function_is_unchanged() {
    let (mut program, mut types, mut ctx) = setup();
    program.push_decl(HirDecl::Function(simple_fn(
        "add",
        vec![HirStmt::Return { value: None }],
    )));

    let stats = monomorphize(&mut program, &mut types, &mut ctx);

    assert_eq!(program.declarations.len(), 1);
    assert_eq!(stats.generic_functions, 0);
    assert_eq!(stats.monomorphized, 0);
    assert_eq!(stats.calls_rewritten, 0);
}

#[test]
fn generic_function_without_calls_is_left_alone() {
    let (mut program, mut types, mut ctx) = setup();
    program.push_decl(HirDecl::Function(generic_fn(
        "identity",
        vec![GenericParamId::from_raw(0)],
        vec![HirStmt::Return { value: None }],
    )));

    let stats = monomorphize(&mut program, &mut types, &mut ctx);

    assert_eq!(program.declarations.len(), 1);
    assert_eq!(stats.generic_functions, 1);
    assert_eq!(stats.monomorphized, 0, "no mono copy when not called");
    assert_eq!(stats.calls_rewritten, 0);
}

#[test]
fn generic_function_with_one_call_creates_one_mono_copy() {
    let (mut program, mut types, mut ctx) = setup();
    program.push_decl(HirDecl::Function(generic_fn(
        "identity",
        vec![GenericParamId::from_raw(0)],
        vec![HirStmt::Return { value: None }],
    )));
    let caller_body = vec![HirStmt::Expr {
        expr: HirExpr::Call {
            callee: HirCallee::Function(FunctionId::from_raw(0)),
            args: vec![HirExpr::Int(42)],
            ty: TypeId::from_raw(0),
        },
    }];
    program.push_decl(HirDecl::Function(simple_fn("caller", caller_body)));

    let stats = monomorphize(&mut program, &mut types, &mut ctx);

    assert_eq!(program.declarations.len(), 3, "identity + caller + mono");
    assert_eq!(stats.generic_functions, 1);
    assert_eq!(stats.monomorphized, 1);
    assert_eq!(stats.calls_rewritten, 1);

    let mono =
        find_mono_for(&program, "identity").expect("mono copy must be appended with mangled name");
    assert!(mono.type_params.is_empty(), "mono copy has no type_params");
}

#[test]
fn call_to_generic_rewrites_callee_to_mono() {
    let (mut program, mut types, mut ctx) = setup();
    program.push_decl(HirDecl::Function(generic_fn(
        "identity",
        vec![GenericParamId::from_raw(0)],
        vec![HirStmt::Return { value: None }],
    )));
    let caller_body = vec![HirStmt::Expr {
        expr: HirExpr::Call {
            callee: HirCallee::Function(FunctionId::from_raw(0)),
            args: vec![HirExpr::Int(42)],
            ty: TypeId::from_raw(0),
        },
    }];
    program.push_decl(HirDecl::Function(simple_fn("caller", caller_body)));

    let _ = monomorphize(&mut program, &mut types, &mut ctx);

    let caller = find_fn(&program, "caller").expect("caller must exist");
    let call = caller.body.iter().find_map(|s| match s {
        HirStmt::Expr {
            expr: HirExpr::Call { callee, .. },
        } => Some(callee),
        _ => None,
    });
    let callee = call.expect("call must exist in caller body");
    if let HirCallee::Function(fid) = callee {
        assert_ne!(
            *fid,
            FunctionId::from_raw(0),
            "callee must be rewritten from generic_fid(0)"
        );
    } else {
        panic!("expected Function callee after rewrite");
    }
}

#[test]
fn multiple_calls_to_same_generic_share_one_mono_copy() {
    let (mut program, mut types, mut ctx) = setup();
    program.push_decl(HirDecl::Function(generic_fn(
        "identity",
        vec![GenericParamId::from_raw(0)],
        vec![HirStmt::Return { value: None }],
    )));
    let caller_body = vec![
        HirStmt::Expr {
            expr: HirExpr::Call {
                callee: HirCallee::Function(FunctionId::from_raw(0)),
                args: vec![HirExpr::Int(1)],
                ty: TypeId::from_raw(0),
            },
        },
        HirStmt::Expr {
            expr: HirExpr::Call {
                callee: HirCallee::Function(FunctionId::from_raw(0)),
                args: vec![HirExpr::Int(2)],
                ty: TypeId::from_raw(0),
            },
        },
    ];
    program.push_decl(HirDecl::Function(simple_fn("caller", caller_body)));

    let stats = monomorphize(&mut program, &mut types, &mut ctx);

    assert_eq!(
        stats.monomorphized, 1,
        "two calls to same generic share one mono copy"
    );
    assert_eq!(stats.calls_rewritten, 2);

    let mono_count = count_mono_for(&program, "identity");
    assert_eq!(
        mono_count, 1,
        "exactly one identity_mono specialization in program"
    );
}

#[test]
fn generic_calling_generic_rewrites_inner_call_to_inner_mono() {
    let (mut program, mut types, mut ctx) = setup();

    let a_body = vec![HirStmt::Expr {
        expr: HirExpr::Call {
            callee: HirCallee::Function(FunctionId::from_raw(1)),
            args: vec![HirExpr::Int(42)],
            ty: TypeId::from_raw(0),
        },
    }];
    program.push_decl(HirDecl::Function(generic_fn(
        "A",
        vec![GenericParamId::from_raw(0)],
        a_body,
    )));
    program.push_decl(HirDecl::Function(generic_fn(
        "B",
        vec![GenericParamId::from_raw(0)],
        vec![HirStmt::Return { value: None }],
    )));
    let caller_body = vec![HirStmt::Expr {
        expr: HirExpr::Call {
            callee: HirCallee::Function(FunctionId::from_raw(0)),
            args: vec![HirExpr::Int(1)],
            ty: TypeId::from_raw(0),
        },
    }];
    program.push_decl(HirDecl::Function(simple_fn("caller", caller_body)));

    let stats = monomorphize(&mut program, &mut types, &mut ctx);

    assert_eq!(stats.generic_functions, 2);
    assert_eq!(stats.monomorphized, 2);
    assert_eq!(
        stats.calls_rewritten, 3,
        "1 in caller (→A), 1 in A (→B), 1 in A_mono (→B); extend-before-rewrite required to catch A_mono's inner call"
    );

    let closure_names: HashSet<Atom> = HashSet::new();
    let mir = convert_program(&program, &mut ctx, &closure_names);

    let (a_mono_id, _) = find_mono_mir_fn(&mir, "A").expect("A_mono must exist in MIR");
    let (b_mono_id, _) = find_mono_mir_fn(&mir, "B").expect("B_mono must exist in MIR");

    let a_mono_mir = find_mono_mir_fn(&mir, "A")
        .map(|(_, f)| f)
        .expect("A_mono must exist in MIR");
    let mut a_inner_call: Option<FunctionId> = None;
    for stmt in &a_mono_mir.body.block.stmts {
        if let ts_aot_ir_mir::MirStmt::Expr(expr) = stmt
            && let ts_aot_ir_mir::MirExpr::Call { callee, .. } = expr
        {
            a_inner_call = Some(*callee);
        }
    }
    assert_eq!(
        a_inner_call,
        Some(b_mono_id),
        "A_mono's inner call must resolve to B_mono (proves extend-before-rewrite)"
    );

    let caller_mir = mir
        .declarations
        .iter()
        .find_map(|d| match d {
            MirDecl::Function(f) if f.name.as_str() == "caller" => Some(f),
            _ => None,
        })
        .expect("caller must exist in MIR");
    let mut caller_call: Option<FunctionId> = None;
    for stmt in &caller_mir.body.block.stmts {
        if let ts_aot_ir_mir::MirStmt::Expr(expr) = stmt
            && let ts_aot_ir_mir::MirExpr::Call { callee, .. } = expr
        {
            caller_call = Some(*callee);
        }
    }
    assert_eq!(
        caller_call,
        Some(a_mono_id),
        "caller's MIR call must resolve to A_mono"
    );
}

#[test]
fn generic_class_method_with_call_is_monomorphized() {
    let (mut program, mut types, mut ctx) = setup();
    let t_ty = types.intern(&Type::GenericParam {
        id: GenericParamId::from_raw(0),
    });
    let method = ts_aot_ir_hir::HirFunction {
        name: Atom::from("wrap"),
        params: vec![ts_aot_ir_hir::HirParam {
            name: Atom::from("x"),
            ty: t_ty,
        }],
        ret: t_ty,
        throws: None,
        body: vec![HirStmt::Return { value: None }],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: vec![GenericParamId::from_raw(0)],
        async_info: None,
    };
    let class_with_method = ts_aot_ir_hir::HirClass {
        name: Atom::from("Box"),
        ty: TypeId::from_raw(0),
        fields: vec![],
        methods: vec![method],
        extends: None,
        type_params: vec![],
    };
    program.push_decl(HirDecl::Class(class_with_method));
    let caller_body = vec![HirStmt::Expr {
        expr: HirExpr::Call {
            callee: HirCallee::Function(FunctionId::from_raw(0)),
            args: vec![HirExpr::Int(7)],
            ty: t_ty,
        },
    }];
    program.push_decl(HirDecl::Function(simple_fn("caller", caller_body)));

    let stats = monomorphize(&mut program, &mut types, &mut ctx);

    assert_eq!(stats.generic_functions, 1);
    assert_eq!(stats.monomorphized, 1);
    assert_eq!(stats.calls_rewritten, 1);
    assert!(
        find_mono_for(&program, "wrap").is_some(),
        "mono copy of generic method must exist"
    );
}

#[test]
fn empty_param_generic_class_method_is_not_monomorphized() {
    let (mut program, mut types, mut ctx) = setup();
    let method = generic_fn(
        "wrap",
        vec![GenericParamId::from_raw(0)],
        vec![HirStmt::Return { value: None }],
    );
    let class_with_method = ts_aot_ir_hir::HirClass {
        name: Atom::from("Box"),
        ty: TypeId::from_raw(0),
        fields: vec![],
        methods: vec![method],
        extends: None,
        type_params: vec![],
    };
    program.push_decl(HirDecl::Class(class_with_method));
    let caller_body = vec![HirStmt::Expr {
        expr: HirExpr::Call {
            callee: HirCallee::Function(FunctionId::from_raw(0)),
            args: vec![HirExpr::Int(7)],
            ty: TypeId::from_raw(0),
        },
    }];
    program.push_decl(HirDecl::Function(simple_fn("caller", caller_body)));

    let stats = monomorphize(&mut program, &mut types, &mut ctx);

    assert_eq!(
        stats.generic_functions, 0,
        "empty-param class methods are invisible to monomorphize (convert_program skips them too)"
    );
    assert_eq!(stats.monomorphized, 0);
    assert_eq!(stats.calls_rewritten, 0);
    assert!(
        find_mono_for(&program, "wrap").is_none(),
        "no mono copy for skipped empty-param method"
    );
}

#[test]
fn monomorphize_class_method_with_call_e2e_keeps_function_ids_aligned() {
    let (mut program, mut types, mut ctx) = setup();
    let t_ty = types.intern(&Type::GenericParam {
        id: GenericParamId::from_raw(0),
    });
    let method = ts_aot_ir_hir::HirFunction {
        name: Atom::from("wrap"),
        params: vec![ts_aot_ir_hir::HirParam {
            name: Atom::from("x"),
            ty: t_ty,
        }],
        ret: t_ty,
        throws: None,
        body: vec![HirStmt::Return { value: None }],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: vec![GenericParamId::from_raw(0)],
        async_info: None,
    };
    let class_with_method = ts_aot_ir_hir::HirClass {
        name: Atom::from("Box"),
        ty: TypeId::from_raw(0),
        fields: vec![],
        methods: vec![method],
        extends: None,
        type_params: vec![],
    };
    program.push_decl(HirDecl::Class(class_with_method));
    let caller_body = vec![HirStmt::Expr {
        expr: HirExpr::Call {
            callee: HirCallee::Function(FunctionId::from_raw(0)),
            args: vec![HirExpr::Int(7)],
            ty: t_ty,
        },
    }];
    program.push_decl(HirDecl::Function(simple_fn("caller", caller_body)));

    let mono_stats = monomorphize(&mut program, &mut types, &mut ctx);
    assert_eq!(mono_stats.monomorphized, 1);
    assert_eq!(mono_stats.calls_rewritten, 1);

    let closure_names: HashSet<Atom> = HashSet::new();
    let mir = convert_program(&program, &mut ctx, &closure_names);

    assert!(
        find_mono_mir_fn(&mir, "wrap").is_some(),
        "wrap_mono must be in MIR (non-empty params survive both passes)"
    );

    let caller_mir = mir
        .declarations
        .iter()
        .find_map(|d| match d {
            MirDecl::Function(f) if f.name.as_str() == "caller" => Some(f),
            _ => None,
        })
        .expect("caller must exist in MIR");

    let mut found_call: Option<FunctionId> = None;
    for stmt in &caller_mir.body.block.stmts {
        if let ts_aot_ir_mir::MirStmt::Expr(expr) = stmt
            && let ts_aot_ir_mir::MirExpr::Call { callee, .. } = expr
        {
            found_call = Some(*callee);
        }
    }
    let callee = found_call.expect("caller's MIR call must exist");
    let (mono_id, _) = find_mono_mir_fn(&mir, "wrap").unwrap();
    assert_eq!(
        callee, mono_id,
        "caller's MIR call must resolve to wrap_mono's FunctionId"
    );
}

#[test]
fn generic_function_inside_namespace_is_not_classified() {
    let (mut program, mut types, mut ctx) = setup();
    let inner = generic_fn(
        "ns_identity",
        vec![GenericParamId::from_raw(0)],
        vec![HirStmt::Return { value: None }],
    );
    program.push_decl(HirDecl::Namespace {
        name: Atom::from("outer"),
        members: vec![HirDecl::Function(inner)],
    });
    let caller_body = vec![HirStmt::Expr {
        expr: HirExpr::Call {
            callee: HirCallee::Function(FunctionId::from_raw(0)),
            args: vec![HirExpr::Int(1)],
            ty: TypeId::from_raw(0),
        },
    }];
    program.push_decl(HirDecl::Function(simple_fn("caller", caller_body)));

    let stats = monomorphize(&mut program, &mut types, &mut ctx);

    assert_eq!(
        stats.generic_functions, 0,
        "namespace fns are invisible to monomorphize (convert_program skips them too)"
    );
    assert_eq!(stats.monomorphized, 0);
    assert_eq!(stats.calls_rewritten, 0);
    assert!(
        find_mono_for(&program, "ns_identity").is_none(),
        "no mono copy created for skipped namespace fn"
    );
}

#[test]
fn monomorphize_then_convert_program_keeps_function_ids_aligned() {
    let (mut program, mut types, mut ctx) = setup();
    program.push_decl(HirDecl::Function(generic_fn(
        "identity",
        vec![GenericParamId::from_raw(0)],
        vec![HirStmt::Return { value: None }],
    )));
    let caller_body = vec![HirStmt::Expr {
        expr: HirExpr::Call {
            callee: HirCallee::Function(FunctionId::from_raw(0)),
            args: vec![HirExpr::Int(42)],
            ty: TypeId::from_raw(0),
        },
    }];
    program.push_decl(HirDecl::Function(simple_fn("caller", caller_body)));

    let mono_stats = monomorphize(&mut program, &mut types, &mut ctx);
    assert_eq!(mono_stats.monomorphized, 1);
    assert_eq!(mono_stats.calls_rewritten, 1);

    let closure_names: HashSet<Atom> = HashSet::new();
    let mir = convert_program(&program, &mut ctx, &closure_names);

    let functions: Vec<_> = mir
        .declarations
        .iter()
        .filter_map(|d| match d {
            MirDecl::Function(f) => Some((f.id, f.name.clone())),
            _ => None,
        })
        .collect();
    assert_eq!(
        functions.len(),
        3,
        "identity + caller + identity_mono in MIR"
    );

    let (mono_fid, _) =
        find_mono_mir_fn(&mir, "identity").expect("identity_mono must exist in MIR");
    assert_eq!(
        mono_fid,
        FunctionId::from_raw(2),
        "mono copy must get FunctionId 2 (after identity=0 and caller=1)"
    );

    let caller_mir = mir
        .declarations
        .iter()
        .find_map(|d| match d {
            MirDecl::Function(f) if f.name.as_str() == "caller" => Some(f),
            _ => None,
        })
        .expect("caller must exist in MIR");

    let mut found_call: Option<FunctionId> = None;
    for stmt in &caller_mir.body.block.stmts {
        if let ts_aot_ir_mir::MirStmt::Expr(expr) = stmt
            && let ts_aot_ir_mir::MirExpr::Call { callee, .. } = expr
        {
            found_call = Some(*callee);
        }
    }
    assert_eq!(
        found_call,
        Some(mono_fid),
        "caller's MIR call must resolve to mono_fid"
    );
}

#[test]
fn monomorphize_namespace_skips_does_not_break_convert_program() {
    let (mut program, mut types, mut ctx) = setup();
    let inner = generic_fn(
        "ns_identity",
        vec![GenericParamId::from_raw(0)],
        vec![HirStmt::Return { value: None }],
    );
    program.push_decl(HirDecl::Namespace {
        name: Atom::from("outer"),
        members: vec![HirDecl::Function(inner)],
    });
    let caller_body = vec![HirStmt::Expr {
        expr: HirExpr::Call {
            callee: HirCallee::Function(FunctionId::from_raw(0)),
            args: vec![HirExpr::Int(1)],
            ty: TypeId::from_raw(0),
        },
    }];
    program.push_decl(HirDecl::Function(simple_fn("caller", caller_body)));

    let _ = monomorphize(&mut program, &mut types, &mut ctx);

    let closure_names: HashSet<Atom> = HashSet::new();
    let mir = convert_program(&program, &mut ctx, &closure_names);

    let functions: Vec<_> = mir
        .declarations
        .iter()
        .filter_map(|d| match d {
            MirDecl::Function(f) => Some((f.id, f.name.clone())),
            _ => None,
        })
        .collect();
    assert_eq!(
        functions.len(),
        1,
        "only caller survives (namespace skipped, no mono copy appended)"
    );
    assert_eq!(functions[0].0, FunctionId::from_raw(0));
    assert_eq!(functions[0].1.as_str(), "caller");
}

#[test]
fn inference_with_concrete_param_before_generic_binds_to_generic() {
    let (mut program, mut types, mut ctx) = setup();

    let i64_ty = types.intern(&Type::I64);
    let t_ty = types.intern(&Type::GenericParam {
        id: GenericParamId::from_raw(0),
    });

    program.push_decl(HirDecl::Function(HirFunction {
        name: Atom::from("f"),
        params: vec![
            HirParam {
                name: Atom::from("prefix"),
                ty: i64_ty,
            },
            HirParam {
                name: Atom::from("value"),
                ty: t_ty,
            },
        ],
        ret: t_ty,
        throws: None,
        body: vec![HirStmt::Return { value: None }],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: vec![GenericParamId::from_raw(0)],
        async_info: None,
    }));

    program.push_decl(HirDecl::Function(simple_fn(
        "caller",
        vec![HirStmt::Expr {
            expr: HirExpr::Call {
                callee: HirCallee::Function(FunctionId::from_raw(0)),
                args: vec![HirExpr::Int(42), HirExpr::String(Atom::from("hi"))],
                ty: t_ty,
            },
        }],
    )));

    let stats = monomorphize(&mut program, &mut types, &mut ctx);

    assert_eq!(stats.monomorphized, 1);
    assert_eq!(stats.calls_rewritten, 1);

    let mono = find_mono_for(&program, "f").expect("mono copy of f");

    let value_resolved = types
        .resolve(mono.params[1].ty)
        .expect("value param type resolves");
    assert_eq!(
        value_resolved,
        &Type::String,
        "T should be substituted with String (arg 1), not left as generic or bound to i64 (arg 0)"
    );
}

#[test]
fn inference_with_array_param_binds_to_element_not_whole_array() {
    let (mut program, mut types, mut ctx) = setup();

    let t_ty = types.intern(&Type::GenericParam {
        id: GenericParamId::from_raw(0),
    });
    let array_t = types.intern(&Type::Array { element: t_ty });

    program.push_decl(HirDecl::Function(HirFunction {
        name: Atom::from("f"),
        params: vec![HirParam {
            name: Atom::from("arr"),
            ty: array_t,
        }],
        ret: t_ty,
        throws: None,
        body: vec![HirStmt::Return { value: None }],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: vec![GenericParamId::from_raw(0)],
        async_info: None,
    }));

    let i64_ty = types.intern(&Type::I64);
    let array_i64 = types.intern(&Type::Array { element: i64_ty });

    program.push_decl(HirDecl::Function(simple_fn(
        "caller",
        vec![HirStmt::Expr {
            expr: HirExpr::Call {
                callee: HirCallee::Function(FunctionId::from_raw(0)),
                args: vec![HirExpr::ArrayLiteral {
                    elements: vec![HirExpr::Int(1), HirExpr::Int(2)],
                    ty: array_i64,
                }],
                ty: t_ty,
            },
        }],
    )));

    let stats = monomorphize(&mut program, &mut types, &mut ctx);

    assert_eq!(stats.monomorphized, 1);
    assert_eq!(stats.calls_rewritten, 1);

    let mono = find_mono_for(&program, "f").expect("mono copy of f");

    let ret_resolved = types.resolve(mono.ret).expect("ret type resolves");
    assert_eq!(
        ret_resolved,
        &Type::I64,
        "T (return) should be substituted with i64 (element), not Array<i64>"
    );
}

#[test]
fn fallback_does_not_bind_unrelated_concrete_param_to_generic() {
    let (mut program, mut types, mut ctx) = setup();

    let i64_ty = types.intern(&Type::I64);
    let t_ty = types.intern(&Type::GenericParam {
        id: GenericParamId::from_raw(0),
    });

    program.push_decl(HirDecl::Function(HirFunction {
        name: Atom::from("f"),
        params: vec![HirParam {
            name: Atom::from("x"),
            ty: i64_ty,
        }],
        ret: t_ty,
        throws: None,
        body: vec![HirStmt::Return { value: None }],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: vec![GenericParamId::from_raw(0)],
        async_info: None,
    }));

    program.push_decl(HirDecl::Function(simple_fn(
        "caller",
        vec![HirStmt::Expr {
            expr: HirExpr::Call {
                callee: HirCallee::Function(FunctionId::from_raw(0)),
                args: vec![HirExpr::Int(1)],
                ty: t_ty,
            },
        }],
    )));

    let stats = monomorphize(&mut program, &mut types, &mut ctx);

    assert_eq!(
        stats.monomorphized, 0,
        "T is not inferable from x (concrete i64 param) — no specialization should be created"
    );
    assert_eq!(stats.calls_rewritten, 0);
    assert!(
        find_mono_for(&program, "f").is_none(),
        "no mono copy should be created when T cannot be structurally inferred"
    );
}

#[test]
fn composite_type_with_generic_param_inside_is_not_resolved() {
    let (mut program, mut types, mut ctx) = setup();

    let t_ty = types.intern(&Type::GenericParam {
        id: GenericParamId::from_raw(0),
    });
    let array_t = types.intern(&Type::Array { element: t_ty });

    program.push_decl(HirDecl::Function(HirFunction {
        name: Atom::from("B"),
        params: vec![HirParam {
            name: Atom::from("x"),
            ty: t_ty,
        }],
        ret: t_ty,
        throws: None,
        body: vec![HirStmt::Return { value: None }],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: vec![GenericParamId::from_raw(0)],
        async_info: None,
    }));

    program.push_decl(HirDecl::Function(HirFunction {
        name: Atom::from("A"),
        params: vec![HirParam {
            name: Atom::from("arr"),
            ty: array_t,
        }],
        ret: array_t,
        throws: None,
        body: vec![HirStmt::Expr {
            expr: HirExpr::Call {
                callee: HirCallee::Function(FunctionId::from_raw(0)),
                args: vec![HirExpr::Local {
                    id: LocalId::from_raw(0),
                    ty: array_t,
                }],
                ty: t_ty,
            },
        }],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: vec![GenericParamId::from_raw(0)],
        async_info: None,
    }));

    program.push_decl(HirDecl::Function(simple_fn(
        "caller",
        vec![HirStmt::Expr {
            expr: HirExpr::Call {
                callee: HirCallee::Function(FunctionId::from_raw(1)),
                args: vec![HirExpr::Int(42)],
                ty: t_ty,
            },
        }],
    )));

    let stats = monomorphize(&mut program, &mut types, &mut ctx);

    assert_eq!(
        stats.monomorphized, 0,
        "A.body.Call(B) arg has Array<T> (with GenericParam inside), type_args not resolved, no specialization"
    );
    assert_eq!(stats.calls_rewritten, 0);
}

#[test]
fn worklist_creates_transitive_specialization_for_generic_calling_generic() {
    let (mut program, mut types, mut ctx) = setup();

    let t_ty = types.intern(&Type::GenericParam {
        id: GenericParamId::from_raw(0),
    });
    let x_local = LocalId::from_raw(0);

    let a_body = vec![HirStmt::Expr {
        expr: HirExpr::Call {
            callee: HirCallee::Function(FunctionId::from_raw(1)),
            args: vec![HirExpr::Local {
                id: x_local,
                ty: t_ty,
            }],
            ty: t_ty,
        },
    }];
    program.push_decl(HirDecl::Function(HirFunction {
        name: Atom::from("A"),
        params: vec![HirParam {
            name: Atom::from("x"),
            ty: t_ty,
        }],
        ret: t_ty,
        throws: None,
        body: a_body,
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: vec![GenericParamId::from_raw(0)],
        async_info: None,
    }));

    program.push_decl(HirDecl::Function(HirFunction {
        name: Atom::from("B"),
        params: vec![HirParam {
            name: Atom::from("x"),
            ty: t_ty,
        }],
        ret: t_ty,
        throws: None,
        body: vec![HirStmt::Return { value: None }],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: vec![GenericParamId::from_raw(0)],
        async_info: None,
    }));

    program.push_decl(HirDecl::Function(simple_fn(
        "caller",
        vec![HirStmt::Expr {
            expr: HirExpr::Call {
                callee: HirCallee::Function(FunctionId::from_raw(0)),
                args: vec![HirExpr::Int(42)],
                ty: t_ty,
            },
        }],
    )));

    let stats = monomorphize(&mut program, &mut types, &mut ctx);

    assert_eq!(
        stats.monomorphized, 2,
        "worklist must create both A<i64> and B<i64> (A_mono's inner call exposes B specialization)"
    );

    let closure_names: HashSet<Atom> = HashSet::new();
    let mir = convert_program(&program, &mut ctx, &closure_names);

    let (a_mono_id, _) = find_mono_mir_fn(&mir, "A").expect("A mono in MIR");
    let (b_mono_id, _) = find_mono_mir_fn(&mir, "B").expect("B mono in MIR");

    let a_mono_mir = find_mono_mir_fn(&mir, "A")
        .map(|(_, f)| f)
        .expect("A mono in MIR");
    let mut a_inner_call: Option<FunctionId> = None;
    for stmt in &a_mono_mir.body.block.stmts {
        if let ts_aot_ir_mir::MirStmt::Expr(expr) = stmt
            && let ts_aot_ir_mir::MirExpr::Call { callee, .. } = expr
        {
            a_inner_call = Some(*callee);
        }
    }
    assert_eq!(
        a_inner_call,
        Some(b_mono_id),
        "A<i64>_mono must call B<i64>_mono (worklist discovered B specialization from A's substituted body)"
    );

    let caller_mir = mir
        .declarations
        .iter()
        .find_map(|d| match d {
            MirDecl::Function(f) if f.name.as_str() == "caller" => Some(f),
            _ => None,
        })
        .expect("caller in MIR");
    let mut caller_call: Option<FunctionId> = None;
    for stmt in &caller_mir.body.block.stmts {
        if let ts_aot_ir_mir::MirStmt::Expr(expr) = stmt
            && let ts_aot_ir_mir::MirExpr::Call { callee, .. } = expr
        {
            caller_call = Some(*callee);
        }
    }
    assert_eq!(caller_call, Some(a_mono_id), "caller must call A<i64>_mono");
}

#[test]
fn indirect_callee_with_nested_generic_call_is_monomorphized() {
    let (mut program, mut types, mut ctx) = setup();

    let t_ty = types.intern(&Type::GenericParam {
        id: GenericParamId::from_raw(0),
    });

    program.push_decl(HirDecl::Function(HirFunction {
        name: Atom::from("g"),
        params: vec![HirParam {
            name: Atom::from("x"),
            ty: t_ty,
        }],
        ret: t_ty,
        throws: None,
        body: vec![HirStmt::Return { value: None }],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: vec![GenericParamId::from_raw(0)],
        async_info: None,
    }));

    program.push_decl(HirDecl::Function(simple_fn(
        "caller",
        vec![HirStmt::Expr {
            expr: HirExpr::Call {
                callee: HirCallee::Indirect(Box::new(HirExpr::Call {
                    callee: HirCallee::Function(FunctionId::from_raw(0)),
                    args: vec![HirExpr::Int(42)],
                    ty: t_ty,
                })),
                args: vec![HirExpr::Int(3)],
                ty: t_ty,
            },
        }],
    )));

    let stats = monomorphize(&mut program, &mut types, &mut ctx);

    assert_eq!(
        stats.monomorphized, 1,
        "g<i64>_mono must be created when generic call sits inside HirCallee::Indirect"
    );
    assert!(
        find_mono_for(&program, "g").is_some(),
        "mono copy of g must exist"
    );
}

#[test]
fn closure_params_in_mono_copy_are_type_substituted() {
    let (mut program, mut types, mut ctx) = setup();

    let t_ty = types.intern(&Type::GenericParam {
        id: GenericParamId::from_raw(0),
    });
    let i64_ty = types.intern(&Type::I64);

    let closure = HirExpr::Closure {
        id: LocalId::from_raw(1),
        params: vec![HirParam {
            name: Atom::from("y"),
            ty: t_ty,
        }],
        captures: vec![],
        body: vec![HirStmt::Return { value: None }],
        ty: t_ty,
    };

    program.push_decl(HirDecl::Function(HirFunction {
        name: Atom::from("wrap"),
        params: vec![HirParam {
            name: Atom::from("x"),
            ty: t_ty,
        }],
        ret: t_ty,
        throws: None,
        body: vec![HirStmt::Return {
            value: Some(closure),
        }],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: vec![GenericParamId::from_raw(0)],
        async_info: None,
    }));

    program.push_decl(HirDecl::Function(simple_fn(
        "caller",
        vec![HirStmt::Expr {
            expr: HirExpr::Call {
                callee: HirCallee::Function(FunctionId::from_raw(0)),
                args: vec![HirExpr::Int(42)],
                ty: t_ty,
            },
        }],
    )));

    let stats = monomorphize(&mut program, &mut types, &mut ctx);

    assert_eq!(stats.monomorphized, 1, "wrap<i64>_mono must be created");

    let mono_fn = find_mono_for(&program, "wrap").expect("wrap mono");
    let mut closure_param_ty: Option<TypeId> = None;
    for stmt in &mono_fn.body {
        if let HirStmt::Return {
            value: Some(HirExpr::Closure { params, .. }),
        } = stmt
        {
            closure_param_ty = Some(params[0].ty);
        }
    }
    assert_eq!(
        closure_param_ty,
        Some(i64_ty),
        "closure param ty must be substituted from T (GenericParam) to i64"
    );
}
