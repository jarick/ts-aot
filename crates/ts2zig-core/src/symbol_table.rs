use std::collections::HashMap;

use crate::ids::SymbolId;

#[derive(Debug, Default, Clone)]
pub struct SymbolTable {
    names: Vec<String>,
    ids_by_name: HashMap<String, SymbolId>,
}

impl SymbolTable {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.names.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }

    pub fn intern(&mut self, name: &str) -> SymbolId {
        if let Some(&id) = self.ids_by_name.get(name) {
            return id;
        }

        let id = u32::try_from(self.names.len()).expect("symbol table overflow");
        self.names.push(name.to_string());
        let id = SymbolId::from_raw(id);
        self.ids_by_name.insert(name.to_string(), id);
        id
    }

    #[must_use]
    pub fn resolve(&self, id: SymbolId) -> Option<&str> {
        self.names.get(id.raw() as usize).map(String::as_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_table_has_no_symbols() {
        let table = SymbolTable::new();
        assert!(table.is_empty());
    }
}
