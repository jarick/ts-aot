use std::fmt::Write as _;

use ts_aot_driver::{CompileOptions, Driver, EmitStage};
use ts_aot_test_support::{check_generated_crate, run_generated_crate};

mod common;

fn compile_rust(source: &str) -> ts_aot_driver::DriverOutput {
    Driver::new().compile_source(
        "test.ts",
        source,
        &CompileOptions {
            emit: EmitStage::Rust,
            module: true,
        },
    )
}

fn compile_script(source: &str) -> ts_aot_driver::DriverOutput {
    Driver::new().compile_source(
        "test.ts",
        source,
        &CompileOptions {
            emit: EmitStage::Rust,
            module: false,
        },
    )
}

fn first_call_arg<'a>(rust: &'a str, call: &str) -> &'a str {
    let start = rust
        .find(call)
        .unwrap_or_else(|| panic!("no `{call}` in generated code:\n{rust}"))
        + call.len();
    let mut depth = 1i32;
    for (i, ch) in rust[start..].char_indices() {
        match ch {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => {
                depth -= 1;
                if depth == 0 {
                    return &rust[start..start + i];
                }
            }
            ',' if depth == 1 => return &rust[start..start + i],
            _ => {}
        }
    }
    panic!("unbalanced `{call}` call in generated code:\n{rust}");
}

fn receiver_temporary(norm: &str) -> Option<String> {
    let prefix = "__ts_receiver";
    let mut from = 0;
    while let Some(at) = norm[from..].find("let") {
        let at = from + at;
        from = at + "let".len();
        if at > 0 {
            let prev = norm.as_bytes()[at - 1];
            if prev.is_ascii_alphanumeric() || prev == b'_' {
                continue;
            }
        }
        let rest = norm[at + "let".len()..].trim_start();
        let rest = rest.strip_prefix("mut").map_or(rest, str::trim_start);
        if rest.strip_prefix(prefix).is_none() {
            continue;
        }
        let stop = rest
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .unwrap_or(rest.len());
        return Some(rest[..stop].to_owned());
    }
    None
}

#[test]
fn the_receiver_temporary_helper_detects_both_binding_spellings() {
    let plain = common::normalize_rust("let\n    __ts_receiver_1 = __ts_get (g) ;");
    assert_eq!(
        receiver_temporary(&plain).as_deref(),
        Some("__ts_receiver_1"),
        "a plain `let` binding must be detected even though normalization keeps a space after \
         `let`; got `{plain}`"
    );
    let mutable = common::normalize_rust("let\n    mut __ts_receiver_1 = __ts_get (g) ;");
    assert_eq!(
        receiver_temporary(&mutable).as_deref(),
        Some("__ts_receiver_1"),
        "a `let mut` binding must be detected; got `{mutable}`"
    );
    let later = common::normalize_rust("let __ts_arg_0 = 1 ; let mut __ts_receiver_2 = 2 ;");
    assert_eq!(
        receiver_temporary(&later).as_deref(),
        Some("__ts_receiver_2"),
        "the scan must skip unrelated bindings instead of stopping at the first `let`; got \
         `{later}`"
    );
    let unrelated = common::normalize_rust("let __ts_arg_0 = 1 ; let counter = 2 ;");
    assert_eq!(
        receiver_temporary(&unrelated),
        None,
        "an unrelated `let` must not be reported as a receiver temporary; got `{unrelated}`"
    );
}

#[test]
fn mutable_global_read_write_is_wrapped_in_unsafe() {
    let out = compile_rust(
        "var counter: i64 = 0; \
         export function bump(): i64 { counter = counter + 1; return counter; }",
    );
    assert!(
        !out.has_errors(),
        "mutable global assignment must compile through the full pipeline; got {:?}",
        out.diagnostics
    );
    let source = out
        .rust_source
        .expect("emit-rust must populate rust_source for the global check");
    assert!(
        source.contains("unsafe"),
        "reads/writes of a mutable `static mut` global must be wrapped in `unsafe`; got:\n{source}"
    );
    assert!(
        source.contains("static mut counter"),
        "a reassigned global must be emitted as a mutable static; got:\n{source}"
    );
}

#[test]
fn a_mutating_method_on_a_mutable_global_borrows_the_receiver_after_its_arguments() {
    let out = compile_rust(
        "class Counter { n: number = 0; add(x: number): void { this.n = this.n + x; } } \
         let g = new Counter(); \
         function bump(): number { g.add(1); return 1; } \
         export function got(): number { g.add(bump()); return g.n; }",
    );
    assert!(
        !out.has_errors(),
        "a mutating method on a mutable global must compile; got {:?}",
        out.diagnostics
    );
    let source = out.rust_source.expect("rust_source");
    let got_at = source.find("pub fn got").expect("got must be emitted");
    let body = &source[got_at..];
    let arg_at = body
        .find("let __ts_arg_0")
        .expect("the argument must be bound to a temporary");
    let call_at = body
        .find("Counter__add (")
        .expect("the callee must be emitted");
    assert!(
        arg_at < call_at,
        "the argument runs before the receiver is borrowed; an argument that reassigns the \
         receiver global is refused with E0419, so the borrow still names the same instance; \
         got:\n{body}"
    );
    let norm: String = body.chars().filter(|c| !c.is_whitespace()).collect();
    let receiver = first_call_arg(&norm, "Counter__add(");
    assert!(
        receiver.starts_with("&(") && receiver.contains("get_or_insert_with"),
        "the callee must run on a shared borrow of the lazily initialized global receiver; got \
         receiver `{receiver}` in:\n{body}"
    );
    assert!(
        !body.contains("& mut ("),
        "the receiver must never be borrowed mutably; got:\n{body}"
    );
    check_generated_crate("global_receiver_capture", &source);
}

#[test]
fn a_mutating_call_on_a_global_whose_argument_reassigns_it_is_refused() {
    let out = compile_rust(
        "class Counter { n: number = 0; add(x: number): void { this.n = this.n + x; } } \
         let g = new Counter(); \
         function swap(): number { g = new Counter(); g.add(100); return 1; } \
         export function got(): number { g.add(swap()); return g.n; }",
    );
    assert!(
        out.diagnostics
            .iter()
            .any(|d| d.code.as_str() == "E0419" && d.message.contains("may reassign")),
        "an argument that reassigns the receiver global must be reported, got {:?}",
        out.diagnostics
    );
}

#[test]
fn a_mutating_call_on_a_global_whose_argument_reassigns_it_indirectly_is_refused() {
    let out = compile_rust(
        "class Counter { n: number = 0; add(x: number): void { this.n = this.n + x; } } \
         let g = new Counter(); \
         function inner(): number { g = new Counter(); return 1; } \
         function outer(): number { return inner(); } \
         export function got(): number { g.add(outer()); return g.n; }",
    );
    assert!(
        out.diagnostics
            .iter()
            .any(|d| d.code.as_str() == "E0419" && d.message.contains("may reassign")),
        "a reassignment reached through a call chain must be reported, got {:?}",
        out.diagnostics
    );
}

#[test]
fn a_mutating_call_on_a_global_whose_argument_only_mutates_it_compiles() {
    let out = compile_rust(
        "class Counter { n: number = 0; add(x: number): void { this.n = this.n + x; } } \
         let g = new Counter(); \
         class Other { poke(): number { return 1; } } \
         const other = new Other(); \
         function bump(): number { g.add(1); return 1; } \
         export function got(): number { g.add(bump() + other.poke()); return g.n; }",
    );
    assert!(
        !out.has_errors(),
        "an argument that only mutates the receiver (or leaves the binding alone) must compile, \
         got {:?}",
        out.diagnostics
    );
    let source = out.rust_source.expect("rust_source");
    check_generated_crate("global_argument_only_mutates", &source);
}

#[test]
fn a_mutating_call_whose_argument_is_a_local_arrow_compiles() {
    let out = compile_rust(
        "class Counter { n: number = 0; add(x: number): void { this.n = this.n + x; } } \
         let g = new Counter(); \
         export function got(): number { const h = (): number => 1; g.add(h()); return g.n; }",
    );
    assert!(
        !out.has_errors(),
        "an argument that calls a local arrow which cannot reassign the receiver must compile, \
         got {:?}",
        out.diagnostics
    );
    let source = out.rust_source.expect("rust_source");
    check_generated_crate("global_local_arrow_argument", &source);
}

#[test]
fn a_mutating_call_whose_argument_is_a_local_arrow_inside_a_named_function_compiles() {
    let out = compile_rust(
        "class Counter { n: number = 0; add(x: number): void { this.n = this.n + x; } } \
         let g = new Counter(); \
         function f(): number { const h = (): number => 1; return h(); } \
         export function got(): number { g.add(f()); return g.n; }",
    );
    assert!(
        !out.has_errors(),
        "a local arrow reached through a named function must be inspected, not treated as \
         opaque; got {:?}",
        out.diagnostics
    );
    let source = out.rust_source.expect("rust_source");
    check_generated_crate("global_named_function_local_arrow", &source);
}

#[test]
fn a_mutating_call_whose_local_arrow_argument_reassigns_the_global_is_refused() {
    let out = compile_rust(
        "class Counter { n: number = 0; add(x: number): void { this.n = this.n + x; } } \
         let g = new Counter(); \
         export function got(): number { \
             const h = (): number => { g = new Counter(); return 1; }; \
             g.add(h()); \
             return g.n; \
         }",
    );
    assert!(
        out.diagnostics
            .iter()
            .any(|d| d.code.as_str() == "E0419" && d.message.contains("may reassign")),
        "a local arrow that reassigns the receiver global must be reported, got {:?}",
        out.diagnostics
    );
}

#[test]
fn a_mutating_method_without_arguments_on_a_mutable_global_compiles() {
    let out = compile_rust(
        "class Counter { n: number = 0; bump(): void { this.n = this.n + 1; } } \
         let g = new Counter(); \
         export function got(): number { g.bump(); return g.n; }",
    );
    assert!(!out.has_errors(), "got {:?}", out.diagnostics);
    let source = out.rust_source.expect("rust_source");
    let norm = common::normalize_rust(&source);
    assert!(
        norm.contains("addr_of_mut ! (g)"),
        "a zero-argument mutating call on a mutable global must still capture the receiver as an \
         address; got:\n{source}"
    );
    check_generated_crate("global_zero_arg_call", &source);
}

#[test]
fn a_mutating_call_whose_argument_is_a_zero_argument_call_on_another_global_compiles() {
    let out = compile_rust(
        "class Counter { n: number = 0; add(x: number): void { this.n = this.n + x; } } \
         class Other { m: number = 0; bump(): number { this.m = this.m + 1; return 1; } } \
         let g = new Counter(); let o = new Other(); \
         export function got(): number { g.add(o.bump()); return g.n; }",
    );
    assert!(!out.has_errors(), "got {:?}", out.diagnostics);
    let source = out.rust_source.expect("rust_source");
    check_generated_crate("global_nested_zero_arg_call", &source);
}

#[test]
fn a_self_recursive_local_arrow_argument_does_not_crash_the_compiler() {
    let out = compile_rust(
        "class Counter { n: number = 0; add(x: number): void { this.n = this.n + x; } } \
         let g = new Counter(); \
         export function got(): number { \
             const a = (): number => a(); \
             g.add(a()); \
             return g.n; \
         }",
    );
    assert!(
        !out.has_errors(),
        "inspecting a self-recursive local arrow must terminate, not overflow the stack; got {:?}",
        out.diagnostics
    );
}

#[test]
fn a_mutually_recursive_local_arrow_argument_does_not_crash_the_compiler() {
    let out = compile_rust(
        "class Counter { n: number = 0; add(x: number): void { this.n = this.n + x; } } \
         let g = new Counter(); \
         export function got(): number { \
             const a = (): number => b(); \
             const b = (): number => a(); \
             g.add(a()); \
             return g.n; \
         }",
    );
    assert!(
        !out.has_errors(),
        "inspecting mutually recursive local arrows must terminate, not overflow the stack; \
         got {:?}",
        out.diagnostics
    );
}

#[test]
fn a_conditional_local_arrow_argument_compiles() {
    const SOURCE: &str = "class Counter { n: number = 0; add(x: number): void { this.n = this.n + x; } } \
         let g = new Counter(); \
         export function got(): number { \
             const c = true; \
             const h = c ? (): number => 1 : (): number => 2; \
             g.add(h()); \
             return g.n; \
         }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(global_conditional_local_arrow::got(&ctx), 1.0);
    println!("ok");
}
"#;
    let out = compile_rust(SOURCE);
    assert!(
        !out.has_errors(),
        "a local binding that may hold any of several closures must be inspected, got {:?}",
        out.diagnostics
    );
    let source = out.rust_source.expect("rust_source");
    check_generated_crate("global_conditional_local_arrow", &source);
    let stdout = run_generated_crate("global_conditional_local_arrow", &source, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_conditional_local_arrow_whose_branch_reassigns_the_global_is_refused() {
    let out = compile_rust(
        "class Counter { n: number = 0; add(x: number): void { this.n = this.n + x; } } \
         let g = new Counter(); \
         export function got(): number { \
             const c = true; \
             const h = c ? (): number => 1 : (): number => { g = new Counter(); return 2; }; \
             g.add(h()); \
             return g.n; \
         }",
    );
    assert!(
        out.diagnostics
            .iter()
            .any(|d| d.code.as_str() == "E0419" && d.message.contains("may reassign")),
        "any branch of a conditional local arrow that reassigns the receiver global must be \
         reported, got {:?}",
        out.diagnostics
    );
}

#[test]
fn a_closure_reassigned_inside_a_block_still_reassigns_the_global() {
    let out = compile_rust(
        "class Counter { n: number = 0; add(x: number): void { this.n = this.n + x; } } \
         let g = new Counter(); \
         export function got(): number { \
             let h = (): number => 1; \
             if (true) { h = (): number => { g = new Counter(); return 2; }; } \
             g.add(h()); \
             return g.n; \
         }",
    );
    assert!(
        out.diagnostics
            .iter()
            .any(|d| d.code.as_str() == "E0419" && d.message.contains("may reassign")),
        "a closure assigned to an outer binding inside a nested block must stay visible after the \
         block ends, got {:?}",
        out.diagnostics
    );
}

#[test]
fn a_closure_reassigned_in_a_branch_keeps_the_body_it_replaced() {
    let out = compile_rust(
        "class Counter { n: number = 0; add(x: number): void { this.n = this.n + x; } } \
         let g = new Counter(); \
         export function got(c: boolean): number { \
             let h = (): number => { g = new Counter(); return 2; }; \
             if (c) { h = (): number => 1; } \
             g.add(h()); \
             return g.n; \
         }",
    );
    assert!(
        out.diagnostics
            .iter()
            .any(|d| d.code.as_str() == "E0419" && d.message.contains("may reassign")),
        "a binding that may still hold the closure the branch replaced must be reported, got {:?}",
        out.diagnostics
    );
    assert!(
        out.rust_source.is_none(),
        "a refused program must not be emitted as a crate"
    );
}

#[test]
fn a_conditional_reassignment_to_a_safe_closure_still_compiles_and_runs() {
    const SOURCE: &str = "class Counter { n: number = 0; add(x: number): void { this.n = this.n + x; } } \
         let g = new Counter(); \
         export function got(c: boolean): number { \
             let h = (): number => 1; \
             if (c) { h = (): number => 1; } \
             g.add(h()); \
             return g.n; \
         }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(global_conditional_safe_closure::got(&ctx, false), 1.0);
    assert_eq!(global_conditional_safe_closure::got(&ctx, true), 2.0);
    println!("ok");
}
"#;
    let out = compile_rust(SOURCE);
    assert!(
        !out.has_errors(),
        "a branch that reassigns a local to another safe closure must keep compiling, got {:?}",
        out.diagnostics
    );
    let source = out.rust_source.expect("rust_source");
    check_generated_crate("global_conditional_safe_closure", &source);
    let stdout = run_generated_crate("global_conditional_safe_closure", &source, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_mutating_call_whose_callback_argument_is_a_safe_literal_compiles_and_runs() {
    const SOURCE: &str = "class C { n: number = 0; add(x: number, cb: () => number): void { this.n = this.n + x; cb(); } } \
         let g = new C(); \
         export function got(): number { g.add(1, (): number => 1); return g.n; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(global_callback_literal::got(&ctx), 1.0);
    println!("ok");
}
"#;
    let out = compile_rust(SOURCE);
    assert!(
        !out.has_errors(),
        "calling a callback parameter that the call site binds to a safe closure must compile, \
         got {:?}",
        out.diagnostics
    );
    let source = out.rust_source.expect("rust_source");
    check_generated_crate("global_callback_literal", &source);
    let stdout = run_generated_crate("global_callback_literal", &source, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_mutating_call_whose_method_passes_a_safe_callback_on_compiles_and_runs() {
    const SOURCE: &str = "function apply(cb: () => number): number { return cb(); } \
         class C { n: number = 0; add(x: number): void { this.n = this.n + x; apply((): number => 1); } } \
         let g = new C(); \
         export function got(): number { g.add(1); return g.n; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(global_callback_forwarded::got(&ctx), 1.0);
    println!("ok");
}
"#;
    let out = compile_rust(SOURCE);
    assert!(
        !out.has_errors(),
        "a callback forwarded through the method body must be resolved from the call site, \
         got {:?}",
        out.diagnostics
    );
    let source = out.rust_source.expect("rust_source");
    check_generated_crate("global_callback_forwarded", &source);
    let stdout = run_generated_crate("global_callback_forwarded", &source, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_mutating_call_whose_callback_literal_reassigns_the_global_is_refused() {
    let out = compile_rust(
        "class C { n: number = 0; add(x: number, cb: () => number): void { this.n = this.n + x; cb(); } } \
         let g = new C(); \
         export function got(): number { g.add(1, (): number => { g = new C(); return 1; }); return g.n; }",
    );
    assert!(
        out.diagnostics
            .iter()
            .any(|d| d.code.as_str() == "E0419" && d.message.contains("may reassign")),
        "a callback literal bound to a called parameter must be inspected, got {:?}",
        out.diagnostics
    );
}

#[test]
fn a_mutating_call_whose_callback_local_reassigns_the_global_is_refused() {
    let out = compile_rust(
        "class C { n: number = 0; add(x: number, cb: () => number): void { this.n = this.n + x; cb(); } } \
         let g = new C(); \
         export function got(): number { \
             const h = (): number => { g = new C(); return 1; }; \
             g.add(1, h); \
             return g.n; \
         }",
    );
    assert!(
        out.diagnostics
            .iter()
            .any(|d| d.code.as_str() == "E0419" && d.message.contains("may reassign")),
        "a local arrow passed as a callback must be inspected, got {:?}",
        out.diagnostics
    );
}

#[test]
fn a_mutating_call_whose_callback_function_reference_reassigns_the_global_is_refused() {
    let out = compile_rust(
        "function bad(): number { g = new C(); return 1; } \
         class C { n: number = 0; add(x: number, cb: () => number): void { this.n = this.n + x; cb(); } } \
         let g = new C(); \
         export function got(): number { g.add(1, bad); return g.n; }",
    );
    assert!(
        out.diagnostics
            .iter()
            .any(|d| d.code.as_str() == "E0419" && d.message.contains("may reassign")),
        "a function reference passed as a callback must be inspected, got {:?}",
        out.diagnostics
    );
}

#[test]
fn a_mutating_method_that_reassigns_the_global_is_refused() {
    let out = compile_rust(
        "class C { n: number = 0; add(x: number): void { g = new C(); this.n = this.n + x; } } \
         let g = new C(); \
         export function got(): number { g.add(1); return g.n; }",
    );
    assert!(
        out.diagnostics
            .iter()
            .any(|d| d.code.as_str() == "E0419" && d.message.contains("may reassign")),
        "a method body that writes the receiver global itself must be reported, got {:?}",
        out.diagnostics
    );
}

#[test]
fn a_long_chain_of_local_arrows_is_expanded_once_per_binding() {
    let mut source = String::from(
        "class Counter { n: number = 0; add(x: number): void { this.n = this.n + x; } } \
         let g = new Counter(); \
         export function got(): number { \
             const a0 = (): number => 1; ",
    );
    for index in 1..200 {
        let _ = write!(source, "const a{index} = (): number => a{}(); ", index - 1);
    }
    source.push_str("g.add(a199()); return g.n; }");
    let out = compile_rust(&source);
    assert!(
        !out.has_errors(),
        "a deep chain of local arrows must be inspected once per binding instead of being \
         expanded exponentially, got {:?}",
        out.diagnostics
    );
}

#[test]
fn hoisted_call_temporaries_do_not_shadow_parameters() {
    const SOURCE: &str = "class Counter { n: number = 0; add(x: number): void { this.n = this.n + x; } } \
         let g = new Counter(); \
         export function got(__ts_receiver: number, __ts_arg_0: number): number { \
             g.add(__ts_receiver + __ts_arg_0); \
             return g.n; \
         }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(hoisted_temporaries::got(&ctx, 2.0, 3.0), 5.0);
    println!("ok");
}
"#;
    let out = compile_rust(SOURCE);
    assert!(!out.has_errors(), "got {:?}", out.diagnostics);
    let source = out.rust_source.expect("rust_source");
    assert!(
        common::normalize_rust(&source).contains("__ts_arg_0_"),
        "the hoisted argument temporary must avoid a parameter with the same name; got:\n{source}"
    );
    check_generated_crate("hoisted_temporaries", &source);
    let stdout = run_generated_crate("hoisted_temporaries", &source, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn hoisted_call_temporaries_do_not_shadow_module_globals() {
    const SOURCE: &str = "class Counter { n: number = 0; add(x: number): void { this.n = this.n + x; } } \
         class Pair { m: number = 0; add2(a: number, b: number): void { this.m = this.m + a + b; } } \
         let g = new Counter(); \
         let p = new Pair(); \
         let __ts_receiver: number = 7; \
         let __ts_arg_0: number = 100; \
         export function first(): number { g.add(__ts_receiver); return g.n; } \
         export function second(): number { p.add2(1, __ts_arg_0); return p.m; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(hoisted_vs_globals::first(&ctx), 7.0);
    assert_eq!(hoisted_vs_globals::second(&ctx), 101.0);
    println!("ok");
}
"#;
    let out = compile_rust(SOURCE);
    assert!(!out.has_errors(), "got {:?}", out.diagnostics);
    let source = out.rust_source.expect("rust_source");
    let norm = common::normalize_rust(&source);
    assert!(
        norm.contains("__ts_arg_0_"),
        "a hoisted argument temporary must avoid a module global with the same name; got:\n{source}"
    );
    let hoisted = receiver_temporary(&norm);
    assert!(
        hoisted.is_none(),
        "a mutating call no longer hoists the receiver into a local temporary, so no local binding \
         may reuse or extend the module global's name `__ts_receiver`; got `{}` in:\n{source}",
        hoisted.unwrap_or_default()
    );
    assert!(
        !norm.contains("(*__ts_receiver)"),
        "a mutating call no longer hoists the receiver into a temporary; got:\n{source}"
    );
    check_generated_crate("hoisted_vs_globals", &source);
    let stdout = run_generated_crate("hoisted_vs_globals", &source, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

fn assert_receiver_reassignment_refused(out: &ts_aot_driver::DriverOutput, what: &str) {
    assert!(out.has_errors(), "{what}, got {:?}", out.diagnostics);
    assert!(
        out.diagnostics
            .iter()
            .any(|d| d.code.as_str() == "E0419" && d.message.contains("may reassign")),
        "{what}, got {:?}",
        out.diagnostics
    );
    assert!(
        out.rust_source.is_none(),
        "a refused program must not be emitted as a crate"
    );
}

#[test]
fn a_static_method_argument_that_reassigns_the_receiver_is_refused() {
    let out = compile_rust(
        "class Counter { n: number = 0; add(x: number): void { this.n = this.n + x; } } \
         let g = new Counter(); \
         class D { static bump(): number { g = new Counter(); return 2; } } \
         export function got(): number { g.add(D.bump()); return g.n; }",
    );
    assert_receiver_reassignment_refused(
        &out,
        "a static method called in the argument of a mutating call must be inspected",
    );
}

#[test]
fn a_static_method_argument_with_parameters_that_reassigns_the_receiver_is_refused() {
    let out = compile_rust(
        "class Counter { n: number = 0; add(x: number): void { this.n = this.n + x; } } \
         let g = new Counter(); \
         class D { static bump(x: number): number { g = new Counter(); return 2 + x; } } \
         export function got(): number { g.add(D.bump(1)); return g.n; }",
    );
    assert_receiver_reassignment_refused(
        &out,
        "a static method with parameters must still be inspected",
    );
}

#[test]
fn a_static_method_reached_through_another_static_method_is_refused() {
    let out = compile_rust(
        "class Counter { n: number = 0; add(x: number): void { this.n = this.n + x; } } \
         let g = new Counter(); \
         class D { \
             static inner(): number { g = new Counter(); return 2; } \
             static outer(): number { return D.inner(); } \
         } \
         export function got(): number { g.add(D.outer()); return g.n; }",
    );
    assert_receiver_reassignment_refused(
        &out,
        "a reassignment reached through a chain of static methods must be reported",
    );
}

#[test]
fn a_static_method_of_a_generic_class_that_reassigns_the_receiver_is_refused() {
    let out = compile_rust(
        "class Counter { n: number = 0; add(x: number): void { this.n = this.n + x; } } \
         let g = new Counter(); \
         class Box<T> { static make(x: number): number { g = new Counter(); return x; } } \
         export function got(): number { g.add(Box.make(1)); return g.n; }",
    );
    assert_receiver_reassignment_refused(
        &out,
        "a static method of a generic class must be inspected",
    );
}

#[test]
fn a_static_method_reference_that_reassigns_the_receiver_is_refused() {
    let out = compile_rust(
        "class Counter { n: number = 0; add(cb: () => number): void { this.n = this.n + cb(); } } \
         let g = new Counter(); \
         class D { static bump(): number { g = new Counter(); return 2; } } \
         export function got(): number { g.add(D.bump); return g.n; }",
    );
    assert_receiver_reassignment_refused(
        &out,
        "a static method passed as a callback must be inspected",
    );
}

#[test]
fn a_static_field_receiver_whose_argument_reassigns_it_is_refused() {
    let out = compile_rust(
        "class Counter { n: number = 0; add(x: number): void { this.n = this.n + x; } } \
         class S { \
             static c: Counter = new Counter(); \
             static bump(): number { S.c = new Counter(); return 2; } \
         } \
         export function got(): number { S.c.add(S.bump()); return S.c.n; }",
    );
    assert_receiver_reassignment_refused(
        &out,
        "a static field used as the receiver must be treated as a mutable place",
    );
}

#[test]
fn a_safe_static_method_argument_compiles_and_runs() {
    const SOURCE: &str = "class Counter { n: number = 0; add(x: number): void { this.n = this.n + x; } } \
         let g = new Counter(); \
         class D { static safe(): number { return 2; } } \
         export function got(): number { g.add(D.safe()); return g.n; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(static_method_safe::got(&ctx), 2.0);
    println!("ok");
}
"#;
    let out = compile_rust(SOURCE);
    assert!(
        !out.has_errors(),
        "a static method that leaves the receiver alone must keep compiling, got {:?}",
        out.diagnostics
    );
    let source = out.rust_source.expect("rust_source");
    check_generated_crate("static_method_safe", &source);
    let stdout = run_generated_crate("static_method_safe", &source, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_safe_static_field_receiver_compiles_and_runs() {
    const SOURCE: &str = "class Counter { n: number = 0; add(x: number): void { this.n = this.n + x; } } \
         class S { \
             static c: Counter = new Counter(); \
             static safe(): number { return 2; } \
         } \
         export function got(): number { S.c.add(S.safe()); return S.c.n; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(static_field_receiver_safe::got(&ctx), 2.0);
    println!("ok");
}
"#;
    let out = compile_rust(SOURCE);
    assert!(
        !out.has_errors(),
        "a static field receiver whose argument leaves it alone must keep compiling, got {:?}",
        out.diagnostics
    );
    let source = out.rust_source.expect("rust_source");
    check_generated_crate("static_field_receiver_safe", &source);
    let stdout = run_generated_crate("static_field_receiver_safe", &source, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_static_method_behind_a_shadowing_local_that_reassigns_the_receiver_is_refused() {
    let out = compile_rust(
        "class Counter { n: number = 0; add(x: number): void { this.n = this.n + x; } } \
         let g = new Counter(); \
         class D { static bump(): number { return 1; } } \
         class E { bump(): number { g = new Counter(); return 2; } } \
         export function got(): number { \
             const D = new E(); \
             const outer = (): number => { const inner = (): number => D.bump(); return inner(); }; \
             g.add(outer()); \
             return g.n; \
         }",
    );
    assert_receiver_reassignment_refused(
        &out,
        "a local binding that shadows a class name must supply the owner type of `D.bump()`, so \
         the instance method that reassigns the receiver is inspected",
    );
}

#[test]
fn a_safe_instance_method_behind_a_shadowing_local_compiles() {
    let out = compile_rust(
        "class Counter { n: number = 0; add(x: number): void { this.n = this.n + x; } } \
         let g = new Counter(); \
         class D { static bump(): number { g = new Counter(); return 1; } } \
         class E { bump(): number { return 2; } } \
         export function got(): number { \
             const D = new E(); \
             const outer = (): number => { const inner = (): number => D.bump(); return inner(); }; \
             g.add(outer()); \
             return g.n; \
         }",
    );
    assert!(
        !out.has_errors(),
        "a shadowing local whose own method leaves the receiver alone must not be refused on \
         account of the shadowed class's static method; got {:?}",
        out.diagnostics
    );
    let source = out.rust_source.expect("rust_source");
    check_generated_crate("shadowing_local_safe_instance", &source);
}

#[test]
fn a_shadowing_local_whose_class_static_may_write_the_receiver_compiles_and_runs() {
    const SOURCE: &str = "class Counter { n: number = 0; add(x: number): void { this.n = this.n + x; } } \
         let g = new Counter(); \
         class D { static bump(): number { if (false) { g = new Counter(); } return 2; } } \
         class E { bump(): number { return 2; } } \
         export function got(): number { \
             const D = new E(); \
             const outer = (): number => { const inner = (): number => D.bump(); return inner(); }; \
             g.add(outer()); \
             return g.n; \
         }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(shadowing_local_class_static_may_write::got(&ctx), 2.0);
    println!("ok");
}
"#;
    let out = compile_rust(SOURCE);
    assert!(
        !out.has_errors(),
        "the write to the receiver lives in the shadowed class's static method, not in the \
         instance method the call actually reaches; got {:?}",
        out.diagnostics
    );
    let source = out.rust_source.expect("rust_source");
    check_generated_crate("shadowing_local_class_static_may_write", &source);
    let stdout = run_generated_crate("shadowing_local_class_static_may_write", &source, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_shadowing_local_that_is_not_a_class_name_is_refused() {
    let out = compile_rust(
        "class Counter { n: number = 0; add(x: number): void { this.n = this.n + x; } } \
         let g = new Counter(); \
         class E { bump(): number { g = new Counter(); return 2; } } \
         export function got(): number { \
             const X = new E(); \
             const outer = (): number => { const inner = (): number => X.bump(); return inner(); }; \
             g.add(outer()); \
             return g.n; \
         }",
    );
    assert_receiver_reassignment_refused(
        &out,
        "a local whose name is not a class must resolve to its own class and report the instance \
         method that reassigns the receiver",
    );
}

#[test]
fn a_shadowing_local_whose_class_lacks_the_method_is_refused() {
    let out = compile_rust(
        "class Counter { n: number = 0; add(x: number): void { this.n = this.n + x; } } \
         let g = new Counter(); \
         class D { static bump(): number { return 1; } } \
         class E { other(): number { return 2; } } \
         export function got(): number { \
             const D = new E(); \
             const outer = (): number => { const inner = (): number => D.bump(); return inner(); }; \
             g.add(outer()); \
             return g.n; \
         }",
    );
    assert_receiver_reassignment_refused(
        &out,
        "the shadowed class has no such method, so the call cannot be resolved and must not be \
         treated as safe",
    );
}

#[test]
fn a_static_method_behind_a_shadowing_parameter_that_reassigns_the_receiver_is_refused() {
    let out = compile_rust(
        "class Counter { n: number = 0; add(x: number): void { this.n = this.n + x; } } \
         let g = new Counter(); \
         class D { static bump(): number { return 1; } } \
         class E { bump(): number { g = new Counter(); return 2; } } \
         export function got(D: E): number { \
             const outer = (): number => { const inner = (): number => D.bump(); return inner(); }; \
             g.add(outer()); \
             return g.n; \
         }",
    );
    assert_receiver_reassignment_refused(
        &out,
        "a parameter that shadows a class name must supply the owner type of `D.bump()`",
    );
}

#[test]
fn a_safe_instance_method_behind_a_shadowing_parameter_compiles() {
    let out = compile_rust(
        "class Counter { n: number = 0; add(x: number): void { this.n = this.n + x; } } \
         let g = new Counter(); \
         class D { static bump(): number { g = new Counter(); return 1; } } \
         class E { bump(): number { return 2; } } \
         export function got(D: E): number { \
             const outer = (): number => { const inner = (): number => D.bump(); return inner(); }; \
             g.add(outer()); \
             return g.n; \
         }",
    );
    assert!(
        !out.has_errors(),
        "a shadowing parameter whose own method leaves the receiver alone must not be refused; \
         got {:?}",
        out.diagnostics
    );
    let source = out.rust_source.expect("rust_source");
    check_generated_crate("shadowing_param_safe_instance", &source);
}

#[test]
fn a_static_method_behind_a_local_shadowed_in_a_nested_block_is_refused() {
    let out = compile_rust(
        "class Counter { n: number = 0; add(x: number): void { this.n = this.n + x; } } \
         let g = new Counter(); \
         class D { static bump(): number { return 1; } } \
         class E { bump(): number { g = new Counter(); return 2; } } \
         export function got(): number { \
             { \
                 const D = new E(); \
                 const f = (): number => D.bump(); \
                 g.add(f()); \
             } \
             return g.n; \
         }",
    );
    assert_receiver_reassignment_refused(
        &out,
        "a local declared in a nested block shadows the class name for that block",
    );
}

#[test]
fn a_static_method_behind_a_local_declared_after_its_use_is_refused() {
    let out = compile_rust(
        "class Counter { n: number = 0; add(x: number): void { this.n = this.n + x; } } \
         let g = new Counter(); \
         class D { static bump(): number { return 1; } } \
         class E { bump(): number { g = new Counter(); return 2; } } \
         export function got(): number { \
             const f = (): number => D.bump(); \
             const D = new E(); \
             g.add(f()); \
             return g.n; \
         }",
    );
    assert_receiver_reassignment_refused(
        &out,
        "the local declared after the arrow still shadows the class name when the arrow runs, so \
         `D.bump()` must not resolve to the class's static method",
    );
}

#[test]
fn a_static_method_behind_a_shadowing_local_of_unknown_type_is_refused() {
    let out = compile_rust(
        "class Counter { n: number = 0; add(x: number): void { this.n = this.n + x; } } \
         let g = new Counter(); \
         class D { static bump(): number { return 1; } } \
         function make(): any { return 0; } \
         export function got(): number { \
             const D = make(); \
             const outer = (): number => { const inner = (): number => D.bump(); return inner(); }; \
             g.add(outer()); \
             return g.n; \
         }",
    );
    assert_receiver_reassignment_refused(
        &out,
        "an unknown local type leaves the method's owner unknown, so the call must be refused \
         instead of resolving to the shadowed class",
    );
}

#[test]
fn a_static_method_behind_a_captured_local_in_a_called_function_is_refused() {
    let out = compile_rust(
        "class Counter { n: number = 0; add(x: number): void { this.n = this.n + x; } } \
         let g = new Counter(); \
         class D { static bump(): number { return 1; } } \
         class E { bump(): number { g = new Counter(); return 2; } } \
         function callIt(): number { \
             const D = new E(); \
             const outer = (): number => { const inner = (): number => D.bump(); return inner(); }; \
             return outer(); \
         } \
         export function got(): number { g.add(callIt()); return g.n; }",
    );
    assert_receiver_reassignment_refused(
        &out,
        "a captured local reached through a called function must supply the owner type of \
         `D.bump()`",
    );
}

#[test]
fn a_shadowing_local_that_left_the_scope_before_the_call_is_not_refused() {
    const SOURCE: &str = "class Counter { n: number = 0; add(x: number): void { this.n = this.n + x; } } \
         let g = new Counter(); \
         class D { static safe(): number { return 2; } } \
         class E { bump(): number { g = new Counter(); return 3; } } \
         export function got(): number { \
             { const D = new E(); const f = (): number => D.bump(); f(); } \
             g.add(D.safe()); \
             return g.n; \
         }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(shadowing_local_out_of_scope::got(&ctx), 2.0);
    println!("ok");
}
"#;
    let out = compile_rust(SOURCE);
    assert!(
        !out.has_errors(),
        "an out-of-scope local must not keep shadowing the class name at the call site; got {:?}",
        out.diagnostics
    );
    let source = out.rust_source.expect("rust_source");
    check_generated_crate("shadowing_local_out_of_scope", &source);
    let stdout = run_generated_crate("shadowing_local_out_of_scope", &source, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_safe_arrow_wrapper_argument_compiles_and_runs() {
    const SOURCE: &str = "class Counter { n: number = 0; add(x: number): void { this.n = this.n + x; } } \
         let g = new Counter(); \
         export function got(): number { \
             const f = (): number => 1; \
             const wrap = (cb: () => number): number => cb(); \
             g.add(wrap(f)); \
             return g.n; \
         }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(global_arrow_wrapper_safe::got(&ctx), 1.0);
    println!("ok");
}
"#;
    let out = compile_rust(SOURCE);
    assert!(
        !out.has_errors(),
        "a local arrow wrapper that calls a safe callback must compile, got {:?}",
        out.diagnostics
    );
    let source = out.rust_source.expect("rust_source");
    check_generated_crate("global_arrow_wrapper_safe", &source);
    let stdout = run_generated_crate("global_arrow_wrapper_safe", &source, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_safe_two_level_arrow_wrapper_argument_compiles_and_runs() {
    const SOURCE: &str = "class Counter { n: number = 0; add(x: number): void { this.n = this.n + x; } } \
         let g = new Counter(); \
         export function got(): number { \
             const f = (): number => 1; \
             const outer = (cb: () => number): number => ((c: () => number): number => c())(cb); \
             g.add(outer(f)); \
             return g.n; \
         }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(global_two_level_wrapper_safe::got(&ctx), 1.0);
    println!("ok");
}
"#;
    let out = compile_rust(SOURCE);
    assert!(
        !out.has_errors(),
        "a two-level local arrow wrapper around a safe callback must compile, got {:?}",
        out.diagnostics
    );
    let source = out.rust_source.expect("rust_source");
    check_generated_crate("global_two_level_wrapper_safe", &source);
    let stdout = run_generated_crate("global_two_level_wrapper_safe", &source, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_writing_callback_in_an_arrow_wrapper_argument_is_refused() {
    let out = compile_rust(
        "class Counter { n: number = 0; add(x: number): void { this.n = this.n + x; } } \
         let g = new Counter(); \
         export function got(): number { \
             const f = (): number => { g = new Counter(); return 1; }; \
             const wrap = (cb: () => number): number => cb(); \
             g.add(wrap(f)); \
             return g.n; \
         }",
    );
    assert_receiver_reassignment_refused(
        &out,
        "a callback bound through a local arrow wrapper must be inspected",
    );
}

#[test]
fn a_writing_callback_in_a_two_level_arrow_wrapper_argument_is_refused() {
    let out = compile_rust(
        "class Counter { n: number = 0; add(x: number): void { this.n = this.n + x; } } \
         let g = new Counter(); \
         export function got(): number { \
             const f = (): number => { g = new Counter(); return 1; }; \
             const inner = (cb: () => number): number => cb(); \
             const outer = (cb: () => number): number => inner(cb); \
             g.add(outer(f)); \
             return g.n; \
         }",
    );
    assert_receiver_reassignment_refused(
        &out,
        "a callback reached through two levels of local arrow wrappers must be inspected",
    );
}

#[test]
fn a_writing_callback_invoked_in_a_loop_by_an_arrow_wrapper_is_refused() {
    let out = compile_rust(
        "class Counter { n: number = 0; add(x: number): void { this.n = this.n + x; } } \
         let g = new Counter(); \
         export function got(): number { \
             const f = (): number => { g = new Counter(); return 1; }; \
             const wrap = (cb: () => number): number => { \
                 let i = 0; \
                 while (i < 1) { i = i + 1; cb(); } \
                 return 1; \
             }; \
             g.add(wrap(f)); \
             return g.n; \
         }",
    );
    assert_receiver_reassignment_refused(
        &out,
        "a callback called from a loop inside a local arrow wrapper must be inspected",
    );
}

const GLOBAL_ZERO_ARG_MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(global_zero_arg_call::got(&ctx), 1.0);
    println!("ok");
}
"#;

#[test]
fn a_zero_argument_mutating_global_call_runs_the_method() {
    let out = compile_rust(
        "class Counter { n: number = 0; bump(): void { this.n = this.n + 1; } } \
         let g = new Counter(); \
         export function got(): number { g.bump(); return g.n; }",
    );
    assert!(!out.has_errors(), "got {:?}", out.diagnostics);
    let source = out.rust_source.expect("rust_source");
    let stdout = run_generated_crate("global_zero_arg_call", &source, GLOBAL_ZERO_ARG_MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn immutable_global_is_not_wrapped_in_unsafe() {
    let out = compile_rust("const answer: i64 = 42; export function get(): i64 { return answer; }");
    assert!(!out.has_errors(), "got {:?}", out.diagnostics);
    let source = out.rust_source.expect("rust_source");
    assert!(
        source.contains("static answer"),
        "an immutable global must be emitted as a plain `static`; got:\n{source}"
    );
    assert!(
        !source.contains("unsafe"),
        "reads of an immutable global must not require `unsafe`; got:\n{source}"
    );
}

#[test]
fn mutable_string_global_uses_owned_storage_that_outlives_the_arena() {
    let out = compile_rust(
        "let greeting: string; \
         export function setGreeting(s: string): void { greeting = s; } \
         export function getGreeting(): string { return greeting; }",
    );
    assert!(
        !out.has_errors(),
        "a mutable string global must compile through the full pipeline; got {:?}",
        out.diagnostics
    );
    let source = out
        .rust_source
        .expect("emit-rust must populate rust_source for the string-global check");
    let norm = common::normalize_rust(&source);
    assert!(
        source.contains("static mut greeting"),
        "a reassigned string global must be a mutable static; got:\n{source}"
    );
    assert!(
        norm.contains("::std::string::String"),
        "a string global must use the owned `String` representation (the arena-borrowing \
         `JsString<'static>` cannot store a value past the call's arena); got:\n{source}"
    );
    assert!(
        !source.contains("'static"),
        "a string global must not extend the arena borrow to `'static`; got:\n{source}"
    );
    assert!(
        source.contains("to_string_lossy ()"),
        "assigning a borrowed string to the global must convert through `to_string_lossy`; \
         got:\n{source}"
    );
    check_generated_crate("mutable_string_global", &source);
}

const ARENA_RESET_MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host, JsString};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let mut bump = Bump::new();

    {
        let ctx = Ctx { arena: &bump, host: &host };
        arena_string_global::setGreeting(&ctx, JsString::from("hello"));
        assert_eq!(
            arena_string_global::getGreeting(&ctx).to_string_lossy(),
            "hello",
            "the greeting must be readable before the arena resets"
        );
    }

    bump.reset();

    {
        let ctx = Ctx { arena: &bump, host: &host };
        assert_eq!(
            arena_string_global::getGreeting(&ctx).to_string_lossy(),
            "hello",
            "a string global must keep its value after the arena resets"
        );
    }

    println!("STRING_GLOBAL_SURVIVED_ARENA_RESET");
}
"#;

#[test]
fn mutable_string_global_survives_an_arena_reset_at_runtime() {
    let out = compile_script(
        "let greeting: string; \
         export function setGreeting(s: string): void { greeting = s; } \
         export function getGreeting(): string { return greeting; }",
    );
    assert!(
        !out.has_errors(),
        "a mutable string global must compile through the full pipeline; got {:?}",
        out.diagnostics
    );
    let source = out
        .rust_source
        .expect("emit-rust must populate rust_source for the arena-reset check");
    let stdout = run_generated_crate("arena_string_global", &source, ARENA_RESET_MAIN);
    assert!(
        stdout.contains("STRING_GLOBAL_SURVIVED_ARENA_RESET"),
        "the generated crate must store a string global outside the arena; got stdout:\n{stdout}"
    );
}

#[test]
fn optional_string_global_accepts_a_parameter_write() {
    let out = compile_rust(
        "let g: string | null = null; \
         export function set(v: string | null): void { g = v; } \
         export function get(): string | null { return g; }",
    );
    assert!(
        !out.has_errors(),
        "a mutable `string | null` global must compile through the full pipeline; got {:?}",
        out.diagnostics
    );
    let source = out
        .rust_source
        .expect("emit-rust must populate rust_source for the optional-string-global check");
    let norm = common::normalize_rust(&source);
    assert!(
        norm.contains("Option<::std::string::String>"),
        "a `string | null` global must use the owned `Option<String>` representation so a write \
         from a `string | null` parameter is accepted (a borrowed `Option<JsString<'static>>` \
         would reject it); got:\n{source}"
    );
    assert!(
        !source.contains("'static"),
        "an optional-string global must not extend the arena borrow to `'static`; got:\n{source}"
    );
    check_generated_crate("optional_string_global", &source);
}

const OPTIONAL_ARENA_RESET_MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host, JsString};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let mut bump = Bump::new();

    {
        let ctx = Ctx { arena: &bump, host: &host };
        optional_string_global::set(&ctx, Some(JsString::from("hello")));
        assert_eq!(
            optional_string_global::get(&ctx).map(|s| s.to_string_lossy()),
            Some("hello".to_string()),
            "the optional greeting must be readable before the arena resets"
        );
    }

    bump.reset();

    {
        let ctx = Ctx { arena: &bump, host: &host };
        assert_eq!(
            optional_string_global::get(&ctx).map(|s| s.to_string_lossy()),
            Some("hello".to_string()),
            "an optional string global must keep its value after the arena resets"
        );
    }

    println!("OPTIONAL_STRING_GLOBAL_SURVIVED_ARENA_RESET");
}
"#;

#[test]
fn optional_string_global_survives_an_arena_reset_at_runtime() {
    let out = compile_script(
        "let g: string | null; \
         export function set(v: string | null): void { g = v; } \
         export function get(): string | null { return g; }",
    );
    assert!(
        !out.has_errors(),
        "a mutable `string | null` global must compile through the full pipeline; got {:?}",
        out.diagnostics
    );
    let source = out
        .rust_source
        .expect("emit-rust must populate rust_source for the optional arena-reset check");
    let stdout = run_generated_crate("optional_string_global", &source, OPTIONAL_ARENA_RESET_MAIN);
    assert!(
        stdout.contains("OPTIONAL_STRING_GLOBAL_SURVIVED_ARENA_RESET"),
        "the generated crate must store an optional string global outside the arena; got \
         stdout:\n{stdout}"
    );
}

const OPTIONAL_STRING_WRITE_SOURCE: &str = "let g: string | null = null; \
     export function setStr(v: string): void { g = v; } \
     export function setNull(): void { g = null; } \
     export function setLit(): void { g = \"lit\"; } \
     export function get(): string | null { return g; } \
     export function localOps(): void { \
         let a: string | null = null; \
         a = \"hi\"; \
         let b: string | null = a; \
         b = null; \
         let c: string | null = \"c\"; \
         c = a; \
     }";

#[test]
fn optional_string_global_accepts_string_null_literal_and_local_writes() {
    let out = compile_rust(OPTIONAL_STRING_WRITE_SOURCE);
    assert!(
        !out.has_errors(),
        "writes of every supported source shape (string, null, literal, local) into a \
         `string | null` global must compile through the full pipeline; got {:?}",
        out.diagnostics
    );
    let source = out
        .rust_source
        .expect("emit-rust must populate rust_source for the optional-write check");
    let norm = common::normalize_rust(&source);
    assert!(
        norm.contains("Option::Some"),
        "a plain-string source must be wrapped in `Option::Some(..)`, not mapped, when written \
         into a `string | null` global; got:\n{source}"
    );
    assert!(
        norm.contains("::core::option::Option::None"),
        "a `null` source must be written into a `string | null` global as `Option::None`; \
         got:\n{source}"
    );
    assert!(
        !source.contains("'static"),
        "an optional-string global must not extend the arena borrow to `'static`; got:\n{source}"
    );
    check_generated_crate("optional_string_writes", &source);
}

const OPTIONAL_STRING_WRITE_MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host, JsString};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let mut bump = Bump::new();

    {
        let ctx = Ctx { arena: &bump, host: &host };
        assert_eq!(
            optional_string_writes::get(&ctx).map(|s| s.to_string_lossy()),
            None,
            "the optional global must start as null"
        );
        optional_string_writes::setStr(&ctx, JsString::from("plain"));
        assert_eq!(
            optional_string_writes::get(&ctx).map(|s| s.to_string_lossy()),
            Some("plain".to_string()),
            "writing a plain `string` must set the optional global"
        );
        optional_string_writes::setLit(&ctx);
        assert_eq!(
            optional_string_writes::get(&ctx).map(|s| s.to_string_lossy()),
            Some("lit".to_string()),
            "writing a string literal must set the optional global"
        );
        optional_string_writes::setNull(&ctx);
        assert_eq!(
            optional_string_writes::get(&ctx).map(|s| s.to_string_lossy()),
            None,
            "writing `null` must clear the optional global"
        );
        optional_string_writes::localOps(&ctx);
    }

    bump.reset();

    {
        let ctx = Ctx { arena: &bump, host: &host };
        optional_string_writes::setStr(&ctx, JsString::from("after_reset"));
        assert_eq!(
            optional_string_writes::get(&ctx).map(|s| s.to_string_lossy()),
            Some("after_reset".to_string()),
            "an optional string global must accept a write and keep its value after the arena \
             resets"
        );
    }

    println!("OPTIONAL_STRING_WRITES_OK");
}
"#;

#[test]
fn optional_string_global_writes_compile_and_run() {
    let out = compile_script(OPTIONAL_STRING_WRITE_SOURCE);
    assert!(
        !out.has_errors(),
        "writes of every supported source shape into a `string | null` global must compile; \
         got {:?}",
        out.diagnostics
    );
    let source = out
        .rust_source
        .expect("emit-rust must populate rust_source for the optional-write run check");
    let stdout = run_generated_crate(
        "optional_string_writes",
        &source,
        OPTIONAL_STRING_WRITE_MAIN,
    );
    assert!(
        stdout.contains("OPTIONAL_STRING_WRITES_OK"),
        "the generated crate must accept string, literal, and null writes (and survive an arena \
         reset); got stdout:\n{stdout}"
    );
}
