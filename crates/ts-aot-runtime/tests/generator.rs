use ts_aot_runtime::{
    __ts_aot_generator_done, __ts_aot_generator_done_with, __ts_aot_generator_yielded,
    GENERATOR_DONE_STATE, Generator, GeneratorResult,
};

fn simple_dispatch(g: &mut Generator<i64>) -> GeneratorResult<i64> {
    if g.state == 0 {
        g.set_state(1);
        g.store(1);
        GeneratorResult::Yielded(Some(1))
    } else if g.state == 1 {
        g.set_state(2);
        g.store(2);
        GeneratorResult::Yielded(Some(2))
    } else if g.state == 2 {
        g.set_state(GENERATOR_DONE_STATE);
        GeneratorResult::Done(None)
    } else {
        GeneratorResult::Done(None)
    }
}

fn dispatch_via_runtime_helpers(g: &mut Generator<i64>) -> GeneratorResult<i64> {
    if g.state == 0 {
        g.set_state(1);
        g.store(42);
        __ts_aot_generator_yielded(g)
    } else if g.state == 1 {
        g.set_state(GENERATOR_DONE_STATE);
        __ts_aot_generator_done()
    } else {
        GeneratorResult::Done(None)
    }
}

#[test]
fn generator_yields_each_value_then_done() {
    let mut g = Generator::new(simple_dispatch);
    assert_eq!(g.next(), GeneratorResult::Yielded(Some(1)));
    assert_eq!(g.next(), GeneratorResult::Yielded(Some(2)));
    assert_eq!(g.next(), GeneratorResult::Done(None));
    assert_eq!(g.next(), GeneratorResult::Done(None));
}

#[test]
fn generator_done_state_is_max() {
    assert_eq!(GENERATOR_DONE_STATE, u32::MAX);
}

#[test]
fn generator_starts_at_state_zero() {
    let g = Generator::<i64>::new(simple_dispatch);
    assert_eq!(g.state, 0);
    assert!(g.stored.is_none());
}

#[test]
fn generator_dispatch_via_runtime_helpers_returns_stored_value() {
    let mut g = Generator::new(dispatch_via_runtime_helpers);
    assert_eq!(g.next(), GeneratorResult::Yielded(Some(42)));
    assert_eq!(g.next(), GeneratorResult::Done(None));
    assert_eq!(g.next(), GeneratorResult::Done(None));
}

#[test]
fn generator_yielded_helper_returns_stored_value_not_state() {
    let mut g = Generator::<i64>::new(|g| {
        g.store(99);
        __ts_aot_generator_yielded(g)
    });
    assert_eq!(
        g.next(),
        GeneratorResult::Yielded(Some(99)),
        "Yielded must return stored value, not state"
    );
}

#[test]
fn generator_done_helper_returns_done() {
    let mut g = Generator::<i64>::new(|_| __ts_aot_generator_done());
    assert_eq!(g.next(), GeneratorResult::Done(None));
}

#[test]
fn generator_next_short_circuits_when_state_is_done() {
    use std::sync::atomic::{AtomicU32, Ordering};

    static CALLS: AtomicU32 = AtomicU32::new(0);

    fn dispatch(g: &mut Generator<i64>) -> GeneratorResult<i64> {
        CALLS.fetch_add(1, Ordering::SeqCst);
        g.set_state(GENERATOR_DONE_STATE);
        GeneratorResult::Done(None)
    }

    CALLS.store(0, Ordering::SeqCst);
    let mut g = Generator::new(dispatch);
    assert_eq!(g.next(), GeneratorResult::Done(None));
    let calls_after_first_done = CALLS.load(Ordering::SeqCst);
    assert_eq!(
        calls_after_first_done, 1,
        "dispatch must be invoked exactly once before short-circuit"
    );
    for _ in 0..5 {
        assert_eq!(g.next(), GeneratorResult::Done(None));
    }
    assert_eq!(
        CALLS.load(Ordering::SeqCst),
        calls_after_first_done,
        "subsequent next() calls must not re-invoke the dispatch"
    );
}

#[test]
fn generator_next_after_done_returns_done_without_invoking_dispatch() {
    let dispatch: fn(&mut Generator<i64>) -> GeneratorResult<i64> = |g| {
        if g.state == 0 {
            g.set_state(1);
            g.store(7);
            __ts_aot_generator_yielded(g)
        } else if g.state == 1 {
            g.set_state(GENERATOR_DONE_STATE);
            __ts_aot_generator_done()
        } else {
            __ts_aot_generator_done()
        }
    };
    let mut g = Generator::new(dispatch);
    assert_eq!(g.next(), GeneratorResult::Yielded(Some(7)));
    assert_eq!(g.next(), GeneratorResult::Done(None));
    assert_eq!(g.next(), GeneratorResult::Done(None));
    assert_eq!(g.next(), GeneratorResult::Done(None));
}

#[test]
fn generator_done_with_preserves_return_value_through_done_variant() {
    let dispatch: fn(&mut Generator<i64>) -> GeneratorResult<i64> = |g| {
        if g.state == 0 {
            g.set_state(1);
            g.store(10);
            __ts_aot_generator_yielded(g)
        } else if g.state == 1 {
            g.set_state(GENERATOR_DONE_STATE);
            __ts_aot_generator_done_with(99)
        } else {
            __ts_aot_generator_done()
        }
    };
    let mut g = Generator::new(dispatch);
    assert_eq!(g.next(), GeneratorResult::Yielded(Some(10)));
    assert_eq!(
        g.next(),
        GeneratorResult::Done(Some(99)),
        "Done with explicit return value must be preserved in the variant"
    );
}

#[test]
fn generator_bare_yield_yields_none_not_done() {
    let mut g = Generator::<i64>::new(|g| {
        g.set_state(GENERATOR_DONE_STATE);
        __ts_aot_generator_yielded(g)
    });
    assert_eq!(
        g.next(),
        GeneratorResult::Yielded(None),
        "bare yield (no stored value) must still be a yield, not completion"
    );
    assert_eq!(g.next(), GeneratorResult::Done(None));
}

#[test]
fn generator_done_with_and_done_are_distinct_variants() {
    assert_ne!(
        GeneratorResult::Done(None),
        GeneratorResult::Done(Some(0_i64)),
        "Done(None) and Done(Some(_)) must be distinguishable"
    );
    assert_ne!(
        GeneratorResult::<i64>::Yielded(None),
        GeneratorResult::Yielded(Some(0)),
        "Yielded(None) and Yielded(Some(_)) must be distinguishable"
    );
    assert_ne!(
        GeneratorResult::Yielded(Some(0_i64)),
        GeneratorResult::Done(Some(0_i64)),
        "Yielded and Done carrying the same value must be distinguishable"
    );
}
