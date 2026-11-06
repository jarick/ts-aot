use gen_ts_app::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let mut bump = Bump::new();
    let ctx = Ctx {
        arena: &bump,
        host: &host,
    };
    let value = gen_ts_app::add(&ctx, 2, 3);
    assert_eq!(value, 5, "compiled TS add(2, 3) must be 5");
    println!("add(2, 3) = {value}");
    bump.reset();
}
