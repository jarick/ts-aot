use crate::ids::SymbolId;
use crate::interner::Interner;

#[derive(Debug, Default, Clone)]
pub struct SymbolTable(Interner<String, SymbolId>);

impl SymbolTable {
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

    pub fn intern(&mut self, name: &str) -> SymbolId {
        self.0.intern(name)
    }

    #[must_use]
    pub fn resolve(&self, id: SymbolId) -> Option<&str> {
        self.0.resolve(id).map(String::as_str)
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
