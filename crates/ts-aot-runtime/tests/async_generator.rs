use genawaiter::sync::Co;
use ts_aot_runtime::{__ts_aot_async_generator_new, __ts_aot_await, AsyncGenYield};

#[test]
fn async_generator_yields_each_value_in_order() {
    let mut ag =
        __ts_aot_async_generator_new::<i64, _>(|co: Co<AsyncGenYield<i64>, ()>| async move {
            co.yield_(AsyncGenYield::yielded(1)).await;
            co.yield_(AsyncGenYield::yielded(2)).await;
            co.yield_(AsyncGenYield::yielded(3)).await;
            None
        });
    let p1 = ag.next();
    let y1 = __ts_aot_await(&p1);
    assert_eq!(y1.value, Some(1));
    assert!(!y1.done);
    let p2 = ag.next();
    let y2 = __ts_aot_await(&p2);
    assert_eq!(y2.value, Some(2));
    assert!(!y2.done);
    let p3 = ag.next();
    let y3 = __ts_aot_await(&p3);
    assert_eq!(y3.value, Some(3));
    assert!(!y3.done);
}

#[test]
fn async_generator_signals_done_after_last_yield() {
    let mut ag =
        __ts_aot_async_generator_new::<i64, _>(|co: Co<AsyncGenYield<i64>, ()>| async move {
            co.yield_(AsyncGenYield::yielded(10)).await;
            None
        });
    let _ = ag.next();
    let p_done = ag.next();
    let y = __ts_aot_await(&p_done);
    assert!(y.done);
    assert_eq!(y.value, None);
}

#[test]
fn async_generator_with_zero_yields_resolves_done_immediately() {
    let mut ag = __ts_aot_async_generator_new::<i64, _>(
        |_co: Co<AsyncGenYield<i64>, ()>| async move { None },
    );
    let p = ag.next();
    let y = __ts_aot_await(&p);
    assert!(y.done);
    assert_eq!(y.value, None);
}

#[test]
fn async_generator_yields_string_values() {
    let mut ag =
        __ts_aot_async_generator_new::<String, _>(|co: Co<AsyncGenYield<String>, ()>| async move {
            co.yield_(AsyncGenYield::yielded(String::from("hello")))
                .await;
            co.yield_(AsyncGenYield::yielded(String::from("world")))
                .await;
            None
        });
    let y1 = __ts_aot_await(&ag.next());
    assert_eq!(y1.value.as_deref(), Some("hello"));
    assert!(!y1.done);
    let y2 = __ts_aot_await(&ag.next());
    assert_eq!(y2.value.as_deref(), Some("world"));
    assert!(!y2.done);
    let y3 = __ts_aot_await(&ag.next());
    assert!(y3.done);
}

#[test]
fn async_generator_producer_awaits_pending_future_before_first_yield() {
    use std::cell::Cell;
    use std::rc::Rc;
    use ts_aot_runtime::{__ts_aot_promise_create, __ts_aot_promise_resolve};
    let awaited = Rc::new(Cell::new(false));
    let awaited_clone = Rc::clone(&awaited);
    let mut ag =
        __ts_aot_async_generator_new::<i64, _>(move |co: Co<AsyncGenYield<i64>, ()>| async move {
            let p: ts_aot_runtime::Promise<()> = __ts_aot_promise_create();
            __ts_aot_promise_resolve(&p, ());
            let _: () = __ts_aot_await(&p);
            awaited_clone.set(true);
            co.yield_(AsyncGenYield::yielded(42)).await;
            None
        });
    let y1 = __ts_aot_await(&ag.next());
    assert!(
        awaited.get(),
        "producer must have awaited the pending future before the first yield"
    );
    assert_eq!(y1.value, Some(42));
    assert!(!y1.done);
    let y2 = __ts_aot_await(&ag.next());
    assert!(y2.done);
}
