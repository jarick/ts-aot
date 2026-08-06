use std::time::{SystemTime, UNIX_EPOCH};

use ts_aot_runtime::{
    __ts_aot_date_get_date, __ts_aot_date_get_full_year, __ts_aot_date_get_hours,
    __ts_aot_date_get_milliseconds, __ts_aot_date_get_minutes, __ts_aot_date_get_month,
    __ts_aot_date_get_seconds, __ts_aot_date_get_time, __ts_aot_date_is_invalid,
    __ts_aot_date_new_from_ms, __ts_aot_date_now, __ts_aot_date_parse, __ts_aot_date_to_iso_string,
    __ts_aot_date_value_of, JsString, MS_PER_DAY, parse_iso8601_to_ms,
};

#[test]
fn parse_iso8601_basic() {
    let ms = parse_iso8601_to_ms("1970-01-01T00:00:00Z").unwrap();
    assert_eq!(ms, 0);
}

#[test]
fn parse_iso8601_with_millis() {
    let ms = parse_iso8601_to_ms("1970-01-01T00:00:00.123Z").unwrap();
    assert_eq!(ms, 123);
}

#[test]
fn parse_iso8601_known_date() {
    let ms = parse_iso8601_to_ms("2000-01-01T00:00:00Z").unwrap();
    assert_eq!(ms, 10_957 * MS_PER_DAY);
}

#[test]
fn parse_iso8601_invalid() {
    assert!(parse_iso8601_to_ms("not a date").is_none());
    assert!(parse_iso8601_to_ms("2024-13-01T00:00:00Z").is_none());
    assert!(parse_iso8601_to_ms("2024-02-30T00:00:00Z").is_none());
}

#[test]
fn parse_iso8601_offset_positive() {
    let ms_plus_5_30 = parse_iso8601_to_ms("2020-01-01T12:00:00+05:30").unwrap();
    let ms_utc = parse_iso8601_to_ms("2020-01-01T06:30:00Z").unwrap();
    assert_eq!(
        ms_plus_5_30, ms_utc,
        "2020-01-01T12:00:00+05:30 must equal 2020-01-01T06:30:00Z (offset subtracted to UTC)"
    );
}

#[test]
fn parse_iso8601_offset_negative() {
    let ms_minus_8 = parse_iso8601_to_ms("2020-01-01T04:00:00-08:00").unwrap();
    let ms_utc = parse_iso8601_to_ms("2020-01-01T12:00:00Z").unwrap();
    assert_eq!(
        ms_minus_8, ms_utc,
        "2020-01-01T04:00:00-08:00 must equal 2020-01-01T12:00:00Z"
    );
}

#[test]
fn parse_iso8601_offset_malformed() {
    assert!(parse_iso8601_to_ms("2020-01-01T12:00:00+10:99").is_none());
    assert!(parse_iso8601_to_ms("2020-01-01T12:00:00+1:00").is_none());
    assert!(parse_iso8601_to_ms("2020-01-01T12:00:00+100:00").is_none());
    assert!(parse_iso8601_to_ms("2020-01-01T12:00:00+").is_none());
    assert!(parse_iso8601_to_ms("2020-01-01T12:00:00+abc").is_none());
}

#[test]
fn parse_iso8601_offset_compact_multibyte_does_not_panic() {
    assert!(parse_iso8601_to_ms("2020-01-01T00:00:00+€a").is_none());
    let js = JsString::from("2020-01-01T00:00:00+€a");
    assert_eq!(__ts_aot_date_parse(&js), i64::MIN);
}

#[test]
fn parse_iso8601_expanded_year_offset_multibyte_does_not_panic() {
    assert!(parse_iso8601_to_ms("+010000-01-01T00:00:00.000+€a").is_none());
}

#[test]
fn parse_iso8601_rejects_underscore_date_separator() {
    assert!(parse_iso8601_to_ms("2020_01_01T00:00:00Z").is_none());
}

#[test]
fn parse_iso8601_rejects_extra_time_segments() {
    assert!(parse_iso8601_to_ms("2020-01-01T12:00:00:00Z").is_none());
    assert!(parse_iso8601_to_ms("2020-01-01T12:00:00:00:00Z").is_none());
}

#[test]
fn parse_iso8601_rejects_extra_fractional_components() {
    assert!(parse_iso8601_to_ms("2020-01-01T12:00:00.5.5Z").is_none());
    assert!(parse_iso8601_to_ms("2020-01-01T12:00:00..5Z").is_none());
}

#[test]
fn parse_iso8601_rejects_unconsumed_date_content() {
    assert!(parse_iso8601_to_ms("2020-01-01-extra").is_none());
    assert!(parse_iso8601_to_ms("2020-01-1").is_none());
    assert!(parse_iso8601_to_ms("2020-1").is_none());
}

#[test]
fn get_full_year_epoch() {
    assert_eq!(__ts_aot_date_get_full_year(0), 1970);
}

#[test]
fn get_full_year_y2k() {
    let ms = 946_684_800_000;
    assert_eq!(__ts_aot_date_get_full_year(ms), 2000);
}

#[test]
fn to_iso_string_epoch() {
    let s = __ts_aot_date_to_iso_string(0);
    assert_eq!(s.to_string_lossy(), "1970-01-01T00:00:00.000Z");
}

#[test]
fn to_iso_string_known() {
    let s = __ts_aot_date_to_iso_string(946_684_800_000);
    assert_eq!(s.to_string_lossy(), "2000-01-01T00:00:00.000Z");
}

#[test]
fn parse_iso8601_expanded_year_positive() {
    let ms = parse_iso8601_to_ms("+010000-01-01T00:00:00.000Z").unwrap();
    let formatted = __ts_aot_date_to_iso_string(ms).to_string_lossy();
    assert_eq!(
        formatted, "+010000-01-01T00:00:00.000Z",
        "round-trip expanded year +010000"
    );
    assert_eq!(
        __ts_aot_date_get_full_year(ms),
        10_000,
        "expanded year getter must return 10000"
    );
}

#[test]
fn parse_iso8601_expanded_year_negative() {
    let ms = parse_iso8601_to_ms("-010000-01-01T00:00:00.000Z").unwrap();
    let formatted = __ts_aot_date_to_iso_string(ms).to_string_lossy();
    assert_eq!(
        formatted, "-010000-01-01T00:00:00.000Z",
        "round-trip expanded year -010000"
    );
    assert_eq!(__ts_aot_date_get_full_year(ms), -10_000);
}

#[test]
fn parse_iso8601_bce_expanded_year_round_trip() {
    let ms = parse_iso8601_to_ms("-000001-01-01T00:00:00.000Z").unwrap();
    let formatted = __ts_aot_date_to_iso_string(ms).to_string_lossy();
    assert_eq!(
        formatted, "-000001-01-01T00:00:00.000Z",
        "BCE year -000001 must round-trip exactly as six-digit expanded format"
    );
    assert_eq!(__ts_aot_date_get_full_year(ms), -1);
}

#[test]
fn parse_iso8601_ecmascript_min_endpoint_round_trip() {
    let ms = parse_iso8601_to_ms("-271821-04-20T00:00:00.000Z").unwrap();
    let formatted = __ts_aot_date_to_iso_string(ms).to_string_lossy();
    assert_eq!(
        formatted, "-271821-04-20T00:00:00.000Z",
        "ECMAScript min endpoint must round-trip exactly"
    );
    assert_eq!(__ts_aot_date_get_full_year(ms), -271_821);
}

#[test]
fn parse_iso8601_ecmascript_max_endpoint_round_trip() {
    let ms = parse_iso8601_to_ms("+275760-09-13T00:00:00.000Z").unwrap();
    let formatted = __ts_aot_date_to_iso_string(ms).to_string_lossy();
    assert_eq!(
        formatted, "+275760-09-13T00:00:00.000Z",
        "ECMAScript max endpoint must round-trip exactly"
    );
    assert_eq!(__ts_aot_date_get_full_year(ms), 275_760);
}

#[test]
fn parse_iso8601_date_only_is_utc_midnight() {
    assert_eq!(
        parse_iso8601_to_ms("1970-01-01").unwrap(),
        0,
        "date-only ISO 8601 must parse as UTC midnight per ECMAScript spec"
    );
    assert_eq!(
        parse_iso8601_to_ms("2000-01-01").unwrap(),
        946_684_800_000,
        "date-only ISO 8601 must parse as UTC midnight per ECMAScript spec"
    );
}

#[cfg(feature = "tz-utc-only")]
#[test]
fn tz_utc_only_mode_interprets_offset_free_as_utc() {
    let ms = parse_iso8601_to_ms("2020-01-01T12:00:00").unwrap();
    assert_eq!(
        ms, 1_577_880_000_000,
        "in tz-utc-only mode, offset-free DateTime parses as UTC 12:00"
    );
    assert_eq!(
        __ts_aot_date_to_iso_string(ms).to_string_lossy(),
        "2020-01-01T12:00:00.000Z"
    );
    assert_eq!(
        __ts_aot_date_get_full_year(ms),
        2020,
        "year invariant across tz"
    );
}

#[test]
fn parse_iso8601_rejects_out_of_timeclip_range() {
    assert!(parse_iso8601_to_ms("2024-01-01T00:00:00+99:00").is_none());
}

#[test]
fn now_is_close_to_current_time() {
    let before = i64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis(),
    )
    .unwrap_or(0);
    let now = __ts_aot_date_now();
    let after = i64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis(),
    )
    .unwrap_or(0);
    assert!(now >= before, "now {now} should be >= before {before}");
    assert!(now <= after, "now {now} should be <= after {after}");
}

#[test]
fn local_getters_differ_across_distinct_epochs() {
    let epochs: [(i64, i64); 4] = [
        (0, 1970),
        (946_684_800_000, 2000),
        (1_577_836_800_000, 2020),
        (1_704_067_200_000, 2024),
    ];
    for (ms, expected_year) in epochs {
        assert_eq!(
            __ts_aot_date_get_full_year(ms),
            expected_year,
            "get_full_year({ms}) must return {expected_year}"
        );
    }
}

#[test]
fn invalid_sentinel_round_trips() {
    assert_eq!(__ts_aot_date_new_from_ms(i64::MIN), i64::MIN);
    assert_eq!(__ts_aot_date_value_of(i64::MIN), i64::MIN);
    assert_eq!(__ts_aot_date_get_time(i64::MIN), i64::MIN);
    assert!(__ts_aot_date_is_invalid(i64::MIN));
    assert_eq!(__ts_aot_date_to_iso_string(i64::MIN).to_string_lossy(), "");
}

#[test]
fn new_from_ms_clamps_out_of_timeclip_to_invalid() {
    assert_eq!(
        __ts_aot_date_new_from_ms(0),
        0,
        "valid in-range ms preserved"
    );
    assert_eq!(
        __ts_aot_date_new_from_ms(8_640_000_000_000_000),
        8_640_000_000_000_000,
        "TimeClip max preserved"
    );
    assert_eq!(
        __ts_aot_date_new_from_ms(-8_640_000_000_000_000),
        -8_640_000_000_000_000,
        "TimeClip min preserved"
    );
    assert_eq!(
        __ts_aot_date_new_from_ms(8_640_000_000_000_001),
        i64::MIN,
        "just past TimeClip max -> invalid sentinel"
    );
    assert_eq!(
        __ts_aot_date_new_from_ms(-8_640_000_000_000_001),
        i64::MIN,
        "just past TimeClip min -> invalid sentinel"
    );
}

#[test]
fn get_month_january_returns_zero() {
    let jan_ms = parse_iso8601_to_ms("2020-01-15T00:00:00Z").unwrap();
    assert_eq!(
        __ts_aot_date_get_month(jan_ms),
        0,
        "January must return 0 (JS spec: getMonth is 0-indexed)"
    );
}

#[test]
fn get_month_december_returns_eleven() {
    let dec_ms = parse_iso8601_to_ms("2020-12-15T00:00:00Z").unwrap();
    assert_eq!(
        __ts_aot_date_get_month(dec_ms),
        11,
        "December must return 11 (JS spec: getMonth is 0-indexed, 0..=11)"
    );
}

#[test]
fn getters_return_zero_on_invalid() {
    assert_eq!(__ts_aot_date_get_full_year(i64::MIN), 0);
    assert_eq!(__ts_aot_date_get_month(i64::MIN), 0);
    assert_eq!(__ts_aot_date_get_date(i64::MIN), 0);
    assert_eq!(__ts_aot_date_get_hours(i64::MIN), 0);
    assert_eq!(__ts_aot_date_get_minutes(i64::MIN), 0);
    assert_eq!(__ts_aot_date_get_seconds(i64::MIN), 0);
    assert_eq!(__ts_aot_date_get_milliseconds(i64::MIN), 0);
}
