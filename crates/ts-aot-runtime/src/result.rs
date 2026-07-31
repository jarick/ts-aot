pub fn __ts_aot_result_ok<T, E>(value: T) -> Result<T, E> {
    Ok(value)
}

pub fn __ts_aot_result_err<T, E>(error: E) -> Result<T, E> {
    Err(error)
}

pub fn __ts_aot_result_unwrap_ok<T, E: std::fmt::Debug>(result: Result<T, E>) -> T {
    result.expect("__ts_aot_result_unwrap_ok: called on Err value")
}
