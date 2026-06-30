use super::*;

#[test]
fn block_starts_empty() {
    let b = MirBlock::new();
    assert!(b.is_empty());
    assert_eq!(b.len(), 0);
    assert!(b.stmts.is_empty());
}

#[test]
fn block_push_appends_in_order() {
    let mut b = MirBlock::new();
    b.push(MirStmt::Break);
    b.push(MirStmt::Continue);
    assert_eq!(b.len(), 2);
    assert!(matches!(b.stmts[0], MirStmt::Break));
    assert!(matches!(b.stmts[1], MirStmt::Continue));
}

#[test]
fn block_with_single_stmt() {
    let b = MirBlock::with(MirStmt::Return(None));
    assert_eq!(b.len(), 1);
    assert!(matches!(b.stmts[0], MirStmt::Return(None)));
}

#[test]
fn body_default_is_empty() {
    let body = MirBody::default();
    assert!(body.locals.is_empty());
    assert!(body.block.is_empty());
}

#[test]
fn binary_op_variants_are_distinct() {
    let ops = [
        BinaryOp::Add,
        BinaryOp::Sub,
        BinaryOp::Mul,
        BinaryOp::Div,
        BinaryOp::Mod,
        BinaryOp::Eq,
        BinaryOp::Ne,
        BinaryOp::Lt,
        BinaryOp::Le,
        BinaryOp::Gt,
        BinaryOp::Ge,
        BinaryOp::And,
        BinaryOp::Or,
        BinaryOp::BitAnd,
        BinaryOp::BitOr,
        BinaryOp::BitXor,
        BinaryOp::Shl,
        BinaryOp::Shr,
    ];
    for (i, a) in ops.iter().enumerate() {
        for (j, b) in ops.iter().enumerate() {
            if i != j {
                assert_ne!(a, b, "ops[{i}]={a:?} should differ from ops[{j}]={b:?}");
            }
        }
    }
}

#[test]
fn unary_op_variants_are_distinct() {
    assert_ne!(UnaryOp::Neg, UnaryOp::Not);
    assert_ne!(UnaryOp::Not, UnaryOp::BitNot);
    assert_ne!(UnaryOp::Neg, UnaryOp::BitNot);
}

#[test]
fn runtime_op_variants_are_distinct() {
    let ops = [
        RuntimeOp::StringConcat,
        RuntimeOp::StringEquals,
        RuntimeOp::StringLen,
        RuntimeOp::ArrayCreate,
        RuntimeOp::ArrayGet,
        RuntimeOp::ArraySet,
        RuntimeOp::ArrayLen,
        RuntimeOp::MapGet,
        RuntimeOp::MapSet,
        RuntimeOp::ResultOk,
        RuntimeOp::ResultErr,
        RuntimeOp::ResultUnwrapOk,
        RuntimeOp::PromiseCreate,
        RuntimeOp::PromiseResolve,
        RuntimeOp::HostConsoleLog,
        RuntimeOp::MathSqrt,
    ];
    for (i, a) in ops.iter().enumerate() {
        for (j, b) in ops.iter().enumerate() {
            if i != j {
                assert_ne!(a, b, "ops[{i}]={a:?} should differ from ops[{j}]={b:?}");
            }
        }
    }
}

#[test]
fn stmt_break_and_continue_are_unit_variants() {
    assert!(matches!(MirStmt::Break, MirStmt::Break));
    assert!(matches!(MirStmt::Continue, MirStmt::Continue));
}

#[test]
fn stmt_let_carries_local_ty_init_mutable() {
    let s = MirStmt::Let {
        local: LocalId::from_raw(1),
        ty: TypeId::from_raw(2),
        init: Some(MirExpr::Unit),
        mutable: true,
    };
    match s {
        MirStmt::Let {
            local,
            ty,
            init,
            mutable,
        } => {
            assert_eq!(local, LocalId::from_raw(1));
            assert_eq!(ty, TypeId::from_raw(2));
            assert!(init.is_some());
            assert!(mutable);
        }
        _ => panic!("expected Let"),
    }
}

#[test]
fn stmt_assign_carries_place_and_value() {
    let s = MirStmt::Assign {
        target: MirPlace::Local {
            id: LocalId::from_raw(0),
        },
        value: MirExpr::Bool(true),
    };
    match s {
        MirStmt::Assign { target, value } => {
            assert!(matches!(target, MirPlace::Local { .. }));
            assert!(matches!(value, MirExpr::Bool(true)));
        }
        _ => panic!("expected Assign"),
    }
}

#[test]
fn stmt_if_carries_cond_and_blocks() {
    let s = MirStmt::If {
        cond: MirExpr::Bool(true),
        then_block: MirBlock::with(MirStmt::Return(None)),
        else_block: None,
    };
    match s {
        MirStmt::If {
            cond,
            then_block,
            else_block,
        } => {
            assert!(matches!(cond, MirExpr::Bool(true)));
            assert_eq!(then_block.len(), 1);
            assert!(else_block.is_none());
        }
        _ => panic!("expected If"),
    }
}

#[test]
fn stmt_for_of_carries_item_iterable_body() {
    let s = MirStmt::ForOf {
        item: LocalId::from_raw(0),
        iterable: MirExpr::Unit,
        body: MirBlock::new(),
    };
    match s {
        MirStmt::ForOf { item, body, .. } => {
            assert_eq!(item, LocalId::from_raw(0));
            assert!(body.is_empty());
        }
        _ => panic!("expected ForOf"),
    }
}

#[test]
fn stmt_runtime_carries_op_args_ty() {
    let s = MirStmt::Runtime {
        op: RuntimeOp::StringConcat,
        args: vec![MirExpr::Unit, MirExpr::Unit],
        dest: None,
        ty: TypeId::from_raw(0),
    };
    match s {
        MirStmt::Runtime { op, args, ty, .. } => {
            assert_eq!(op, RuntimeOp::StringConcat);
            assert_eq!(args.len(), 2);
            assert_eq!(ty, TypeId::from_raw(0));
        }
        _ => panic!("expected Runtime"),
    }
}

#[test]
fn stmt_await_carries_promise_dest_state_ty() {
    let s = MirStmt::Await {
        promise: MirExpr::Unit,
        dest: LocalId::from_raw(3),
        next_state: 7,
        ty: TypeId::from_raw(0),
    };
    match s {
        MirStmt::Await {
            promise,
            dest,
            next_state,
            ty,
        } => {
            assert!(matches!(promise, MirExpr::Unit));
            assert_eq!(dest, LocalId::from_raw(3));
            assert_eq!(next_state, 7);
            assert_eq!(ty, TypeId::from_raw(0));
        }
        _ => panic!("expected Await"),
    }
}

#[test]
fn stmt_set_state_carries_int_value() {
    assert!(matches!(
        MirStmt::SetState { value: 0 },
        MirStmt::SetState { value: 0 }
    ));
    assert!(matches!(
        MirStmt::SetState { value: -1 },
        MirStmt::SetState { value: -1 }
    ));
}

#[test]
fn place_local_carries_id() {
    let p = MirPlace::Local {
        id: LocalId::from_raw(5),
    };
    match p {
        MirPlace::Local { id } => assert_eq!(id, LocalId::from_raw(5)),
        _ => panic!("expected Local"),
    }
}

#[test]
fn place_field_carries_base_field_ty() {
    let p = MirPlace::Field {
        base: Box::new(MirPlaceBase::Local(LocalId::from_raw(0))),
        field: FieldId::from_raw(1),
        ty: TypeId::from_raw(2),
    };
    match p {
        MirPlace::Field { base, field, ty } => {
            assert!(matches!(*base, MirPlaceBase::Local(_)));
            assert_eq!(field, FieldId::from_raw(1));
            assert_eq!(ty, TypeId::from_raw(2));
        }
        _ => panic!("expected Field"),
    }
}

#[test]
fn place_index_carries_base_index_ty() {
    let p = MirPlace::Index {
        base: Box::new(MirExpr::Local(LocalId::from_raw(0))),
        index: Box::new(MirExpr::Int {
            value: 0,
            ty: TypeId::from_raw(0),
        }),
        ty: TypeId::from_raw(0),
    };
    match p {
        MirPlace::Index { ty, .. } => assert_eq!(ty, TypeId::from_raw(0)),
        _ => panic!("expected Index"),
    }
}

#[test]
fn expr_unit_is_unit_variant() {
    assert!(matches!(MirExpr::Unit, MirExpr::Unit));
}

#[test]
fn expr_bool_carries_value() {
    assert!(matches!(MirExpr::Bool(true), MirExpr::Bool(true)));
    assert!(matches!(MirExpr::Bool(false), MirExpr::Bool(false)));
}

#[test]
fn expr_int_carries_value_and_ty() {
    let e = MirExpr::Int {
        value: 42,
        ty: TypeId::from_raw(1),
    };
    match e {
        MirExpr::Int { value, ty } => {
            assert_eq!(value, 42);
            assert_eq!(ty, TypeId::from_raw(1));
        }
        _ => panic!("expected Int"),
    }
}

#[test]
fn expr_call_carries_callee_args_ty() {
    let e = MirExpr::Call {
        callee: FunctionId::from_raw(0),
        args: vec![MirExpr::Unit],
        ty: TypeId::from_raw(0),
    };
    match e {
        MirExpr::Call { callee, args, ty } => {
            assert_eq!(callee, FunctionId::from_raw(0));
            assert_eq!(args.len(), 1);
            assert_eq!(ty, TypeId::from_raw(0));
        }
        _ => panic!("expected Call"),
    }
}

#[test]
fn expr_struct_literal_carries_struct_id_fields_ty() {
    let e = MirExpr::StructLiteral {
        struct_id: StructId::from_raw(0),
        fields: vec![(FieldId::from_raw(0), MirExpr::Bool(true))],
        ty: TypeId::from_raw(0),
    };
    match e {
        MirExpr::StructLiteral {
            struct_id,
            fields,
            ty,
        } => {
            assert_eq!(struct_id, StructId::from_raw(0));
            assert_eq!(fields.len(), 1);
            assert_eq!(ty, TypeId::from_raw(0));
        }
        _ => panic!("expected StructLiteral"),
    }
}

#[test]
fn expr_binary_carries_op_left_right_ty() {
    let e = MirExpr::Binary {
        op: BinaryOp::Add,
        left: Box::new(MirExpr::Int {
            value: 1,
            ty: TypeId::from_raw(0),
        }),
        right: Box::new(MirExpr::Int {
            value: 2,
            ty: TypeId::from_raw(0),
        }),
        ty: TypeId::from_raw(0),
    };
    match e {
        MirExpr::Binary { op, ty, .. } => {
            assert_eq!(op, BinaryOp::Add);
            assert_eq!(ty, TypeId::from_raw(0));
        }
        _ => panic!("expected Binary"),
    }
}

#[test]
fn expr_unary_carries_op_expr_ty() {
    let e = MirExpr::Unary {
        op: UnaryOp::Neg,
        expr: Box::new(MirExpr::Int {
            value: 1,
            ty: TypeId::from_raw(0),
        }),
        ty: TypeId::from_raw(0),
    };
    match e {
        MirExpr::Unary { op, ty, .. } => {
            assert_eq!(op, UnaryOp::Neg);
            assert_eq!(ty, TypeId::from_raw(0));
        }
        _ => panic!("expected Unary"),
    }
}

#[test]
fn local_decl_carries_id_name_ty_mutable() {
    let d = MirLocalDecl {
        id: LocalId::from_raw(1),
        name: SymbolId::from_raw(2),
        ty: TypeId::from_raw(3),
        mutable: true,
    };
    assert_eq!(d.id, LocalId::from_raw(1));
    assert_eq!(d.name, SymbolId::from_raw(2));
    assert_eq!(d.ty, TypeId::from_raw(3));
    assert!(d.mutable);
}
