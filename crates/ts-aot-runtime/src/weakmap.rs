use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::{Rc, Weak};

type Entry<V> = (Weak<()>, V);

pub struct WeakMapHandle<V> {
    entries: Rc<RefCell<HashMap<usize, Entry<V>>>>,
}

impl<V> Clone for WeakMapHandle<V> {
    fn clone(&self) -> Self {
        Self {
            entries: Rc::clone(&self.entries),
        }
    }
}

impl<V> Default for WeakMapHandle<V> {
    fn default() -> Self {
        Self {
            entries: Rc::new(RefCell::new(HashMap::new())),
        }
    }
}

fn purge_dead<V>(entries: &mut HashMap<usize, Entry<V>>) {
    entries.retain(|_, (weak, _)| weak.strong_count() > 0);
}

#[must_use]
pub fn __ts_aot_weak_map_new<V>() -> WeakMapHandle<V> {
    WeakMapHandle::default()
}

pub fn __ts_aot_weak_map_set<V>(
    handle: &WeakMapHandle<V>,
    liveness: &Rc<()>,
    key: *const (),
    value: V,
) -> i64 {
    let mut entries = handle.entries.borrow_mut();
    purge_dead(&mut entries);
    entries.insert(key as usize, (Rc::downgrade(liveness), value));
    1
}

#[must_use]
pub fn __ts_aot_weak_map_get<V: Clone>(
    handle: &WeakMapHandle<V>,
    _liveness: &Rc<()>,
    key: *const (),
) -> Option<V> {
    let mut entries = handle.entries.borrow_mut();
    purge_dead(&mut entries);
    entries.get(&(key as usize)).map(|(_, v)| v.clone())
}

#[must_use]
pub fn __ts_aot_weak_map_has<V>(
    handle: &WeakMapHandle<V>,
    _liveness: &Rc<()>,
    key: *const (),
) -> i64 {
    let mut entries = handle.entries.borrow_mut();
    purge_dead(&mut entries);
    i64::from(entries.contains_key(&(key as usize)))
}

#[must_use]
pub fn __ts_aot_weak_map_delete<V>(
    handle: &WeakMapHandle<V>,
    _liveness: &Rc<()>,
    key: *const (),
) -> i64 {
    let mut entries = handle.entries.borrow_mut();
    purge_dead(&mut entries);
    i64::from(entries.remove(&(key as usize)).is_some())
}

pub fn __ts_aot_weak_map_clear<V>(handle: &WeakMapHandle<V>) {
    handle.entries.borrow_mut().clear();
}
