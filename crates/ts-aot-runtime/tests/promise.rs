use std::cell::RefCell;
use std::rc::Rc;
use ts_aot_runtime::{
    __ts_aot_await, __ts_aot_dynamic_import, __ts_aot_module_register, __ts_aot_promise_create,
    __ts_aot_promise_reject, __ts_aot_promise_resolve, __ts_aot_promise_then, Promise,
};

#[derive(Clone, Debug, PartialEq)]
struct ModAlpha {
    answer: i64,
    name: String,
}

fn shared_capture() -> Rc<RefCell<Option<i64>>> {
    Rc::new(RefCell::new(None))
}

#[test]
fn promise_create_then_resolve_yields_value_on_await() {
    let p: Promise<i64> = __ts_aot_promise_create();
    __ts_aot_promise_resolve(&p, 42);
    assert_eq!(__ts_aot_await(&p), 42);
}

#[test]
fn promise_then_on_resolved_fires_callback_immediately() {
    let p: Promise<i64> = __ts_aot_promise_create();
    __ts_aot_promise_resolve(&p, 7);
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
    assert_eq!(
        *captured.borrow(),
        Some("later".to_owned()),
        "resolve must fire queued callback"
    );
}

#[test]
fn promise_reject_transitions_to_rejected_and_await_panics() {
    let p: Promise<i64> = __ts_aot_promise_create();
    __ts_aot_promise_reject(&p, "boom".to_owned());
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| __ts_aot_await(&p)));
    let err = result.expect_err("await on rejected promise must panic");
    let msg = err
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| err.downcast_ref::<&str>().copied())
        .unwrap_or("");
    assert!(
        msg.contains("rejected") && msg.contains("boom"),
        "panic message must mention rejected + reason, got: {msg}"
    );
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
    assert_eq!(*captured.borrow(), Some("nope".to_owned()));
}

#[test]
fn promise_resolve_after_reject_is_no_op() {
    let p: Promise<String> = __ts_aot_promise_create();
    __ts_aot_promise_reject(&p, "first".to_owned());
    __ts_aot_promise_resolve(&p, "second".to_owned());
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| __ts_aot_await(&p)));
    let err = result.expect_err("await must still panic with first rejection reason");
    let msg = err
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| err.downcast_ref::<&str>().copied())
        .unwrap_or("");
    assert!(msg.contains("first"), "first reject must win, got: {msg}");
}

#[test]
fn promise_await_on_pending_panics() {
    let p: Promise<i64> = __ts_aot_promise_create();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| __ts_aot_await(&p)));
    let err = result.expect_err("await on pending promise must panic");
    let msg = err
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| err.downcast_ref::<&str>().copied())
        .unwrap_or("");
    assert!(
        msg.contains("pending"),
        "panic message must mention pending, got: {msg}"
    );
}

#[test]
fn dynamic_import_unknown_specifier_rejects_promise() {
    let p: Promise<ModAlpha> = __ts_aot_dynamic_import("nonexistent.ts");
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| __ts_aot_await(&p)));
    let err = result.expect_err("await on missing-module promise must panic");
    let msg = err
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| err.downcast_ref::<&str>().copied())
        .unwrap_or("");
    assert!(
        msg.contains("nonexistent.ts") && msg.contains("not registered"),
        "panic message must name specifier and missing-module reason, got: {msg}"
    );
}

#[test]
fn dynamic_import_registered_module_returns_typed_namespace() {
    let ns = ModAlpha {
        answer: 42,
        name: "alpha".to_owned(),
    };
    __ts_aot_module_register("./fixtures/alpha.ts", ns.clone());

    let p: Promise<ModAlpha> = __ts_aot_dynamic_import("./fixtures/alpha.ts");
    let loaded = __ts_aot_await(&p);
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
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| __ts_aot_await(&p)));
    let err = result.expect_err("wrong-type await must panic");
    let msg = err
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| err.downcast_ref::<&str>().copied())
        .unwrap_or("");
    assert!(
        msg.contains("beta.ts") && msg.contains("does not match"),
        "panic message must name specifier and type-mismatch reason, got: {msg}"
    );
}
