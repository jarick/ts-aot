pub const RUNTIME_SOURCE: &str = include_str!("runtime_source.rs");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_source_is_non_empty() {
        assert!(!RUNTIME_SOURCE.is_empty());
    }

    #[test]
    fn runtime_source_contains_host_console_log() {
        assert!(
            RUNTIME_SOURCE.contains("__ts_aot_host_console_log"),
            "runtime must define __ts_aot_host_console_log"
        );
    }

    #[test]
    fn runtime_source_contains_math_sqrt() {
        assert!(
            RUNTIME_SOURCE.contains("__ts_aot_math_sqrt"),
            "runtime must define __ts_aot_math_sqrt"
        );
    }

    #[test]
    fn runtime_source_contains_string_helpers() {
        assert!(RUNTIME_SOURCE.contains("__ts_aot_string_concat"));
        assert!(RUNTIME_SOURCE.contains("__ts_aot_string_equals"));
        assert!(RUNTIME_SOURCE.contains("__ts_aot_string_len"));
    }

    #[test]
    fn runtime_source_contains_array_helpers() {
        assert!(RUNTIME_SOURCE.contains("__ts_aot_array_create"));
        assert!(RUNTIME_SOURCE.contains("__ts_aot_array_get"));
        assert!(RUNTIME_SOURCE.contains("__ts_aot_array_set"));
        assert!(RUNTIME_SOURCE.contains("__ts_aot_array_len"));
    }

    #[test]
    fn runtime_source_contains_map_helpers() {
        assert!(RUNTIME_SOURCE.contains("__ts_aot_map_create"));
        assert!(RUNTIME_SOURCE.contains("__ts_aot_map_get"));
        assert!(RUNTIME_SOURCE.contains("__ts_aot_map_set"));
    }
}
