use super::common::*;

#[test]
fn converter_starts_with_empty_state() {
    let c = ExprConverter::new();
    assert_eq!(c.peek_next_local(), 0);
}

#[test]
fn default_matches_new() {
    let a = ExprConverter::default();
    let b = ExprConverter::new();
    assert_eq!(a.peek_next_local(), b.peek_next_local());
}

#[test]
fn fresh_local_increments_counter() {
    let mut c = ExprConverter::new();
    let l0 = c.map_local_id(LocalId::from_raw(0));
    let l1 = c.map_local_id(LocalId::from_raw(1));
    assert_ne!(l0, l1);
    assert_eq!(c.peek_next_local(), 2);
}

#[test]
fn with_function_remap_and_offset_starts_past_offset() {
    let c = ExprConverter::with_function_remap_and_offset(HashMap::new(), 5);
    assert_eq!(c.peek_next_local(), 5);
    let c2 = ExprConverter::with_function_remap(HashMap::new());
    assert_eq!(c2.peek_next_local(), 0);
}

#[test]
fn seed_params_advances_next_local_past_param_count() {
    let mut c = ExprConverter::with_function_remap_and_offset(HashMap::new(), 0);
    c.seed_params(3);
    assert_eq!(c.peek_next_local(), 3);
    let fresh = c.map_local_id(LocalId::from_raw(99));
    assert_eq!(fresh, LocalId::from_raw(3));
}

#[test]
fn map_local_returns_same_id_for_same_old() {
    let mut c = ExprConverter::new();
    let src = LocalId::from_raw(42);
    let a = c.map_local(src);
    let b = c.map_local(src);
    assert_eq!(a, b);
    assert_eq!(c.peek_next_local(), 1);
}

#[test]
fn map_local_id_returns_local_id() {
    let mut c = ExprConverter::new();
    let old = LocalId::from_raw(7);
    let new = c.map_local_id(old);
    assert_eq!(c.map_local_id(old), new);
}

#[test]
fn register_local_name_does_not_panic() {
    let mut c = ExprConverter::new();
    let id = LocalId::from_raw(0);
    c.register_local_name(id, Atom::new_inline("11"));
    assert_eq!(
        c.local_name(id),
        Some(Atom::new_inline("11")),
        "register_local_name must record the (LocalId, Atom) mapping; the lookup path is the \
         narrowest observable API to verify the registration took effect"
    );
}

#[test]
fn resolve_callee_function_uses_remap() {
    let mut remap = HashMap::new();
    remap.insert(FunctionId::from_raw(3), FunctionId::from_raw(99));
    let mut c = ExprConverter::with_function_remap(remap);
    let mut cx = ctx();
    let fid = c.resolve_callee(&HirCallee::Function(FunctionId::from_raw(3)), &mut cx);
    assert_eq!(fid, FunctionId::from_raw(99));
}

#[test]
fn resolve_callee_function_without_remap_returns_input() {
    let mut c = ExprConverter::new();
    let mut cx = ctx();
    let fid = c.resolve_callee(&HirCallee::Function(FunctionId::from_raw(7)), &mut cx);
    assert_eq!(fid, FunctionId::from_raw(7));
}

#[test]
fn resolve_callee_indirect_is_placeholder_and_warning() {
    let mut c = ExprConverter::new();
    let mut cx = ctx();
    let fid = c.resolve_callee(&HirCallee::Indirect(Box::new(int_lit(1))), &mut cx);
    assert_eq!(fid, PLACEHOLDER_FUNCTION);
    assert!(
        !cx.has_errors(),
        "PR 1.2: unresolved indirect callee downgrades P0005 to warning (runtime fallback handles it)"
    );
    let p0005_count = cx
        .diagnostics()
        .iter()
        .filter(|d| d.code.as_str() == "P0005")
        .count();
    assert_eq!(
        p0005_count, 1,
        "P0005 must still be emitted as a warning, got {p0005_count} diags"
    );
}

#[test]
fn resolve_callee_closure_is_placeholder_and_diagnostics() {
    let mut c = ExprConverter::new();
    let mut cx = ctx();
    let fid = c.resolve_callee(&HirCallee::Closure(LocalId::from_raw(0)), &mut cx);
    assert_eq!(fid, PLACEHOLDER_FUNCTION);
    assert!(cx.has_errors());
}

#[test]
fn resolve_callee_runtime_is_placeholder_and_diagnostics() {
    let mut c = ExprConverter::new();
    let mut cx = ctx();
    let fid = c.resolve_callee(
        &HirCallee::Runtime {
            name: Atom::new_inline("0"),
            ty: TypeId::from_raw(0),
        },
        &mut cx,
    );
    assert_eq!(fid, PLACEHOLDER_FUNCTION);
    assert!(cx.has_errors());
}
