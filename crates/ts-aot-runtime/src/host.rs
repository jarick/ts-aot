use std::any::Any;
use std::panic::panic_any;

use crate::string::JsString;

pub fn __ts_aot_host_console_log(s: &JsString) {
    println!("{}", s.to_string_lossy());
}

pub fn __ts_aot_throw<T: Any + Send + 'static>(value: T) -> ! {
    panic_any(value)
}

pub(crate) struct TsAotAsyncGenThrown(pub Box<dyn Any + Send>);

pub fn __ts_aot_async_generator_throw<T: Any + Send + 'static>(value: T) -> ! {
    panic_any(TsAotAsyncGenThrown(Box::new(value)))
}

#[must_use]
pub fn __ts_aot_async_generator_rethrow(payload: Box<dyn Any + Send>) -> Box<dyn Any + Send> {
    Box::new(TsAotAsyncGenThrown(payload))
}
