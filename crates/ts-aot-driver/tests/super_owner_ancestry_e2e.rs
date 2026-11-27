use ts_aot_driver::{CompileOptions, Driver, DriverOutput, EmitStage};
use ts_aot_test_support::{check_generated_crate, run_generated_crate};

fn compile(source: &str) -> DriverOutput {
    Driver::new().compile_source(
        "test.ts",
        source,
        &CompileOptions {
            emit: EmitStage::Rust,
            module: true,
        },
    )
}

fn rust_of(source: &str) -> String {
    let out = compile(source);
    assert!(
        !out.has_errors(),
        "must compile through the full pipeline; got {:?}",
        out.diagnostics
    );
    out.rust_source
        .expect("emit-rust must populate rust_source")
}

#[test]
fn a_super_alias_whose_owner_is_not_an_ancestor_is_rejected_without_rust() {
    const SOURCE: &str = "class A { m(): number { return 7; } } \
         class C {} \
         class B extends C { g(): number { return this.__super_1_A_m(); } } \
         export function got(): number { return new B().g(); }";
    let out = compile(SOURCE);
    let rejected = out
        .diagnostics
        .iter()
        .find(|d| d.code.as_str() == "P0023")
        .unwrap_or_else(|| {
            panic!(
                "expected P0023 for a super alias whose owner is not an ancestor; got {:?}",
                out.diagnostics
            )
        });
    assert!(
        rejected.message.contains("ancestor"),
        "the P0023 must explain that the alias owner is not a declared ancestor; got: {}",
        rejected.message
    );
    assert!(
        out.rust_source.is_none(),
        "no Rust may be emitted for a super alias of a class that is not an ancestor; got:\n{:?}",
        out.rust_source
    );
}

#[test]
fn a_super_call_through_a_middle_class_that_skips_the_method_resolves_the_ancestor() {
    const SOURCE: &str = "class A { m(x: number): number { return x + 1; } } \
         class B extends A {} \
         class C extends B { g(x: number): number { return super.m(x); } } \
         export function got(): number { return new C().g(4); }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(super_owner_middle_skip::got(&ctx), 5.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    check_generated_crate("super_owner_middle_skip", &rust);
    let stdout = run_generated_crate("super_owner_middle_skip", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_three_level_super_chain_keeps_each_levels_own_ancestor() {
    const SOURCE: &str = "class A { m(): number { return 100; } } \
         class B extends A { m(): number { return super.m() + 10; } } \
         class C extends B { m(): number { return super.m() + 1; } } \
         export function gotB(): number { return new B().m(); } \
         export function gotC(): number { return new C().m(); }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(super_owner_three_level::gotB(&ctx), 110.0);
    assert_eq!(super_owner_three_level::gotC(&ctx), 111.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    check_generated_crate("super_owner_three_level", &rust);
    let stdout = run_generated_crate("super_owner_three_level", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn two_siblings_keep_their_shared_parent_as_the_alias_owner() {
    const SOURCE: &str = "class P { m(x: number): number { return x * 2; } } \
         class B extends P { g(): number { return super.m(3); } } \
         class D extends P { g(): number { return super.m(5); } } \
         export function gotB(): number { return new B().g(); } \
         export function gotD(): number { return new D().g(); }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(super_owner_siblings::gotB(&ctx), 6.0);
    assert_eq!(super_owner_siblings::gotD(&ctx), 10.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    check_generated_crate("super_owner_siblings", &rust);
    let stdout = run_generated_crate("super_owner_siblings", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}
