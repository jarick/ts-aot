use ts_aot_driver::{CompileOptions, Driver, EmitStage};
use ts_aot_test_support::run_generated_crate;

fn rust_of(source: &str) -> String {
    let out = Driver::new().compile_source(
        "test.ts",
        source,
        &CompileOptions {
            emit: EmitStage::Rust,
            module: true,
        },
    );
    assert!(
        !out.has_errors(),
        "must compile through the full pipeline; got {:?}",
        out.diagnostics
    );
    out.rust_source
        .expect("emit-rust must populate rust_source")
}

#[test]
fn object_is_sees_two_bindings_of_one_class_instance_as_the_same_object() {
    const SOURCE: &str = "class Point { x: number = 1; } \
         export function aliased(): number { \
             const p = new Point(); \
             const q = p; \
             return Object.is(p, q) ? 1 : 0; \
         } \
         export function unrelated(): number { \
             const p = new Point(); \
             const q = new Point(); \
             return Object.is(p, q) ? 1 : 0; \
         }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(object_is_identity::aliased(&ctx), 1.0);
    assert_eq!(object_is_identity::unrelated(&ctx), 0.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let norm: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
    let aliased = norm
        .split("pubfnaliased<'a>(")
        .nth(1)
        .and_then(|rest| rest.split("pubfn").next())
        .unwrap_or_else(|| panic!("the emitted module must define `aliased`; got:\n{rust}"));
    let identity_expansion = "{let__ts_aot_a=&p;let__ts_aot_b=&q;\
                              ::std::rc::Rc::as_ptr(&(__ts_aot_a).x)==\
                              ::std::rc::Rc::as_ptr(&(__ts_aot_b).x)}";
    assert!(
        aliased.contains(identity_expansion),
        "the `Object.is(p, q)` expansion of `aliased` must compare the shared field cells of the \
         two bindings `p` and `q`; got:\n{aliased}"
    );
    let stdout = run_generated_crate("object_is_identity", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}
