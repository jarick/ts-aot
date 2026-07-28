#[must_use]
pub fn canonical_integer_index(s: &str) -> Option<u64> {
    let bytes = s.as_bytes();
    if bytes.is_empty() {
        return None;
    }
    if bytes[0] == b'0' {
        return if bytes.len() == 1 { Some(0) } else { None };
    }
    if !bytes[0].is_ascii_digit() {
        return None;
    }
    for &b in &bytes[1..] {
        if !b.is_ascii_digit() {
            return None;
        }
    }
    s.parse::<u64>().ok().filter(|&n| n < u64::from(u32::MAX))
}

#[cfg(test)]
mod tests {
    use super::canonical_integer_index;

    #[test]
    fn empty_string_returns_none() {
        assert_eq!(canonical_integer_index(""), None);
    }

    #[test]
    fn zero_is_canonical() {
        assert_eq!(canonical_integer_index("0"), Some(0));
    }

    #[test]
    fn leading_zero_variants_rejected() {
        assert_eq!(canonical_integer_index("00"), None);
        assert_eq!(canonical_integer_index("01"), None);
        assert_eq!(canonical_integer_index("0123"), None);
    }

    #[test]
    fn non_digit_prefix_rejected() {
        assert_eq!(canonical_integer_index("a"), None);
        assert_eq!(canonical_integer_index("+1"), None);
        assert_eq!(canonical_integer_index("-1"), None);
        assert_eq!(canonical_integer_index(" 1"), None);
    }

    #[test]
    fn non_digit_in_middle_rejected() {
        assert_eq!(canonical_integer_index("12a"), None);
        assert_eq!(canonical_integer_index("1 2"), None);
    }

    #[test]
    fn u32_max_is_rejected_max_valid_is_max_minus_one() {
        assert_eq!(canonical_integer_index("4294967295"), None);
        assert_eq!(canonical_integer_index("4294967294"), Some(4_294_967_294));
    }

    #[test]
    fn values_above_u32_max_rejected() {
        assert_eq!(canonical_integer_index("4294967296"), None);
        assert_eq!(canonical_integer_index("99999999999"), None);
    }

    #[test]
    fn typical_indices_accepted() {
        assert_eq!(canonical_integer_index("1"), Some(1));
        assert_eq!(canonical_integer_index("42"), Some(42));
        assert_eq!(canonical_integer_index("1000000"), Some(1_000_000));
    }
}
