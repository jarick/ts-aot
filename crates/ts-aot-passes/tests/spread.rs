use ts_aot_core::LocalId;
use ts_aot_frontend::FrontendPass;
use ts_aot_ir_mir::{MirBlock, MirExpr, MirProgram, MirStmt, RuntimeOp};
use ts_aot_passes::{PassContext, convert_program};

fn convert(src: &str) -> (MirProgram, Vec<String>) {
    let mut types = ts_aot_core::TypeTable::new();
    let mut ctx = PassContext::new();
    let frontend = FrontendPass::new().run_with_types("test.ts", src, &mut types, false);
    let mut diags: Vec<String> = frontend
        .diagnostics
        .iter()
        .map(|d| format!("{:?}", d))
        .collect();
    if frontend.diagnostics.has_errors() {
        return (MirProgram::new(ts_aot_core::ModuleId::from_raw(0)), diags);
    }
    let mut hir = frontend.program;
    ts_aot_passes::lower_enums(&mut hir, &mut types, &mut ctx);
    ts_aot_passes::monomorphize(&mut hir, &mut types, &mut ctx);
    ts_aot_passes::lower_closures(&mut hir, &mut types, &mut ctx);
    let _ = ts_aot_passes::lower_async(&mut hir, &mut types, &mut ctx);
    let mir = convert_program(&hir, &mut types, &mut ctx);
    let has_errors = ctx.has_errors();
    diags.extend(ctx.diagnostics().iter().map(|d| format!("{:?}", d)));
    if has_errors {
        return (mir, diags);
    }
    (mir, diags)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OperandKind {
    Literal,
    Spread,
    Hole,
}

fn find_writer_kind(stmts: &[MirStmt], target: LocalId) -> OperandKind {
    for s in stmts.iter().rev() {
        if let MirStmt::Runtime {
            op, dest: Some(d), ..
        } = s
            && *d == target
        {
            return match op {
                RuntimeOp::ArrayCreate => OperandKind::Literal,
                RuntimeOp::ArrayHole => OperandKind::Hole,
                _ => OperandKind::Spread,
            };
        }
    }
    OperandKind::Spread
}

fn collect_array_concat(mir: &MirProgram) -> Vec<Vec<OperandKind>> {
    fn walk_block(block: &MirBlock) -> Vec<Vec<OperandKind>> {
        let mut out = Vec::new();
        for (idx, s) in block.stmts.iter().enumerate() {
            if let MirStmt::Runtime {
                op: RuntimeOp::ArrayConcat,
                args,
                ..
            } = s
            {
                let kinds: Vec<OperandKind> = args
                    .iter()
                    .map(|arg| {
                        if let MirExpr::Local(id) = arg {
                            find_writer_kind(&block.stmts[..idx], *id)
                        } else {
                            OperandKind::Spread
                        }
                    })
                    .collect();
                out.push(kinds);
            }
            match s {
                MirStmt::If {
                    then_block,
                    else_block,
                    ..
                } => {
                    out.extend(walk_block(then_block));
                    if let Some(eb) = else_block {
                        out.extend(walk_block(eb));
                    }
                }
                MirStmt::While { body, .. }
                | MirStmt::DoWhile { body, .. }
                | MirStmt::ForOf { body, .. }
                | MirStmt::ForIn { body, .. } => out.extend(walk_block(body)),
                _ => {}
            }
        }
        out
    }
    let mut out = Vec::new();
    for d in &mir.declarations {
        if let ts_aot_ir_mir::MirDecl::Function(f) = d {
            out.extend(walk_block(&f.body.block));
        }
    }
    out
}

#[test]
fn array_spread_with_one_source_emits_concat_with_two_parts() {
    let (mir, diags) = convert(
        r"
        function f(a: i64[]): i64[] {
            return [...a, 1];
        }
        ",
    );
    assert!(diags.is_empty(), "diags: {diags:?}");
    let seqs = collect_array_concat(&mir);
    assert_eq!(
        seqs,
        vec![vec![OperandKind::Spread, OperandKind::Literal]],
        "[...a, 1] → [Spread, Literal]"
    );
}

#[test]
fn array_spread_with_multiple_sources_preserves_operand_sequence() {
    let (mir, diags) = convert(
        r"
        function f(a: i64[], b: i64[]): i64[] {
            return [0, ...a, 1, ...b, 2];
        }
        ",
    );
    assert!(diags.is_empty(), "diags: {diags:?}");
    let seqs = collect_array_concat(&mir);
    assert_eq!(
        seqs,
        vec![vec![
            OperandKind::Literal,
            OperandKind::Spread,
            OperandKind::Literal,
            OperandKind::Spread,
            OperandKind::Literal,
        ]],
        "[0, ...a, 1, ...b, 2] → [Literal, Spread, Literal, Spread, Literal]"
    );
}

#[test]
fn array_spread_at_start_only() {
    let (mir, diags) = convert(
        r"
        function f(a: i64[]): i64[] {
            return [...a];
        }
        ",
    );
    assert!(diags.is_empty(), "diags: {diags:?}");
    let seqs = collect_array_concat(&mir);
    assert_eq!(seqs, vec![vec![OperandKind::Spread]], "[...a] → [Spread]");
}

#[test]
fn array_without_spread_uses_array_create_not_concat() {
    let (mir, diags) = convert(
        r"
        function f(): i64[] {
            return [1, 2, 3];
        }
        ",
    );
    assert!(diags.is_empty(), "diags: {diags:?}");
    let seqs = collect_array_concat(&mir);
    assert!(
        seqs.is_empty(),
        "plain literal must not emit ArrayConcat, got: {seqs:?}"
    );
    fn count_create(block: &ts_aot_ir_mir::MirBlock) -> usize {
        let mut n = 0;
        for s in &block.stmts {
            if let MirStmt::Runtime {
                op: RuntimeOp::ArrayCreate,
                ..
            } = s
            {
                n += 1;
            }
        }
        n
    }
    let create_count: usize = mir
        .declarations
        .iter()
        .filter_map(|d| {
            if let ts_aot_ir_mir::MirDecl::Function(f) = d {
                Some(count_create(&f.body.block))
            } else {
                None
            }
        })
        .sum();
    assert_eq!(
        create_count, 1,
        "plain literal must emit exactly one ArrayCreate"
    );
}

#[test]
fn array_spread_with_hole_emits_array_hole_call() {
    let (mir, diags) = convert(
        r"
        function f(a: i64[]): i64[] {
            return [...a, , 1];
        }
        ",
    );
    assert!(diags.is_empty(), "diags: {diags:?}");
    let seqs = collect_array_concat(&mir);
    assert_eq!(
        seqs,
        vec![vec![
            OperandKind::Spread,
            OperandKind::Hole,
            OperandKind::Literal,
        ]],
        "[...a, , 1] → [Spread, Hole, Literal]"
    );
    fn count_hole(block: &ts_aot_ir_mir::MirBlock) -> usize {
        let mut n = 0;
        for s in &block.stmts {
            if let MirStmt::Runtime {
                op: RuntimeOp::ArrayHole,
                ..
            } = s
            {
                n += 1;
            }
        }
        n
    }
    let hole_count: usize = mir
        .declarations
        .iter()
        .filter_map(|d| {
            if let ts_aot_ir_mir::MirDecl::Function(f) = d {
                Some(count_hole(&f.body.block))
            } else {
                None
            }
        })
        .sum();
    assert_eq!(hole_count, 1, "hole must emit exactly one ArrayHole call");
}

#[test]
fn array_spread_with_hole_before_typed_source_uses_spread_type() {
    let (mir, diags) = convert(
        r"
        function f(a: i64[]): i64[] {
            return [, ...a];
        }
        ",
    );
    assert!(diags.is_empty(), "diags: {diags:?}");
    let seqs = collect_array_concat(&mir);
    assert_eq!(
        seqs,
        vec![vec![OperandKind::Hole, OperandKind::Spread]],
        "[, ...a] → [Hole, Spread]"
    );
}

#[test]
fn array_spread_without_typed_source_emits_diagnostic() {
    let (_, diags) = convert(
        r"
        function f(x: i64): i64[] {
            return [1, ...x];
        }
        ",
    );
    assert!(
        diags.iter().any(|d| d.contains("element type")),
        "spreading non-array must surface an element-type diagnostic, got: {diags:?}"
    );
}

#[test]
fn array_spread_with_incompatible_element_types_emits_diagnostic() {
    let (_, diags) = convert(
        r"
        function f(a: i64[], b: f64[]): i64[] {
            return [...a, ...b];
        }
        ",
    );
    assert!(
        diags.iter().any(|d| d.contains("incompatible")),
        "spread operands with different element types must surface an incompatible-type diagnostic, got: {diags:?}"
    );
}
