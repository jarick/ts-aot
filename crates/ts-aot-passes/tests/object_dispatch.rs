use ts_aot_frontend::FrontendPass;
use ts_aot_passes::{PassContext, convert_program};

fn convert(src: &str) -> (String, Vec<String>, bool) {
    let mut types = ts_aot_core::TypeTable::new();
    let mut ctx = PassContext::new();
    let frontend = FrontendPass::new().run_with_types("test.ts", src, &mut types);
    let mut diags: Vec<String> = frontend
        .diagnostics
        .iter()
        .map(|d| format!("{:?}", d))
        .collect();
    if frontend.diagnostics.has_errors() {
        return (String::new(), diags, true);
    }
    let mut hir = frontend.program;
    ts_aot_passes::lower_enums(&mut hir, &mut types, &mut ctx);
    ts_aot_passes::monomorphize(&mut hir, &mut types, &mut ctx);
    ts_aot_passes::lower_closures(&mut hir, &mut types, &mut ctx);
    let _ = ts_aot_passes::lower_async(&mut hir, &mut types, &mut ctx);
    let mir = convert_program(&hir, &mut types, &mut ctx);
    let has_errors = ctx.has_errors();
    diags.extend(ctx.diagnostics().iter().map(|d| format!("{:?}", d)));
    (mir.dump_text(), diags, has_errors)
}

#[test]
fn object_set_prototype_of_with_side_effecting_prototype_arg_evaluates_the_arg() {
    let (mir, diags, has_errors) = convert(
        r#"
        function makeProto(): i64 { return 1; }
        function f(target: i64): i64 {
            return Object.setPrototypeOf(target, makeProto());
        }
        "#,
    );
    assert!(!has_errors, "must not produce errors, got diags: {diags:?}");
    assert!(
        mir.contains("makeProto"),
        "Object.setPrototypeOf(target, makeProto()) must still emit a call to makeProto \
         (the prototype arg is a side-effecting expression and must be evaluated), got:\r\n{mir}"
    );
    assert!(
        mir.contains("expr "),
        "side-effect preservation must emit a MirStmt::Expr for the prototype arg, got:\r\n{mir}"
    );
    let call_count = mir.matches("call fn(").count();
    assert!(
        call_count >= 1,
        "expected at least 1 MIR Call block (the makeProto arg must be evaluated as a Call); got {call_count} in:\r\n{mir}"
    );
}

#[test]
fn object_set_prototype_of_with_empty_args_emits_e0406() {
    let (mir, diags, _) = convert(r#"function f(): i64 { return Object.setPrototypeOf(); }"#);
    let has_e0406 = diags
        .iter()
        .any(|d| d.contains("E0406") && d.contains("setPrototypeOf"));
    assert!(
        has_e0406,
        "Object.setPrototypeOf() with no args must emit E0406 about empty arg list, got: {diags:?}"
    );
    assert!(
        !mir.contains("set_prototype"),
        "no set_prototype runtime lowering should happen on an E0406 path, got:\r\n{mir}"
    );
}
