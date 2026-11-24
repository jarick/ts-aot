use ts_aot_driver::{CompileOptions, Driver, EmitStage};
use ts_aot_test_support::{check_generated_crate, run_generated_crate};

fn compile(source: &str) -> ts_aot_driver::DriverOutput {
    Driver::new().compile_source(
        "test.ts",
        source,
        &CompileOptions {
            emit: EmitStage::Rust,
            module: true,
        },
    )
}

fn run(source: &str, expected: f64) {
    let output = compile(source);
    assert!(!output.has_errors(), "{:?}", output.diagnostics);
    let main = format!(
        r"
use ts_aot_runtime::{{Bump, Ctx, Host}};
struct NoHost;
impl Host for NoHost {{}}
fn main() {{
    let bump = Bump::new();
    let host = NoHost;
    let ctx = Ctx {{ arena: &bump, host: &host }};
    assert_eq!(regression::got(&ctx), {expected:?});
}}
"
    );
    let _ = run_generated_crate("regression", &output.rust_source.unwrap(), &main);
}

#[test]
fn a_discarded_async_mutating_method_call_still_runs() {
    let source = "class Counter { n: number = 0; \
             async add(x: number): Promise<void> { this.n = this.n + x; } } \
         export async function got(): Promise<number> { \
             const c = new Counter(); \
             c.add(5); \
             return c.n; \
         }";
    let output = compile(source);
    assert!(!output.has_errors(), "{:?}", output.diagnostics);
    let rust = output
        .rust_source
        .expect("emit-rust must populate rust_source");
    let norm: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        norm.contains("__ts_aot_run_or_poll"),
        "a discarded async mutating method call must be driven through the promise helper, or the \
         generated `Future` is dropped and the method body never runs; got:\n{rust}"
    );
    let main = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    let promise = ts_aot_runtime::__ts_aot_run_or_poll(regression_async::got(&ctx));
    assert_eq!(ts_aot_runtime::__ts_aot_await_value::<f64>(&promise), 5.0);
    println!("ok");
}
"#;
    let stdout = run_generated_crate("regression_async", &rust, main);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn optional_symbol_typeof() {
    let output = compile(
        "export function got(s: Symbol | undefined): number { return typeof s === 'symbol' ? 1 : (typeof s === 'undefined' ? 2 : 0); }",
    );
    assert!(!output.has_errors(), "{:?}", output.diagnostics);
    let _ = run_generated_crate(
        "optional_symbol",
        &output.rust_source.unwrap(),
        r"
use ts_aot_runtime::{Bump, Ctx, Host};
struct NoHost;
impl Host for NoHost {}
fn main() {
 let bump = Bump::new(); let host = NoHost; let ctx = Ctx { arena: &bump, host: &host };
 assert_eq!(optional_symbol::got(&ctx, Some(1)), 1.0);
 assert_eq!(optional_symbol::got(&ctx, None), 2.0);
}
",
    );
}

#[test]
fn switch_cross_arm_assignment() {
    run(
        "export function got(): number { let result = 0; switch (1) { case 1: let x = 1; case 2: x = 4; result = 9; } return result; }",
        9.0,
    );
}

#[test]
fn condition_calls_are_evaluated_once() {
    run(
        "let count = 0; function next(): number { count++; return count; } export function got(): number { if (next()) { return count; } return 0; }",
        1.0,
    );
}

#[test]
fn optional_condition_calls_are_evaluated_once() {
    run(
        "let count = 0; function next(): number | undefined { count++; return count; } export function got(): number { if (next()) { return count; } return 0; }",
        1.0,
    );
}

#[test]
fn lhs_class_classification_precedes_rhs_mutation() {
    run(
        "class Holder { C = class { static k = 1; }; } export function got(): number { const h = new Holder(); return h.C === (h.C = 0) ? 0 : 1; }",
        1.0,
    );
}

#[test]
fn receiver_index_is_selected_before_argument_statements() {
    run(
        "class C { value = 0; set(x: number): void { this.value = x; } } export function got(): number { const a = [new C(), new C()]; let i = 0; a[i].set((i = 1, 7)); return a[0].value * 10 + a[1].value; }",
        70.0,
    );
}

#[test]
fn effectful_class_switch_test_is_refused() {
    let output = compile(
        "class H { C = class {}; } function test(): number { return 0; } export function got(): number { const h = new H(); switch(h.C) { case test(): return 1; default: return 2; } }",
    );
    assert!(
        output
            .diagnostics
            .iter()
            .any(|d| d.code.as_str() == "E0419"),
        "{:?}",
        output.diagnostics
    );
}

#[test]
fn updates_invalidate_local_and_module_class_candidates() {
    for source in [
        "let C = class { static k = 4; }; export function got(): number { C++; return C.k; }",
        "export function got(): number { let C = class { static k = 4; }; C++; return C.k; }",
        "export function got(): number { let C = class { static k = 4; }; C += class { static k = 9; }; return C.k; }",
        "class H { C = class { static k = 4; }; } export function got(): number { const h = new H(); h.C += h.C; return h.C.k; }",
    ] {
        assert!(
            compile(source).has_errors(),
            "stale class candidate accepted: {source}"
        );
    }
}

#[test]
fn conditional_and_loop_writes_do_not_hide_tdz() {
    for prefix in [
        "if (flag) { k = 2; }",
        "while (flag) { switch(k) { case 1: const x = 5; break; case 2: return x; } k = 2; }",
        "do { switch(k) { case 1: const x = 5; break; case 2: return x; } k = 2; } while(flag);",
        "for (; flag; k = 2) { switch(k) { case 1: const x = 5; break; case 2: return x; } }",
    ] {
        let source = format!(
            "export function got(flag: boolean): number {{ let k = 1; {prefix} switch(k) {{ case 1: const x = 5; return x; case 2: return x; }} return 0; }}"
        );
        let output = compile(&source);
        assert!(
            output
                .diagnostics
                .iter()
                .any(|d| d.code.as_str() == "E0419"),
            "stale constant accepted: {source}: {:?}",
            output.diagnostics
        );
    }
}

#[test]
fn initialized_function_condition_stays_truthy() {
    run(
        "export function got(): number { const f = function(): number { return 4; }; if(f) { return f(); } return 0; }",
        4.0,
    );
}

#[test]
fn unset_function_condition_is_false() {
    let output =
        compile("export function got(f: () => number): number { if (f) { return 1; } return 0; }");
    assert!(!output.has_errors(), "{:?}", output.diagnostics);
    let rust = output.rust_source.unwrap();
    let enum_name = rust
        .split("pub enum ")
        .nth(1)
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap();
    let main = format!(
        r"
use ts_aot_runtime::{{Bump, Ctx, Host}};
struct NoHost;
impl Host for NoHost {{}}
fn main() {{
 let bump = Bump::new(); let host = NoHost; let ctx = Ctx {{ arena: &bump, host: &host }};
 assert_eq!(unset_fn::got(&ctx, unset_fn::{enum_name}::__Unset), 0.0);
}}
"
    );
    let _ = run_generated_crate("unset_fn", &rust, &main);
}

#[test]
fn nested_receiver_indices_are_captured() {
    run(
        "class C { value = 0; set(x: number): void { this.value = x; } } export function got(): number { const a = [[new C(), new C()], [new C(), new C()]]; let i = 0; let j = 0; a[i][j].set((i = 1, j = 1, 7)); return a[0][0].value * 10 + a[1][1].value; }",
        70.0,
    );
}

#[test]
fn poisoned_field_truthiness_is_not_folded_as_a_class() {
    run(
        "class H { C = class { static k = 1; }; } export function got(): number { const h = new H(); h.C = 0; return !h.C ? 1 : 0; }",
        1.0,
    );
    run(
        "class H { C = class { static k = 1; }; } export function got(): number { const h = new H(); h.C = 0; return h.C === 0 ? 1 : 0; }",
        1.0,
    );
}

#[test]
fn constructor_arguments_make_receiver_reassignment_analysis_opaque() {
    let output = compile(
        "class Counter { n = 0; add(x: number): void { this.n = x; } } let counter = new Counter(); class Replace { value = 1; constructor() { counter = new Counter(); } } export function got(): number { counter.add(new Replace().value); return counter.n; }",
    );
    assert!(
        output
            .diagnostics
            .iter()
            .any(|d| d.code.as_str() == "E0419"),
        "{:?}",
        output.diagnostics
    );
}

#[test]
fn array_push_stages_an_item_that_reads_the_receiver_cell() {
    run(
        "class Bag { a: number[]; constructor(a: number[]) { this.a = a; } add(): void { this.a.push(this.a[0]); } } \
         export function got(): number { let xs: number[] = []; xs.push(1); xs.push(2); let b = new Bag(xs); b.add(); return b.a.length; }",
        3.0,
    );
}

#[test]
fn a_nullable_struct_global_is_a_mutable_placeholder() {
    let source = "class C { x: number = 0; } let g: C | null = null; \
         export function got(): number { if (g === null) { return 0; } return 1; } \
         export function read_x(): number { return g?.x ?? -1; } \
         export function set_x(v: number): number { const c = new C(); c.x = v; g = c; return read_x(); }";
    let output = compile(source);
    assert!(!output.has_errors(), "{:?}", output.diagnostics);
    let rust = output.rust_source.unwrap();
    let norm: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        norm.contains("staticmutg:Option<C>=::core::option::Option::None;"),
        "a global whose type holds a class instance must be a `static mut` placeholder, because an \
         `Rc<RefCell<..>>` field is not `Sync` and a plain `static` would be rejected; got:\n{rust}"
    );
    check_generated_crate("nullable_struct_global", &rust);
    let main = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(nullable_struct_global::got(&ctx), 0.0);
    assert_eq!(nullable_struct_global::read_x(&ctx), -1.0);
    assert_eq!(nullable_struct_global::set_x(&ctx, 42.0), 42.0);
    assert_eq!(nullable_struct_global::read_x(&ctx), 42.0);
    assert_eq!(nullable_struct_global::got(&ctx), 1.0);
    println!("ok");
}
"#;
    let stdout = run_generated_crate("nullable_struct_global", &rust, main);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn an_instance_argument_is_mutated_through_the_callee() {
    run(
        "class C { x: number = 0; } function bump(c: C): void { c.x = c.x + 3; } \
         export function got(): number { const c = new C(); bump(c); return c.x; }",
        3.0,
    );
}
