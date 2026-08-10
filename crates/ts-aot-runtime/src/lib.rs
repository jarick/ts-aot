pub use ts_aot_core::MAX_DENSE_ARRAY_LEN;

mod array;
mod bigint;
mod date;
mod generator;
mod host;
mod json;
mod map;
mod math;
mod module;
mod promise;
mod regex;
mod result;
mod string;
mod symbol;
mod type_of;

pub use array::{
    __ts_aot_array_create, __ts_aot_array_create_with_len, __ts_aot_array_from,
    __ts_aot_array_from_length_mapped, __ts_aot_array_from_mapped, __ts_aot_array_from_string,
    __ts_aot_array_get, __ts_aot_array_is_array, __ts_aot_array_is_array_false, __ts_aot_array_len,
    __ts_aot_array_push, __ts_aot_array_set, IsArray, TsArrayMarker,
};
pub use bigint::{__ts_aot_bigint_new, BigIntHandle};
pub use date::{
    __ts_aot_date_get_date, __ts_aot_date_get_full_year, __ts_aot_date_get_hours,
    __ts_aot_date_get_milliseconds, __ts_aot_date_get_minutes, __ts_aot_date_get_month,
    __ts_aot_date_get_seconds, __ts_aot_date_get_time, __ts_aot_date_is_invalid,
    __ts_aot_date_new_from_ms, __ts_aot_date_now, __ts_aot_date_parse, __ts_aot_date_to_iso_string,
    __ts_aot_date_value_of, MS_PER_DAY, parse_iso8601_to_ms,
};
pub use generator::{
    __ts_aot_generator_done, __ts_aot_generator_done_with, __ts_aot_generator_get_state,
    __ts_aot_generator_set_state, __ts_aot_generator_store, __ts_aot_generator_yielded,
    GENERATOR_DONE_STATE, Generator, GeneratorDispatch, GeneratorResult,
};
pub use host::{__ts_aot_host_console_log, __ts_aot_throw};
pub use json::{
    __ts_aot_json_parse, __ts_aot_json_parse_string, __ts_aot_json_stringify,
    __ts_aot_json_stringify_string,
};
pub use map::{__ts_aot_map_create, __ts_aot_map_get, __ts_aot_map_set, __ts_aot_object_keys};
pub use math::{
    __ts_aot_math_abs, __ts_aot_math_acos, __ts_aot_math_asin, __ts_aot_math_atan,
    __ts_aot_math_atan2, __ts_aot_math_ceil, __ts_aot_math_cos, __ts_aot_math_exp,
    __ts_aot_math_floor, __ts_aot_math_log, __ts_aot_math_max, __ts_aot_math_min,
    __ts_aot_math_pow, __ts_aot_math_random, __ts_aot_math_round, __ts_aot_math_sign,
    __ts_aot_math_sin, __ts_aot_math_sqrt, __ts_aot_math_tan, __ts_aot_math_trunc,
};
pub use module::{__ts_aot_dynamic_import, __ts_aot_module_register, ModuleNamespace};
pub use promise::{
    __ts_aot_await, __ts_aot_promise_create, __ts_aot_promise_reject, __ts_aot_promise_resolve,
    __ts_aot_promise_then, Promise,
};
pub use regex::{__ts_aot_regex_new, RegExpHandle};
pub use result::{__ts_aot_result_err, __ts_aot_result_ok, __ts_aot_result_unwrap_ok};
pub use string::{
    __ts_aot_string_char_at, __ts_aot_string_concat, __ts_aot_string_equals,
    __ts_aot_string_from_char_code, __ts_aot_string_from_code_point, __ts_aot_string_index_of,
    __ts_aot_string_len, __ts_aot_string_substring_utf16, JsString,
};
pub use symbol::{
    __ts_aot_symbol_for, __ts_aot_symbol_key_for, __ts_aot_symbol_new, __ts_aot_symbol_new_desc,
};
pub use type_of::{
    __ts_aot_op_in, __ts_aot_op_instanceof, __ts_aot_typeof, __ts_aot_typeof_null,
    __ts_aot_typeof_unit, TsClassId,
};
