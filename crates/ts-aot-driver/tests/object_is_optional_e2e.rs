use ts_aot_driver::{CompileOptions, Driver, EmitStage};
use ts_aot_test_support::run_generated_crate;

const SOURCE: &str = "class Point { x: number = 1; } \
     export function aliased(): number { \
         const p: Point | null = new Point(); \
         const q: Point | null = p; \
         return Object.is(p, q) ? 1 : 0; \
     } \
     export function bothAbsent(): number { \
         const p: Point | null = null; \
         const q: Point | null = null; \
         return Object.is(p, q) ? 1 : 0; \
     } \
     export function oneAbsent(): number { \
         const p: Point | null = new Point(); \
         const q: Point | null = null; \
         return Object.is(p, q) ? 1 : 0; \
     } \
     export function distinct(): number { \
         const p: Point | null = new Point(); \
         const q: Point | null = new Point(); \
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
    assert_eq!(object_is_optional::aliased(&ctx), 1.0);
    assert_eq!(object_is_optional::bothAbsent(&ctx), 1.0);
    assert_eq!(object_is_optional::oneAbsent(&ctx), 0.0);
    assert_eq!(object_is_optional::distinct(&ctx), 0.0);
    println!("ok");
}
"#;

#[test]
fn optional_struct_locals_compare_by_object_identity() {
    let out = Driver::new().compile_source(
        "test.ts",
        SOURCE,
        &CompileOptions {
            emit: EmitStage::Rust,
            module: true,
        },
    );
    assert!(
        !out.has_errors(),
        "optional struct Object.is must compile through the full pipeline; got {:?}",
        out.diagnostics
    );
    let rust = out
        .rust_source
        .expect("emit-rust must populate rust_source");
    let norm: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
    let aliased = norm
        .split("pubfnaliased<'a>(")
        .nth(1)
        .and_then(|rest| rest.split("pubfn").next())
        .unwrap_or_else(|| panic!("the emitted module must define `aliased`; got:\n{rust}"));
    assert!(
        aliased.contains("::std::rc::Rc::as_ptr") && aliased.contains("(None,None)=>true"),
        "two optional locals of a declared struct must compare their presence and then their \
         shared field cell, so an alias answers `true` while one absent operand answers `false`; \
         got:\n{aliased}"
    );
    let stdout = run_generated_crate("object_is_optional", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}
