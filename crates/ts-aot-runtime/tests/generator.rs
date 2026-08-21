use std::cell::RefCell;
use std::rc::Rc;

use ts_aot_runtime::{__ts_aot_generator_new, GeneratorResult};

async fn simple_producer(co: genawaiter::sync::Co<i64, ()>) -> Option<i64> {
    co.yield_(1).await;
    co.yield_(2).await;
    None
}

#[test]
fn generator_yields_each_value_then_done() {
    let mut g = __ts_aot_generator_new(simple_producer);
    assert_eq!(g.next(), GeneratorResult::Yielded(1));
    assert_eq!(g.next(), GeneratorResult::Yielded(2));
    assert_eq!(g.next(), GeneratorResult::Done(None));
    assert_eq!(g.next(), GeneratorResult::Done(None));
}

#[test]
fn generator_completion_value_is_preserved_in_done_variant() {
    let mut g = __ts_aot_generator_new(|co| async move {
        co.yield_(10).await;
        Some(99)
    });
    assert_eq!(g.next(), GeneratorResult::Yielded(10));
    assert_eq!(
        g.next(),
        GeneratorResult::Done(Some(99)),
        "explicit return value must be preserved in the Done variant"
    );
    assert_eq!(
        g.next(),
        GeneratorResult::Done(None),
        "subsequent next() calls must return Done(None) without the completion value"
    );
}

#[test]
fn generator_controls_flow_across_yields() {
    let mut g = __ts_aot_generator_new(|co| async move {
        let mut n = 0;
        while n < 3 {
            co.yield_(n).await;
            n += 1;
        }
        Some(n)
    });
    assert_eq!(g.next(), GeneratorResult::Yielded(0));
    assert_eq!(g.next(), GeneratorResult::Yielded(1));
    assert_eq!(g.next(), GeneratorResult::Yielded(2));
    assert_eq!(g.next(), GeneratorResult::Done(Some(3)));
}

#[test]
fn generator_captures_constructor_arguments() {
    let make = |factor: i64| {
        __ts_aot_generator_new(move |co| async move {
            co.yield_(2 * factor).await;
            None
        })
    };
    let mut g = make(21);
    assert_eq!(g.next(), GeneratorResult::Yielded(42));
    assert_eq!(g.next(), GeneratorResult::Done(None));
}

#[test]
fn generator_next_short_circuits_after_completion() {
    use std::sync::atomic::{AtomicU32, Ordering};

    static POLLS: AtomicU32 = AtomicU32::new(0);

    let mut g = __ts_aot_generator_new::<i64, _>(|_co| async move {
        POLLS.fetch_add(1, Ordering::SeqCst);
        None
    });

    POLLS.store(0, Ordering::SeqCst);
    assert_eq!(g.next(), GeneratorResult::Done(None));
    let polls_after_done = POLLS.load(Ordering::SeqCst);
    assert!(polls_after_done >= 1, "producer must run to completion");
    for _ in 0..5 {
        assert_eq!(g.next(), GeneratorResult::Done(None));
    }
    assert_eq!(
        POLLS.load(Ordering::SeqCst),
        polls_after_done,
        "subsequent next() calls must not poll the completed producer again"
    );
}

#[test]
fn generator_done_with_and_done_are_distinct_variants() {
    assert_ne!(
        GeneratorResult::Done(None),
        GeneratorResult::Done(Some(0_i64)),
        "Done(None) and Done(Some(_)) must be distinguishable"
    );
    assert_ne!(
        GeneratorResult::Yielded(0_i64),
        GeneratorResult::Done(Some(0_i64)),
        "Yielded and Done carrying the same value must be distinguishable"
    );
}

#[test]
fn generator_into_iter_yields_each_value_then_terminates() {
    let g = __ts_aot_generator_new(simple_producer);
    let collected: Vec<i64> = g.into_iter().collect();
    assert_eq!(collected, vec![1, 2], "for-of must yield 1, 2 then stop");
}

#[test]
fn generator_into_iter_for_loop_terminates_immediately_for_empty_generator() {
    let g = __ts_aot_generator_new::<i64, _>(|_co| async move { None });
    let mut count = 0;
    for _ in g {
        count += 1;
    }
    assert_eq!(count, 0, "empty generator must iterate 0 times");
}

#[test]
fn generator_into_iter_into_iter_type_alias_is_usable() {
    let g = __ts_aot_generator_new(simple_producer);
    let iter = g.into_iter();
    assert_eq!(
        iter.count(),
        2,
        "type alias must be reachable for downstream code"
    );
}

#[test]
fn generator_new_free_fn_constructs_a_working_generator() {
    let mut g = __ts_aot_generator_new(simple_producer);
    assert_eq!(g.next(), GeneratorResult::Yielded(1));
    assert_eq!(g.next(), GeneratorResult::Yielded(2));
    assert_eq!(g.next(), GeneratorResult::Done(None));
}

#[test]
fn generator_into_iter_for_mut_ref_yields_each_value_then_terminates() {
    let mut g = __ts_aot_generator_new(simple_producer);
    let collected: Vec<i64> = (&mut g).into_iter().collect();
    assert_eq!(
        collected,
        vec![1, 2],
        "for-of via &mut must yield 1, 2 then stop"
    );
    assert_eq!(
        g.next(),
        GeneratorResult::Done(None),
        "generator must be finished after a borrowing for-of (completion preserved)"
    );
}

#[test]
fn generator_into_iter_for_mut_ref_supports_for_loop() {
    let mut g = __ts_aot_generator_new(simple_producer);
    let mut count = 0;
    for v in &mut g {
        assert!(v == 1 || v == 2);
        count += 1;
    }
    assert_eq!(count, 2, "borrowing for-of must iterate 2 times");
}

#[test]
fn generator_into_iter_for_mut_ref_handles_empty_generator() {
    let mut g = __ts_aot_generator_new::<i64, _>(|_co| async move { None });
    let mut count = 0;
    for _ in &mut g {
        count += 1;
    }
    assert_eq!(count, 0, "empty generator must iterate 0 times via &mut");
}

#[test]
fn generator_type_is_usable_as_a_named_binding() {
    let mut g = __ts_aot_generator_new(simple_producer);
    assert_eq!(g.next(), GeneratorResult::Yielded(1));
}

#[test]
fn generator_iter_mut_returns_each_value() {
    let mut g = __ts_aot_generator_new(simple_producer);
    let collected: Vec<i64> = g.iter_mut().collect();
    assert_eq!(collected, vec![1, 2], "iter_mut must yield 1, 2 then stop");
    assert_eq!(
        g.next(),
        GeneratorResult::Done(None),
        "generator must be completed after an iter_mut() borrowing iteration"
    );
}

#[test]
fn generator_iter_mut_partial_iteration_resumes_correctly() {
    let mut g = __ts_aot_generator_new(simple_producer);
    let mut iter = g.iter_mut();
    assert_eq!(iter.next(), Some(1), "first yielded value via iter_mut");
    let second = g.next();
    assert_eq!(
        second,
        GeneratorResult::Yielded(2),
        "after partial iter, next() via .next() must resume from the second yield"
    );
    assert_eq!(g.next(), GeneratorResult::Done(None));
}

#[test]
fn bare_yield_still_produces_one_item_per_yield_in_owned_iteration() {
    let g = __ts_aot_generator_new::<(), _>(|co| async move {
        co.yield_(()).await;
        co.yield_(()).await;
        None
    });
    let items: Vec<()> = g.into_iter().collect();
    assert_eq!(
        items.len(),
        2,
        "bare `yield;` must produce one unit item per yield, not terminate iteration"
    );
}

#[test]
fn bare_yield_still_produces_one_item_per_yield_in_mut_ref_iteration() {
    let mut g = __ts_aot_generator_new::<(), _>(|co| async move {
        co.yield_(()).await;
        None
    });
    let mut count = 0;
    for () in &mut g {
        count += 1;
    }
    assert_eq!(count, 1, "bare `yield;` must iterate once via &mut");
    assert_eq!(
        g.next(),
        GeneratorResult::Done(None),
        "generator must be completed after the bare-yield iteration"
    );
}

#[test]
#[should_panic(expected = "producer panic")]
fn generator_does_not_resume_panicked_producer() {
    let mut g = __ts_aot_generator_new::<i64, _>(|_co| async move {
        panic!("producer panic");
    });
    let _ = g.next();
}

#[test]
fn generator_after_producer_panic_is_finished() {
    use std::panic::{AssertUnwindSafe, catch_unwind};
    let mut g = __ts_aot_generator_new::<i64, _>(|_co| async move {
        panic!("producer panic");
    });
    let first = catch_unwind(AssertUnwindSafe(|| g.next()));
    assert!(first.is_err(), "first call must propagate the panic");
    let second = g.next();
    assert_eq!(
        second,
        GeneratorResult::Done(None),
        "after panic, generator must be finished and return Done(None) without resuming"
    );
}

#[test]
fn generator_accepts_producer_holding_non_send_value_across_await() {
    let counter = Rc::new(RefCell::new(0_i64));
    let counter_for_producer = Rc::clone(&counter);
    let mut g = __ts_aot_generator_new(move |co| async move {
        co.yield_(10).await;
        *counter_for_producer.borrow_mut() += 1;
        co.yield_(20).await;
        *counter_for_producer.borrow_mut() += 1;
        None
    });
    assert_eq!(g.next(), GeneratorResult::Yielded(10));
    assert_eq!(g.next(), GeneratorResult::Yielded(20));
    assert_eq!(
        *counter.borrow(),
        1,
        "Rc must survive the first await point and be observable after the producer resumes"
    );
    assert_eq!(g.next(), GeneratorResult::Done(None));
    assert_eq!(
        *counter.borrow(),
        2,
        "Rc must survive the second await point and remain usable after the producer completes"
    );
}

#[test]
fn generator_holding_non_send_value_remains_usable() {
    let counter = Rc::new(RefCell::new(0_i64));
    let counter_for_producer = Rc::clone(&counter);
    let mut g = __ts_aot_generator_new(move |co| async move {
        *counter_for_producer.borrow_mut() += 1;
        co.yield_(7).await;
        None
    });
    assert_eq!(g.next(), GeneratorResult::Yielded(7));
    assert_eq!(*counter.borrow(), 1, "Rc must survive the await point");
    assert_eq!(g.next(), GeneratorResult::Done(None));
}
