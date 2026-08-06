use std::cell::RefCell;
use std::rc::Rc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PromiseState {
    Pending,
    Fulfilled,
    Rejected,
}

pub struct Promise<T> {
    inner: Rc<RefCell<PromiseInner<T>>>,
}

struct PromiseInner<T> {
    state: PromiseState,
    value: Option<T>,
    error: Option<String>,
    callbacks: Vec<PromiseCallback<T>>,
}

type PromiseCallback<T> = Box<dyn FnOnce(Result<T, String>)>;

#[must_use]
pub fn __ts_aot_promise_create<T>() -> Promise<T> {
    Promise {
        inner: Rc::new(RefCell::new(PromiseInner {
            state: PromiseState::Pending,
            value: None,
            error: None,
            callbacks: Vec::new(),
        })),
    }
}

pub fn __ts_aot_promise_resolve<T: Clone + 'static>(promise: &Promise<T>, value: T) {
    let to_fire: Vec<PromiseCallback<T>> = {
        let mut inner = promise.inner.borrow_mut();
        if inner.state != PromiseState::Pending {
            return;
        }
        inner.state = PromiseState::Fulfilled;
        inner.value = Some(value.clone());
        std::mem::take(&mut inner.callbacks)
    };
    for cb in to_fire {
        cb(Ok(value.clone()));
    }
}

pub fn __ts_aot_promise_reject<T: 'static>(promise: &Promise<T>, reason: String) {
    let to_fire: Vec<PromiseCallback<T>> = {
        let mut inner = promise.inner.borrow_mut();
        if inner.state != PromiseState::Pending {
            return;
        }
        inner.state = PromiseState::Rejected;
        inner.error = Some(reason.clone());
        std::mem::take(&mut inner.callbacks)
    };
    for cb in to_fire {
        cb(Err(reason.clone()));
    }
}

pub fn __ts_aot_promise_then<T: Clone + 'static>(
    promise: &Promise<T>,
    callback: Box<dyn FnOnce(Result<T, String>)>,
) {
    let inner = promise.inner.borrow();
    if matches!(
        inner.state,
        PromiseState::Fulfilled | PromiseState::Rejected
    ) {
        let result = if inner.state == PromiseState::Fulfilled {
            Ok(inner
                .value
                .clone()
                .expect("fulfilled promise must have a value"))
        } else {
            Err(inner
                .error
                .clone()
                .expect("rejected promise must have an error"))
        };
        drop(inner);
        callback(result);
    } else {
        drop(inner);
        promise.inner.borrow_mut().callbacks.push(callback);
    }
}

#[must_use]
pub fn __ts_aot_await<T: Clone>(promise: &Promise<T>) -> T {
    let inner = promise.inner.borrow();
    match &inner.state {
        PromiseState::Fulfilled => inner
            .value
            .clone()
            .expect("fulfilled promise must have a value"),
        PromiseState::Rejected => {
            panic!(
                "await on a rejected promise: {}",
                inner.error.as_deref().unwrap_or("unknown error")
            );
        }
        PromiseState::Pending => {
            panic!("__ts_aot_await only works on settled promises; this one is pending")
        }
    }
}
