use ts_aot_runtime::{__ts_aot_array_buffer_new, __ts_aot_array_buffer_slice, ArrayBufferHandle};

#[test]
fn array_buffer_new_with_zero_size_produces_empty_buffer() {
    let buf = __ts_aot_array_buffer_new(0);
    assert_eq!(buf.byte_length(), 0);
    assert!(buf.bytes().is_empty());
}

#[test]
fn array_buffer_new_initializes_bytes_to_zero() {
    let buf = __ts_aot_array_buffer_new(16);
    assert_eq!(buf.byte_length(), 16);
    assert_eq!(buf.bytes().len(), 16);
    assert!(
        buf.bytes().iter().all(|&b| b == 0),
        "new ArrayBuffer must zero-initialize all bytes (per spec AllocateArrayBuffer)"
    );
}

#[test]
fn array_buffer_slice_basic_clones_subset() {
    let src = ArrayBufferHandle::from_slice(&[10, 20, 30, 40, 50, 60, 70, 80]);
    let slice = __ts_aot_array_buffer_slice(&src, 2, 5);
    assert_eq!(slice.byte_length(), 3);
    assert_eq!(
        slice.bytes(),
        &[30, 40, 50],
        "slice(2, 5) must copy bytes [2..5) of source, in order"
    );
}

#[test]
fn array_buffer_slice_negative_begin_or_end_beyond_byte_length_saturate_to_byte_length() {
    let src = ArrayBufferHandle::from_slice(&[10, 20, 30, 40]);
    let slice = __ts_aot_array_buffer_slice(&src, -10, 100);
    assert_eq!(slice.byte_length(), 4);
    assert_eq!(
        slice.bytes(),
        &[10, 20, 30, 40],
        "begin < -byteLength saturates to 0, end > byteLength clamps to byteLength; result is full copy"
    );
}

#[test]
fn array_buffer_slice_negative_begin_means_offset_from_end() {
    let src = ArrayBufferHandle::from_slice(&[10, 20, 30, 40, 50, 60, 70, 80]);
    let slice = __ts_aot_array_buffer_slice(&src, -1, 8);
    assert_eq!(
        slice.bytes(),
        &[80],
        "slice(-1, 8) on len=8 must return last 1 byte per ECMAScript spec"
    );
    let slice = __ts_aot_array_buffer_slice(&src, -3, 8);
    assert_eq!(
        slice.bytes(),
        &[60, 70, 80],
        "slice(-3, 8) on len=8 must return last 3 bytes [5..8)"
    );
    let slice = __ts_aot_array_buffer_slice(&src, -3, -1);
    assert_eq!(
        slice.bytes(),
        &[60, 70],
        "slice(-3, -1) on len=8 must return bytes [5..7) (last 3 [60,70,80] excluding last 1 [80])"
    );
}

#[test]
fn array_buffer_slice_zero_begin_negative_end_excludes_last_bytes() {
    let src = ArrayBufferHandle::from_slice(&[10, 20, 30, 40, 50, 60, 70, 80]);
    let slice = __ts_aot_array_buffer_slice(&src, 0, -1);
    assert_eq!(
        slice.bytes(),
        &[10, 20, 30, 40, 50, 60, 70],
        "slice(0, -1) on len=8 must return 7 bytes [0..7) per ECMAScript spec"
    );
}

#[test]
fn array_buffer_slice_begin_after_end_yields_empty() {
    let src = ArrayBufferHandle::from_slice(&[10, 20, 30, 40, 50, 60, 70, 80, 90, 100]);
    let slice = __ts_aot_array_buffer_slice(&src, 7, 3);
    assert_eq!(slice.byte_length(), 0);
    assert!(slice.bytes().is_empty());
}

#[test]
fn array_buffer_slice_produces_independent_storage_from_source() {
    let src = ArrayBufferHandle::from_slice(&[10, 20, 30, 40, 50]);
    let slice = __ts_aot_array_buffer_slice(&src, 1, 4);
    assert_eq!(slice.bytes(), &[20, 30, 40]);

    let slice_a = __ts_aot_array_buffer_slice(&src, 0, 3);
    let slice_b = __ts_aot_array_buffer_slice(&src, 2, 5);
    assert_eq!(slice_a.bytes(), &[10, 20, 30]);
    assert_eq!(slice_b.bytes(), &[30, 40, 50]);
    assert_ne!(
        slice_a.bytes().as_ptr(),
        slice_b.bytes().as_ptr(),
        "two slices must have separate backing storage (different Vec allocations)"
    );
    assert_ne!(
        src.bytes().as_ptr(),
        slice_a.bytes().as_ptr(),
        "source and slice must have separate backing storage"
    );
}

#[test]
fn array_buffer_slice_with_i64_min_begin_does_not_overflow_and_clamps_to_zero() {
    let src = ArrayBufferHandle::from_slice(&[10, 20, 30, 40, 50]);
    let slice = __ts_aot_array_buffer_slice(&src, i64::MIN, i64::MAX);
    assert_eq!(slice.byte_length(), 5);
    assert_eq!(
        slice.bytes(),
        &[10, 20, 30, 40, 50],
        "i64::MIN begin must clamp to 0 (checked_abs returns None for i64::MIN, no overflow), \
         i64::MAX end must clamp to byteLength"
    );
}

#[test]
fn array_buffer_slice_with_i64_min_end_does_not_overflow_and_clamps_to_zero() {
    let src = ArrayBufferHandle::from_slice(&[10, 20, 30, 40, 50]);
    let slice = __ts_aot_array_buffer_slice(&src, 0, i64::MIN);
    assert_eq!(slice.byte_length(), 0);
    assert!(
        slice.bytes().is_empty(),
        "i64::MIN end must clamp to 0 (checked_abs returns None for i64::MIN, no overflow)"
    );
}
