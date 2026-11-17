use ts_aot_driver::{CompileOptions, Driver, EmitStage};
use ts_aot_test_support::run_generated_crate;

fn compile_module(source: &str) -> ts_aot_driver::DriverOutput {
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
    let out = compile_module(source);
    assert!(
        !out.has_errors(),
        "must compile through the full pipeline; got {:?}",
        out.diagnostics
    );
    out.rust_source
        .expect("emit-rust must populate rust_source")
}

fn run(source: &str, name: &str, main: &str) -> String {
    let rust = rust_of(source);
    run_generated_crate(name, &rust, main)
}

#[test]
fn an_f64_discriminant_switch_compares_the_cases_by_value() {
    const SOURCE: &str = "export function got(n: number): number { \
         switch (n) { \
             case 1: return 10; \
             case 2: return 20; \
             default: return 30; \
         } \
     }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(f64_switch::got(&ctx, 1.0), 10.0);
    assert_eq!(f64_switch::got(&ctx, 2.0), 20.0);
    assert_eq!(f64_switch::got(&ctx, 3.0), 30.0);
    assert_eq!(f64_switch::got(&ctx, 0.0), 30.0);
    println!("ok");
}
"#;
    let stdout = run(SOURCE, "f64_switch", MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn an_f64_discriminant_switch_keeps_fall_through() {
    const SOURCE: &str = "function f(n: number): number { \
         let s = 0; \
         switch (n) { \
             case 1: s = 1; \
             case 2: s = s + 10; break; \
             default: s = 100; \
         } \
         return s; \
     } \
     export function one(): number { return f(1); } \
     export function two(): number { return f(2); } \
     export function three(): number { return f(3); }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(f64_switch_fallthrough::one(&ctx), 11.0);
    assert_eq!(f64_switch_fallthrough::two(&ctx), 10.0);
    assert_eq!(f64_switch_fallthrough::three(&ctx), 100.0);
    println!("ok");
}
"#;
    let stdout = run(SOURCE, "f64_switch_fallthrough", MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn an_f64_discriminant_switch_breaks_the_enclosing_loop() {
    const SOURCE: &str = "function h(n: number): number { \
         let total = 0; \
         let i = 0; \
         while (i < 3) { \
             switch (n) { \
                 case 1: total = total + 1; break; \
                 default: break; \
             } \
             i = i + 1; \
         } \
         return total; \
     } \
     export function hit(): number { return h(1); } \
     export function miss(): number { return h(2); }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(f64_switch_loop::hit(&ctx), 3.0);
    assert_eq!(f64_switch_loop::miss(&ctx), 0.0);
    println!("ok");
}
"#;
    let stdout = run(SOURCE, "f64_switch_loop", MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_nested_f64_discriminant_switch_picks_the_inner_case() {
    const SOURCE: &str = "function k(n: number, m: number): number { \
         switch (n) { \
             case 1: \
                 switch (m) { \
                     case 2: return 12; \
                     default: return 10; \
                 } \
             default: return 20; \
         } \
     } \
     export function inner(): number { return k(1, 2); } \
     export function fallback(): number { return k(1, 3); } \
     export function outer(): number { return k(2, 1); }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(nested_f64_switch::inner(&ctx), 12.0);
    assert_eq!(nested_f64_switch::fallback(&ctx), 10.0);
    assert_eq!(nested_f64_switch::outer(&ctx), 20.0);
    println!("ok");
}
"#;
    let stdout = run(SOURCE, "nested_f64_switch", MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn an_f64_discriminant_switch_hoists_a_local_read_from_another_case() {
    const SOURCE: &str = "function f(n: number): number { \
         switch (n) { \
             case 1: const v = 5; return v; \
             default: return 0; \
         } \
     } \
     export function hit(): number { return f(1); } \
     export function miss(): number { return f(0); }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(f64_switch_hoist::hit(&ctx), 5.0);
    assert_eq!(f64_switch_hoist::miss(&ctx), 0.0);
    println!("ok");
}
"#;
    let stdout = run(SOURCE, "f64_switch_hoist", MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_string_discriminant_switch_compares_the_cases_by_value() {
    const SOURCE: &str = "function s(x: string): number { \
         switch (x) { \
             case \"a\": return 1; \
             case \"b\": return 2; \
             case \"c\": return 3; \
             default: return 0; \
         } \
     } \
     export function sa(): number { return s(\"a\"); } \
     export function sb(): number { return s(\"b\"); } \
     export function sc(): number { return s(\"c\"); } \
     export function sz(): number { return s(\"z\"); }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(string_switch::sa(&ctx), 1.0);
    assert_eq!(string_switch::sb(&ctx), 2.0);
    assert_eq!(string_switch::sc(&ctx), 3.0);
    assert_eq!(string_switch::sz(&ctx), 0.0);
    println!("ok");
}
"#;
    let stdout = run(SOURCE, "string_switch", MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_string_discriminant_switch_breaks_the_enclosing_loop() {
    const SOURCE: &str = "function t(x: string): number { \
         let total = 0; \
         let i = 0; \
         while (i < 2) { \
             switch (x) { \
                 case \"a\": total = total + 5; break; \
                 default: total = total + 1; break; \
             } \
             i = i + 1; \
         } \
         return total; \
     } \
     export function hit(): number { return t(\"a\"); } \
     export function miss(): number { return t(\"z\"); }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(string_switch_loop::hit(&ctx), 10.0);
    assert_eq!(string_switch_loop::miss(&ctx), 2.0);
    println!("ok");
}
"#;
    let stdout = run(SOURCE, "string_switch_loop", MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn an_integer_discriminant_switch_keeps_the_match_lowering() {
    const SOURCE: &str = "export function got(): number { \
         const x = 2; \
         switch (x) { \
             case 1: return 10; \
             case 2: return 20; \
             default: return 30; \
         } \
     }";
    let rust = rust_of(SOURCE);
    assert!(
        rust.contains("match"),
        "an integer discriminant must keep the `match` lowering; got:\n{rust}"
    );
}

#[test]
fn an_f64_discriminant_switch_avoids_the_match_lowering() {
    const SOURCE: &str = "export function got(): number { \
         const x: number = 2; \
         switch (x) { \
             case 1: return 10; \
             case 2: return 20; \
             default: return 30; \
         } \
     }";
    let rust = rust_of(SOURCE);
    assert!(
        !rust.contains("match"),
        "an f64 discriminant cannot be a `match` scrutinee; got:\n{rust}"
    );
}
