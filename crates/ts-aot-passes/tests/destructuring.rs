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

fn array_read_is_optional(op: &RuntimeOp) -> Option<bool> {
    match op {
        RuntimeOp::ArrayGetOrDefault => Some(false),
        RuntimeOp::ArrayGetOptional => Some(true),
        _ => None,
    }
}

fn collect_array_element_reads(mir: &MirProgram) -> Vec<(i64, bool)> {
    fn walk_block(block: &MirBlock, reads: &mut Vec<(i64, bool)>) {
        for s in &block.stmts {
            if let MirStmt::Runtime { op, args, .. } = s
                && let Some(optional) = array_read_is_optional(op)
                && let Some(ts_aot_ir_mir::MirExpr::Int { value, .. }) = args.get(1)
            {
                reads.push((*value as i64, optional));
            }
            match s {
                MirStmt::If {
                    then_block,
                    else_block,
                    ..
                } => {
                    walk_block(then_block, reads);
                    if let Some(eb) = else_block {
                        walk_block(eb, reads);
                    }
                }
                MirStmt::While { body, .. }
                | MirStmt::DoWhile { body, .. }
                | MirStmt::ForOf { body, .. }
                | MirStmt::ForIn { body, .. } => walk_block(body, reads),
                _ => {}
            }
        }
    }
    let mut reads = Vec::new();
    for d in &mir.declarations {
        if let ts_aot_ir_mir::MirDecl::Function(f) = d {
            walk_block(&f.body.block, &mut reads);
        }
    }
    reads
}

#[test]
fn array_destructuring_emits_one_runtime_call_per_named_element_with_correct_indices() {
    let (mir, diags) = convert(
        r#"
        function f(arr: i64[]): i64 {
            let [a, b, c] = arr;
            return (a === undefined ? 0 : 1) + (b === undefined ? 0 : 1) + (c === undefined ? 0 : 1);
        }
        "#,
    );
    assert!(diags.is_empty(), "diags: {diags:?}");
    let reads = collect_array_element_reads(&mir);
    assert_eq!(
        reads,
        vec![(0, true), (1, true), (2, true)],
        "indices must be 0, 1, 2 in order, and every element of a parameter array is unprovable, \
         so each binding must be read through `ArrayGetOptional`; accepting `ArrayGetOrDefault` \
         here would let a revert of the optional gate keep this test green"
    );
}

#[test]
fn array_destructuring_with_skip_hole_emits_calls_only_for_named_indices() {
    let (mir, diags) = convert(
        r#"
        function f(arr: i64[]): i64 {
            let [a, , b] = arr;
            return (a === undefined ? 0 : 1) + (b === undefined ? 0 : 1);
        }
        "#,
    );
    assert!(diags.is_empty(), "diags: {diags:?}");
    let reads = collect_array_element_reads(&mir);
    assert_eq!(
        reads,
        vec![(0, true), (2, true)],
        "hole at index 1 is skipped, and both remaining elements of a parameter array must be read \
         through `ArrayGetOptional`; accepting `ArrayGetOrDefault` here would let a revert of the \
         optional gate keep this test green"
    );
}

#[test]
fn a_provably_present_element_reads_through_the_default_accessor() {
    let (mir, diags) = convert(
        r#"
        function f(): i64 {
            const xs: i64[] = [1, 2, 3];
            const [a, b, c] = xs;
            return a + b + c;
        }
        "#,
    );
    assert!(diags.is_empty(), "diags: {diags:?}");
    let reads = collect_array_element_reads(&mir);
    assert_eq!(
        reads,
        vec![(0, false), (1, false), (2, false)],
        "an element the array literal proves present must stay a plain `i64` read through \
         `ArrayGetOrDefault`; the optional gate must not turn it into `Option<i64>`"
    );
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
