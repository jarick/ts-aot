use crate::ids::StringId;
use crate::interner::Interner;

#[derive(Debug, Default, Clone)]
pub struct StringTable(Interner<String, StringId>);

impl StringTable {
    #[must_use]
    pub fn new() -> Self {
        Self(Interner::new())
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn intern(&mut self, s: &str) -> StringId {
        self.0.intern(s)
    }

    #[must_use]
    pub fn resolve(&self, id: StringId) -> Option<&str> {
        self.0.resolve(id).map(String::as_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_table_has_no_strings() {
        let table = StringTable::new();
        assert!(table.is_empty());
    }
}
