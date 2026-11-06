use bumpalo::collections::Vec as BumpVec;
use ts_aot_runtime::{__ts_aot_array_concat, __ts_aot_array_hole, Ctx, Host, run_in_arena};

fn with_ctx<R>(f: impl for<'a> FnOnce(&Ctx<'a>) -> R) -> R {
    struct H;
    impl Host for H {}
    run_in_arena(&H, f)
}

#[test]
fn array_concat_merges_two_vectors() {
    let out = with_ctx(|ctx| {
        let a = BumpVec::from_iter_in([1_i64, 2, 3], ctx.arena);
        let b = BumpVec::from_iter_in([4_i64, 5, 6], ctx.arena);
        let parts = BumpVec::from_iter_in([a, b], ctx.arena);
        __ts_aot_array_concat(ctx, parts).as_slice().to_vec()
    });
    assert_eq!(out, vec![1, 2, 3, 4, 5, 6]);
}

#[test]
fn array_concat_with_empty_vector_preserves_other_parts() {
    let out = with_ctx(|ctx| {
        let a = BumpVec::from_iter_in([1_i64, 2, 3], ctx.arena);
        let b = BumpVec::new_in(ctx.arena);
        let c = BumpVec::from_iter_in([4_i64, 5], ctx.arena);
        let parts = BumpVec::from_iter_in([a, b, c], ctx.arena);
        __ts_aot_array_concat(ctx, parts).as_slice().to_vec()
    });
    assert_eq!(out, vec![1, 2, 3, 4, 5]);
}

#[test]
fn array_concat_with_all_empty_returns_empty() {
    let out = with_ctx(|ctx| {
        let a: BumpVec<i64> = BumpVec::new_in(ctx.arena);
        let b: BumpVec<i64> = BumpVec::new_in(ctx.arena);
        let parts = BumpVec::from_iter_in([a, b], ctx.arena);
        __ts_aot_array_concat(ctx, parts).as_slice().to_vec()
    });
    assert_eq!(out, [] as [i64; 0]);
}

#[test]
fn array_concat_preserves_source_order() {
    let out = with_ctx(|ctx| {
        let a = BumpVec::from_iter_in([1_i64], ctx.arena);
        let b = BumpVec::from_iter_in([2_i64], ctx.arena);
        let c = BumpVec::from_iter_in([3_i64], ctx.arena);
        let parts = BumpVec::from_iter_in([a, b, c], ctx.arena);
        __ts_aot_array_concat(ctx, parts).as_slice().to_vec()
    });
    assert_eq!(out, vec![1, 2, 3]);
}

#[test]
fn array_hole_returns_single_default_value() {
    let out = with_ctx(|ctx| __ts_aot_array_hole::<i64>(ctx).as_slice().to_vec());
    assert_eq!(out, vec![0]);
}

#[test]
fn array_hole_for_f64_returns_zero() {
    let out = with_ctx(|ctx| __ts_aot_array_hole::<f64>(ctx).as_slice().to_vec());
    assert_eq!(out, vec![0.0]);
}

#[test]
fn array_hole_for_string_returns_empty_string() {
    let out = with_ctx(|ctx| __ts_aot_array_hole::<String>(ctx).as_slice().to_vec());
    assert_eq!(out, vec![String::new()]);
}
