use bumpalo::Bump;

pub struct Ctx<'a> {
    pub arena: &'a Bump,
    pub host: &'a dyn Host,
}

pub trait Host: Send + Sync {}

pub fn run_in_arena<R, F>(host: &dyn Host, f: F) -> R
where
    F: for<'a> FnOnce(&Ctx<'a>) -> R,
{
    let mut bump = Bump::new();
    let ctx = Ctx { arena: &bump, host };
    let out = f(&ctx);
    bump.reset();
    out
}

#[cfg(test)]
mod tests {
    use super::{Ctx, Host, run_in_arena};

    struct NoHost;
    impl Host for NoHost {}

    #[test]
    fn run_in_arena_provides_arena_and_host() {
        let host = NoHost;
        let value = run_in_arena(&host, |ctx: &Ctx| {
            let s = ctx.arena.alloc_str("hello");
            assert_eq!(s, "hello");
            42
        });
        assert_eq!(value, 42);
    }

    #[test]
    fn run_in_arena_returns_owned_value_after_reset() {
        let host = NoHost;
        let owned = run_in_arena(&host, |ctx: &Ctx| {
            let s = ctx.arena.alloc_str("world");
            s.to_owned()
        });
        assert_eq!(owned, "world");
    }

    #[test]
    fn host_is_object_safe_and_send_sync() {
        fn assert_send_sync<T: Send + Sync + ?Sized>() {}
        assert_send_sync::<dyn Host>();
        let host: &dyn Host = &NoHost;
        run_in_arena(host, |_ctx: &Ctx| ());
    }
}
