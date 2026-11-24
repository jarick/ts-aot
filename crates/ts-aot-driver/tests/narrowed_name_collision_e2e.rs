use ts_aot_driver::{CompileOptions, Driver, EmitStage};
use ts_aot_test_support::run_generated_crate;

const B1_SOURCE: &str = "const _u5f_narrowed_u5f_0: number = 100; \
     export function b1(): number { \
         let _narrowed_0: number = 7; \
         const read = (): number => _u5f_narrowed_u5f_0 + _narrowed_0; \
         return read(); \
     }";

const B1_MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(
        narrowed_name_collision::b1(&ctx),
        107.0,
        "the module binding `_u5f_narrowed_u5f_0` (100) and the captured user local \
         `_narrowed_0` (7) are distinct bindings; node prints 107"
    );
    println!("ok");
}
"#;

const B2_SOURCE: &str = "export function b2(): number { \
         let _narrowed_0: number = 7; \
         const outer = (): number => { \
             const inner = (): number => _narrowed_0 + 1; \
             return inner(); \
         }; \
         return outer(); \
     }";

const B2_SHADOWED_SOURCE: &str = "const _narrowed_0: number = 100; \
     export function b2(): number { \
         let _narrowed_0: number = 7; \
         const outer = (): number => { \
             const inner = (): number => _narrowed_0 + 1; \
             return inner(); \
         }; \
         return outer(); \
     }";

const B2_MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(
        narrowed_name_collision::b2(&ctx),
        8.0,
        "the nested closure must read the enclosing user local `_narrowed_0` (7); node prints 8"
    );
    println!("ok");
}
"#;

fn emit(source: &str) -> String {
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
        "the repro must compile through the full pipeline; got {:?}",
        out.diagnostics
    );
    out.rust_source
        .expect("emit-rust must populate rust_source")
}

#[test]
fn a_user_local_is_never_mangled_into_a_lookalike_of_a_module_binding() {
    let rust = emit(B1_SOURCE);
    let norm: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        norm.contains("_narrowed_0"),
        "the user local `_narrowed_0` must reach the emitted Rust under its own name; got:\n{rust}"
    );
    let stdout = run_generated_crate("narrowed_name_collision", &rust, B1_MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_nested_closure_captures_a_user_local_named_narrowed() {
    let rust = emit(B2_SOURCE);
    let stdout = run_generated_crate("narrowed_name_collision", &rust, B2_MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_nested_closure_capture_wins_over_a_same_named_module_binding() {
    let rust = emit(B2_SHADOWED_SOURCE);
    let stdout = run_generated_crate("narrowed_name_collision", &rust, B2_MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}
