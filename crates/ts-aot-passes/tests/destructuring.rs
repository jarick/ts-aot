use ts_aot_frontend::FrontendPass;
use ts_aot_ir_mir::{MirBlock, MirProgram, MirStmt, RuntimeOp};
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

fn collect_array_get_or_default(mir: &MirProgram) -> Vec<i64> {
    fn walk_block(block: &MirBlock, indices: &mut Vec<i64>) {
        for s in &block.stmts {
            if let MirStmt::Runtime {
                op: RuntimeOp::ArrayGetOrDefault,
                args,
                ..
            } = s
                && let Some(ts_aot_ir_mir::MirExpr::Int { value, .. }) = args.get(1)
            {
                indices.push(*value as i64);
            }
            match s {
                MirStmt::If {
                    then_block,
                    else_block,
                    ..
                } => {
                    walk_block(then_block, indices);
                    if let Some(eb) = else_block {
                        walk_block(eb, indices);
                    }
                }
                MirStmt::While { body, .. }
                | MirStmt::DoWhile { body, .. }
                | MirStmt::ForOf { body, .. }
                | MirStmt::ForIn { body, .. } => walk_block(body, indices),
                _ => {}
            }
        }
    }
    let mut indices = Vec::new();
    for d in &mir.declarations {
        if let ts_aot_ir_mir::MirDecl::Function(f) = d {
            walk_block(&f.body.block, &mut indices);
        }
    }
    indices
}

#[test]
fn array_destructuring_emits_one_runtime_call_per_named_element_with_correct_indices() {
    let (mir, diags) = convert(
        r#"
        function f(arr: i64[]): i64 {
            let [a, b, c] = arr;
            return a + b + c;
        }
        "#,
    );
    assert!(diags.is_empty(), "diags: {diags:?}");
    let indices = collect_array_get_or_default(&mir);
    assert_eq!(indices, vec![0, 1, 2], "indices must be 0, 1, 2 in order");
}

#[test]
fn array_destructuring_with_skip_hole_emits_calls_only_for_named_indices() {
    let (mir, diags) = convert(
        r#"
        function f(arr: i64[]): i64 {
            let [a, , b] = arr;
            return a + b;
        }
        "#,
    );
    assert!(diags.is_empty(), "diags: {diags:?}");
    let indices = collect_array_get_or_default(&mir);
    assert_eq!(indices, vec![0, 2], "hole at index 1 is skipped");
}

#[test]
fn array_destructuring_with_rest_emits_unsupported_diagnostic() {
    let (_, diags) = convert(
        r#"
        function f(arr: i64[]): i64 {
            let [a, ...rest] = arr;
            return a;
        }
        "#,
    );
    assert!(
        diags.iter().any(|d| d.contains("rest")),
        "let [a, ...rest] = arr must emit a diagnostic about unsupported rest, got: {diags:?}"
    );
}

#[test]
fn array_destructuring_without_initializer_emits_diagnostic() {
    let (_, diags) = convert(
        r#"
        function f(): i64 {
            let [a, b];
            return a;
        }
        "#,
    );
    assert!(
        diags.iter().any(|d| d.contains("initializer")),
        "let [a, b] without rhs must emit a diagnostic about missing initializer, got: {diags:?}"
    );
}

#[test]
fn array_destructuring_on_non_array_rhs_emits_diagnostic() {
    let (_, diags) = convert(
        r#"
        function f(): i64 {
            let [a, b] = 42;
            return a;
        }
        "#,
    );
    assert!(
        diags
            .iter()
            .any(|d| d.contains("array type") || d.contains("destructuring")),
        "destructuring non-array rhs must emit a diagnostic, got: {diags:?}"
    );
}

#[test]
fn nested_array_destructuring_emits_diagnostic() {
    let (_, diags) = convert(
        r#"
        function f(arr: i64[][]): i64 {
            let [[a, b], c] = arr;
            return a + b + c;
        }
        "#,
    );
    assert!(
        diags
            .iter()
            .any(|d| d.contains("nested") || d.contains("simple binding name")),
        "nested array destructuring must emit a diagnostic, got: {diags:?}"
    );
}

#[test]
fn array_destructuring_with_default_value_emits_diagnostic() {
    let (_, diags) = convert(
        r#"
        function f(arr: i64[]): i64 {
            let [a, b = 5] = arr;
            return a + b;
        }
        "#,
    );
    assert!(
        diags
            .iter()
            .any(|d| d.contains("simple binding name") || d.contains("default")),
        "default values in array destructuring must emit a diagnostic, got: {diags:?}"
    );
}
