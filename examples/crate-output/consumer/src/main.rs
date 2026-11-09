fn main() {
    let value = gen_ts_app::add(2.0, 3.0);
    assert_eq!(value, 5.0, "compiled TS add(2, 3) must be 5");
    println!("add(2, 3) = {value}");
}
