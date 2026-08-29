use std::cell::RefCell;
use std::rc::Rc;
use ts_aot_runtime::{
    __ts_aot_await, __ts_aot_dynamic_import, __ts_aot_module_register, __ts_aot_promise_all,
    __ts_aot_promise_all_settled, __ts_aot_promise_any, __ts_aot_promise_catch,
    __ts_aot_promise_catch_value, __ts_aot_promise_create, __ts_aot_promise_finally,
    __ts_aot_promise_finally_value, __ts_aot_promise_race, __ts_aot_promise_reject,
    __ts_aot_promise_reject_value, __ts_aot_promise_resolve, __ts_aot_promise_resolve_value,
    __ts_aot_promise_then, __ts_aot_promise_then_value, __ts_aot_runtime_run, AggregateError,
    Promise,
};

#[derive(Clone, Debug, PartialEq)]
struct ModAlpha {
    answer: i64,
    name: String,
}

fn shared_capture() -> Rc<RefCell<Option<i64>>> {
    Rc::new(RefCell::new(None))
}

fn block<T: std::fmt::Debug>(future: impl std::future::Future<Output = T>) -> T {
    __ts_aot_runtime_run(future)
}

fn flush_microtasks() {
    block(async {});
}

#[test]
fn promise_create_then_resolve_yields_value_on_await() {
    let p: Promise<i64> = __ts_aot_promise_create();
    __ts_aot_promise_resolve(&p, 42);
    assert_eq!(block(__ts_aot_await(&p)).unwrap(), 42);
}

#[test]
fn promise_then_on_resolved_fires_callback_via_microtask() {
    let p: Promise<i64> = __ts_aot_promise_create();
    let captured = shared_capture();
    let captured_clone = Rc::clone(&captured);
    __ts_aot_promise_then(
        &p,
        Box::new(move |r: Result<i64, String>| {
            if let Ok(v) = r {
                *captured_clone.borrow_mut() = Some(v);
            }
        }),
    );
    assert!(
        captured.borrow().is_none(),
        "callback must not fire before resolve; microtask scheduling requires the runtime run loop"
    );
    __ts_aot_promise_resolve(&p, 7);
    let value = block(__ts_aot_await(&p)).unwrap();
    assert_eq!(value, 7);
    assert_eq!(*captured.borrow(), Some(7));
}

#[test]
fn promise_then_on_pending_queues_callback_for_resolve() {
    let p: Promise<String> = __ts_aot_promise_create();
    let captured: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
    let captured_clone = Rc::clone(&captured);
    __ts_aot_promise_then(
        &p,
        Box::new(move |r: Result<String, String>| {
            if let Ok(v) = r {
                *captured_clone.borrow_mut() = Some(v);
            }
        }),
    );
    assert!(captured.borrow().is_none(), "pending promise must not fire");
    __ts_aot_promise_resolve(&p, "later".to_owned());
    let value = block(__ts_aot_await(&p)).unwrap();
    assert_eq!(value, "later");
    assert_eq!(*captured.borrow(), Some("later".to_owned()));
}

#[test]
fn promise_reject_transitions_to_rejected_and_await_returns_err() {
    let p: Promise<i64> = __ts_aot_promise_create();
    __ts_aot_promise_reject(&p, "boom".to_owned());
    let result = block(__ts_aot_await(&p));
    let err = result.expect_err("await on rejected promise must return Err");
    assert!(err.contains("boom"), "Err must carry reason, got: {err}");
}

#[test]
fn promise_reject_then_fires_callback_with_reason() {
    let p: Promise<i64> = __ts_aot_promise_create();
    let captured: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
    let captured_clone = Rc::clone(&captured);
    __ts_aot_promise_then(
        &p,
        Box::new(move |r: Result<i64, String>| {
            *captured_clone.borrow_mut() = r.err();
        }),
    );
    __ts_aot_promise_reject(&p, "nope".to_owned());
    let result = block(__ts_aot_await(&p));
    assert!(result.is_err());
    assert_eq!(captured.borrow().as_deref(), Some("nope"));
}

#[test]
fn promise_resolve_after_reject_is_no_op() {
    let p: Promise<String> = __ts_aot_promise_create();
    __ts_aot_promise_reject(&p, "first".to_owned());
    __ts_aot_promise_resolve(&p, "second".to_owned());
    let result = block(__ts_aot_await(&p));
    let err = result.expect_err("first reject must win, second resolve is no-op");
    assert!(err.contains("first"), "first reject must win, got: {err}");
}

#[test]
fn promise_await_on_pending_suspends_until_resolved() {
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::task::{Context, Poll, Wake, Waker};

    struct FlagWaker(AtomicBool);
    impl Wake for FlagWaker {
        fn wake(self: Arc<Self>) {
            self.0.store(true, Ordering::SeqCst);
        }
        fn wake_by_ref(self: &Arc<Self>) {
            self.0.store(true, Ordering::SeqCst);
        }
    }

    let p: Promise<i64> = __ts_aot_promise_create();
    let flag = Arc::new(FlagWaker(AtomicBool::new(false)));
    let waker: Waker = Arc::clone(&flag).into();
    let mut cx = Context::from_waker(&waker);
    let mut future = __ts_aot_await(&p);

    let first = Pin::new(&mut future).poll(&mut cx);
    assert!(
        matches!(first, Poll::Pending),
        "first poll on a pending promise must suspend (registering the waker), got Ready"
    );
    assert!(
        p.waker_count() > 0,
        "first poll must register the waker in the promise's wakers list"
    );
    assert!(
        !flag.0.load(Ordering::SeqCst),
        "waker must not have fired before the promise is resolved"
    );

    __ts_aot_promise_resolve(&p, 99);

    assert!(
        flag.0.load(Ordering::SeqCst),
        "waker must have fired after the promise is resolved"
    );

    let second = Pin::new(&mut future).poll(&mut cx);
    assert!(
        matches!(second, Poll::Ready(Ok(99))),
        "second poll after the promise is resolved must yield the value 99, got {second:?}"
    );
}

#[test]
fn dynamic_import_unknown_specifier_rejects_promise() {
    let p: Promise<ModAlpha> = __ts_aot_dynamic_import("nonexistent.ts");
    let result = block(__ts_aot_await(&p));
    let err = result.expect_err("await on missing-module promise must return Err");
    assert!(err.contains("nonexistent.ts") && err.contains("not registered"));
}

#[test]
fn dynamic_import_registered_module_returns_typed_namespace() {
    let ns = ModAlpha {
        answer: 42,
        name: "alpha".to_owned(),
    };
    __ts_aot_module_register("./fixtures/alpha.ts", ns.clone());

    let p: Promise<ModAlpha> = __ts_aot_dynamic_import("./fixtures/alpha.ts");
    let loaded = block(__ts_aot_await(&p)).unwrap();
    assert_eq!(loaded, ns);
}

#[test]
fn dynamic_import_wrong_type_rejects_promise() {
    let ns = ModAlpha {
        answer: 1,
        name: "alpha".to_owned(),
    };
    __ts_aot_module_register("./fixtures/beta.ts", ns);

    let p: Promise<String> = __ts_aot_dynamic_import("./fixtures/beta.ts");
    let result = block(__ts_aot_await(&p));
    let err = result.expect_err("wrong-type await must return Err");
    assert!(err.contains("beta.ts") && err.contains("does not match"));
}

#[test]
fn promise_all_resolves_with_all_values_when_all_fulfill() {
    let p1: Promise<i64> = __ts_aot_promise_create();
    let p2: Promise<i64> = __ts_aot_promise_create();
    let p3: Promise<i64> = __ts_aot_promise_create();
    __ts_aot_promise_resolve(&p1, 1);
    __ts_aot_promise_resolve(&p2, 2);
    __ts_aot_promise_resolve(&p3, 3);
    let combined = __ts_aot_promise_all(vec![p1, p2, p3]);
    assert_eq!(block(__ts_aot_await(&combined)).unwrap(), vec![1, 2, 3]);
}

#[test]
fn promise_all_rejects_on_first_rejection() {
    let p1: Promise<i64> = __ts_aot_promise_create();
    let p2: Promise<i64> = __ts_aot_promise_create();
    let p3: Promise<i64> = __ts_aot_promise_create();
    __ts_aot_promise_resolve(&p1, 1);
    __ts_aot_promise_reject(&p2, "p2-failed".to_owned());
    __ts_aot_promise_resolve(&p3, 3);
    let combined = __ts_aot_promise_all(vec![p1, p2, p3]);
    let result = block(__ts_aot_await(&combined));
    let err = result.expect_err("await on rejected promise must return Err");
    assert!(err.contains("p2-failed"), "expected p2 failure, got: {err}");
}

#[test]
fn promise_all_empty_resolves_with_empty_vec() {
    let combined: Promise<Vec<i64>> = __ts_aot_promise_all(vec![]);
    assert_eq!(block(__ts_aot_await(&combined)).unwrap(), Vec::<i64>::new());
}

#[test]
fn promise_race_settles_on_first_to_resolve() {
    let fast: Promise<i64> = __ts_aot_promise_create();
    let slow: Promise<i64> = __ts_aot_promise_create();
    __ts_aot_promise_resolve(&fast, 1);
    __ts_aot_promise_resolve(&slow, 99);
    let race = __ts_aot_promise_race(vec![fast, slow]);
    assert_eq!(block(__ts_aot_await(&race)).unwrap(), 1);
}

#[test]
fn promise_all_settled_collects_fulfilled_and_rejected_results() {
    let p1: Promise<i64> = __ts_aot_promise_create();
    let p2: Promise<i64> = __ts_aot_promise_create();
    let p3: Promise<i64> = __ts_aot_promise_create();
    __ts_aot_promise_resolve(&p1, 10);
    __ts_aot_promise_reject(&p2, "boom".to_owned());
    __ts_aot_promise_resolve(&p3, 30);
    let combined = __ts_aot_promise_all_settled(vec![p1, p2, p3]);
    let result = block(__ts_aot_await(&combined)).unwrap();
    assert_eq!(result.len(), 3);
    assert_eq!(result[0].status, "fulfilled");
    assert_eq!(result[0].value, Some(10));
    assert_eq!(result[0].reason, None);
    assert_eq!(result[1].status, "rejected");
    assert_eq!(result[1].value, None);
    assert_eq!(result[1].reason.as_deref(), Some("boom"));
    assert_eq!(result[2].status, "fulfilled");
    assert_eq!(result[2].value, Some(30));
    assert_eq!(result[2].reason, None);
}

#[test]
fn promise_any_resolves_with_first_fulfilled_and_ignores_rest() {
    let p1: Promise<i64> = __ts_aot_promise_create();
    let p2: Promise<i64> = __ts_aot_promise_create();
    let p3: Promise<i64> = __ts_aot_promise_create();
    __ts_aot_promise_reject(&p1, "nope".to_owned());
    __ts_aot_promise_resolve(&p2, 7);
    __ts_aot_promise_reject(&p3, "still-nope".to_owned());
    let any = __ts_aot_promise_any(vec![p1, p2, p3]);
    assert_eq!(block(__ts_aot_await(&any)).unwrap(), 7);
}

#[test]
fn promise_any_rejects_with_aggregate_error_when_every_input_rejects() {
    let p1: Promise<i64> = __ts_aot_promise_create();
    let p2: Promise<i64> = __ts_aot_promise_create();
    let p3: Promise<i64> = __ts_aot_promise_create();
    __ts_aot_promise_reject(&p1, "first-fail".to_owned());
    __ts_aot_promise_reject(&p2, "second-fail".to_owned());
    __ts_aot_promise_reject(&p3, "third-fail".to_owned());
    let any = __ts_aot_promise_any(vec![p1, p2, p3]);
    let err =
        block(__ts_aot_await(&any)).expect_err("Promise.any of all-rejected inputs must reject");
    assert!(
        err.contains("AggregateError"),
        "all-rejected Promise.any must surface AggregateError; got: {err}"
    );
    for reason in ["first-fail", "second-fail", "third-fail"] {
        assert!(
            err.contains(reason),
            "AggregateError must include rejection reason `{reason}`; got: {err}"
        );
    }
}

#[test]
fn promise_catch_invokes_handler_on_rejection_only() {
    let captured = Rc::new(RefCell::new(None));
    {
        let captured = Rc::clone(&captured);
        let p: Promise<i64> = __ts_aot_promise_create();
        __ts_aot_promise_reject(&p, "rejected".to_owned());
        __ts_aot_promise_catch(&p, move |e| {
            *captured.borrow_mut() = Some(e);
        });
    }
    flush_microtasks();
    assert_eq!(captured.borrow().as_deref(), Some("rejected"));
}

#[test]
fn promise_catch_skips_handler_on_fulfillment() {
    let invoked = Rc::new(RefCell::new(false));
    {
        let invoked = Rc::clone(&invoked);
        let p: Promise<i64> = __ts_aot_promise_create();
        __ts_aot_promise_resolve(&p, 42);
        __ts_aot_promise_catch(&p, move |_| {
            *invoked.borrow_mut() = true;
        });
    }
    flush_microtasks();
    assert!(
        !*invoked.borrow(),
        "catch handler must not run for fulfilled promise"
    );
}

#[test]
fn promise_finally_runs_for_both_fulfillment_and_rejection() {
    let counter = Rc::new(RefCell::new(0u32));
    {
        let counter = Rc::clone(&counter);
        let p: Promise<i64> = __ts_aot_promise_create();
        __ts_aot_promise_resolve(&p, 1);
        __ts_aot_promise_finally(&p, move || {
            *counter.borrow_mut() += 1;
        });
    }
    {
        let counter = Rc::clone(&counter);
        let p: Promise<i64> = __ts_aot_promise_create();
        __ts_aot_promise_reject(&p, "bad".to_owned());
        __ts_aot_promise_finally(&p, move || {
            *counter.borrow_mut() += 1;
        });
    }
    flush_microtasks();
    assert_eq!(*counter.borrow(), 2);
}

#[test]
fn promise_resolve_value_returns_fulfilled_promise_with_value() {
    let p: Promise<i64> = __ts_aot_promise_resolve_value(123);
    assert_eq!(block(__ts_aot_await(&p)).unwrap(), 123);
}

#[test]
fn promise_resolve_value_with_string_returns_fulfilled_promise() {
    let p: Promise<String> = __ts_aot_promise_resolve_value("ok".to_owned());
    assert_eq!(block(__ts_aot_await(&p)).unwrap(), "ok".to_owned());
}

#[test]
fn promise_reject_value_returns_rejected_promise_with_reason() {
    let p: Promise<i64> = __ts_aot_promise_reject_value("nope".to_owned());
    let result = block(__ts_aot_await(&p));
    let err = result.expect_err("await on rejected promise must return Err");
    assert!(err.contains("nope"), "Err must carry reason, got: {err}");
}

#[test]
fn promise_resolve_value_is_independent_of_other_promises() {
    let p1: Promise<i64> = __ts_aot_promise_resolve_value(1);
    let p2: Promise<i64> = __ts_aot_promise_resolve_value(2);
    assert_eq!(block(__ts_aot_await(&p1)).unwrap(), 1);
    assert_eq!(block(__ts_aot_await(&p2)).unwrap(), 2);
}

#[test]
fn aggregate_error_display_includes_all_reasons() {
    let err = AggregateError::new(vec!["e1".to_owned(), "e2".to_owned()]);
    let rendered = format!("{err}");
    assert!(rendered.contains("AggregateError"));
    assert!(rendered.contains("e1"));
    assert!(rendered.contains("e2"));
}

#[test]
fn aggregate_error_constructs_with_empty_errors() {
    let err = AggregateError::new(Vec::new());
    assert!(err.errors.is_empty());
    assert_eq!(format!("{err}"), "AggregateError: []");
}

#[test]
fn microtask_runs_then_callback_after_sync_code() {
    let order: Rc<RefCell<Vec<&'static str>>> = Rc::new(RefCell::new(Vec::new()));
    let p: Promise<i64> = __ts_aot_promise_create();
    let order_for_then = Rc::clone(&order);
    __ts_aot_promise_then(
        &p,
        Box::new(move |_r: Result<i64, String>| {
            order_for_then.borrow_mut().push("then");
        }),
    );
    order.borrow_mut().push("sync-before-resolve");
    __ts_aot_promise_resolve(&p, 0);
    order.borrow_mut().push("sync-after-resolve");
    block(async {
        __ts_aot_await(&p).await.ok();
    });
    let snapshot = order.borrow().clone();
    assert_eq!(
        snapshot,
        vec!["sync-before-resolve", "sync-after-resolve", "then"],
        "then-callback must run in microtask AFTER current sync code; got {snapshot:?}"
    );
}

#[test]
fn promise_then_value_invokes_named_handler_with_value() {
    let captured: Rc<RefCell<Option<i64>>> = Rc::new(RefCell::new(None));
    {
        let captured = Rc::clone(&captured);
        let p: Promise<i64> = __ts_aot_promise_create();
        __ts_aot_promise_then_value(&p, move |v: i64| -> i64 {
            *captured.borrow_mut() = Some(v * 2);
            v
        });
        __ts_aot_promise_resolve(&p, 21);
    }
    flush_microtasks();
    assert_eq!(*captured.borrow(), Some(42));
}

#[test]
fn promise_then_value_changes_fulfilled_type_number_to_string() {
    let p: Promise<i64> = __ts_aot_promise_create();
    let mapped: Promise<String> =
        __ts_aot_promise_then_value(&p, move |v: i64| -> String { v.to_string() });
    __ts_aot_promise_resolve(&p, 42);
    let result: Result<String, String> = block(__ts_aot_await(&mapped));
    assert_eq!(
        result.unwrap(),
        "42",
        "number->string then handler must propagate the mapped String through the new \
         Promise<String>; regression: __ts_aot_promise_then_value forced F: FnOnce(T) -> T \
         and returned Promise<T>, blocking any type change (Promise<number> -> Promise<string>)"
    );
}

#[test]
fn promise_then_value_skips_handler_on_rejection() {
    let invoked = Rc::new(RefCell::new(false));
    {
        let invoked = Rc::clone(&invoked);
        let p: Promise<i64> = __ts_aot_promise_create();
        __ts_aot_promise_then_value(&p, move |_v: i64| -> i64 {
            *invoked.borrow_mut() = true;
            0
        });
        __ts_aot_promise_reject(&p, "boom".to_owned());
    }
    flush_microtasks();
    assert!(
        !*invoked.borrow(),
        "then handler must not fire on rejected promise"
    );
}

#[test]
fn promise_catch_value_invokes_named_handler_only_on_rejection() {
    let invoked = Rc::new(RefCell::new(false));
    {
        let invoked = Rc::clone(&invoked);
        let p: Promise<i64> = __ts_aot_promise_create();
        __ts_aot_promise_catch_value(&p, move |_e: String| -> i64 {
            *invoked.borrow_mut() = true;
            0
        });
        __ts_aot_promise_resolve(&p, 42);
    }
    flush_microtasks();
    assert!(
        !*invoked.borrow(),
        "catch handler must not run for fulfilled promise"
    );
}

#[test]
fn promise_catch_value_runs_handler_on_rejection() {
    let captured: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
    {
        let captured = Rc::clone(&captured);
        let p: Promise<i64> = __ts_aot_promise_create();
        __ts_aot_promise_catch_value(&p, move |e: String| -> i64 {
            *captured.borrow_mut() = Some(e);
            0
        });
        __ts_aot_promise_reject(&p, "captured-error".to_owned());
    }
    flush_microtasks();
    assert_eq!(captured.borrow().as_deref(), Some("captured-error"));
}

#[test]
fn promise_finally_value_runs_for_both_settlement_kinds() {
    let counter = Rc::new(RefCell::new(0u32));
    {
        let counter = Rc::clone(&counter);
        let p: Promise<i64> = __ts_aot_promise_create();
        __ts_aot_promise_finally_value(&p, move || {
            *counter.borrow_mut() += 1;
        });
        __ts_aot_promise_resolve(&p, 1);
    }
    {
        let counter = Rc::clone(&counter);
        let p: Promise<i64> = __ts_aot_promise_create();
        __ts_aot_promise_finally_value(&p, move || {
            *counter.borrow_mut() += 1;
        });
        __ts_aot_promise_reject(&p, "bad".to_owned());
    }
    flush_microtasks();
    assert_eq!(*counter.borrow(), 2);
}

#[test]
fn promise_finally_value_does_not_observe_result() {
    let p2: Promise<i64> = __ts_aot_promise_create();
    let out = __ts_aot_promise_finally_value(&p2, || {});
    __ts_aot_promise_resolve(&p2, 99);
    let value = block(__ts_aot_await(&out)).unwrap();
    assert_eq!(value, 99);
}

#[test]
fn microtask_chains_run_in_order() {
    let order: Rc<RefCell<Vec<i64>>> = Rc::new(RefCell::new(Vec::new()));
    let p1: Promise<i64> = __ts_aot_promise_create();
    let p2: Promise<i64> = __ts_aot_promise_create();
    let p2_for_resolve = p2.clone();
    let order_c1 = Rc::clone(&order);
    __ts_aot_promise_then(
        &p1,
        Box::new(move |_r: Result<i64, String>| {
            order_c1.borrow_mut().push(1);
            __ts_aot_promise_resolve(&p2_for_resolve, 2);
        }),
    );
    let order_c2 = Rc::clone(&order);
    __ts_aot_promise_then(
        &p2,
        Box::new(move |_r: Result<i64, String>| {
            order_c2.borrow_mut().push(2);
        }),
    );
    __ts_aot_promise_resolve(&p1, 1);
    block(async {
        __ts_aot_await(&p1).await.ok();
        __ts_aot_await(&p2).await.ok();
    });
    assert_eq!(
        *order.borrow(),
        vec![1, 2],
        "microtask chain must run in registration order; got {:?}",
        *order.borrow()
    );
}
