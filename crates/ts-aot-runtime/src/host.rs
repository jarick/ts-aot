use std::any::Any;
use std::panic::panic_any;

use crate::string::JsString;

pub fn __ts_aot_host_console_log(s: &JsString) {
    println!("{}", s.to_string_lossy());
}

pub fn __ts_aot_throw<T: Any + Send + 'static>(value: T) -> ! {
    panic_any(value)
}
