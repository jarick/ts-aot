use std::time::{SystemTime, UNIX_EPOCH};

use jiff::Timestamp;
use jiff::civil::{Date, DateTime};
use jiff::tz::TimeZone;

use crate::string::JsString;

const INVALID_DATE_MS: i64 = i64::MIN;
pub const MS_PER_DAY: i64 = 86_400_000;
const MS_PER_HOUR: i64 = 3_600_000;
const MS_PER_MINUTE: i64 = 60_000;
const MS_PER_SECOND: i64 = 1_000;
const NS_PER_MS: i64 = 1_000_000;
const TIMECLIP_MIN_MS: i64 = -8_640_000_000_000_000;
const TIMECLIP_MAX_MS: i64 = 8_640_000_000_000_000;

fn timeclip(ms: i64) -> Option<i64> {
    if !(TIMECLIP_MIN_MS..=TIMECLIP_MAX_MS).contains(&ms) {
        return None;
    }
    Some(ms)
}

struct Civil {
    year: i64,
    month: u8,
    day: u8,
    hour: u8,
    minute: u8,
    second: u8,
    millisecond: i32,
}

impl Civil {
    fn from_jiff(dt: DateTime) -> Self {
        Self {
            year: i64::from(dt.year()),
            month: u8::try_from(i64::from(dt.month())).unwrap_or(0),
            day: u8::try_from(i64::from(dt.day())).unwrap_or(0),
            hour: u8::try_from(i64::from(dt.hour())).unwrap_or(0),
            minute: u8::try_from(i64::from(dt.minute())).unwrap_or(0),
            second: u8::try_from(i64::from(dt.second())).unwrap_or(0),
            millisecond: dt.subsec_nanosecond() / i32::try_from(NS_PER_MS).unwrap_or(1_000_000),
        }
    }

    fn from_ms_unchecked(ms: i64) -> Self {
        let days = ms.div_euclid(MS_PER_DAY);
        let ms_in_day = ms.rem_euclid(MS_PER_DAY);
        let (year, month, day) = civil_from_days_unchecked(days);
        Self {
            year,
            month,
            day,
            hour: u8::try_from(ms_in_day / MS_PER_HOUR).unwrap_or(0),
            minute: u8::try_from((ms_in_day % MS_PER_HOUR) / MS_PER_MINUTE).unwrap_or(0),
            second: u8::try_from((ms_in_day % MS_PER_MINUTE) / MS_PER_SECOND).unwrap_or(0),
            millisecond: i32::try_from(ms_in_day % MS_PER_SECOND).unwrap_or(0),
        }
    }
}

fn host_tz() -> TimeZone {
    #[cfg(feature = "tz-utc-only")]
    {
        TimeZone::UTC
    }
    #[cfg(not(feature = "tz-utc-only"))]
    {
        TimeZone::system()
    }
}

fn ms_to_civil_local(ms: i64) -> Option<Civil> {
    if let Ok(ts) = Timestamp::from_millisecond(ms) {
        return Some(Civil::from_jiff(ts.to_zoned(host_tz()).datetime()));
    }
    if (TIMECLIP_MIN_MS..=TIMECLIP_MAX_MS).contains(&ms) {
        return Some(Civil::from_ms_unchecked(ms));
    }
    None
}

fn ms_to_civil_utc(ms: i64) -> Option<Civil> {
    if let Ok(ts) = Timestamp::from_millisecond(ms) {
        return Some(Civil::from_jiff(ts.to_zoned(TimeZone::UTC).datetime()));
    }
    if (TIMECLIP_MIN_MS..=TIMECLIP_MAX_MS).contains(&ms) {
        return Some(Civil::from_ms_unchecked(ms));
    }
    None
}

fn days_from_civil_unchecked(year: i64, month: i64, day: i64) -> i64 {
    let mut y = year;
    let m = month;
    if m <= 2 {
        y -= 1;
    }
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let m_ = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * m_ + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days_unchecked(z: i64) -> (i64, u8, u8) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = u8::try_from(doy - (153 * mp + 2) / 5 + 1).unwrap_or(0);
    let m = if mp < 10 {
        u8::try_from(mp + 3).unwrap_or(0)
    } else {
        u8::try_from(mp - 9).unwrap_or(0)
    };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

fn parse_offset_minutes(offset: &str) -> Option<i64> {
    if offset.is_empty() {
        return None;
    }
    let (sign, body) = match offset.as_bytes()[0] {
        b'+' => (1_i64, &offset[1..]),
        b'-' => (-1_i64, &offset[1..]),
        _ => return None,
    };
    let (h, m) = if body.contains(':') {
        let segs: Vec<&str> = body.split(':').collect();
        if segs.len() != 2 || segs[0].len() != 2 || segs[1].len() != 2 {
            return None;
        }
        (segs[0].parse::<i64>().ok()?, segs[1].parse::<i64>().ok()?)
    } else if body.len() == 2 {
        (body.parse::<i64>().ok()?, 0)
    } else if body.len() == 4 {
        (
            body.get(..2)?.parse::<i64>().ok()?,
            body.get(2..)?.parse::<i64>().ok()?,
        )
    } else {
        return None;
    };
    if !(0..=23).contains(&h) || !(0..=59).contains(&m) {
        return None;
    }
    Some(sign * (h * 60 + m))
}

fn parse_hms_frac_to_ms(s: &str) -> Option<i64> {
    if s.is_empty() {
        return Some(0);
    }
    let end = s.find('Z').unwrap_or(s.len());
    let s_no_z = &s[..end];
    let (hms_frac, offset_min) = if let Some(idx) = s_no_z[1..].find(['+', '-']).map(|i| i + 1) {
        (&s_no_z[..idx], Some(parse_offset_minutes(&s_no_z[idx..])?))
    } else {
        (s_no_z, None)
    };
    let mut parts = hms_frac.split('.');
    let hms = parts.next()?;
    let segs: Vec<&str> = hms.split(':').collect();
    if segs.len() < 2 || segs.len() > 3 {
        return None;
    }
    let h: i64 = segs[0].parse().ok()?;
    let m: i64 = segs[1].parse().ok()?;
    let sec = if segs.len() >= 3 {
        segs[2].parse().ok()?
    } else {
        0
    };
    let ms = if let Some(frac) = parts.next() {
        if parts.next().is_some() {
            return None;
        }
        let mut padded = frac.to_string();
        padded.truncate(3);
        while padded.len() < 3 {
            padded.push('0');
        }
        padded.parse::<i64>().ok()?
    } else {
        0
    };
    if !(0..=23).contains(&h) || m > 59 || sec > 59 {
        return None;
    }
    let time_ms = h * MS_PER_HOUR + m * MS_PER_MINUTE + sec * MS_PER_SECOND + ms;
    let total = if let Some(off) = offset_min {
        time_ms - off * MS_PER_MINUTE
    } else {
        time_ms
    };
    Some(total)
}

fn parse_expanded_year(s: &str) -> Option<i64> {
    let bytes = s.as_bytes();
    let (sign, start) = match bytes.first()? {
        b'+' => (1_i64, 1_usize),
        b'-' => (-1_i64, 1_usize),
        _ => return None,
    };
    let mut i = start;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    if i == start {
        return None;
    }
    let year_str = &s[start..i];
    let year_digits: i64 = year_str.parse().ok()?;
    let year = sign * year_digits;
    if bytes.get(i) != Some(&b'-') {
        return None;
    }
    let rest = &s[i + 1..];
    let (date_part, time_part) = match rest.find('T') {
        Some(t) => (&rest[..t], &rest[t + 1..]),
        None => (rest, ""),
    };
    let dash = date_part.find('-')?;
    let month: i64 = date_part[..dash].parse().ok()?;
    let day: i64 = date_part[dash + 1..].parse().ok()?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let days = days_from_civil_unchecked(year, month, day);
    let time_ms = parse_hms_frac_to_ms(time_part)?;
    let total_ms = days.checked_mul(MS_PER_DAY)?.checked_add(time_ms)?;
    timeclip(total_ms)
}

#[must_use]
pub fn parse_iso8601_to_ms(s: &str) -> Option<i64> {
    let s = s.trim();
    let raw = if let Ok(ts) = s.parse::<Timestamp>() {
        ts.as_millisecond()
    } else if let Ok(dt) = s.parse::<DateTime>() {
        dt.to_zoned(host_tz()).ok()?.timestamp().as_millisecond()
    } else if let Ok(date) = s.parse::<Date>() {
        date.at(0, 0, 0, 0)
            .to_zoned(TimeZone::UTC)
            .ok()?
            .timestamp()
            .as_millisecond()
    } else {
        parse_expanded_year(s)?
    };
    timeclip(raw)
}

fn format_iso(ms: i64) -> String {
    let Some(c) = ms_to_civil_utc(ms) else {
        return String::new();
    };
    let time = format!(
        "{:02}:{:02}:{:02}.{:03}Z",
        c.hour, c.minute, c.second, c.millisecond
    );
    let abs_year = c.year.unsigned_abs();
    let month = c.month;
    let day = c.day;
    let date = if c.year >= 10_000 {
        format!("+{abs_year:06}-{month:02}-{day:02}")
    } else if c.year < 0 {
        format!("-{abs_year:06}-{month:02}-{day:02}")
    } else {
        format!("{:04}-{month:02}-{day:02}", c.year)
    };
    format!("{date}T{time}")
}

#[must_use]
pub fn __ts_aot_date_now() -> i64 {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_nanos()).unwrap_or(i64::MAX));
    nanos / NS_PER_MS
}

#[must_use]
pub fn __ts_aot_date_new_from_ms(ms: i64) -> i64 {
    timeclip(ms).unwrap_or(INVALID_DATE_MS)
}

#[must_use]
pub fn __ts_aot_date_parse(s: &JsString) -> i64 {
    parse_iso8601_to_ms(&s.to_string_lossy()).unwrap_or(INVALID_DATE_MS)
}

#[must_use]
pub fn __ts_aot_date_value_of(ms: i64) -> i64 {
    if ms == INVALID_DATE_MS {
        INVALID_DATE_MS
    } else {
        ms
    }
}

#[must_use]
pub fn __ts_aot_date_get_time(ms: i64) -> i64 {
    __ts_aot_date_value_of(ms)
}

#[must_use]
pub fn __ts_aot_date_get_full_year(ms: i64) -> i64 {
    if ms == INVALID_DATE_MS {
        return 0;
    }
    ms_to_civil_local(ms).map_or(0, |c| c.year)
}

#[must_use]
pub fn __ts_aot_date_get_month(ms: i64) -> i64 {
    if ms == INVALID_DATE_MS {
        return 0;
    }
    ms_to_civil_local(ms).map_or(0, |c| i64::from(c.month) - 1)
}

#[must_use]
pub fn __ts_aot_date_get_date(ms: i64) -> i64 {
    if ms == INVALID_DATE_MS {
        return 0;
    }
    ms_to_civil_local(ms).map_or(0, |c| i64::from(c.day))
}

#[must_use]
pub fn __ts_aot_date_get_hours(ms: i64) -> i64 {
    if ms == INVALID_DATE_MS {
        return 0;
    }
    ms_to_civil_local(ms).map_or(0, |c| i64::from(c.hour))
}

#[must_use]
pub fn __ts_aot_date_get_minutes(ms: i64) -> i64 {
    if ms == INVALID_DATE_MS {
        return 0;
    }
    ms_to_civil_local(ms).map_or(0, |c| i64::from(c.minute))
}

#[must_use]
pub fn __ts_aot_date_get_seconds(ms: i64) -> i64 {
    if ms == INVALID_DATE_MS {
        return 0;
    }
    ms_to_civil_local(ms).map_or(0, |c| i64::from(c.second))
}

#[must_use]
pub fn __ts_aot_date_get_milliseconds(ms: i64) -> i64 {
    if ms == INVALID_DATE_MS {
        return 0;
    }
    ms_to_civil_local(ms).map_or(0, |c| i64::from(c.millisecond))
}

#[must_use]
pub fn __ts_aot_date_to_iso_string(ms: i64) -> JsString {
    if ms == INVALID_DATE_MS {
        return JsString::Valid(String::new());
    }
    JsString::Valid(format_iso(ms))
}

#[must_use]
pub fn __ts_aot_date_is_invalid(ms: i64) -> bool {
    ms == INVALID_DATE_MS
}
