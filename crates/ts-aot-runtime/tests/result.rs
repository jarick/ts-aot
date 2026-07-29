use ts_aot_runtime::{__ts_aot_result_err, __ts_aot_result_ok, __ts_aot_result_unwrap_ok};

#[test]
fn runtime_result_ok_wraps_value_in_ok() {
    let r: Result<i64, String> = __ts_aot_result_ok(42);
    assert_eq!(r, Ok(42));
}

#[test]
fn runtime_result_err_wraps_value_in_err() {
    let r: Result<i64, String> = __ts_aot_result_err("boom".to_owned());
    assert_eq!(r, Err("boom".to_owned()));
}

#[test]
fn runtime_result_unwrap_ok_extracts_ok_value() {
    let r: Result<i64, String> = Ok(7);
    assert_eq!(__ts_aot_result_unwrap_ok(r), 7);
}

#[test]
#[should_panic(expected = "called on Err value")]
fn runtime_result_unwrap_ok_panics_on_err() {
    let r: Result<i64, String> = Err("nope".to_owned());
    let _ = __ts_aot_result_unwrap_ok(r);
}
