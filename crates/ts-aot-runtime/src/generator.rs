use std::future::Future;
use std::pin::Pin;

use genawaiter::GeneratorState;
use genawaiter::sync::{Co, Gen as GenStateMachine};

use crate::promise::{
    __ts_aot_panic_message, __ts_aot_promise_create, __ts_aot_promise_reject,
    __ts_aot_promise_resolve, __ts_aot_run_or_poll, Promise,
};

type BoxedProducer<T> = Pin<Box<dyn Future<Output = Option<T>> + 'static>>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GeneratorResult<T> {
    Yielded(T),
    Done(Option<T>),
}

pub struct Generator<T> {
    inner: GenStateMachine<T, (), BoxedProducer<T>>,
    finished: bool,
}

impl<T> Generator<T> {
    #[must_use]
    pub fn next(&mut self) -> GeneratorResult<T> {
        if self.finished {
            return GeneratorResult::Done(None);
        }
        self.finished = true;
        match self.inner.resume() {
            GeneratorState::Yielded(value) => {
                self.finished = false;
                GeneratorResult::Yielded(value)
            }
            GeneratorState::Complete(value) => GeneratorResult::Done(value),
        }
    }

    fn next_iter(&mut self) -> Option<T> {
        match self.next() {
            GeneratorResult::Yielded(value) => Some(value),
            GeneratorResult::Done(_) => None,
        }
    }
}

#[must_use]
pub fn __ts_aot_generator_new<T, F>(producer: impl FnOnce(Co<T, ()>) -> F) -> Generator<T>
where
    F: Future<Output = Option<T>> + 'static,
{
    Generator {
        inner: GenStateMachine::new(move |co| {
            let fut: BoxedProducer<T> = Box::pin(producer(co));
            fut
        }),
        finished: false,
    }
}

pub struct GeneratorIntoIter<T> {
    inner: Generator<T>,
}

impl<T> Iterator for GeneratorIntoIter<T> {
    type Item = T;
    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next_iter()
    }
}

impl<T> IntoIterator for Generator<T> {
    type Item = T;
    type IntoIter = GeneratorIntoIter<T>;
    fn into_iter(self) -> Self::IntoIter {
        GeneratorIntoIter { inner: self }
    }
}

pub struct GeneratorRefIntoIter<'a, T> {
    inner: &'a mut Generator<T>,
}

impl<T> Iterator for GeneratorRefIntoIter<'_, T> {
    type Item = T;
    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next_iter()
    }
}

impl<'a, T> IntoIterator for &'a mut Generator<T> {
    type Item = T;
    type IntoIter = GeneratorRefIntoIter<'a, T>;
    fn into_iter(self) -> Self::IntoIter {
        GeneratorRefIntoIter { inner: self }
    }
}

impl<T> Generator<T> {
    pub fn iter_mut(&mut self) -> GeneratorRefIntoIter<'_, T> {
        GeneratorRefIntoIter { inner: self }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AsyncGenYield<T> {
    pub value: Option<T>,
    pub done: bool,
}

impl<T> AsyncGenYield<T> {
    pub fn yielded(value: T) -> Self {
        Self {
            value: Some(value),
            done: false,
        }
    }

    pub fn finished(value: Option<T>) -> Self {
        Self { value, done: true }
    }
}

pub struct AsyncGenerator<T> {
    inner: Generator<AsyncGenYield<T>>,
    finished: bool,
}

impl<T> AsyncGenerator<T> {
    #[must_use]
    pub fn next(&mut self) -> Promise<AsyncGenYield<T>>
    where
        T: Clone + 'static,
    {
        let promise = __ts_aot_promise_create::<AsyncGenYield<T>>();
        if self.finished {
            __ts_aot_promise_resolve(
                &promise,
                AsyncGenYield {
                    value: None,
                    done: true,
                },
            );
            return promise;
        }
        let resumed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            __ts_aot_run_or_poll(self.inner.inner.async_resume())
        }));
        let result = match resumed {
            Ok(result) => result,
            Err(payload) => {
                self.finished = true;
                if let Some(thrown) = payload.downcast_ref::<crate::host::TsAotAsyncGenThrown>() {
                    __ts_aot_promise_reject(&promise, __ts_aot_panic_message(&*thrown.0));
                } else {
                    std::panic::resume_unwind(payload);
                }
                return promise;
            }
        };
        self.finished = matches!(result, GeneratorState::Complete(_));
        match result {
            GeneratorState::Yielded(yielded) => {
                __ts_aot_promise_resolve(&promise, yielded);
            }
            GeneratorState::Complete(value) => {
                let completed = match value {
                    Some(AsyncGenYield {
                        value: inner_value,
                        done: true,
                    }) => AsyncGenYield {
                        value: inner_value,
                        done: true,
                    },
                    _ => AsyncGenYield {
                        value: None,
                        done: true,
                    },
                };
                __ts_aot_promise_resolve(&promise, completed);
            }
        }
        promise
    }
}

#[must_use]
pub fn __ts_aot_async_generator_new<T, F>(
    producer: impl FnOnce(Co<AsyncGenYield<T>, ()>) -> F,
) -> AsyncGenerator<T>
where
    T: Clone + 'static,
    F: Future<Output = Option<AsyncGenYield<T>>> + 'static,
{
    AsyncGenerator {
        inner: __ts_aot_generator_new(producer),
        finished: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::promise::__ts_aot_await_value;

    #[test]
    #[should_panic(expected = "boom")]
    fn async_generator_throw_rejects_next_promise() {
        let mut generator = __ts_aot_async_generator_new::<i64, _>(|_co| async move {
            crate::host::__ts_aot_async_generator_throw("boom".to_string())
        });
        let promise = generator.next();
        let _ = __ts_aot_await_value(&promise);
    }

    #[test]
    #[should_panic(expected = "boom")]
    fn async_generator_reraised_throw_rejects_next_promise() {
        let mut generator = __ts_aot_async_generator_new::<i64, _>(|_co| async move {
            let payload: Box<dyn std::any::Any + Send> = Box::new("boom".to_string());
            std::panic::resume_unwind(crate::host::__ts_aot_async_generator_rethrow(payload))
        });
        let promise = generator.next();
        let _ = __ts_aot_await_value(&promise);
    }

    #[test]
    #[should_panic(expected = "boom")]
    fn async_generator_nested_try_finally_rethrow_rejects_next_promise() {
        let mut generator = __ts_aot_async_generator_new::<i64, _>(|_co| async move {
            let outer = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let inner = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> () {
                    crate::host::__ts_aot_throw("boom".to_string());
                }));
                let mut inner_pending: Option<Box<dyn std::any::Any + Send>> = None;
                if let Err(__e) = inner {
                    inner_pending = Some(__e);
                }
                if let Some(__e) = inner_pending {
                    std::panic::resume_unwind(__e);
                }
            }));
            let mut outer_pending: Option<Box<dyn std::any::Any + Send>> = None;
            if let Err(__e) = outer {
                outer_pending = Some(__e);
            }
            if let Some(__e) = outer_pending {
                std::panic::resume_unwind(crate::host::__ts_aot_async_generator_rethrow(__e));
            }
            unreachable!("nested generator must have escaped via the pending throw")
        });
        let promise = generator.next();
        let _ = __ts_aot_await_value(&promise);
    }

    #[test]
    #[should_panic(expected = "boom")]
    fn async_generator_catch_body_throw_rejects_next_promise() {
        let mut generator = __ts_aot_async_generator_new::<i64, _>(|_co| async move {
            let mut pending: Option<Box<dyn std::any::Any + Send>> = None;
            let try_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> () {
                crate::host::__ts_aot_throw("boom".to_string());
            }));
            if let Err(__e) = try_result {
                let catch_result =
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> () {
                        std::panic::resume_unwind(__e);
                    }));
                if let Err(__e2) = catch_result {
                    pending = Some(__e2);
                }
            }
            if let Some(__e) = pending {
                std::panic::resume_unwind(crate::host::__ts_aot_async_generator_rethrow(__e));
            }
            unreachable!("generator must have escaped via the pending throw")
        });
        let promise = generator.next();
        let _ = __ts_aot_await_value(&promise);
    }

    #[test]
    fn nested_finally_throw_is_caught_by_enclosing_try() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicBool, Ordering};

        let caught = Arc::new(AtomicBool::new(false));
        let caught_inner = caught.clone();
        let mut generator = __ts_aot_async_generator_new::<i64, _>(move |_co| async move {
            let outer = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> () {
                let inner = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> () {
                    crate::host::__ts_aot_throw("boom".to_string());
                }));
                if let Err(__e) = inner {
                    std::panic::resume_unwind(__e);
                }
            }));
            if let Err(__e) = outer
                && __e.downcast_ref::<String>().is_some_and(|s| s == "boom")
            {
                caught_inner.store(true, Ordering::SeqCst);
            }
            Some(AsyncGenYield {
                value: None,
                done: true,
            })
        });
        let promise = generator.next();
        let _ = __ts_aot_await_value(&promise);
        assert!(
            caught.load(Ordering::SeqCst),
            "the enclosing catch must run for a nested-finally throw"
        );
    }

    #[test]
    fn await_value_raw_panics_with_raw_reason() {
        let rejected: Promise<i64> =
            crate::promise::__ts_aot_promise_reject_value("boom".to_string());
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = crate::promise::__ts_aot_await_value_raw(&rejected);
        }));
        let payload = outcome.expect_err("awaiting a rejected promise must panic");
        let reason = payload
            .downcast_ref::<crate::string::JsString>()
            .expect("the raw await must throw the runtime string type so a catch can receive it");
        assert_eq!(reason.to_string_lossy(), "boom");
    }

    #[test]
    #[should_panic(expected = "boom")]
    fn async_generator_awaited_rejection_rejects_next_promise() {
        let mut generator = __ts_aot_async_generator_new::<i64, _>(|_co| async move {
            let rejected: Promise<i64> =
                crate::promise::__ts_aot_promise_reject_value("boom".to_string());
            let _ = crate::promise::__ts_aot_await_value_gen(&rejected);
            unreachable!("await on a rejected promise must panic")
        });
        let promise = generator.next();
        let _ = __ts_aot_await_value(&promise);
    }

    #[test]
    #[should_panic(expected = "internal boom")]
    fn async_generator_internal_panic_propagates() {
        let mut generator =
            __ts_aot_async_generator_new::<i64, _>(|_co| async move { panic!("internal boom") });
        let _ = generator.next();
    }
}
