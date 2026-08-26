use std::future::Future;
use std::pin::Pin;

use genawaiter::GeneratorState;
use genawaiter::sync::{Co, Gen as GenStateMachine};

use crate::promise::{
    __ts_aot_promise_create, __ts_aot_promise_resolve, __ts_aot_runtime_run, Promise,
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
        let result = __ts_aot_runtime_run(self.inner.inner.async_resume());
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
