use ts_aot_driver::{CompileOptions, Driver, EmitStage};
use ts_aot_test_support::{check_generated_crate, run_generated_crate};

fn compile_with(source: &str, emit: EmitStage) -> ts_aot_driver::DriverOutput {
    Driver::new().compile_source("test.ts", source, &CompileOptions { emit, module: true })
}

fn compile_module(source: &str) -> ts_aot_driver::DriverOutput {
    compile_with(source, EmitStage::Rust)
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

fn mir_of(source: &str) -> String {
    let out = compile_with(source, EmitStage::Mir);
    assert!(
        !out.has_errors(),
        "must compile through the full pipeline; got {:?}",
        out.diagnostics
    );
    out.artifact(EmitStage::Mir)
        .expect("emit-mir must populate the mir artifact")
        .to_owned()
}

fn compact(rust: &str) -> String {
    rust.chars().filter(|c| !c.is_whitespace()).collect()
}

const NUMBER_MAIN_HEADER: &str = "use ts_aot_runtime::{Bump, Ctx, Host};\n\
     \n\
     struct NoHost;\n\
     impl Host for NoHost {}\n\
     \n";

fn main_that_asserts(package: &str, expected: &str) -> String {
    format!(
        "{NUMBER_MAIN_HEADER}fn main() {{\n    \
         let host = NoHost;\n    \
         let bump = Bump::new();\n    \
         let ctx = Ctx {{ arena: &bump, host: &host }};\n    \
         assert_eq!({package}::got(&ctx), {expected});\n    \
         println!(\"ok\");\n}}\n"
    )
}

#[test]
fn a_tuple_element_array_literal_erases_its_payloads_and_runs_to_the_node_value() {
    const SOURCE: &str = "export function got(): number { \
         const rows: [number, number][] = [[1, 2]]; const [a] = rows; return 13; }";
    let rust = rust_of(SOURCE);
    let norm = compact(&rust);
    assert!(
        norm.contains("vec![in__ctx.arena;{let_="),
        "the declared element type of the literal is a tuple, which has no Rust value \
         representation of its own, so the `Vec<i64>` payload must be evaluated and discarded \
         instead of being handed to a `Vec<()>` element type; got:\n{rust}"
    );
    check_generated_crate("tuple_payload_erased", &rust);
    let stdout = run_generated_crate(
        "tuple_payload_erased",
        &rust,
        &main_that_asserts("tuple_payload_erased", "13.0"),
    );
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_heterogeneous_tuple_element_array_literal_erases_its_unresolved_inner_literal() {
    const SOURCE: &str = "export function got(): number { \
         const rows: [string, number][] = [[\"x\", 1]]; const [r] = rows; return 14; }";
    let rust = rust_of(SOURCE);
    let norm = compact(&rust);
    assert!(
        norm.contains("{let_=__ts_aot_array_create::<()>("),
        "an inner literal whose elements resolve to no single Rust type produces `Vec<()>` while \
         its binding is `()`, so the creation must itself collapse to `()`; got:\n{rust}"
    );
    check_generated_crate("heterogeneous_tuple_erased", &rust);
    let stdout = run_generated_crate(
        "heterogeneous_tuple_erased",
        &rust,
        &main_that_asserts("heterogeneous_tuple_erased", "14.0"),
    );
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_thirteen_element_tuple_takes_the_fallible_read_because_rust_default_stops_at_twelve() {
    const SOURCE: &str = "export function got(): number { \
         const rows: [number, number, number, number, number, number, number, number, number, \
         number, number, number, number][] = [[1,2,3,4,5,6,7,8,9,10,11,12,13]]; \
         const [a] = rows; return 15; }";
    let rust = rust_of(SOURCE);
    let norm = compact(&rust);
    assert!(
        norm.contains("match__ts_aot_array_get::<()>("),
        "Rust implements `Default` for tuples of up to twelve elements, so a thirteen-element \
         tuple has none and its element read must not use the `Default`-bound accessor; got:\n{rust}"
    );
    assert!(
        !norm.contains("__ts_aot_array_get_or_default"),
        "the thirteen-element tuple must not be read through `_or_default`; got:\n{rust}"
    );
    check_generated_crate("thirteen_element_tuple", &rust);
    let stdout = run_generated_crate(
        "thirteen_element_tuple",
        &rust,
        &main_that_asserts("thirteen_element_tuple", "15.0"),
    );
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn an_index_read_bound_to_a_local_keeps_the_optional_chain_safe_on_an_empty_array() {
    const SOURCE: &str = "class K { m(): number { return 5; } } \
         export function run(xs: (K | undefined)[]): number { \
             const first = xs[0]; return first?.m() ?? 0; } \
         export function direct(xs: (K | undefined)[]): number { return xs[0]?.m() ?? 0; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    use ::core::option::Option::{None, Some};
    let empty = ts_aot_runtime::bumpalo::collections::Vec::new_in(ctx.arena);
    assert_eq!(bound_index_chain::run(&ctx, empty.clone()), 0.0);
    assert_eq!(bound_index_chain::direct(&ctx, empty), 0.0);
    let present = ts_aot_runtime::bumpalo::vec![in ctx.arena; Some(bound_index_chain::K {})];
    assert_eq!(bound_index_chain::run(&ctx, present.clone()), 5.0);
    assert_eq!(bound_index_chain::direct(&ctx, present), 5.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let norm = compact(&rust);
    assert!(
        norm.contains("__ts_aot_array_get_or_default::<Option<K>>("),
        "a local bound from an index read of an `(K | undefined)[]` must be read through the \
         checked accessor, so an out-of-range index yields `None` rather than panicking; got:\n{rust}"
    );
    assert!(
        !norm.contains(":Option<K>=xs[0asusize];"),
        "the bound local must not index the array unchecked; got:\n{rust}"
    );
    check_generated_crate("bound_index_chain", &rust);
    let stdout = run_generated_crate("bound_index_chain", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_non_optional_element_array_still_indexes_unchecked() {
    const SOURCE: &str = "export function got(xs: number[]): number { const v = xs[0]; return v; }";
    let rust = rust_of(SOURCE);
    let norm = compact(&rust);
    assert!(
        norm.contains("xs[0asusize]"),
        "only `Optional`-element arrays take the checked accessor; a `number[]` read must stay a \
         plain index; got:\n{rust}"
    );
}

#[test]
fn the_e0513_taint_arm_for_array_get_optional_refuses_an_escaping_this_capture() {
    const SOURCE: &str = "function make(f: () => number): (() => number)[] { return [f]; } \
         class C { \
             x: number = 3; \
             grab(): (() => number) | undefined { \
                 const a = make((): number => this.x); \
                 const [f] = a; \
                 return f; \
             } \
         } \
         export function got0(): number { \
             const c = new C(); \
             const f = c.grab() as () => number; \
             c.x = 9; \
             return f(); \
         }";
    let mir = mir_of(SOURCE);
    assert!(
        mir.contains("array_get_optional"),
        "the fixture must destructure through `ArrayGetOptional` for the taint arm to be the one \
         under test; got:\n{mir}"
    );
    let out = compile_module(SOURCE);
    let refused = out
        .diagnostics
        .errors()
        .find(|d| d.code.as_str() == "E0513")
        .unwrap_or_else(|| {
            panic!(
                "a `this` capture read back out of an array through the optional element accessor \
                 and returned must stay refused; got {:?}",
                out.diagnostics
            )
        });
    assert!(
        refused.message.contains("outlive the method frame"),
        "the refusal must be the backend frame-lifetime E0513; got `{}`",
        refused.message
    );
    assert!(
        out.rust_source.is_none(),
        "a refused source must not emit rust; got:\n{:?}",
        out.rust_source
    );
}

#[test]
fn the_mir_dump_names_the_optional_array_element_read_and_it_runs() {
    const SOURCE: &str = "export function got(xs: number[]): number { \
         const [a] = xs; return a === undefined ? 1 : 2; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    let empty = ts_aot_runtime::bumpalo::collections::Vec::new_in(ctx.arena);
    assert_eq!(mir_optional_element_read::got(&ctx, empty), 1.0);
    let present = ts_aot_runtime::bumpalo::vec![in ctx.arena; 7.0];
    assert_eq!(mir_optional_element_read::got(&ctx, present), 2.0);
    println!("ok");
}
"#;
    let mir = mir_of(SOURCE);
    assert!(
        mir.contains("runtime array_get_optional("),
        "the `--emit mir` dump must name the optional element read; got:\n{mir}"
    );
    let rust = rust_of(SOURCE);
    let norm = compact(&rust);
    assert!(
        norm.contains("__ts_aot_array_get::<f64>("),
        "an absent array element must stay `None` instead of falling back to the element's \
         `Default`; got:\n{rust}"
    );
    assert!(
        !norm.contains("__ts_aot_array_get_or_default"),
        "the optional element read must not use the `Default`-bound accessor; got:\n{rust}"
    );
    check_generated_crate("mir_optional_element_read", &rust);
    let stdout = run_generated_crate("mir_optional_element_read", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}
