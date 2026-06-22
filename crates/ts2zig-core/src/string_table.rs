use std::collections::HashMap;

use crate::ids::StringId;

#[derive(Debug, Default, Clone)]
pub struct StringTable {
    strings: Vec<String>,
    ids_by_string: HashMap<String, StringId>,
}

impl StringTable {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.strings.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.strings.is_empty()
    }

    pub fn intern(&mut self, s: &str) -> StringId {
        if let Some(&id) = self.ids_by_string.get(s) {
            return id;
        }

        let id = u32::try_from(self.strings.len()).expect("string table overflow");
        self.strings.push(s.to_string());
        let id = StringId::from_raw(id);
        self.ids_by_string.insert(s.to_string(), id);
        id
    }

    #[must_use]
    pub fn resolve(&self, id: StringId) -> Option<&str> {
        self.strings.get(id.raw() as usize).map(String::as_str)
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
