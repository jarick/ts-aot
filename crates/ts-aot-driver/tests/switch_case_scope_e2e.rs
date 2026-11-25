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

fn codes(out: &ts_aot_driver::DriverOutput) -> Vec<&str> {
    out.diagnostics.iter().map(|d| d.code.as_str()).collect()
}

fn run(source: &str, name: &str, main: &str) -> String {
    let rust = rust_of(source);
    run_generated_crate(name, &rust, main)
}

#[test]
fn a_const_declared_in_one_case_is_visible_in_another() {
    const SOURCE: &str = "class Holder { C: number = 1; } \
         export function got(): number { \
             switch (1) { case 1: const c = new Holder(); c.C = 5; default: return c.C; } \
         }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(case_scope_const::got(&ctx), 5.0);
    println!("ok");
}
"#;
    let stdout = run(SOURCE, "case_scope_const", MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_const_declared_in_one_case_is_visible_in_a_later_case() {
    const SOURCE: &str = "class Holder { C: number = 1; } \
         export function got(): number { \
             switch (1) { case 1: const c = new Holder(); c.C = 5; case 2: return c.C; } \
         }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(case_scope_const_later::got(&ctx), 5.0);
    println!("ok");
}
"#;
    let stdout = run(SOURCE, "case_scope_const_later", MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_let_declared_in_a_case_is_visible_in_the_default() {
    const SOURCE: &str = "class Holder { C: number = 1; } \
         export function got(): number { \
             switch (1) { case 1: let c = new Holder(); c.C = 5; default: return c.C; } \
         }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(case_scope_let::got(&ctx), 5.0);
    println!("ok");
}
"#;
    let stdout = run(SOURCE, "case_scope_let", MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_const_declared_in_the_default_is_not_visible_in_an_earlier_case() {
    const SOURCE: &str = "class Holder { C: number = 1; } \
         export function got(): number { \
             switch (1) { case 1: return c.C; default: const c = new Holder(); c.C = 5; } \
         }";
    let out = compile_module(SOURCE);
    assert!(
        out.has_errors(),
        "reading `c` in a case that runs before its declaration is a temporal-dead-zone error at \
         run time, so the program must be refused; got {:?}",
        out.diagnostics
    );
}

#[test]
fn a_nested_switch_keeps_its_own_case_declarations() {
    const SOURCE: &str = "class Holder { C: number = 1; } \
         export function got(): number { \
             switch (1) { \
                 case 1: { \
                     const outer = new Holder(); outer.C = 7; \
                     switch (1) { \
                         case 1: const inner = new Holder(); inner.C = 3; \
                         case 2: return inner.C + outer.C; \
                     } \
                 } \
             } \
             return 0; \
         }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(nested_case_scope::got(&ctx), 10.0);
    println!("ok");
}
"#;
    let stdout = run(SOURCE, "nested_case_scope", MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_const_inside_a_case_block_stays_inside_its_block() {
    const SOURCE: &str = "class Holder { C: number = 1; } \
         export function got(): number { \
             switch (1) { \
                 case 1: { const c = new Holder(); c.C = 5; return c.C; } \
                 default: return 0; \
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
    assert_eq!(case_block_scope::got(&ctx), 5.0);
    println!("ok");
}
"#;
    let stdout = run(SOURCE, "case_block_scope", MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_switch_case_declaration_used_by_another_case_matches_node() {
    const SOURCE: &str = "class Holder { C = class { static k: number = 1; }; } \
         export function got(): number { \
             let total = 0; let i = 0; \
             const a = new Holder(); const b = new Holder(); let r = a; \
             while (i < 1) { \
                 switch (1) { \
                     case 1:  const c84 = new Holder(); c84.C ||= 0; break; \
                     case 0:  const c16 = new Holder(); c84.C ||= 7; break; \
                     default: total = total * 10 + c16.C.k; break; \
                 } \
                 const c16 = new Holder(); \
                 r = a; i = i + 1; \
             } \
             b.C ||= class { static k: number = 101; }; \
             return total; \
         }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(switch_case_declaration_fuzz::got(&ctx), 0.0);
    println!("ok");
}
"#;
    let stdout = run(SOURCE, "switch_case_declaration_fuzz", MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_case_declaration_does_not_shadow_the_switch_discriminant() {
    const SOURCE: &str = "export function got(): number { \
         let c = 1; \
         switch (c) { case 1: const c = 5; return c; case 2: return c; } \
         return 0; \
     }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(case_shadow_const::got(&ctx), 5.0);
    println!("ok");
}
"#;
    let stdout = run(SOURCE, "case_shadow_const", MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_hoisted_let_does_not_shadow_the_switch_discriminant() {
    const SOURCE: &str = "export function got(): number { \
         let c = 1; \
         switch (c) { case 1: let c = 5; return c; case 2: return c; } \
         return 0; \
     }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(case_shadow_let::got(&ctx), 5.0);
    println!("ok");
}
"#;
    let stdout = run(SOURCE, "case_shadow_let", MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_case_declaration_skipped_by_a_constant_discriminant_is_refused() {
    const SOURCE: &str = "class Holder { C = class { static k: number = 5; }; } \
         export function got(): number { \
             switch (1) { \
                 case 2: const c = new Holder(); break; \
                 case 1: return c.C.k; \
             } \
             return 0; \
         }";
    let out = compile_module(SOURCE);
    assert!(
        codes(&out).contains(&"E0419"),
        "the `const c` declaration lives in the skipped `case 2`, so reading `c` in `case 1` is a \
         temporal-dead-zone error at run time; got {:?}",
        out.diagnostics
    );
}

#[test]
fn a_case_declaration_skipped_by_a_variable_discriminant_is_refused() {
    const SOURCE: &str = "class Holder { C = class { static k: number = 5; }; } \
         export function got(): number { \
             let k = 2; \
             switch (k) { \
                 case 1: const c = new Holder(); c.C = 5; break; \
                 case 2: return c.C; \
             } \
             return 0; \
         }";
    let out = compile_module(SOURCE);
    assert!(
        codes(&out).contains(&"E0419"),
        "the discriminant `2` enters `case 2` without running the `const c` declaration in \
         `case 1`, which is a temporal-dead-zone error at run time; got {:?}",
        out.diagnostics
    );
}

#[test]
fn a_write_to_a_case_declaration_on_a_skipping_path_is_refused() {
    const SOURCE: &str = "export function got(): number { \
         let k = 2; \
         switch (k) { \
             case 1: let x = 5; break; \
             case 2: x = 7; return x; \
         } \
         return 0; \
     }";
    let out = compile_module(SOURCE);
    assert!(
        codes(&out).contains(&"E0419"),
        "writing `x` in `case 2` before its `let x` in `case 1` has run is a temporal-dead-zone \
         error at run time; got {:?}",
        out.diagnostics
    );
}

#[test]
fn a_var_declared_in_a_skipped_case_is_hoisted_with_the_target_zero_value() {
    const SOURCE: &str = "export function assigned(): number { \
         let k = 1; \
         switch (k) { case 1: var v = 5; return v; case 2: return v; } \
         return 0; \
     } \
     export function skipped(): number { \
         let k = 2; \
         switch (k) { case 1: var v = 5; break; case 2: return v; } \
         return 0; \
     }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(var_case_scope::assigned(&ctx), 5.0);
    assert_eq!(
        var_case_scope::skipped(&ctx),
        0.0,
        "AOT extension: the `var v` is hoisted out of the skipped `case 1` and zero-initialized, \
         so reading it in `case 2` yields the numeric zero value. `node --experimental-strip-types` \
         yields `undefined` for this program, so this assertion records a deliberate divergence of \
         the target (a hoisted numeric `var` has no `undefined` representation) rather than a \
         JavaScript-conformant result"
    );
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let norm: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        norm.contains("letmutv:i64=0;"),
        "AOT extension: a `var` hoisted out of a skipped `switch` case is zero-initialized by the \
         target, while JavaScript reads `undefined` there; the zero initializer is pinned here so \
         the divergence cannot change silently. got:\n{rust}"
    );
    let stdout = run_generated_crate("var_case_scope", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_dead_if_does_not_drop_a_switch_discriminant_constant() {
    const SOURCE: &str = "export function got(): number { \
         let k = 2; let flag = false; \
         if (flag) { k = 1; } \
         switch (k) { case 2: const y = 5; return y; case 1: return y; } \
         return 0; \
     }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(dead_if_switch::got(&ctx), 5.0);
    println!("ok");
}
"#;
    let stdout = run(SOURCE, "dead_if_switch", MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_dead_if_does_not_hide_a_switch_temporal_dead_zone_error() {
    const SOURCE: &str = "export function c(): number { \
         let k = 2; let flag = false; \
         if (flag) { k = 1; } \
         switch (k) { case 1: const x = 5; return x; case 2: return x; } \
         return 0; \
     }";
    let out = compile_module(SOURCE);
    assert!(
        codes(&out).contains(&"E0419"),
        "`k` stays `2`, so `case 2` reads the `const x` from `case 1` before its declaration, a \
         temporal-dead-zone error at run time; got {:?}",
        out.diagnostics
    );
}

#[test]
fn a_case_declaration_that_shadows_a_parameter_is_refused() {
    const SOURCE: &str = "function inner(v: number): number { \
         let k = 0; let r = 0; \
         switch (k) { \
             case 0: r = v; break; \
             case 1: const v = 7; r = v; break; \
             default: r = v; break; \
         } \
         return r; \
     } \
     export function e(): number { return inner(100); }";
    let out = compile_module(SOURCE);
    assert!(
        codes(&out).contains(&"E0419"),
        "the `const v` in `case 1` shadows the parameter `v` for the whole switch block, so the \
         `r = v` in `case 0` is a temporal-dead-zone error at run time; got {:?}",
        out.diagnostics
    );
}

#[test]
fn a_comparison_branch_does_not_keep_a_stale_switch_discriminant_constant() {
    const SOURCE: &str = "export function got(): number { \
         let k = 1; const c = 0; \
         if (c > 0) { k = 3; } else { k = 2; } \
         let r = 0; \
         switch (k) { case 1: const a = 10; r = a; break; case 2: r = a; break; } \
         return r; \
     }";
    let out = compile_module(SOURCE);
    assert!(
        codes(&out).contains(&"E0419"),
        "the two branches leave `k` at `3` or `2`, so `case 2` may read the `const a` from `case 1` \
         before its declaration, a temporal-dead-zone error at run time; got {:?}",
        out.diagnostics
    );
}

#[test]
fn a_comparison_branch_that_agrees_keeps_the_switch_discriminant_constant() {
    const SOURCE: &str = "export function got(): number { \
         let v = 1; let k = 1; const c = 0; \
         if (c > 0) { k = 2; } else { k = 2; } \
         let r = 0; \
         switch (k) { case 1: r = v; break; case 2: const v = 5; r = v; break; } \
         return r; \
     }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(if_else_agree::got(&ctx), 5.0);
    println!("ok");
}
"#;
    let stdout = run(SOURCE, "if_else_agree", MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_string_var_read_in_another_case_is_refused() {
    const SOURCE: &str = "export function got(): number { \
         let r = 0; const k = 2; \
         switch (k) { case 1: var s: string = \"abc\"; break; case 2: r = s.length; break; } \
         return r; \
     }";
    let out = compile_module(SOURCE);
    assert!(
        codes(&out).contains(&"E0419"),
        "the `var s: string` declaration lives in the skipped `case 1`, so `s` is `undefined` in \
         `case 2` and `s.length` throws at run time; a string has no default value, so the program \
         must be refused; got {:?}",
        out.diagnostics
    );
}
