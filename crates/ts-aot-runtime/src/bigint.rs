#[derive(Debug, Clone)]
pub struct BigIntHandle {
    value: String,
}

impl BigIntHandle {
    #[must_use]
    pub fn new(value: &str) -> Self {
        Self {
            value: value.to_owned(),
        }
    }

    #[must_use]
    pub fn value(&self) -> &str {
        &self.value
    }
}

#[must_use]
pub fn __ts_aot_bigint_new(value: &str) -> BigIntHandle {
    BigIntHandle::new(value)
}
