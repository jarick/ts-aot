use super::*;
use ts_aot_core::{Atom, LocalId, ModuleId, Span, TypeId};
use ts_aot_ir_hir::{
    HirBinaryOp, HirCallee, HirClass, HirDecl, HirExpr, HirFunction, HirParam, HirProgram, HirStmt,
};

fn unit_ty() -> TypeId {
    TypeId::from_raw(0)
}

fn int_ty() -> TypeId {
    TypeId::from_raw(1)
}

fn param(name: &str, ty: TypeId) -> HirParam {
    HirParam {
        name: Atom::from(name),
        ty,
    }
}

fn empty_function(name: &str) -> HirFunction {
    HirFunction {
        name: Atom::from(name),
        params: Vec::new(),
        ret: unit_ty(),
        throws: None,
        body: Vec::new(),
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    }
}

#[test]
fn non_capturing_closure_is_emitted_as_top_level_fn() {
    let mut f = empty_function("outer");
    f.body = vec![HirStmt::Let {
        id: LocalId::from_raw(0),
        name: Atom::from("add"),
        ty: int_ty(),
        init: Some(HirExpr::Closure {
            id: LocalId::from_raw(0),
            params: vec![param("a", int_ty()), param("b", int_ty())],
            captures: Vec::new(),
            body: vec![HirStmt::Return {
                value: Some(HirExpr::Int(0, Span::default())),
            }],
            ty: int_ty(),

            span: Span::default(),
        }),
    }];
    let mut program = HirProgram::new(ModuleId::from_raw(0));
    program.declarations.push(HirDecl::Function(f));

    let mut ctx = PassContext::new();
    let result = lower_closures(&mut program, &mut ctx);
    let stats = &result.stats;

    assert_eq!(stats.emitted_fns, 1);
    assert_eq!(stats.deferred_capturing, 0);

    let mut found = false;
    for d in &program.declarations {
        if let HirDecl::Function(HirFunction { name, params, .. }) = d
            && name.as_str().starts_with("__ts_aot_closure_")
        {
            found = true;
            assert_eq!(params.len(), 2);
        }
    }
    assert!(found, "expected a generated __ts_aot_closure_N fn");

    if let HirDecl::Function(HirFunction { body, .. }) = &program.declarations[0] {
        if let HirStmt::Let { init: Some(e), .. } = &body[0] {
            assert!(
                matches!(e, HirExpr::Global { .. }),
                "closure literal must be replaced with HirExpr::Global, got {e:?}"
            );
        } else {
            panic!("expected Let with init");
        }
    }
}

#[test]
fn call_to_closure_is_rewritten_to_indirect_global() {
    let mut f = empty_function("outer");
    let closure_id = LocalId::from_raw(0);
    let call_id = LocalId::from_raw(1);
    f.body = vec![
        HirStmt::Let {
            id: closure_id,
            name: Atom::from("cb"),
            ty: int_ty(),
            init: Some(HirExpr::Closure {
                id: closure_id,
                params: vec![param("x", int_ty())],
                captures: Vec::new(),
                body: vec![HirStmt::Return {
                    value: Some(HirExpr::Local {
                        id: call_id,
                        ty: int_ty(),

                        span: Span::default(),
                    }),
                }],
                ty: int_ty(),

                span: Span::default(),
            }),
        },
        HirStmt::Expr {
            expr: HirExpr::Call {
                callee: HirCallee::Closure(closure_id),
                args: vec![HirExpr::Int(7, Span::default())],
                ty: int_ty(),

                span: Span::default(),
            },
        },
    ];
    let mut program = HirProgram::new(ModuleId::from_raw(0));
    program.declarations.push(HirDecl::Function(f));

    let mut ctx = PassContext::new();
    let _result = lower_closures(&mut program, &mut ctx);

    let HirDecl::Function(HirFunction { body, .. }) = &program.declarations[0] else {
        panic!("expected Function");
    };
    if let HirStmt::Expr { expr } = &body[1] {
        if let HirExpr::Call { callee, .. } = expr {
            let HirCallee::Indirect(inner) = callee else {
                panic!(
                    "call site must be rewritten to HirCallee::Indirect(Global(name)), got {callee:?}"
                );
            };
            assert!(
                matches!(&**inner, HirExpr::Global { name, .. } if name.as_str().starts_with("__ts_aot_closure_")),
                "indirect callee must wrap HirExpr::Global pointing at a __ts_aot_closure_ fn, got {inner:?}"
            );
        } else {
            panic!("expected Call");
        }
    } else {
        panic!("expected Expr");
    }
}

#[test]
fn capturing_closure_emits_warning_and_is_unchanged() {
    let mut f = empty_function("outer");
    f.body = vec![HirStmt::Let {
        id: LocalId::from_raw(0),
        name: Atom::from("cb"),
        ty: int_ty(),
        init: Some(HirExpr::Closure {
            id: LocalId::from_raw(0),
            params: vec![param("x", int_ty())],
            captures: vec![HirExpr::Local {
                id: LocalId::from_raw(1),
                ty: int_ty(),

                span: Span::default(),
            }],
            body: vec![HirStmt::Return {
                value: Some(HirExpr::Binary {
                    op: HirBinaryOp::Add,
                    lhs: Box::new(HirExpr::Local {
                        id: LocalId::from_raw(0),
                        ty: int_ty(),

                        span: Span::default(),
                    }),
                    rhs: Box::new(HirExpr::Local {
                        id: LocalId::from_raw(1),
                        ty: int_ty(),

                        span: Span::default(),
                    }),
                    ty: int_ty(),

                    span: Span::default(),
                }),
            }],
            ty: int_ty(),

            span: Span::default(),
        }),
    }];
    let mut program = HirProgram::new(ModuleId::from_raw(0));
    program.declarations.push(HirDecl::Function(f));

    let mut ctx = PassContext::new();
    let result = lower_closures(&mut program, &mut ctx);
    let stats = &result.stats;

    assert_eq!(stats.emitted_fns, 0);
    assert_eq!(stats.deferred_capturing, 1);
    let diag = ctx
        .diagnostics()
        .iter()
        .find(|d| d.code.as_str() == "P0007")
        .expect("expected P0007 warning");
    assert!(diag.message.contains("capturing closures"));

    let HirDecl::Function(HirFunction { body, .. }) = &program.declarations[0] else {
        panic!("expected Function");
    };
    if let HirStmt::Let { init: Some(e), .. } = &body[0] {
        assert!(
            matches!(e, HirExpr::Closure { .. }),
            "capturing closure must be left intact, got {e:?}"
        );
    } else {
        panic!("expected Let with init");
    }
}

#[test]
fn two_functions_with_closure_at_local_zero_get_distinct_names() {
    let mut fa = empty_function("a");
    fa.body = vec![HirStmt::Let {
        id: LocalId::from_raw(0),
        name: Atom::from("cb"),
        ty: int_ty(),
        init: Some(HirExpr::Closure {
            id: LocalId::from_raw(0),
            params: vec![param("x", int_ty())],
            captures: Vec::new(),
            body: vec![HirStmt::Return {
                value: Some(HirExpr::Local {
                    id: LocalId::from_raw(1),
                    ty: int_ty(),

                    span: Span::default(),
                }),
            }],
            ty: int_ty(),

            span: Span::default(),
        }),
    }];

    let mut fb = empty_function("b");
    fb.body = vec![HirStmt::Let {
        id: LocalId::from_raw(0),
        name: Atom::from("cb"),
        ty: int_ty(),
        init: Some(HirExpr::Closure {
            id: LocalId::from_raw(0),
            params: vec![param("y", int_ty())],
            captures: Vec::new(),
            body: vec![HirStmt::Return {
                value: Some(HirExpr::Local {
                    id: LocalId::from_raw(1),
                    ty: int_ty(),

                    span: Span::default(),
                }),
            }],
            ty: int_ty(),

            span: Span::default(),
        }),
    }];

    let mut program = HirProgram::new(ModuleId::from_raw(0));
    program.declarations.push(HirDecl::Function(fa));
    program.declarations.push(HirDecl::Function(fb));

    let mut ctx = PassContext::new();
    let result = lower_closures(&mut program, &mut ctx);
    let stats = &result.stats;

    assert_eq!(stats.emitted_fns, 2);
    assert_eq!(stats.deferred_capturing, 0);
    assert!(!ctx.has_errors());

    let mut generated_names: Vec<String> = Vec::new();
    for d in &program.declarations {
        if let HirDecl::Function(HirFunction { name, .. }) = d
            && name.as_str().starts_with("__ts_aot_closure_")
        {
            generated_names.push(name.to_string());
        }
    }
    assert_eq!(generated_names.len(), 2);
    assert_ne!(
        generated_names[0], generated_names[1],
        "two scopes each with LocalId(0) must produce distinct fn names"
    );
}

#[test]
fn closure_inside_namespace_is_walked_and_emitted() {
    let mut inner = empty_function("inner_fn");
    inner.body = vec![HirStmt::Let {
        id: LocalId::from_raw(0),
        name: Atom::from("cb"),
        ty: int_ty(),
        init: Some(HirExpr::Closure {
            id: LocalId::from_raw(0),
            params: vec![param("x", int_ty())],
            captures: Vec::new(),
            body: vec![HirStmt::Return {
                value: Some(HirExpr::Local {
                    id: LocalId::from_raw(1),
                    ty: int_ty(),

                    span: Span::default(),
                }),
            }],
            ty: int_ty(),

            span: Span::default(),
        }),
    }];

    let mut program = HirProgram::new(ModuleId::from_raw(0));
    program.declarations.push(HirDecl::Namespace {
        name: Atom::from("ns"),
        members: vec![HirDecl::Function(inner)],
    });

    let mut ctx = PassContext::new();
    let result = lower_closures(&mut program, &mut ctx);
    let stats = &result.stats;

    assert_eq!(stats.emitted_fns, 1);
    assert_eq!(stats.deferred_capturing, 0);
    assert!(!ctx.has_errors());

    let HirDecl::Namespace { members, .. } = &program.declarations[0] else {
        panic!("expected Namespace decl at top");
    };
    let HirDecl::Function(f) = &members[0] else {
        panic!("expected Function inside Namespace");
    };
    if let HirStmt::Let { init: Some(e), .. } = &f.body[0] {
        assert!(
            matches!(e, HirExpr::Global { .. }),
            "closure literal inside namespace must be replaced with HirExpr::Global, got {e:?}"
        );
    } else {
        panic!("expected Let with init");
    }

    let mut found_in_namespace = false;
    for d in &program.declarations {
        if let HirDecl::Namespace { members, .. } = d {
            for m in members {
                if let HirDecl::Function(HirFunction { name, .. }) = m
                    && name.as_str().starts_with("__ts_aot_closure_")
                {
                    found_in_namespace = true;
                }
            }
        }
    }
    assert!(
        !found_in_namespace,
        "lower_closures hoists generated fns to top-level declarations, not nested namespaces"
    );

    let mut hoisted = false;
    for d in &program.declarations {
        if let HirDecl::Function(HirFunction { name, .. }) = d
            && name.as_str().starts_with("__ts_aot_closure_")
        {
            hoisted = true;
        }
    }
    assert!(
        hoisted,
        "generated fn must be hoisted to top-level program.declarations"
    );
}

#[test]
fn nested_non_capturing_closure_is_walked_and_rewritten() {
    let mut f = empty_function("outer");
    f.body = vec![HirStmt::Let {
        id: LocalId::from_raw(0),
        name: Atom::from("outer_cb"),
        ty: int_ty(),
        init: Some(HirExpr::Closure {
            id: LocalId::from_raw(0),
            params: vec![param("a", int_ty())],
            captures: Vec::new(),
            body: vec![HirStmt::Let {
                id: LocalId::from_raw(1),
                name: Atom::from("inner_cb"),
                ty: int_ty(),
                init: Some(HirExpr::Closure {
                    id: LocalId::from_raw(1),
                    params: vec![param("b", int_ty())],
                    captures: Vec::new(),
                    body: vec![HirStmt::Return {
                        value: Some(HirExpr::Local {
                            id: LocalId::from_raw(2),
                            ty: int_ty(),

                            span: Span::default(),
                        }),
                    }],
                    ty: int_ty(),

                    span: Span::default(),
                }),
            }],
            ty: int_ty(),

            span: Span::default(),
        }),
    }];
    let mut program = HirProgram::new(ModuleId::from_raw(0));
    program.declarations.push(HirDecl::Function(f));

    let mut ctx = PassContext::new();
    let result = lower_closures(&mut program, &mut ctx);
    let stats = &result.stats;

    assert_eq!(stats.emitted_fns, 2);
    assert_eq!(stats.deferred_capturing, 0);
    assert!(!ctx.has_errors());

    let HirDecl::Function(HirFunction { body, .. }) = &program.declarations[0] else {
        panic!("expected Function");
    };
    let HirStmt::Let { init: Some(e), .. } = &body[0] else {
        panic!("expected outer Let with init");
    };
    assert!(
        matches!(e, HirExpr::Global { .. }),
        "outer closure literal must be replaced with HirExpr::Global, got {e:?}"
    );

    let HirExpr::Global {
        name: outer_name, ..
    } = e
    else {
        unreachable!()
    };
    let outer_atom = outer_name.clone();
    let mut outer_body: Option<&Vec<HirStmt>> = None;
    for d in &program.declarations {
        if let HirDecl::Function(HirFunction { name, body: b, .. }) = d
            && *name == outer_atom
        {
            outer_body = Some(b);
        }
    }
    let outer_body = outer_body.expect("outer generated fn must be in declarations");

    let HirStmt::Let {
        init: Some(inner_init),
        ..
    } = &outer_body[0]
    else {
        panic!("expected inner Let in generated outer fn body");
    };
    assert!(
        matches!(inner_init, HirExpr::Global { .. }),
        "inner closure literal must be replaced with HirExpr::Global before outer body is cloned, got {inner_init:?}"
    );

    let mut generated: Vec<String> = Vec::new();
    for d in &program.declarations {
        if let HirDecl::Function(HirFunction { name, .. }) = d
            && name.as_str().starts_with("__ts_aot_closure_")
        {
            generated.push(name.to_string());
        }
    }
    assert_eq!(generated.len(), 2);
    assert_ne!(generated[0], generated[1]);
}

#[test]
fn no_closures_is_a_noop() {
    let mut program = HirProgram::new(ModuleId::from_raw(0));
    program
        .declarations
        .push(HirDecl::Function(empty_function("f")));
    let mut ctx = PassContext::new();
    let result = lower_closures(&mut program, &mut ctx);
    let stats = &result.stats;
    assert_eq!(stats.emitted_fns, 0);
    assert_eq!(stats.deferred_capturing, 0);
    assert_eq!(program.declarations.len(), 1);
    assert!(!ctx.has_errors());
}

#[test]
fn generated_name_skips_names_already_taken_by_user_decls() {
    let mut f = empty_function("outer");
    f.body = vec![HirStmt::Let {
        id: LocalId::from_raw(0),
        name: Atom::from("cb"),
        ty: int_ty(),
        init: Some(HirExpr::Closure {
            id: LocalId::from_raw(0),
            params: Vec::new(),
            captures: Vec::new(),
            body: Vec::new(),
            ty: int_ty(),

            span: Span::default(),
        }),
    }];
    let mut program = HirProgram::new(ModuleId::from_raw(0));
    program.declarations.push(HirDecl::Function(f));
    program.declarations.push(HirDecl::Function(HirFunction {
        name: Atom::new_inline("__ts_aot_closure_0"),
        params: Vec::new(),
        ret: unit_ty(),
        throws: None,
        body: Vec::new(),
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    }));

    let mut ctx = PassContext::new();
    let result = lower_closures(&mut program, &mut ctx);
    let stats = &result.stats;

    assert_eq!(stats.emitted_fns, 1);
    let generated = result.closure_names.first().expect("one generated name");
    assert_ne!(
        generated.as_str(),
        "__ts_aot_closure_0",
        "generated name must skip the user's __ts_aot_closure_0"
    );
    assert!(
        generated.as_str().starts_with("__ts_aot_closure_"),
        "generated name must still follow the __ts_aot_closure_N shape, got {generated:?}"
    );
    assert_eq!(
        generated.as_str(),
        "__ts_aot_closure_1",
        "with one user collision, first free id is 1, got {generated:?}"
    );

    let mut names: Vec<String> = program
        .declarations
        .iter()
        .filter_map(|d| match d {
            HirDecl::Function(HirFunction { name, .. }) => Some(name.as_str().to_owned()),
            _ => None,
        })
        .collect();
    names.sort();
    let original_len = names.len();
    let unique: std::collections::HashSet<String> = names.iter().cloned().collect();
    assert_eq!(
        original_len,
        unique.len(),
        "after lower_closures no two functions may share a name; got {names:?}"
    );
}

#[test]
fn generated_name_skips_names_taken_by_class_methods() {
    let class_with_method = HirDecl::Class(HirClass {
        name: Atom::new_inline("C"),
        ty: int_ty(),
        fields: Vec::new(),
        methods: vec![
            HirFunction {
                name: Atom::new_inline("__ts_aot_closure_0"),
                params: vec![HirParam {
                    name: Atom::from("self"),
                    ty: unit_ty(),
                }],
                ret: unit_ty(),
                throws: None,
                body: Vec::new(),
                is_async: false,
                is_generator: false,
                is_exported: false,
                type_params: Vec::new(),
                async_info: None,
            },
            HirFunction {
                name: Atom::new_inline("__ts_aot_closure_1"),
                params: vec![HirParam {
                    name: Atom::from("self"),
                    ty: unit_ty(),
                }],
                ret: unit_ty(),
                throws: None,
                body: Vec::new(),
                is_async: false,
                is_generator: false,
                is_exported: false,
                type_params: Vec::new(),
                async_info: None,
            },
        ],
        extends: None,
        type_params: Vec::new(),
    });

    let outer = empty_function("outer");
    let mut outer = outer;
    outer.body = vec![HirStmt::Let {
        id: LocalId::from_raw(0),
        name: Atom::from("cb"),
        ty: int_ty(),
        init: Some(HirExpr::Closure {
            id: LocalId::from_raw(0),
            params: Vec::new(),
            captures: Vec::new(),
            body: Vec::new(),
            ty: int_ty(),

            span: Span::default(),
        }),
    }];

    let mut program = HirProgram::new(ModuleId::from_raw(0));
    program.declarations.push(class_with_method);
    program.declarations.push(HirDecl::Function(outer));

    let mut ctx = PassContext::new();
    let result = lower_closures(&mut program, &mut ctx);
    let stats = &result.stats;

    assert_eq!(stats.emitted_fns, 1);
    let generated = result.closure_names.first().expect("one generated name");
    assert_eq!(
        generated.as_str(),
        "__ts_aot_closure_2",
        "class methods __ts_aot_closure_0 and __ts_aot_closure_1 must be skipped, so first free id is 2; got {generated:?}"
    );

    let mut names: Vec<String> = program
        .declarations
        .iter()
        .filter_map(|d| match d {
            HirDecl::Function(HirFunction { name, .. }) => Some(name.as_str().to_owned()),
            HirDecl::Class(c) => Some(c.name.as_str().to_owned()),
            _ => None,
        })
        .collect();
    names.sort();
    let unique: std::collections::HashSet<String> = names.iter().cloned().collect();
    assert_eq!(
        names.len(),
        unique.len(),
        "after lower_closures no two decls (incl. classes) may share a name; got {names:?}"
    );
}
