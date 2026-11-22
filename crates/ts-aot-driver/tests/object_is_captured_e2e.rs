use ts_aot_driver::{CompileOptions, Driver, EmitStage};
use ts_aot_test_support::{check_generated_crate, run_generated_crate};

const SOURCE: &str = "class Point { x: number = 1; } \
     export function capturedOptional(): number { \
         const p: Point | null = new Point(); \
         const q: Point | null = p; \
         const f = (): boolean => Object.is(p, q); \
         return f() ? 1 : 0; \
     } \
     export function capturedPlain(): number { \
         const p: Point = new Point(); \
         const q: Point = p; \
         const f = (): boolean => Object.is(p, q); \
         return f() ? 1 : 0; \
     } \
     export function capturedDistinct(): number { \
         const p: Point = new Point(); \
         const q: Point = new Point(); \
         const f = (): boolean => Object.is(p, q); \
         return f() ? 1 : 0; \
     } \
     export function optionalVersusPlain(q: Point): number { \
         const p: Point | null = q; \
         return Object.is(p, q) ? 1 : 0; \
     } \
     export function plainVersusOptional(q: Point): number { \
         const p: Point | null = q; \
         return Object.is(q, p) ? 1 : 0; \
     } \
     export function optionalVersusPlainDistinct(q: Point): number { \
         const p: Point | null = new Point(); \
         return Object.is(p, q) ? 1 : 0; \
     } \
     export function absentVersusPlain(q: Point): number { \
         const p: Point | null = null; \
         return Object.is(p, q) ? 1 : 0; \
     } \
     export function nullHeld(p: Point | null): number { \
         return Object.is(p, null) ? 1 : 0; \
     } \
     export function nullVersusUndefined(p: Point | null): number { \
         return Object.is(p, undefined) ? 1 : 0; \
     } \
     export function makePoint(): Point { return new Point(); }";

const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(object_is_captured::capturedOptional(&ctx), 1.0);
    assert_eq!(object_is_captured::capturedPlain(&ctx), 1.0);
    assert_eq!(object_is_captured::capturedDistinct(&ctx), 0.0);
    assert_eq!(
        object_is_captured::optionalVersusPlain(&ctx, object_is_captured::makePoint(&ctx)),
        1.0
    );
    assert_eq!(
        object_is_captured::plainVersusOptional(&ctx, object_is_captured::makePoint(&ctx)),
        1.0
    );
    assert_eq!(
        object_is_captured::optionalVersusPlainDistinct(&ctx, object_is_captured::makePoint(&ctx)),
        0.0
    );
    assert_eq!(
        object_is_captured::absentVersusPlain(&ctx, object_is_captured::makePoint(&ctx)),
        0.0
    );
    assert_eq!(object_is_captured::nullHeld(&ctx, None), 1.0);
    assert_eq!(
        object_is_captured::nullHeld(&ctx, Some(object_is_captured::makePoint(&ctx))),
        0.0
    );
    assert_eq!(object_is_captured::nullVersusUndefined(&ctx, None), 0.0);
    assert_eq!(
        object_is_captured::nullVersusUndefined(&ctx, Some(object_is_captured::makePoint(&ctx))),
        0.0
    );
    println!("ok");
}
"#;

#[test]
fn object_is_resolves_captured_and_mixed_optional_struct_operands() {
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
        "Object.is on captured struct locals must compile through the full pipeline; got {:?}",
        out.diagnostics
    );
    let rust = out
        .rust_source
        .expect("emit-rust must populate rust_source");
    let norm: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
    let mixed = norm
        .split("pubfnoptionalVersusPlain<'a>(")
        .nth(1)
        .and_then(|rest| rest.split("pubfn").next())
        .unwrap_or_else(|| {
            panic!("the emitted module must define `optionalVersusPlain`; got:\n{rust}")
        });
    assert!(
        mixed.contains("::std::rc::Rc::as_ptr"),
        "`optionalVersusPlain` compares an `Optional<Point>` with a plain `Point` holding the \
         same object, so it must compare their shared field cell; got:\n{mixed}"
    );
    let closures: Vec<&str> = norm.split("fn__ts_closure_").skip(1).collect();
    assert_eq!(
        closures.len(),
        3,
        "the three capturing closures must be emitted; got:\n{rust}"
    );
    for body in closures {
        assert!(
            body.contains("::std::rc::Rc::as_ptr"),
            "`Object.is` inside a closure body sees captured struct locals, so it must compare \
             their shared field cell instead of the compile-time place answer; got:\n{body}"
        );
    }
    check_generated_crate("object_is_captured", &rust);
    let stdout = run_generated_crate("object_is_captured", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}
