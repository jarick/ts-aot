use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;

use crate::promise::{
    __ts_aot_promise_create, __ts_aot_promise_reject, __ts_aot_promise_resolve, Promise,
};

pub trait ModuleNamespace: Sized + Clone + 'static {}
impl<T: Sized + Clone + 'static> ModuleNamespace for T {}

type AnyNamespace = Box<dyn Any>;

fn with_module_registry<R>(f: impl FnOnce(&mut HashMap<String, AnyNamespace>) -> R) -> R {
    thread_local! {
        static REGISTRY: RefCell<HashMap<String, AnyNamespace>> = RefCell::new(HashMap::new());
    }
    REGISTRY.with(|cell| f(&mut cell.borrow_mut()))
}

pub fn __ts_aot_module_register<T: ModuleNamespace>(specifier: &str, namespace: T) {
    with_module_registry(|reg| {
        reg.insert(specifier.to_owned(), Box::new(namespace));
    });
}

#[must_use]
pub fn __ts_aot_dynamic_import<T: ModuleNamespace>(specifier: &str) -> Promise<T> {
    let promise = __ts_aot_promise_create();
    let result: Result<T, String> = with_module_registry(|reg| {
        let Some(boxed) = reg.get(specifier) else {
            return Err(format!("module '{specifier}' is not registered"));
        };
        boxed.downcast_ref::<T>().cloned().ok_or_else(|| {
            format!(
                "module '{specifier}' is registered but the requested type does not match the registered namespace"
            )
        })
    });
    match result {
        Ok(value) => __ts_aot_promise_resolve(&promise, value),
        Err(reason) => __ts_aot_promise_reject(&promise, reason),
    }
    promise
}
