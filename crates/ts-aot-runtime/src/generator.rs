pub const GENERATOR_DONE_STATE: u32 = u32::MAX;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GeneratorResult<T> {
    Yielded(Option<T>),
    Done(Option<T>),
}

pub type GeneratorDispatch<T> = fn(&mut Generator<T>) -> GeneratorResult<T>;

pub struct Generator<T> {
    pub state: u32,
    pub stored: Option<T>,
    pub dispatch: GeneratorDispatch<T>,
}

impl<T> Generator<T> {
    #[must_use]
    pub fn new(dispatch: GeneratorDispatch<T>) -> Self {
        Self {
            state: 0,
            stored: None,
            dispatch,
        }
    }

    pub fn store(&mut self, value: T) {
        self.stored = Some(value);
    }

    pub fn set_state(&mut self, state: u32) {
        self.state = state;
    }

    pub fn next(&mut self) -> GeneratorResult<T> {
        if self.state == GENERATOR_DONE_STATE {
            return GeneratorResult::Done(None);
        }
        let result = (self.dispatch)(self);
        if matches!(result, GeneratorResult::Done(_)) {
            self.state = GENERATOR_DONE_STATE;
        }
        result
    }
}

#[must_use]
pub fn __ts_aot_generator_get_state<T>(g: &Generator<T>) -> u32 {
    g.state
}

pub fn __ts_aot_generator_set_state<T>(g: &mut Generator<T>, state: u32) {
    g.state = state;
}

pub fn __ts_aot_generator_store<T>(g: &mut Generator<T>, value: T) {
    g.stored = Some(value);
}

#[must_use]
pub fn __ts_aot_generator_yielded<T>(g: &mut Generator<T>) -> GeneratorResult<T> {
    GeneratorResult::Yielded(g.stored.take())
}

#[must_use]
pub fn __ts_aot_generator_done<T>() -> GeneratorResult<T> {
    GeneratorResult::Done(None)
}

#[must_use]
pub fn __ts_aot_generator_done_with<T>(value: T) -> GeneratorResult<T> {
    GeneratorResult::Done(Some(value))
}
