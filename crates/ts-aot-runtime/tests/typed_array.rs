use ts_aot_runtime::{__ts_aot_typed_array_new, TypedArrayHandle, TypedArrayKind};

#[test]
fn typed_array_new_int8_with_length_allocates_zero_filled_buffer() {
    let arr = __ts_aot_typed_array_new(8, 0);
    assert_eq!(arr.kind(), TypedArrayKind::Int8);
    assert_eq!(arr.length(), 8);
    assert_eq!(arr.byte_length(), 8);
    assert_eq!(arr.byte_offset(), 0);
    assert!(
        arr.raw_buffer().bytes().iter().all(|&b| b == 0),
        "new Int8Array(8) must zero-initialize 8 bytes (per spec AllocateTypedArray)"
    );
}

#[test]
fn typed_array_new_uint8_with_length_byte_length_matches_bytes_per_element() {
    let arr = __ts_aot_typed_array_new(4, 1);
    assert_eq!(arr.kind(), TypedArrayKind::Uint8);
    assert_eq!(arr.length(), 4);
    assert_eq!(arr.byte_length(), 4);
    assert_eq!(arr.byte_offset(), 0);
}

#[test]
fn typed_array_new_int16_byte_length_is_length_times_two() {
    let arr = __ts_aot_typed_array_new(4, 3);
    assert_eq!(arr.kind(), TypedArrayKind::Int16);
    assert_eq!(arr.length(), 4);
    assert_eq!(arr.byte_length(), 8, "Int16Array has BYTES_PER_ELEMENT = 2");
}

#[test]
fn typed_array_new_float64_byte_length_is_length_times_eight() {
    let arr = __ts_aot_typed_array_new(3, 8);
    assert_eq!(arr.kind(), TypedArrayKind::Float64);
    assert_eq!(arr.length(), 3);
    assert_eq!(
        arr.byte_length(),
        24,
        "Float64Array has BYTES_PER_ELEMENT = 8"
    );
}

#[test]
fn typed_array_new_zero_length_produces_empty_buffer() {
    let arr = __ts_aot_typed_array_new(0, 0);
    assert_eq!(arr.length(), 0);
    assert_eq!(arr.byte_length(), 0);
    assert!(arr.raw_buffer().bytes().is_empty());
}

#[test]
fn typed_array_kind_from_id_round_trips_all_nine_variants() {
    let cases: &[(i64, TypedArrayKind)] = &[
        (0, TypedArrayKind::Int8),
        (1, TypedArrayKind::Uint8),
        (2, TypedArrayKind::Uint8Clamped),
        (3, TypedArrayKind::Int16),
        (4, TypedArrayKind::Uint16),
        (5, TypedArrayKind::Int32),
        (6, TypedArrayKind::Uint32),
        (7, TypedArrayKind::Float32),
        (8, TypedArrayKind::Float64),
    ];
    for &(id, expected) in cases {
        let arr = __ts_aot_typed_array_new(0, id);
        assert_eq!(
            arr.kind(),
            expected,
            "kind id {id} must map to {expected:?}"
        );
    }
}

#[test]
fn typed_array_handle_buffer_returns_shared_view_to_underlying_array_buffer() {
    let arr: TypedArrayHandle = __ts_aot_typed_array_new(4, 0);
    let buf1 = arr.buffer();
    let buf2 = arr.buffer();
    assert_eq!(buf1.byte_length(), 4);
    assert_eq!(buf2.byte_length(), 4);
    assert_eq!(
        buf1.bytes().as_ptr(),
        buf2.bytes().as_ptr(),
        "buffer() must return a shared view (same Rc backing)"
    );
}

#[test]
fn typed_array_kind_from_id_with_out_of_range_id_throws_range_error() {
    let result = std::panic::catch_unwind(|| {
        let _ = TypedArrayKind::from_id(9);
    });
    let err = result.expect_err("from_id(9) must throw (valid ids 0..=8)");
    let msg = err
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| err.downcast_ref::<&str>().copied())
        .unwrap_or("");
    assert!(
        msg.contains("RangeError") && msg.contains("kind id 9"),
        "panic payload must contain 'RangeError' and 'kind id 9', got: {msg}"
    );
}

#[test]
fn typed_array_kind_from_id_with_negative_id_throws_range_error() {
    let result = std::panic::catch_unwind(|| {
        let _ = TypedArrayKind::from_id(-1);
    });
    let err = result.expect_err("from_id(-1) must throw (valid ids 0..=8)");
    let msg = err
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| err.downcast_ref::<&str>().copied())
        .unwrap_or("");
    assert!(
        msg.contains("RangeError") && msg.contains("kind id -1"),
        "panic payload must contain 'RangeError' and 'kind id -1', got: {msg}"
    );
}

#[test]
fn typed_array_new_with_invalid_kind_id_throws_range_error() {
    let result = std::panic::catch_unwind(|| {
        let _ = __ts_aot_typed_array_new(8, 99);
    });
    let err = result.expect_err("__ts_aot_typed_array_new(8, 99) must throw on invalid kind_id");
    let msg = err
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| err.downcast_ref::<&str>().copied())
        .unwrap_or("");
    assert!(
        msg.contains("RangeError") && msg.contains("kind id 99"),
        "panic payload must contain 'RangeError' and 'kind id 99', got: {msg}"
    );
}
