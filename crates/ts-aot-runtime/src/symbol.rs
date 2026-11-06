use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard, OnceLock};

use crate::arena::Ctx;
use crate::string::JsString;

struct RegistryState {
    next_id: i64,
    forward: HashMap<String, i64>,
    reverse: HashMap<i64, String>,
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
pub fn __ts_aot_symbol_new_desc(description: &JsString<'_>) -> i64 {
    let _ = description;
    allocate_unique_id(&mut lock_registry())
}

#[must_use]
pub fn __ts_aot_symbol_for(key: &JsString<'_>) -> i64 {
    let owned = key.to_string_lossy();
    let mut state = lock_registry();
    if let Some(&id) = state.forward.get(&owned) {
        return id;
    }
    let id = allocate_unique_id(&mut state);
    state.forward.insert(owned.clone(), id);
    state.reverse.insert(id, owned);
    id
}

#[must_use]
pub fn __ts_aot_symbol_key_for<'a>(ctx: &Ctx<'a>, sym: i64) -> Option<JsString<'a>> {
    lock_registry()
        .reverse
        .get(&sym)
        .map(|owned| JsString::Valid(ctx.arena.alloc_str(owned)))
}
