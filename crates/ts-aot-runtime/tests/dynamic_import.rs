use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use ts_aot_runtime::{
    __ts_aot_await, __ts_aot_dynamic_import, __ts_aot_module_register, __ts_aot_promise_create,
    __ts_aot_promise_reject, __ts_aot_promise_resolve, __ts_aot_promise_then, DynamicValue,
    Promise,
};

fn collect_value(value: DynamicValue) -> String {
    match value {
        DynamicValue::String(s) => s,
        other => panic!("expected String, got {other:?}"),
    }
}

#[test]
fn promise_create_then_resolve_yields_value_on_await() {
    let p: Promise = __ts_aot_promise_create();
    __ts_aot_promise_resolve(&p, &DynamicValue::String("hello".to_owned()));
    assert_eq!(collect_value(__ts_aot_await(&p)), "hello");
}

#[test]
fn promise_then_on_resolved_fires_callback_immediately() {
    let p: Promise = __ts_aot_promise_create();
    __ts_aot_promise_resolve(&p, &DynamicValue::Integer(42));
    let captured: Rc<RefCell<Option<DynamicValue>>> = Rc::new(RefCell::new(None));
    let captured_clone = Rc::clone(&captured);
    __ts_aot_promise_then(
        &p,
        Box::new(move |v| {
            *captured_clone.borrow_mut() = Some(v);
        }),
    );
    assert!(captured.borrow().is_some());
}

#[test]
fn promise_then_on_pending_queues_callback_for_resolve() {
    let p: Promise = __ts_aot_promise_create();
    let captured: Rc<RefCell<Option<DynamicValue>>> = Rc::new(RefCell::new(None));
    let captured_clone = Rc::clone(&captured);
    __ts_aot_promise_then(
        &p,
        Box::new(move |v| {
            *captured_clone.borrow_mut() = Some(v);
        }),
    );
    assert!(captured.borrow().is_none(), "pending promise must not fire");
    __ts_aot_promise_resolve(&p, &DynamicValue::Integer(7));
    assert!(
        captured.borrow().is_some(),
        "resolve must fire queued callback"
    );
}

#[test]
fn promise_reject_transitions_to_rejected_and_await_panics() {
    let p: Promise = __ts_aot_promise_create();
    __ts_aot_promise_reject(&p, &DynamicValue::String("boom".to_owned()));
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
    let p: Promise = __ts_aot_promise_create();
    let captured: Rc<RefCell<Option<DynamicValue>>> = Rc::new(RefCell::new(None));
    let captured_clone = Rc::clone(&captured);
    __ts_aot_promise_then(
        &p,
        Box::new(move |v| {
            *captured_clone.borrow_mut() = Some(v);
        }),
    );
    __ts_aot_promise_reject(&p, &DynamicValue::String("nope".to_owned()));
    let captured = captured.borrow();
    let value = captured.as_ref().expect("callback must fire on reject");
    assert!(matches!(value, DynamicValue::String(s) if s == "nope"));
}

#[test]
fn promise_resolve_after_reject_is_no_op() {
    let p: Promise = __ts_aot_promise_create();
    __ts_aot_promise_reject(&p, &DynamicValue::String("first".to_owned()));
    __ts_aot_promise_resolve(&p, &DynamicValue::String("second".to_owned()));
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
fn dynamic_import_unknown_specifier_returns_esmodule_marker() {
    let p: Promise = __ts_aot_dynamic_import(&DynamicValue::String("nonexistent.ts".to_owned()));
    let value = __ts_aot_await(&p);
    let DynamicValue::Object(obj) = value else {
        panic!("expected Object, got {value:?}");
    };
    let fields = obj.fields.borrow();
    let esmodule = fields.get("__esModule").expect("__esModule field present");
    assert!(matches!(esmodule, DynamicValue::Bool(true)));
}

#[test]
fn dynamic_import_registered_module_returns_registered_namespace() {
    let mut ns: HashMap<String, DynamicValue> = HashMap::new();
    ns.insert("answer".to_owned(), DynamicValue::Integer(42));
    ns.insert("name".to_owned(), DynamicValue::String("alpha".to_owned()));
    __ts_aot_module_register("./fixtures/alpha.ts", ns);

    let p: Promise =
        __ts_aot_dynamic_import(&DynamicValue::String("./fixtures/alpha.ts".to_owned()));
    let value = __ts_aot_await(&p);
    let DynamicValue::Object(obj) = value else {
        panic!("expected Object, got {value:?}");
    };
    let fields = obj.fields.borrow();
    assert!(matches!(
        fields.get("answer"),
        Some(DynamicValue::Integer(42))
    ));
    assert!(matches!(
        fields.get("name"),
        Some(DynamicValue::String(s)) if s == "alpha"
    ));
    assert!(matches!(
        fields.get("__esModule"),
        Some(DynamicValue::Bool(true))
    ));
}

#[test]
#[should_panic(expected = "dynamic import() requires a string specifier")]
fn dynamic_import_panics_on_non_string_specifier() {
    let _ = __ts_aot_dynamic_import(&DynamicValue::Integer(0));
}
