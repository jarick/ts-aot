use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::{Context, Poll, Wake, Waker};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PromiseState {
    Pending,
    Fulfilled,
    Rejected,
}

pub struct Promise<T> {
    inner: Rc<RefCell<PromiseInner<T>>>,
}

impl<T> Clone for Promise<T> {
    fn clone(&self) -> Self {
        Self {
            inner: Rc::clone(&self.inner),
        }
    }
}

struct PromiseInner<T> {
    state: PromiseState,
    value: Option<T>,
    error: Option<String>,
    callbacks: Vec<PromiseCallback<T>>,
    wakers: Vec<Waker>,
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
            wakers: Vec::new(),
        })),
    }
}

impl<T> Promise<T> {
    #[doc(hidden)]
    #[must_use]
    pub fn waker_count(&self) -> usize {
        self.inner.borrow().wakers.len()
    }
}

type SettleResult<T> = (
    Vec<PromiseCallback<T>>,
    Vec<Waker>,
    Option<Result<T, String>>,
);

fn settle_with<T: Clone + 'static>(
    promise: &Promise<T>,
    new_state: PromiseState,
) -> SettleResult<T> {
    let mut inner = promise.inner.borrow_mut();
    if inner.state != PromiseState::Pending {
        return (Vec::new(), Vec::new(), None);
    }
    inner.state = new_state;
    let result = match new_state {
        PromiseState::Fulfilled => inner
            .value
            .clone()
            .map(|v| Ok::<T, String>(v))
            .expect("fulfilled promise must have a value"),
        PromiseState::Rejected => Err(inner
            .error
            .clone()
            .expect("rejected promise must have an error")),
        PromiseState::Pending => unreachable!(),
    };
    let callbacks = std::mem::take(&mut inner.callbacks);
    let wakers = std::mem::take(&mut inner.wakers);
    (callbacks, wakers, Some(result))
}

pub fn __ts_aot_promise_resolve<T: Clone + 'static>(promise: &Promise<T>, value: T) {
    {
        let mut inner = promise.inner.borrow_mut();
        if inner.state != PromiseState::Pending {
            return;
        }
        inner.value = Some(value);
    }
    let (callbacks, wakers, result) = settle_with(promise, PromiseState::Fulfilled);
    let result = result.expect("settle_with returned None on freshly-resolved promise");
    for cb in callbacks {
        let r = result.clone();
        enqueue_microtask(Box::new(move || cb(r)));
    }
    for waker in wakers {
        waker.wake();
    }
}

#[must_use]
pub fn __ts_aot_promise_resolve_value<T: Clone + 'static>(value: T) -> Promise<T> {
    let p = __ts_aot_promise_create::<T>();
    __ts_aot_promise_resolve(&p, value);
    p
}

pub fn __ts_aot_promise_reject<T: Clone + 'static>(promise: &Promise<T>, reason: String) {
    {
        let mut inner = promise.inner.borrow_mut();
        if inner.state != PromiseState::Pending {
            return;
        }
        inner.error = Some(reason.clone());
    }
    let (callbacks, wakers, result) = settle_with(promise, PromiseState::Rejected);
    let result = result.expect("settle_with returned None on freshly-rejected promise");
    for cb in callbacks {
        let r = result.clone();
        enqueue_microtask(Box::new(move || cb(r)));
    }
    for waker in wakers {
        waker.wake();
    }
}

#[must_use]
pub fn __ts_aot_promise_reject_value<T: Clone + 'static>(reason: String) -> Promise<T> {
    let p = __ts_aot_promise_create::<T>();
    __ts_aot_promise_reject(&p, reason);
    p
}

pub fn __ts_aot_promise_then<T, F>(promise: &Promise<T>, callback: F)
where
    T: Clone + 'static,
    F: FnOnce(Result<T, String>) + 'static,
{
    let state = promise.inner.borrow().state;
    match state {
        PromiseState::Fulfilled => {
            let value = promise
                .inner
                .borrow()
                .value
                .clone()
                .expect("fulfilled promise must have a value");
            enqueue_microtask(Box::new(move || callback(Ok(value))));
        }
        PromiseState::Rejected => {
            let error = promise
                .inner
                .borrow()
                .error
                .clone()
                .expect("rejected promise must have an error");
            enqueue_microtask(Box::new(move || callback(Err(error))));
        }
        PromiseState::Pending => {
            promise
                .inner
                .borrow_mut()
                .callbacks
                .push(Box::new(callback));
        }
    }
}

pub struct AwaitFuture<T> {
    promise: Promise<T>,
    registered_waker: Option<Waker>,
}

impl<T: Clone + 'static> Future for AwaitFuture<T> {
    type Output = Result<T, String>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        let new_waker = cx.waker();
        let mut inner = this.promise.inner.borrow_mut();
        match inner.state {
            PromiseState::Fulfilled => {
                let value = inner
                    .value
                    .clone()
                    .expect("fulfilled promise must have a value");
                Poll::Ready(Ok(value))
            }
            PromiseState::Rejected => {
                let error = inner
                    .error
                    .clone()
                    .expect("rejected promise must have an error");
                Poll::Ready(Err(error))
            }
            PromiseState::Pending => {
                let same_waker = this
                    .registered_waker
                    .as_ref()
                    .is_some_and(|stored| stored.will_wake(new_waker));
                if !same_waker {
                    if let Some(stored) = this.registered_waker.as_ref() {
                        inner.wakers.retain(|w| !w.will_wake(stored));
                    }
                    let stored_waker = new_waker.clone();
                    inner.wakers.push(stored_waker.clone());
                    this.registered_waker = Some(stored_waker);
                }
                Poll::Pending
            }
        }
    }
}

#[must_use]
pub fn __ts_aot_await<T: Clone + 'static>(promise: &Promise<T>) -> AwaitFuture<T> {
    AwaitFuture {
        promise: promise.clone(),
        registered_waker: None,
    }
}

thread_local! {
    static MICROTASKS: RefCell<VecDeque<Box<dyn FnOnce()>>> = const { RefCell::new(VecDeque::new()) };
    static IN_RUNTIME_RUN: Cell<bool> = const { Cell::new(false) };
}

fn enqueue_microtask(f: Box<dyn FnOnce()>) {
    MICROTASKS.with(|q| q.borrow_mut().push_back(f));
}

pub fn __ts_aot_enqueue_microtask(f: Box<dyn FnOnce()>) {
    enqueue_microtask(f);
}

fn drain_microtasks() {
    loop {
        let task = MICROTASKS.with(|q| q.borrow_mut().pop_front());
        match task {
            Some(f) => f(),
            None => break,
        }
    }
}

struct TickWaker {
    flag: Arc<AtomicBool>,
}

impl Wake for TickWaker {
    fn wake(self: Arc<Self>) {
        self.flag.store(true, Ordering::SeqCst);
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.flag.store(true, Ordering::SeqCst);
    }
}

pub fn __ts_aot_runtime_run<F>(future: F) -> F::Output
where
    F: Future,
{
    assert!(
        !IN_RUNTIME_RUN.get(),
        "__ts_aot_runtime_run called recursively (nested runtime loop); \
         __ts_aot_await_value cannot be invoked from inside a microtask callback. \
         Re-architect the handler to avoid awaiting from a synchronous callback."
    );
    IN_RUNTIME_RUN.set(true);
    let _guard = RuntimeRunGuard;
    let flag = Arc::new(AtomicBool::new(true));
    let waker: Waker = Arc::new(TickWaker {
        flag: Arc::clone(&flag),
    })
    .into();
    let mut cx = Context::from_waker(&waker);
    let mut future = Box::pin(future);
    loop {
        if let Poll::Ready(v) = future.as_mut().poll(&mut cx) {
            drain_microtasks();
            return v;
        }
        if flag.swap(false, Ordering::SeqCst) {
            drain_microtasks();
            continue;
        }
        drain_microtasks();
        let woke_after_drain = flag.swap(false, Ordering::SeqCst);
        let pending_microtasks = MICROTASKS.with(|q| !q.borrow().is_empty());
        if woke_after_drain || pending_microtasks {
            continue;
        }
        match future.as_mut().poll(&mut cx) {
            Poll::Ready(v) => {
                drain_microtasks();
                return v;
            }
            Poll::Pending => panic!(
                "runtime loop deadlock: future returned Pending but neither the wake flag \
                 nor the microtask queue had work (unsettled promise such as an empty \
                 Promise.race result); cannot make progress"
            ),
        }
    }
}

struct RuntimeRunGuard;
impl Drop for RuntimeRunGuard {
    fn drop(&mut self) {
        IN_RUNTIME_RUN.set(false);
    }
}

#[must_use]
pub fn __ts_aot_promise_all<T: Clone + 'static>(promises: Vec<Promise<T>>) -> Promise<Vec<T>> {
    let out = __ts_aot_promise_create::<Vec<T>>();
    if promises.is_empty() {
        __ts_aot_promise_resolve(&out, Vec::new());
        return out;
    }
    let state: Rc<RefCell<AllState<T>>> = Rc::new(RefCell::new(AllState {
        results: (0..promises.len()).map(|_| None).collect(),
        pending: promises.len(),
        rejected: false,
    }));
    for (idx, p) in promises.into_iter().enumerate() {
        let state = Rc::clone(&state);
        let out = out.clone();
        __ts_aot_promise_then(
            &p,
            Box::new(move |result| match result {
                Ok(v) => {
                    let mut s = state.borrow_mut();
                    s.results[idx] = Some(v);
                    s.pending -= 1;
                    if !s.rejected && s.pending == 0 {
                        let collected: Vec<T> =
                            s.results.iter_mut().map(|o| o.take().unwrap()).collect();
                        drop(s);
                        __ts_aot_promise_resolve(&out, collected);
                    }
                }
                Err(e) => {
                    let mut s = state.borrow_mut();
                    if !s.rejected {
                        s.rejected = true;
                        drop(s);
                        __ts_aot_promise_reject(&out, e);
                    }
                }
            }),
        );
    }
    out
}

struct AllState<T> {
    results: Vec<Option<T>>,
    pending: usize,
    rejected: bool,
}

#[must_use]
pub fn __ts_aot_promise_race<T: Clone + 'static>(promises: Vec<Promise<T>>) -> Promise<T> {
    let out = __ts_aot_promise_create::<T>();
    if promises.is_empty() {
        return out;
    }
    let settled: Rc<RefCell<bool>> = Rc::new(RefCell::new(false));
    for p in promises {
        let settled = Rc::clone(&settled);
        let out = out.clone();
        __ts_aot_promise_then(
            &p,
            Box::new(move |result| {
                let already = *settled.borrow();
                if already {
                    return;
                }
                *settled.borrow_mut() = true;
                match result {
                    Ok(v) => __ts_aot_promise_resolve(&out, v),
                    Err(e) => __ts_aot_promise_reject(&out, e),
                }
            }),
        );
    }
    out
}

#[must_use]
pub fn __ts_aot_promise_all_settled<T: Clone + 'static>(
    promises: Vec<Promise<T>>,
) -> Promise<Vec<PromiseSettledResult<T>>> {
    let out = __ts_aot_promise_create::<Vec<PromiseSettledResult<T>>>();
    if promises.is_empty() {
        __ts_aot_promise_resolve(&out, Vec::new());
        return out;
    }
    let state: Rc<RefCell<AllSettledState<T>>> = Rc::new(RefCell::new(AllSettledState {
        results: (0..promises.len()).map(|_| None).collect(),
        pending: promises.len(),
    }));
    for (idx, p) in promises.into_iter().enumerate() {
        let state = Rc::clone(&state);
        let out = out.clone();
        __ts_aot_promise_then(
            &p,
            Box::new(move |result| {
                let mut s = state.borrow_mut();
                s.results[idx] = Some(match result {
                    Ok(v) => PromiseSettledResult::fulfilled(v),
                    Err(e) => PromiseSettledResult::rejected(e),
                });
                s.pending -= 1;
                if s.pending == 0 {
                    let collected: Vec<PromiseSettledResult<T>> =
                        s.results.iter_mut().map(|o| o.take().unwrap()).collect();
                    drop(s);
                    __ts_aot_promise_resolve(&out, collected);
                }
            }),
        );
    }
    out
}

struct AllSettledState<T> {
    results: Vec<Option<PromiseSettledResult<T>>>,
    pending: usize,
}

#[must_use]
pub fn __ts_aot_promise_any<T: Clone + 'static>(promises: Vec<Promise<T>>) -> Promise<T> {
    let out = __ts_aot_promise_create::<T>();
    if promises.is_empty() {
        __ts_aot_promise_reject(&out, AggregateError::new(Vec::new()).to_string());
        return out;
    }
    let state: Rc<RefCell<AnyState>> = Rc::new(RefCell::new(AnyState {
        errors: (0..promises.len()).map(|_| None).collect(),
        pending: promises.len(),
        settled: false,
    }));
    for (idx, p) in promises.into_iter().enumerate() {
        let state = Rc::clone(&state);
        let out = out.clone();
        __ts_aot_promise_then(
            &p,
            Box::new(move |result| match result {
                Ok(v) => {
                    let mut s = state.borrow_mut();
                    if !s.settled {
                        s.settled = true;
                        drop(s);
                        __ts_aot_promise_resolve(&out, v);
                    }
                }
                Err(e) => {
                    let mut s = state.borrow_mut();
                    s.errors[idx] = Some(e);
                    s.pending -= 1;
                    if !s.settled && s.pending == 0 {
                        let agg = s
                            .errors
                            .iter_mut()
                            .map(|o| o.take().unwrap_or_default())
                            .collect::<Vec<_>>();
                        let agg_err = AggregateError::new(agg).to_string();
                        drop(s);
                        __ts_aot_promise_reject(&out, agg_err);
                    }
                }
            }),
        );
    }
    out
}

struct AnyState {
    errors: Vec<Option<String>>,
    pending: usize,
    settled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromiseSettledResult<T> {
    pub status: &'static str,
    pub value: Option<T>,
    pub reason: Option<String>,
}

impl<T> PromiseSettledResult<T> {
    #[must_use]
    pub fn fulfilled(value: T) -> Self {
        Self {
            status: "fulfilled",
            value: Some(value),
            reason: None,
        }
    }

    #[must_use]
    pub fn rejected(reason: String) -> Self {
        Self {
            status: "rejected",
            value: None,
            reason: Some(reason),
        }
    }
}

pub fn __ts_aot_promise_catch<T, F>(promise: &Promise<T>, on_rejected: F)
where
    T: Clone + 'static,
    F: FnOnce(String) + 'static,
{
    __ts_aot_promise_then(
        promise,
        Box::new(move |result| {
            if let Err(e) = result {
                on_rejected(e);
            }
        }),
    );
}

pub fn __ts_aot_promise_finally<T, F>(promise: &Promise<T>, on_finally: F)
where
    T: Clone + 'static,
    F: FnOnce() + 'static,
{
    __ts_aot_promise_then(
        promise,
        Box::new(move |_result| {
            on_finally();
        }),
    );
}

pub fn __ts_aot_promise_then_value<T, U, F>(promise: &Promise<T>, handler: F) -> Promise<U>
where
    T: Clone + 'static,
    U: Clone + 'static,
    F: FnOnce(T) -> U + 'static,
{
    let out = __ts_aot_promise_create::<U>();
    let out_for_cb = out.clone();
    __ts_aot_promise_then(
        promise,
        Box::new(move |result| match result {
            Ok(v) => {
                let mapped = handler(v);
                __ts_aot_promise_resolve(&out_for_cb, mapped);
            }
            Err(e) => __ts_aot_promise_reject(&out_for_cb, e),
        }),
    );
    out
}

pub fn __ts_aot_promise_catch_value<T, F>(promise: &Promise<T>, handler: F) -> Promise<T>
where
    T: Clone + 'static,
    F: FnOnce(String) -> T + 'static,
{
    let out = __ts_aot_promise_create::<T>();
    let out_for_cb = out.clone();
    __ts_aot_promise_then(
        promise,
        Box::new(move |result| match result {
            Ok(v) => __ts_aot_promise_resolve(&out_for_cb, v),
            Err(e) => {
                let mapped = handler(e);
                __ts_aot_promise_resolve(&out_for_cb, mapped);
            }
        }),
    );
    out
}

pub fn __ts_aot_promise_finally_value<T, F>(promise: &Promise<T>, handler: F) -> Promise<T>
where
    T: Clone + 'static,
    F: FnOnce() + 'static,
{
    let out = __ts_aot_promise_create::<T>();
    let out_for_cb = out.clone();
    __ts_aot_promise_then(
        promise,
        Box::new(move |result| {
            handler();
            match result {
                Ok(v) => __ts_aot_promise_resolve(&out_for_cb, v),
                Err(e) => __ts_aot_promise_reject(&out_for_cb, e),
            }
        }),
    );
    out
}

#[must_use]
pub fn __ts_aot_await_value<T: Clone + 'static>(promise: &Promise<T>) -> T {
    match __ts_aot_runtime_run(__ts_aot_await(promise)) {
        Ok(v) => v,
        Err(e) => panic!("await on rejected promise: {e}"),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AggregateError {
    pub errors: Vec<String>,
}

impl AggregateError {
    #[must_use]
    pub fn new(errors: Vec<String>) -> Self {
        Self { errors }
    }
}

impl std::fmt::Display for AggregateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "AggregateError: [{}]", self.errors.join(", "))
    }
}

impl std::error::Error for AggregateError {}
