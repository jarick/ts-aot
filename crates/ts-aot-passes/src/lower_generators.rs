use ts_aot_core::{Atom, FunctionId, LocalId, Span, Type, TypeId, TypeTable};
use ts_aot_ir_hir::{
    HirBinaryOp, HirCallee, HirDecl, HirExpr, HirFunction, HirParam, HirProgram, HirStmt,
};

use crate::PassContext;

const GENERATOR_DIAG_UNSUPPORTED_YIELD: &str = "E0501";

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LowerGeneratorsStats {
    pub generators_transformed: usize,
    pub generators_rejected: usize,
}

pub const GENERATOR_RUNTIME_NAME: &str = "ts_aot_runtime_Generator";
pub const GENERATOR_RESULT_RUNTIME_NAME: &str = "ts_aot_runtime_GeneratorResult";
pub const GENERATOR_NEW_RUNTIME_NAME: &str = "ts_aot_runtime_Generator_new";
pub const GENERATOR_GET_STATE_NAME: &str = "ts_aot_runtime___ts_aot_generator_get_state";
pub const GENERATOR_SET_STATE_NAME: &str = "ts_aot_runtime___ts_aot_generator_set_state";
pub const GENERATOR_STORE_NAME: &str = "ts_aot_runtime___ts_aot_generator_store";
pub const GENERATOR_YIELDED_NAME: &str = "ts_aot_runtime___ts_aot_generator_yielded";
pub const GENERATOR_DONE_NAME: &str = "ts_aot_runtime___ts_aot_generator_done";
pub const GENERATOR_DONE_WITH_NAME: &str = "ts_aot_runtime___ts_aot_generator_done_with";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum SplitError {
    UnsupportedYield { kind: &'static str },
}

pub fn lower_generators(
    program: &mut HirProgram,
    types: &mut TypeTable,
    ctx: &mut PassContext,
) -> LowerGeneratorsStats {
    let mut stats = LowerGeneratorsStats::default();
    let type_id_zero = TypeId::from_raw(0);
    let generator_ty = types.intern(&Type::Named {
        symbol: Atom::new_inline(GENERATOR_RUNTIME_NAME),
    });
    let result_ty = types.intern(&Type::Named {
        symbol: Atom::new_inline(GENERATOR_RESULT_RUNTIME_NAME),
    });

    let mut dispatch_decls: Vec<HirDecl> = Vec::new();
    for decl in &mut program.declarations {
        let HirDecl::Function(f) = decl else { continue };
        if !f.is_generator {
            continue;
        }
        let dispatch_name = Atom::from(format!("__gen_dispatch_{}", f.name));

        let blocks = match split_body_at_yields(&f.body) {
            Ok(blocks) => blocks,
            Err(SplitError::UnsupportedYield { kind }) => {
                ctx.error(
                    GENERATOR_DIAG_UNSUPPORTED_YIELD,
                    format!(
                        "yield inside `{kind}` is not supported by the sync generator state machine (move the `yield` out of the control-flow wrapper)"
                    ),
                    Span::new(0, 0),
                );
                stats.generators_rejected += 1;
                continue;
            }
        };
        let mut dispatch_body: Vec<HirStmt> = Vec::new();
        for (state, block) in (0_u32..).zip(blocks.iter()) {
            let cond = HirExpr::Binary {
                op: HirBinaryOp::Eq,
                lhs: Box::new(HirExpr::Call {
                    callee: HirCallee::Indirect(Box::new(HirExpr::Global {
                        name: Atom::new_inline(GENERATOR_GET_STATE_NAME),
                        ty: type_id_zero,
                    })),
                    args: vec![HirExpr::Local {
                        id: LocalId::from_raw(0),
                        ty: generator_ty,
                    }],
                    ty: type_id_zero,
                }),
                rhs: Box::new(HirExpr::Int(state as i64)),
                ty: type_id_zero,
            };
            let mut then_stmts: Vec<HirStmt> = block.0.clone();
            match &block.1 {
                BlockEnd::Yield(expr_opt) => {
                    let next_state = state + 1;
                    if let Some(value_expr) = expr_opt.clone() {
                        then_stmts.push(HirStmt::Expr {
                            expr: HirExpr::Call {
                                callee: HirCallee::Indirect(Box::new(HirExpr::Global {
                                    name: Atom::new_inline(GENERATOR_STORE_NAME),
                                    ty: type_id_zero,
                                })),
                                args: vec![
                                    HirExpr::Local {
                                        id: LocalId::from_raw(0),
                                        ty: generator_ty,
                                    },
                                    *value_expr,
                                ],
                                ty: type_id_zero,
                            },
                        });
                    }
                    then_stmts.push(HirStmt::Expr {
                        expr: HirExpr::Call {
                            callee: HirCallee::Indirect(Box::new(HirExpr::Global {
                                name: Atom::new_inline(GENERATOR_SET_STATE_NAME),
                                ty: type_id_zero,
                            })),
                            args: vec![
                                HirExpr::Local {
                                    id: LocalId::from_raw(0),
                                    ty: generator_ty,
                                },
                                HirExpr::Int(next_state as i64),
                            ],
                            ty: type_id_zero,
                        },
                    });
                    then_stmts.push(HirStmt::Return {
                        value: Some(HirExpr::Call {
                            callee: HirCallee::Indirect(Box::new(HirExpr::Global {
                                name: Atom::new_inline(GENERATOR_YIELDED_NAME),
                                ty: result_ty,
                            })),
                            args: vec![HirExpr::Local {
                                id: LocalId::from_raw(0),
                                ty: generator_ty,
                            }],
                            ty: result_ty,
                        }),
                    });
                }
                BlockEnd::Return(ret_expr) => {
                    then_stmts.push(HirStmt::Expr {
                        expr: HirExpr::Call {
                            callee: HirCallee::Indirect(Box::new(HirExpr::Global {
                                name: Atom::new_inline(GENERATOR_SET_STATE_NAME),
                                ty: type_id_zero,
                            })),
                            args: vec![
                                HirExpr::Local {
                                    id: LocalId::from_raw(0),
                                    ty: generator_ty,
                                },
                                HirExpr::Int(u32::MAX as i64),
                            ],
                            ty: type_id_zero,
                        },
                    });
                    let return_call = if let Some(ret_expr) = ret_expr.as_ref() {
                        HirExpr::Call {
                            callee: HirCallee::Indirect(Box::new(HirExpr::Global {
                                name: Atom::new_inline(GENERATOR_DONE_WITH_NAME),
                                ty: result_ty,
                            })),
                            args: vec![ret_expr.clone()],
                            ty: result_ty,
                        }
                    } else {
                        HirExpr::Call {
                            callee: HirCallee::Indirect(Box::new(HirExpr::Global {
                                name: Atom::new_inline(GENERATOR_DONE_NAME),
                                ty: result_ty,
                            })),
                            args: vec![],
                            ty: result_ty,
                        }
                    };
                    then_stmts.push(HirStmt::Return {
                        value: Some(return_call),
                    });
                }
            }
            let then_branch = HirStmt::Block(then_stmts);
            dispatch_body.push(HirStmt::If {
                cond,
                then: Box::new(then_branch),
                otherwise: None,
            });
        }

        let dispatch = HirFunction {
            name: dispatch_name.clone(),
            params: vec![HirParam {
                name: Atom::new_inline("g"),
                ty: generator_ty,
            }],
            ret: result_ty,
            throws: None,
            body: dispatch_body,
            is_async: false,
            is_generator: false,
            is_exported: false,
            type_params: Vec::new(),
            async_info: None,
        };
        dispatch_decls.push(HirDecl::Function(dispatch));

        let _ = FunctionId::from_raw(0);
        let constructor_body = vec![HirStmt::Return {
            value: Some(HirExpr::Call {
                callee: HirCallee::Indirect(Box::new(HirExpr::Global {
                    name: Atom::new_inline(GENERATOR_NEW_RUNTIME_NAME),
                    ty: generator_ty,
                })),
                args: vec![HirExpr::Global {
                    name: dispatch_name,
                    ty: generator_ty,
                }],
                ty: generator_ty,
            }),
        }];
        f.body = constructor_body;
        f.ret = generator_ty;
        f.is_generator = false;
        stats.generators_transformed += 1;
    }
    for d in dispatch_decls {
        program.push_decl(d);
    }
    stats
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum BlockEnd {
    Yield(Option<Box<HirExpr>>),
    Return(Option<HirExpr>),
}

fn split_body_at_yields(body: &[HirStmt]) -> Result<Vec<(Vec<HirStmt>, BlockEnd)>, SplitError> {
    let mut blocks: Vec<(Vec<HirStmt>, BlockEnd)> = Vec::new();
    let mut current: Vec<HirStmt> = Vec::new();
    walk(body, &mut current, &mut blocks)?;
    if !current.is_empty() {
        blocks.push((current, BlockEnd::Return(None)));
    }
    Ok(blocks)
}

fn walk(
    stmts: &[HirStmt],
    current: &mut Vec<HirStmt>,
    blocks: &mut Vec<(Vec<HirStmt>, BlockEnd)>,
) -> Result<(), SplitError> {
    for stmt in stmts {
        match stmt {
            HirStmt::Expr {
                expr: HirExpr::Yield { expr, .. },
            } => {
                blocks.push((std::mem::take(current), BlockEnd::Yield(expr.clone())));
            }
            HirStmt::Return { value, .. } => {
                blocks.push((std::mem::take(current), BlockEnd::Return(value.clone())));
            }
            HirStmt::Block(inner) => {
                walk(inner, current, blocks)?;
            }
            other => {
                check_wrapper(other)?;
                current.push(other.clone());
            }
        }
    }
    Ok(())
}

fn stmt_shape(stmt: &HirStmt) -> (Option<&'static str>, Vec<&HirStmt>) {
    match stmt {
        HirStmt::Block(inner) => (None, inner.iter().collect()),
        HirStmt::If {
            then, otherwise, ..
        } => {
            let mut children = vec![then.as_ref()];
            if let Some(e) = otherwise {
                children.push(e.as_ref());
            }
            (Some("if"), children)
        }
        HirStmt::While { body, .. } => (Some("while"), vec![body.as_ref()]),
        HirStmt::DoWhile { body, .. } => (Some("do-while"), vec![body.as_ref()]),
        HirStmt::ForOf { body, .. } => (Some("for-of"), vec![body.as_ref()]),
        HirStmt::ForIn { body, .. } => (Some("for-in"), vec![body.as_ref()]),
        HirStmt::Switch { cases, .. } => (
            Some("switch"),
            cases.iter().flat_map(|c| c.body.iter()).collect(),
        ),
        HirStmt::Try {
            body,
            catch,
            finally,
        } => {
            let mut children = vec![body.as_ref()];
            if let Some(c) = catch {
                children.push(c.body.as_ref());
            }
            if let Some(f) = finally {
                children.push(f.as_ref());
            }
            (Some("try"), children)
        }
        HirStmt::Decl(_)
        | HirStmt::Let { .. }
        | HirStmt::Expr { .. }
        | HirStmt::Return { .. }
        | HirStmt::Break { .. }
        | HirStmt::Continue { .. }
        | HirStmt::Throw { .. } => (None, Vec::new()),
    }
}

fn has_yield(stmt: &HirStmt) -> bool {
    if let HirStmt::Expr {
        expr: HirExpr::Yield { .. },
    } = stmt
    {
        return true;
    }
    let (_, children) = stmt_shape(stmt);
    children.iter().any(|c| has_yield(c))
}

fn check_wrapper(stmt: &HirStmt) -> Result<(), SplitError> {
    let (wrapper, children) = stmt_shape(stmt);
    if let Some(kind) = wrapper
        && children.iter().any(|c| has_yield(c))
    {
        return Err(SplitError::UnsupportedYield { kind });
    }
    Ok(())
}
