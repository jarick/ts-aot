use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard, OnceLock};

use crate::string::JsString;

struct RegistryState {
    next_id: i64,
    forward: HashMap<JsString, i64>,
    reverse: HashMap<i64, JsString>,
}

impl RegistryState {
    fn new() -> Self {
        Self {
            next_id: 0,
            forward: HashMap::new(),
            reverse: HashMap::new(),
        }
    }
}

fn registry_state() -> &'static Mutex<RegistryState> {
    static STATE: OnceLock<Mutex<RegistryState>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(RegistryState::new()))
}

fn lock_registry() -> MutexGuard<'static, RegistryState> {
    registry_state()
        .lock()
        .expect("symbol registry mutex poisoned")
}

fn allocate_unique_id(state: &mut RegistryState) -> i64 {
    let id = state.next_id;
    state.next_id = id.wrapping_add(1);
    id
}

#[must_use]
pub fn __ts_aot_symbol_new() -> i64 {
    allocate_unique_id(&mut lock_registry())
}

#[must_use]
pub fn __ts_aot_symbol_new_desc(description: &JsString) -> i64 {
    let _ = description;
    allocate_unique_id(&mut lock_registry())
}

#[must_use]
pub fn __ts_aot_symbol_for(key: &JsString) -> i64 {
    let mut state = lock_registry();
    if let Some(&id) = state.forward.get(key) {
        return id;
    }
    let id = allocate_unique_id(&mut state);
    state.forward.insert(key.clone(), id);
    state.reverse.insert(id, key.clone());
    id
}

#[must_use]
pub fn __ts_aot_symbol_key_for(sym: i64) -> Option<JsString> {
    lock_registry().reverse.get(&sym).cloned()
}
