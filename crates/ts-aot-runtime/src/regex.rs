use crate::host::__ts_aot_throw;

#[derive(Clone)]
pub struct RegExpHandle {
    #[allow(dead_code)]
    regex: regex::Regex,
    source: String,
}

impl RegExpHandle {
    pub(crate) fn new(pattern: &str, flags: &str) -> Result<Self, regex::Error> {
        let compiled = compile_js_pattern(pattern, flags);
        let regex = regex::Regex::new(&compiled)?;
        Ok(Self {
            regex,
            source: pattern.to_owned(),
        })
    }

    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }
}

fn compile_js_pattern(pattern: &str, flags: &str) -> String {
    let rust_flags: String = flags
        .chars()
        .filter(|c| matches!(c, 'i' | 's' | 'm'))
        .collect();
    if rust_flags.is_empty() {
        pattern.to_owned()
    } else {
        format!("(?{rust_flags}){pattern}")
    }
}

fn validate_flags(flags: &str) -> Result<(), String> {
    let mut seen = [false; 6];
    for ch in flags.chars() {
        let slot = match ch {
            'g' => 0,
            'i' => 1,
            'm' => 2,
            's' => 3,
            'u' => 4,
            'y' => 5,
            _ => return Err(format!("invalid flag '{ch}'")),
        };
        if seen[slot] {
            return Err(format!("duplicate flag '{ch}'"));
        }
        seen[slot] = true;
    }
    Ok(())
}

#[must_use]
pub fn __ts_aot_regex_new(pattern: &str, flags: &str) -> RegExpHandle {
    if let Err(e) = validate_flags(flags) {
        __ts_aot_throw(format!("SyntaxError: invalid regex flags '{flags}': {e}"));
    }
    match RegExpHandle::new(pattern, flags) {
        Ok(h) => h,
        Err(e) => __ts_aot_throw(format!(
            "SyntaxError: invalid regex pattern '{pattern}': {e}"
        )),
    }
}
