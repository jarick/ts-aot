use std::borrow::{Borrow, ToOwned};
use std::collections::HashMap;
use std::hash::Hash;

#[derive(Debug, Clone)]
pub struct Interner<T, K>
where
    T: Clone + Eq + Hash,
    K: Copy + From<u32> + Into<u32>,
{
    values: Vec<T>,
    ids: HashMap<T, K>,
}

impl<T, K> Default for Interner<T, K>
where
    T: Clone + Eq + Hash,
    K: Copy + From<u32> + Into<u32>,
{
    fn default() -> Self {
        Self {
            values: Vec::new(),
            ids: HashMap::new(),
        }
    }
}

impl<T, K> Interner<T, K>
where
    T: Clone + Eq + Hash,
    K: Copy + From<u32> + Into<u32>,
{
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.values.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    pub fn intern<U>(&mut self, val: &U) -> K
    where
        T: Borrow<U>,
        U: ?Sized + Eq + Hash + ToOwned<Owned = T>,
    {
        if let Some(&id) = self.ids.get(val) {
            return id;
        }

        let owned: T = val.to_owned();
        let raw = u32::try_from(self.values.len()).expect("interner overflow");
        let id = K::from(raw);
        let _ = self.ids.insert(owned.clone(), id);
        self.values.push(owned);
        id
    }

    #[must_use]
    pub fn resolve(&self, id: K) -> Option<&T> {
        let raw: u32 = id.into();
        self.values.get(raw as usize)
    }

    #[must_use]
    pub fn values(&self) -> &[T] {
        &self.values
    }
}
